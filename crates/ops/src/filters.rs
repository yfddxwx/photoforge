//! Spatial filters: separable gaussian blur, O(N) box blur, unsharp masking
//! and clarity. All operate in place on the linear-light working space.

use photoforge_core::color::srgb::luma;
use photoforge_core::math::{clamp01, smoothstep};
use photoforge_core::tile::par_bands;
use photoforge_core::LinearImage;

/// Separable true gaussian blur. `radius` is in pixels; σ = radius / 2, and
/// the kernel support is ±⌈3σ⌉ (clamped edges).
pub fn gaussian_blur(img: &mut LinearImage, radius: f32) {
    if radius <= 0.05 || img.width == 0 || img.height == 0 {
        return;
    }
    let sigma = (radius / 2.0).max(0.05);
    let r = (sigma * 3.0).ceil() as i32;
    let mut kernel = Vec::with_capacity((2 * r + 1) as usize);
    let denom = 2.0 * sigma * sigma;
    for i in -r..=r {
        kernel.push((-(i * i) as f32 / denom).exp());
    }
    let sum: f32 = kernel.iter().sum();
    for k in &mut kernel {
        *k /= sum;
    }
    let (w, h) = (img.width, img.height);
    let stride = w as usize * 4;
    let src = img.data.clone();

    // Horizontal pass: rows are independent.
    let mut temp = vec![0.0f32; src.len()];
    par_bands(&mut temp, stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let srow = &src[y as usize * stride..(y as usize + 1) * stride];
            let off = (y - y0) as usize * stride;
            convolve_row(srow, &kernel, r, &mut band[off..off + stride]);
        }
    });

    // Vertical pass: out rows read temp rows ±r (immutable temp).
    par_bands(&mut img.data, stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            for px in orow.chunks_exact_mut(4) {
                px[0] = 0.0;
                px[1] = 0.0;
                px[2] = 0.0;
                px[3] = 0.0;
            }
            for (k, &weight) in kernel.iter().enumerate() {
                let sy = (y as i64 + k as i64 - r as i64).clamp(0, h as i64 - 1) as u32;
                let srow = &temp[sy as usize * stride..(sy as usize + 1) * stride];
                for (o, s) in orow.chunks_exact_mut(4).zip(srow.chunks_exact(4)) {
                    o[0] += weight * s[0];
                    o[1] += weight * s[1];
                    o[2] += weight * s[2];
                    o[3] += weight * s[3];
                }
            }
        }
    });
}

#[inline]
fn convolve_row(row: &[f32], kernel: &[f32], r: i32, out: &mut [f32]) {
    let w = row.len() / 4;
    for (x, o) in out.chunks_exact_mut(4).enumerate() {
        let mut acc = [0.0f32; 4];
        for (k, &weight) in kernel.iter().enumerate() {
            let sx = (x as i64 + k as i64 - r as i64).clamp(0, w as i64 - 1) as usize;
            let px = &row[sx * 4..sx * 4 + 4];
            acc[0] += weight * px[0];
            acc[1] += weight * px[1];
            acc[2] += weight * px[2];
            acc[3] += weight * px[3];
        }
        o.copy_from_slice(&acc);
    }
}

/// In-place separable box blur via prefix sums: O(N) regardless of radius
/// (used by clarity's large-radius local contrast).
pub fn box_blur(img: &mut LinearImage, radius: u32) {
    if radius == 0 || img.width == 0 || img.height == 0 {
        return;
    }
    let (w, h) = (img.width, img.height);
    let stride = w as usize * 4;
    let src = img.data.clone();
    let mut temp = vec![0.0f32; src.len()];

    // Horizontal moving average.
    par_bands(&mut temp, stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let srow = &src[y as usize * stride..(y as usize + 1) * stride];
            let off = (y - y0) as usize * stride;
            box_axis(srow, radius, &mut band[off..off + stride]);
        }
    });
    // Vertical moving average: run the same 1-D routine over each column by
    // transposing through rows of column-major temp.
    let mut colmajor = vec![0.0f32; src.len()];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                colmajor[(x * h + y) as usize * 4 + c] = temp[(y * w + x) as usize * 4 + c];
            }
        }
    }
    for x in 0..w as usize {
        let col = &colmajor[x * h as usize * 4..(x + 1) * h as usize * 4];
        let mut out = vec![0.0f32; h as usize * 4];
        box_axis(col, radius, &mut out);
        for y in 0..h {
            for c in 0..4 {
                img.data[(y * w + x as u32) as usize * 4 + c] = out[y as usize * 4 + c];
            }
        }
    }
}

