//! Geometry operations: crop, rotate, flip, straighten, perspective and
//! Lanczos3 resize. All operate on 8-bit `PixelBuffer` storage; resampling
//! happens in linear light internally for gamma-correct results (ADR-0002).
//!
//! Canonical geometry order applied by the pipeline (ADR-0005):
//! rotate90/flip → straighten → perspective → crop.

// Index arithmetic over pixel grids is intrinsic here; iterator rewrites
// would obscure the kernels.
#![allow(clippy::needless_range_loop)]

use photoforge_core::{LinearImage, PixelBuffer};

/// Resampling filter quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resample {
    /// Fast, slight softening/aliasing; used during interactive drags.
    Bilinear,
    /// High quality (windowed sinc, support 3); used for exports and
    /// committed geometry.
    Lanczos3,
}

/// How out-of-source pixels are filled during warps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeFill {
    /// Replicate the nearest edge pixel.
    ClampEdge,
    /// Transparent-out treated as opaque black.
    Black,
}

/// Options for warps (straighten / perspective).
#[derive(Clone, Copy, Debug)]
pub struct WarpOptions {
    pub resample: Resample,
    pub fill: EdgeFill,
}

impl Default for WarpOptions {
    fn default() -> Self {
        Self {
            resample: Resample::Lanczos3,
            fill: EdgeFill::ClampEdge,
        }
    }
}

// ---------------------------------------------------------------------------
// Lossless remaps (u8 domain)
// ---------------------------------------------------------------------------

/// Rotates by `quarter` quarter-turns clockwise (0..4; values mod 4).
pub fn rotate90(img: &PixelBuffer, quarter: u32) -> PixelBuffer {
    match quarter % 4 {
        0 => img.clone(),
        1 => {
            // CW: dst dims (H x W); src (x, y) -> dst (sh − 1 − y, x).
            let (sw, sh) = (img.width(), img.height());
            let mut out = PixelBuffer::new(sh, sw);
            for y in 0..sh {
                for x in 0..sw {
                    out.set_pixel(sh - 1 - y, x, img.pixel(x, y));
                }
            }
            out
        }
        2 => {
            let (w, h) = (img.width(), img.height());
            let mut out = PixelBuffer::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    out.set_pixel(x, y, img.pixel(w - 1 - x, h - 1 - y));
                }
            }
            out
        }
        _ => {
            // CCW: dst dims (H x W); src (x, y) -> dst (y, sw − 1 − x).
            let (sw, sh) = (img.width(), img.height());
            let mut out = PixelBuffer::new(sh, sw);
            for y in 0..sh {
                for x in 0..sw {
                    out.set_pixel(y, sw - 1 - x, img.pixel(x, y));
                }
            }
            out
        }
    }
}

/// Mirrors horizontally (left ↔ right).
pub fn flip_h(img: &PixelBuffer) -> PixelBuffer {
    let (w, h) = (img.width(), img.height());
    let mut out = PixelBuffer::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.set_pixel(x, y, img.pixel(w - 1 - x, y));
        }
    }
    out
}

/// Mirrors vertically (top ↔ bottom).
pub fn flip_v(img: &PixelBuffer) -> PixelBuffer {
    let (w, h) = (img.width(), img.height());
    let mut out = PixelBuffer::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.set_pixel(x, y, img.pixel(x, h - 1 - y));
        }
    }
    out
}

/// Crops to `(x, y, w, h)`, clamped to the image bounds.
pub fn crop(img: &PixelBuffer, x: u32, y: u32, w: u32, h: u32) -> PixelBuffer {
    img.region(x, y, w, h)
}

// ---------------------------------------------------------------------------
// 1-D filter machinery (resize, warp sampling)
// ---------------------------------------------------------------------------

