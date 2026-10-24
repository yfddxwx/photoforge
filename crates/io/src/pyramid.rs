//! Mip-level image pyramids and thumbnails.
//!
//! The pyramid halves the image repeatedly with a 2×2 box filter until the
//! longest edge is at most [`MAX_LEVEL_EDGE`]. Levels are shared as
//! `Arc<PixelBuffer>` between threads (the GUI worker renders from them).
//! High-quality resampling (Lanczos3 in `photoforge-ops`) is used only for
//! the final thumbnail step; box halving is fast and adequate for mip levels.

use std::sync::Arc;

use photoforge_core::PixelBuffer;

/// Build levels until the longest edge drops to this size or below.
pub const MAX_LEVEL_EDGE: u32 = 128;

/// A full mip chain; `levels[0]` is the untouched original.
#[derive(Clone, Debug)]
pub struct ImagePyramid {
    levels: Vec<Arc<PixelBuffer>>,
}

impl ImagePyramid {
    /// Builds the pyramid from a base image.
    pub fn build(base: Arc<PixelBuffer>) -> Self {
        let mut levels = vec![base];
        loop {
            let last = levels.last().expect("always has the base level");
            let (w, h) = (last.width(), last.height());
            if w.max(h) <= MAX_LEVEL_EDGE || w == 0 || h == 0 {
                break;
            }
            levels.push(Arc::new(halve_box(last)));
        }
        Self { levels }
    }

    pub fn level(&self, index: usize) -> Arc<PixelBuffer> {
        self.levels[index.min(self.levels.len() - 1)].clone()
    }

    pub fn num_levels(&self) -> usize {
        self.levels.len()
    }

    /// Scale factor of `level` relative to the base (e.g. 0.5 for level 1).
    pub fn scale_at(&self, level: usize) -> f32 {
        let base = &self.levels[0];
        let l = self.level(level);
        if base.width() == 0 {
            return 1.0;
        }
        l.width() as f32 / base.width() as f32
    }

    /// The deepest level whose width and height are both at least `target`
    /// pixels — i.e. the smallest level that still covers the requested
    /// viewport without upscaling.
    pub fn level_for_target(&self, target: u32) -> usize {
        for (i, l) in self.levels.iter().enumerate().rev() {
            if l.width() >= target && l.height() >= target {
                return i;
            }
        }
        0
    }
}

/// Halves the image with a 2×2 box filter (rounded); odd edges replicate the
/// last row/column pair. Returns at least a 1×1 image.
pub fn halve_box(src: &PixelBuffer) -> PixelBuffer {
    let (sw, sh) = (src.width(), src.height());
    if sw == 0 || sh == 0 {
        return PixelBuffer::new(sw, sh);
    }
    let dw = sw.div_ceil(2).max(1);
    let dh = sh.div_ceil(2).max(1);
    let mut out = PixelBuffer::new(dw, dh);
    let stride = dw as usize * 4;
    photoforge_core::tile::par_bands(out.as_raw_mut(), stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let sy0 = (y * 2).min(sh - 1);
            let sy1 = (y * 2 + 1).min(sh - 1);
            let top = src.row(sy0);
            let bot = src.row(sy1);
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            for x in 0..dw {
                let sx0 = (x * 2).min(sw - 1) as usize * 4;
                let sx1 = (x * 2 + 1).min(sw - 1) as usize * 4;
                let ox = x as usize * 4;
                for c in 0..4 {
                    let sum = top[sx0 + c] as u32
                        + top[sx1 + c] as u32
                        + bot[sx0 + c] as u32
                        + bot[sx1 + c] as u32;
                    orow[ox + c] = ((sum + 2) / 4) as u8;
                }
            }
        }
    });
    out
}

/// Produces a thumbnail whose longest edge is `max_edge` (upscales nothing).
pub fn thumbnail(src: &PixelBuffer, max_edge: u32) -> PixelBuffer {
    let (w, h) = (src.width(), src.height());
    let longest = w.max(h);
    if longest == 0 || longest <= max_edge {
        return src.clone();
    }
    // Quick pre-halving to keep the final Lanczos cheap on huge inputs.
    let mut cur = src.clone();
    while cur.width().max(cur.height()) > max_edge * 2 {
        cur = halve_box(&cur);
    }
    let scale = max_edge as f32 / cur.width().max(cur.height()) as f32;
    let dw = ((cur.width() as f32 * scale).round() as u32).clamp(1, max_edge);
    let dh = ((cur.height() as f32 * scale).round() as u32).clamp(1, max_edge);
    photoforge_ops::geometry::resize(&cur, dw, dh, photoforge_ops::geometry::Resample::Lanczos3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halve_box_averages_and_handles_odd() {
        let mut img = PixelBuffer::new(4, 4);
        img.fill_solid([100, 0, 0, 255]);
        assert_eq!(halve_box(&img).pixel(1, 1), [100, 0, 0, 255]);
        // 3×3 → 2×2 (odd edges replicate).
        let mut odd = PixelBuffer::new(3, 3);
        odd.fill_solid([0, 0, 0, 255]);
        odd.set_pixel(0, 0, [255, 0, 0, 255]);
        let half = halve_box(&odd);
        assert_eq!((half.width(), half.height()), (2, 2));
        // (255 + 0 + 0 + 0 + 2) / 4 = 64
        assert_eq!(half.pixel(0, 0)[0], 64);
        let tiny = PixelBuffer::solid(1, 1, [7, 7, 7, 255]);
        assert_eq!(halve_box(&tiny).pixel(0, 0), [7, 7, 7, 255]);
        assert_eq!(halve_box(&PixelBuffer::new(0, 0)).len_bytes(), 0);
    }

    #[test]
    fn pyramid_level_count_and_scales() {
        let base = Arc::new(PixelBuffer::solid(1000, 600, [5, 5, 5, 255]));
        let pyr = ImagePyramid::build(base);
        assert_eq!(pyr.num_levels(), 4); // 1000→500→250→125 (125 ≤ 128)
        assert_eq!(pyr.level(0).width(), 1000);
        assert_eq!(pyr.level(3).width(), 125);
        assert!((pyr.scale_at(1) - 0.5).abs() < 1e-6);
        assert!((pyr.scale_at(2) - 0.25).abs() < 1e-6);
        // Deepest level covering ≥ 300 px wide: 500.
        assert_eq!(pyr.level_for_target(300), 1);
        assert_eq!(pyr.level_for_target(1200), 0);
        // Small images: single level.
        let tiny = ImagePyramid::build(Arc::new(PixelBuffer::solid(64, 64, [0; 4])));
        assert_eq!(tiny.num_levels(), 1);
        assert_eq!(tiny.level_for_target(128), 0);
    }

    #[test]
    fn thumbnail_bounds() {
        let img = PixelBuffer::solid(2000, 1000, [9, 9, 9, 255]);
        let t = thumbnail(&img, 256);
        assert_eq!(t.width(), 256);
        assert_eq!(t.height(), 128);
        assert_eq!(t.pixel(10, 10), [9, 9, 9, 255]);
        // Never upscales.
        let small = PixelBuffer::solid(64, 32, [1, 2, 3, 255]);
        assert_eq!(thumbnail(&small, 256), small);
        let empty = PixelBuffer::new(0, 0);
        assert_eq!(thumbnail(&empty, 64).len_bytes(), 0);
    }
}