/// 1-D clamped box average over interleaved RGBA (prefix sums, O(N)).
fn box_axis(row: &[f32], radius: u32, out: &mut [f32]) {
    let n = row.len() / 4;
    let r = radius as i64;
    // Prefix sums per channel: prefix[i] = sum of channels of px[0..i].
    let mut prefix = vec![0.0f32; (n + 1) * 4];
    for i in 0..n {
        for c in 0..4 {
            prefix[(i + 1) * 4 + c] = prefix[i * 4 + c] + row[i * 4 + c];
        }
    }
    for x in 0..n {
        let lo = (x as i64 - r).clamp(0, n as i64 - 1) as usize;
        let hi = (x as i64 + r).clamp(0, n as i64 - 1) as usize;
        let count = (hi - lo + 1) as f32;
        for c in 0..4 {
            out[x * 4 + c] = (prefix[(hi + 1) * 4 + c] - prefix[lo * 4 + c]) / count;
        }
    }
}

/// Unsharp mask: `out = in + amount · (in − gauss(in, σ))`, gated by a
/// luma-difference threshold so flat noise isn't sharpened.
pub fn unsharp_mask(img: &mut LinearImage, amount: f32, radius: f32, threshold: f32) {
    if amount <= 0.0 {
        return;
    }
    let blurred_src = {
        let mut blur = img.clone();
        gaussian_blur(&mut blur, radius.max(0.3));
        blur
    };
    let t = threshold.clamp(0.0, 0.2);
    let stride = img.width as usize * 4;
    let src = img.data.clone();
    let blur = blurred_src.data;
    par_bands(&mut img.data, stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            let srow = &src[y as usize * stride..(y as usize + 1) * stride];
            let brow = &blur[y as usize * stride..(y as usize + 1) * stride];
            for x in 0..(orow.len() / 4) {
                let s = &srow[x * 4..x * 4 + 4];
                let b = &brow[x * 4..x * 4 + 4];
                let d_luma = luma([s[0], s[1], s[2], 1.0]) - luma([b[0], b[1], b[2], 1.0]);
                let gate = smoothstep(t, t * 2.0 + 1e-4, d_luma.abs());
                for c in 0..3 {
                    let o = s[c] + amount * (s[c] - b[c]) * gate;
                    orow[x * 4 + c] = clamp01(o);
                }
            }
        }
    });
}

/// Clarity: local contrast on luma only, using a large-radius box blur; the
/// luma delta is applied proportionally per channel to preserve hue.
pub fn clarity(img: &mut LinearImage, amount: f32) {
    if amount == 0.0 || img.width == 0 || img.height == 0 {
        return;
    }
    let radius = ((img.width.max(img.height) as f32) * 0.02).clamp(3.0, 60.0) as u32;
    // Luma plane + its blur.
    let (w, h) = (img.width, img.height);
    let mut luma_plane = vec![0.0f32; (w * h) as usize];
    for (i, px) in img.data.chunks_exact(4).enumerate() {
        luma_plane[i] = luma([px[0], px[1], px[2], 1.0]);
    }
    let mut blurred = luma_plane.clone();
    // Reuse the box filter through a temporary LinearImage view.
    let mut blur_img = LinearImage {
        width: w,
        height: h,
        data: blurred,
    };
    box_blur_luma(&mut blur_img, radius);
    blurred = blur_img.data;

    let stride = w as usize * 4;
    let src = img.data.clone();
    par_bands(&mut img.data, stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            let srow = &src[y as usize * stride..(y as usize + 1) * stride];
            for x in 0..w as usize {
                let i = y as usize * w as usize + x;
                let l = luma_plane[i];
                let lb = blurred[i];
                let delta = l - lb;
                let scale = if l.abs() > 1e-3 {
                    1.0 + amount * delta / l
                } else {
                    1.0
                };
                for c in 0..3 {
                    orow[x * 4 + c] = clamp01(srow[x * 4 + c] * scale);
                }
            }
        }
    });
}