/// Lanczos kernel with a = 3.
#[inline]
fn lanczos3(x: f32) -> f32 {
    let x = x.abs();
    if x < f32::EPSILON {
        1.0
    } else if x >= 3.0 {
        0.0
    } else {
        let px = std::f32::consts::PI * x;
        3.0 * px.sin() * (px / 3.0).sin() / (px * px)
    }
}

#[inline]
fn triangle(x: f32) -> f32 {
    (1.0 - x.abs()).max(0.0)
}

/// Precomputed 1-D resampling weights: for each output index, a source start
/// index and normalized weights over `[start, start + weights.len())`.
struct Filter1D {
    starts: Vec<i32>,
    weights: Vec<Vec<f32>>,
    /// Max distance (in source pixels) any weight reaches from its center —
    /// the halo needed for a vertical pass.
    support_px: u32,
}

impl Filter1D {
    /// Builds weights mapping `src_len` samples onto `dst_len` samples.
    /// Downscaling widens the kernel (support scales with `src/dst`) so the
    /// filter properly low-passes.
    fn build(src_len: u32, dst_len: u32, resample: Resample) -> Self {
        assert!(src_len > 0 && dst_len > 0);
        let scale = src_len as f32 / dst_len as f32;
        let fscale = scale.max(1.0);
        let (kern, base_support) = match resample {
            Resample::Lanczos3 => (lanczos3 as fn(f32) -> f32, 3.0f32),
            Resample::Bilinear => (triangle as fn(f32) -> f32, 1.0f32),
        };
        let support = base_support * fscale;
        let mut starts = Vec::with_capacity(dst_len as usize);
        let mut weights = Vec::with_capacity(dst_len as usize);
        for i in 0..dst_len {
            let center = (i as f32 + 0.5) * scale - 0.5;
            let lo = (center - support).floor() as i32;
            let hi = (center + support).ceil() as i32;
            let mut w: Vec<f32> = Vec::with_capacity((hi - lo + 1) as usize);
            let mut sum = 0.0f32;
            for x in lo..=hi {
                let weight = kern((x as f32 - center) / fscale);
                w.push(weight);
                sum += weight;
            }
            if sum.abs() > f32::EPSILON {
                for weight in &mut w {
                    *weight /= sum;
                }
            } else {
                // Degenerate (should not happen); fall back to nearest.
                w.clear();
                w.push(1.0);
            }
            starts.push(lo);
            weights.push(w);
        }
        Filter1D {
            starts,
            weights,
            support_px: support.ceil() as u32 + 1,
        }
    }
}

fn sample_row_filtered(row: &[f32], f: &Filter1D, out: &mut [f32]) {
    let src_len = row.len() / 4;
    for (dst_x, o) in out.chunks_exact_mut(4).enumerate() {
        let start = f.starts[dst_x];
        let weights = &f.weights[dst_x];
        let mut acc = [0.0f32; 4];
        for (k, &weight) in weights.iter().enumerate() {
            let sx = (start + k as i32).clamp(0, src_len as i32 - 1) as usize;
            let px = &row[sx * 4..sx * 4 + 4];
            acc[0] += weight * px[0];
            acc[1] += weight * px[1];
            acc[2] += weight * px[2];
            acc[3] += weight * px[3];
        }
        o.copy_from_slice(&acc);
    }
}

// ---------------------------------------------------------------------------
// Resize
// ---------------------------------------------------------------------------