/// Box blur over a single-channel plane (row-major, stride 1), prefix-sum
/// O(N) per axis.
fn box_blur_luma(img: &mut LinearImage, radius: u32) {
    let (w, h) = (img.width as usize, img.height as usize);
    let r = radius as i64;
    // Horizontal via prefix sums.
    let mut temp = vec![0.0f32; img.data.len()];
    for y in 0..h {
        let row = &img.data[y * w..(y + 1) * w];
        let mut prefix = vec![0.0f32; w + 1];
        for i in 0..w {
            prefix[i + 1] = prefix[i] + row[i];
        }
        for x in 0..w {
            let lo = (x as i64 - r).clamp(0, w as i64 - 1) as usize;
            let hi = (x as i64 + r).clamp(0, w as i64 - 1) as usize;
            temp[y * w + x] = (prefix[hi + 1] - prefix[lo]) / (hi - lo + 1) as f32;
        }
    }
    // Vertical via prefix sums.
    let mut prefix_col = vec![0.0f32; h + 1];
    for x in 0..w {
        for y in 0..h {
            prefix_col[y + 1] = prefix_col[y] + temp[y * w + x];
        }
        for y in 0..h {
            let lo = (y as i64 - r).clamp(0, h as i64 - 1) as usize;
            let hi = (y as i64 + r).clamp(0, h as i64 - 1) as usize;
            img.data[y * w + x] = (prefix_col[hi + 1] - prefix_col[lo]) / (hi - lo + 1) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(w: u32, h: u32, v: f32) -> LinearImage {
        LinearImage::solid(w, h, [v, v, v])
    }

    fn is_constant(img: &LinearImage, v: f32, eps: f32) -> bool {
        img.data
            .chunks_exact(4)
            .all(|px| (px[0] - v).abs() < eps && (px[1] - v).abs() < eps && (px[2] - v).abs() < eps)
    }

    #[test]
    fn gaussian_constant_stays_constant() {
        let mut img = constant(33, 17, 0.4);
        gaussian_blur(&mut img, 5.0);
        assert!(is_constant(&img, 0.4, 1e-5));
    }

    #[test]
    fn gaussian_impulse_spreads_and_preserves_energy() {
        let mut img = LinearImage::new(31, 31);
        img.set_pixel(15, 15, [1.0, 1.0, 1.0, 1.0]);
        gaussian_blur(&mut img, 3.0);
        let total: f32 = img.data.chunks_exact(4).map(|px| px[0]).sum();
        assert!((total - 1.0).abs() < 0.02, "energy {total}");
        // Center remains the max.
        let center = img.pixel(15, 15)[0];
        assert!(img.data.chunks_exact(4).all(|px| px[0] <= center + 1e-6));
        // Symmetric.
        assert!((img.pixel(13, 15)[0] - img.pixel(17, 15)[0]).abs() < 1e-6);
    }

    #[test]
    fn box_blur_constant_and_energy() {
        let mut img = constant(21, 21, 0.7);
        box_blur(&mut img, 4);
        assert!(is_constant(&img, 0.7, 1e-5));
    }

    #[test]
    fn unsharp_constant_is_exact_noop() {
        let mut img = constant(19, 13, 0.5);
        let before = img.clone();
        unsharp_mask(&mut img, 1.5, 2.0, 0.01);
        // Blur of constant is constant to float precision; delta = 0 → gate
        // at 0 → unchanged up to fp rounding.
        for (a, b) in img.data.iter().zip(before.data.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn unsharp_boosts_edges() {
        let mut img = LinearImage::from_fn(64, 8, |x, _| {
            let v = if x < 32 { 0.2 } else { 0.8 };
            [v, v, v, 1.0]
        });
        let before_edge = img.pixel(31, 4)[0];
        unsharp_mask(&mut img, 1.0, 2.0, 0.005);
        let after_edge = img.pixel(31, 4)[0];
        // The dark side of the edge gets darker (contrast up).
        assert!(after_edge < before_edge, "{before_edge} -> {after_edge}");
        // Far from the edge nothing changes.
        assert!((img.pixel(5, 4)[0] - 0.2).abs() < 1e-4);
    }

    #[test]
    fn clarity_constant_is_stable_and_edges_gain_contrast() {
        let mut img = constant(40, 40, 0.5);
        clarity(&mut img, 0.8);
        assert!(is_constant(&img, 0.5, 1e-4));

        let mut step = LinearImage::from_fn(64, 32, |x, _| {
            let v = if x < 32 { 0.3 } else { 0.7 };
            [v, v, v, 1.0]
        });
        clarity(&mut step, 0.6);
        let dark = step.pixel(4, 16)[0];
        assert!(dark < 0.3, "clarity darkened flat dark: {dark}");
    }

    #[test]
    fn degenerate_sizes_no_panic() {
        let mut one = constant(1, 1, 0.3);
        gaussian_blur(&mut one, 50.0);
        box_blur(&mut one, 30);
        unsharp_mask(&mut one, 1.0, 2.0, 0.0);
        clarity(&mut one, 0.5);
        let mut empty = LinearImage::new(0, 0);
        gaussian_blur(&mut empty, 3.0);
    }
}