/// Resamples to `(dst_w, dst_h)`. Separable: horizontal pass then vertical
/// pass, both band-parallel, in linear light.
pub fn resize(src: &PixelBuffer, dst_w: u32, dst_h: u32, resample: Resample) -> PixelBuffer {
    if src.is_empty() || dst_w == 0 || dst_h == 0 {
        return PixelBuffer::new(dst_w, dst_h);
    }
    if src.width() == dst_w && src.height() == dst_h {
        return src.clone();
    }
    let lin = LinearImage::from_pixel_buffer(src);
    let (sw, sh) = (lin.width, lin.height);

    // Horizontal pass: (dst_w x sh); rows are independent.
    let fh = Filter1D::build(sw, dst_w, resample);
    let mut temp = LinearImage::new(dst_w, sh);
    let stride = dst_w as usize * 4;
    photoforge_core::tile::par_bands(&mut temp.data, stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let off = (y - y0) as usize * stride;
            sample_row_filtered(lin.row(y), &fh, &mut band[off..off + stride]);
        }
    });

    // Vertical pass: out rows read temp rows ± support (immutable temp,
    // disjoint output rows — the band framework's halo case).
    let fv = Filter1D::build(sh, dst_h, resample);
    let mut out = LinearImage::new(dst_w, dst_h);
    vertical_pass(&temp, &mut out, &fv);
    out.to_pixel_buffer()
}

/// Vertical resampling pass: output row `y` is a weighted combination of
/// temp rows around `y`. Implemented with halo bands (temp immutable;
/// source and destination heights may differ).
fn vertical_pass(temp: &LinearImage, out: &mut LinearImage, fv: &Filter1D) {
    let th = temp.height as i64;
    let stride = out.width as usize * 4;
    let halo = fv.support_px;
    photoforge_core::tile::par_bands_with_src(
        &temp.data,
        &mut out.data,
        stride,
        temp.height,
        64,
        halo,
        |y0, y1, src_band, out_band| {
            let base = y0.saturating_sub(halo) as i64;
            let band_end = y1 as i64 + halo as i64;
            for y in y0..y1 {
                let off = (y - y0) as usize * stride;
                let orow = &mut out_band[off..off + stride];
                for px in orow.chunks_exact_mut(4) {
                    px[0] = 0.0;
                    px[1] = 0.0;
                    px[2] = 0.0;
                    px[3] = 0.0;
                }
                let start = fv.starts[y as usize];
                for (k, &weight) in fv.weights[y as usize].iter().enumerate() {
                    // Clamp to the image first, then into the band window
                    // (band clamping only matters at the image borders where
                    // edge replication kicks in).
                    let sy = (start + k as i32)
                        .clamp(0, th as i32 - 1)
                        .clamp(base as i32, band_end as i32 - 1)
                        as i64;
                    let soff = (sy - base) as usize * stride;
                    let srow = &src_band[soff..soff + stride];
                    for (o, s) in orow.chunks_exact_mut(4).zip(srow.chunks_exact(4)) {
                        o[0] += weight * s[0];
                        o[1] += weight * s[1];
                        o[2] += weight * s[2];
                        o[3] += weight * s[3];
                    }
                }
            }
        },
    );
}

// ---------------------------------------------------------------------------
// Warps (straighten, perspective)
// ---------------------------------------------------------------------------

/// 3×3 row-major homography.
pub type Homography = [[f32; 3]; 3];

/// Solves the 8-unknown DLT for the homography mapping destination *pixel
/// indices* (0,0)-(w−1,0)-(w−1,h−1)-(0,h−1) onto `quad` (same order: TL, TR,
/// BR, BL, in source pixel-center coordinates), with h33 = 1. Returns the
/// homography and the output size.
pub fn homography_for_quad(quad: [[f32; 2]; 4]) -> (Homography, u32, u32) {
    let [tl, tr, br, bl] = quad;
    // +1: the quad spans pixel centers, so the pixel count is span + 1.
    let w = ((tr[0] - tl[0]).hypot(tr[1] - tl[1]))
        .max((br[0] - bl[0]).hypot(br[1] - bl[1]))
        .round() as i64
        + 1;
    let h = ((bl[0] - tl[0]).hypot(bl[1] - tl[1]))
        .max((br[0] - tr[0]).hypot(br[1] - tr[1]))
        .round() as i64
        + 1;
    let w = w.max(1) as u32;
    let h = h.max(1) as u32;
    let dst = [
        [0.0f32, 0.0f32],
        [w as f32 - 1.0, 0.0],
        [w as f32 - 1.0, h as f32 - 1.0],
        [0.0, h as f32 - 1.0],
    ];
    // Rows of the 8x8 system; rhs vector.
    let mut a = [[0.0f32; 8]; 8];
    let mut b = [0.0f32; 8];
    for (i, &[u, v]) in dst.iter().enumerate() {
        let [x, y] = quad[i];
        let r0 = i * 2;
        a[r0] = [u, v, 1.0, 0.0, 0.0, 0.0, -x * u, -x * v];
        b[r0] = x;
        let r1 = i * 2 + 1;
        a[r1] = [0.0, 0.0, 0.0, u, v, 1.0, -y * u, -y * v];
        b[r1] = y;
    }
    let sol = solve_linear_8(&a, &b).unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
    let homo = [
        [sol[0], sol[1], sol[2]],
        [sol[3], sol[4], sol[5]],
        [sol[6], sol[7], 1.0],
    ];
    (homo, w, h)
}

/// Gaussian elimination with partial pivoting for an 8×8 system.
fn solve_linear_8(a: &[[f32; 8]; 8], b: &[f32; 8]) -> Option<[f32; 8]> {
    let mut a = *a;
    let mut b = *b;
    for col in 0..8 {
        // Pivot.
        let mut pivot = col;
        let mut best = a[col][col].abs();
        for r in col + 1..8 {
            if a[r][col].abs() > best {
                best = a[r][col].abs();
                pivot = r;
            }
        }
        if best < 1e-10 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let d = a[col][col];
        for r in col + 1..8 {
            let factor = a[r][col] / d;
            if factor == 0.0 {
                continue;
            }
            for c in col..8 {
                a[r][c] -= factor * a[col][c];
            }
            b[r] -= factor * b[col];
        }
    }
    let mut x = [0.0f32; 8];
    for row in (0..8).rev() {
        let mut s = b[row];
        for c in row + 1..8 {
            s -= a[row][c] * x[c];
        }
        x[row] = s / a[row][row];
    }
    Some(x)
}

/// Applies a homography to a point (with implicit 1 in z).
pub fn apply_homography(h: &Homography, p: [f32; 2]) -> [f32; 2] {
    let x = h[0][0] * p[0] + h[0][1] * p[1] + h[0][2];
    let y = h[1][0] * p[0] + h[1][1] * p[1] + h[1][2];
    let w = h[2][0] * p[0] + h[2][1] * p[1] + h[2][2];
    if w.abs() < f32::EPSILON {
        return [f32::INFINITY, f32::INFINITY];
    }
    [x / w, y / w]
}

/// Samples the linear image at a float position with the chosen filter.
/// Non-finite positions are sanitized (clamped into the image), so warps
/// through degenerate homographies never panic.
fn sample_filtered(
    src: &LinearImage,
    x: f32,
    y: f32,
    resample: Resample,
    fill: EdgeFill,
) -> [f32; 4] {
    let (w, h) = (src.width as i32, src.height as i32);
    let max_x = (w - 1) as f32;
    let max_y = (h - 1) as f32;
    let inside = x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0 && x <= max_x && y <= max_y;
    if !inside && fill == EdgeFill::Black {
        return [0.0, 0.0, 0.0, 1.0];
    }
    // ClampEdge (and sanitization for non-finite coords).
    let x = if x.is_nan() { 0.0 } else { x.clamp(0.0, max_x) };
    let y = if y.is_nan() { 0.0 } else { y.clamp(0.0, max_y) };
    match resample {
        Resample::Bilinear => {
            let x0 = x.floor() as i32;
            let y0 = y.floor() as i32;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let fx = (x - x0 as f32).clamp(0.0, 1.0);
            let fy = (y - y0 as f32).clamp(0.0, 1.0);
            let p00 = src.pixel(x0 as u32, y0 as u32);
            let p10 = src.pixel(x1 as u32, y0 as u32);
            let p01 = src.pixel(x0 as u32, y1 as u32);
            let p11 = src.pixel(x1 as u32, y1 as u32);
            let mut acc = [0.0f32; 4];
            for c in 0..4 {
                let top = p00[c] * (1.0 - fx) + p10[c] * fx;
                let bot = p01[c] * (1.0 - fx) + p11[c] * fx;
                acc[c] = top * (1.0 - fy) + bot * fy;
            }
            acc
        }
        Resample::Lanczos3 => {
            let x0 = x.floor() as i32;
            let y0 = y.floor() as i32;
            let mut wx = [0.0f32; 6];
            let mut wy = [0.0f32; 6];
            for k in 0..6 {
                wx[k] = lanczos3(x - (x0 + k as i32 - 2) as f32);
                wy[k] = lanczos3(y - (y0 + k as i32 - 2) as f32);
            }
            let mut acc = [0.0f32; 4];
            for ky in 0..6 {
                let sy = (y0 + ky as i32 - 2).clamp(0, h - 1) as u32;
                let row = src.row(sy);
                let wyy = wy[ky];
                for kx in 0..6 {
                    let sx = (x0 + kx as i32 - 2).clamp(0, w - 1) as usize;
                    let weight = wyy * wx[kx];
                    acc[0] += weight * row[sx * 4];
                    acc[1] += weight * row[sx * 4 + 1];
                    acc[2] += weight * row[sx * 4 + 2];
                    acc[3] += weight * row[sx * 4 + 3];
                }
            }
            acc
        }
    }
}

/// Warps the source through `homo` (mapping destination pixel coordinates to
/// source coordinates) into a `dst_w × dst_h` output.
pub fn warp(
    src: &PixelBuffer,
    homo: &Homography,
    dst_w: u32,
    dst_h: u32,
    opts: WarpOptions,
) -> PixelBuffer {
    if src.is_empty() || dst_w == 0 || dst_h == 0 {
        return PixelBuffer::new(dst_w, dst_h);
    }
    let lin = LinearImage::from_pixel_buffer(src);
    let mut out = LinearImage::new(dst_w, dst_h);
    let stride = dst_w as usize * 4;
    photoforge_core::tile::par_bands(&mut out.data, stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            for (x, o) in orow.chunks_exact_mut(4).enumerate() {
                let s = apply_homography(homo, [x as f32, y as f32]);
                o.copy_from_slice(&sample_filtered(&lin, s[0], s[1], opts.resample, opts.fill));
            }
        }
    });
    out.to_pixel_buffer()
}

/// Rotates the image about its center by `deg` degrees (clockwise positive),
/// keeping the same output size. Out-of-image corners are filled per
/// `opts.fill`.
pub fn straighten(src: &PixelBuffer, deg: f32, opts: WarpOptions) -> PixelBuffer {
    if src.is_empty() {
        return src.clone();
    }
    let (w, h) = (src.width() as f32, src.height() as f32);
    let cx = (w - 1.0) / 2.0;
    let cy = (h - 1.0) / 2.0;
    let rad = -deg.to_radians(); // inverse map: rotate by −deg
    let (sin, cos) = rad.sin_cos();
    // dst → src: src = R(−deg)·(p − c_dst) + c_src (same size ⇒ same center).
    let homo = [
        [cos, -sin, cx - cos * cx + sin * cy],
        [sin, cos, cy - sin * cx - cos * cy],
        [0.0, 0.0, 1.0],
    ];
    warp(src, &homo, src.width(), src.height(), opts)
}

/// Perspective correction: `quad` (TL, TR, BR, BL in source pixel coords) is
/// mapped to fill the whole output rectangle.
pub fn perspective_correct(
    src: &PixelBuffer,
    quad: [[f32; 2]; 4],
    opts: WarpOptions,
) -> PixelBuffer {
    let (homo, w, h) = homography_for_quad(quad);
    warp(src, &homo, w, h, opts)
}
