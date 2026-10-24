//! 16-bit linear-light image storage for pipeline stage caches.
//!
//! Per-stage caches quantize to 16-bit linear (65,536 levels) instead of
//! 8-bit sRGB: quantization error (~1.5e-5 linear) stays invisible even when
//! amplified by a dozen subsequent adjustments (tested: staged-vs-fused
//! drift ≤ 2 LSB on the 8-bit display grid), at 2× the memory of u8.

use crate::buffer::PixelBuffer;
use crate::color::srgb;

/// Linear-light RGBA stored as 16-bit integers (v ∈ [0, 65535]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinearImageQ16 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u16>,
}

impl LinearImageQ16 {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width as usize) * (height as usize) * 4],
        }
    }

    /// Quantizes a linear f32 image (clamped to [0, 1], NaN → 0).
    pub fn from_linear(src: &crate::LinearImage) -> Self {
        let mut out = Self::new(src.width, src.height);
        for (d, s) in out.data.chunks_exact_mut(4).zip(src.data.chunks_exact(4)) {
            for c in 0..4 {
                let v = s[c];
                d[c] = if v.is_finite() {
                    (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16
                } else {
                    0
                };
            }
        }
        out
    }

    /// Exact conversion back to f32 linear.
    pub fn to_linear(&self) -> crate::LinearImage {
        let mut out = crate::LinearImage::new(self.width, self.height);
        for (d, s) in out.data.chunks_exact_mut(4).zip(self.data.chunks_exact(4)) {
            for c in 0..4 {
                d[c] = s[c] as f32 / 65535.0;
            }
        }
        out
    }

    /// From 8-bit sRGB storage (exact per-channel decode).
    pub fn from_pixel_buffer(src: &PixelBuffer) -> Self {
        let mut out = Self::new(src.width(), src.height());
        for (d, s) in out
            .data
            .chunks_exact_mut(4)
            .zip(src.as_raw().chunks_exact(4))
        {
            d[0] = (srgb::decode_u8(s[0]) * 65535.0 + 0.5) as u16;
            d[1] = (srgb::decode_u8(s[1]) * 65535.0 + 0.5) as u16;
            d[2] = (srgb::decode_u8(s[2]) * 65535.0 + 0.5) as u16;
            d[3] = u16::from(s[3]) * 257;
        }
        out
    }

    /// To 8-bit sRGB storage (clamp + encode + round).
    pub fn to_pixel_buffer(&self) -> PixelBuffer {
        let mut out = vec![0u8; self.data.len()];
        for (d, s) in out.chunks_exact_mut(4).zip(self.data.chunks_exact(4)) {
            d[0] = srgb::encode_u8(s[0] as f32 / 65535.0);
            d[1] = srgb::encode_u8(s[1] as f32 / 65535.0);
            d[2] = srgb::encode_u8(s[2] as f32 / 65535.0);
            d[3] = (s[3] / 257) as u8;
        }
        PixelBuffer::from_raw(self.width, self.height, out)
            .expect("Q16 conversion produced a mismatched buffer")
    }

    /// Flat row slice (4 × u16 per pixel).
    #[inline]
    pub fn row(&self, y: u32) -> &[u16] {
        let stride = self.width as usize * 4;
        &self.data[y as usize * stride..(y as usize + 1) * stride]
    }

    pub fn size_bytes(&self) -> usize {
        self.data.len() * 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::XorShift64;
    use crate::LinearImage;

    #[test]
    fn q16_roundtrip_tight() {
        let mut rng = XorShift64::new(5);
        let lin = LinearImage::from_fn(17, 9, |_, _| {
            [rng.next_f32(), rng.next_f32(), rng.next_f32(), 1.0]
        });
        let q = LinearImageQ16::from_linear(&lin);
        let back = q.to_linear();
        for (a, b) in lin.data.chunks_exact(4).zip(back.data.chunks_exact(4)) {
            for c in 0..4 {
                assert!((a[c] - b[c]).abs() <= 1.6e-5, "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn q16_u8_grid_roundtrip_exact() {
        let mut rng = XorShift64::new(9);
        let mut pb = PixelBuffer::new(11, 5);
        for px in pb.as_raw_mut().chunks_exact_mut(4) {
            px[0] = rng.next_u8();
            px[1] = rng.next_u8();
            px[2] = rng.next_u8();
            px[3] = 255;
        }
        let q = LinearImageQ16::from_pixel_buffer(&pb);
        assert_eq!(q.to_pixel_buffer(), pb);
    }

    #[test]
    fn q16_sanitizes() {
        let lin = LinearImage::from_fn(2, 1, |x, _| match x {
            0 => [-1.0, 2.0, f32::NAN, 1.0],
            _ => [0.5, 0.5, 0.5, 1.0],
        });
        let q = LinearImageQ16::from_linear(&lin);
        assert_eq!(q.row(0)[0], 0);
        assert_eq!(q.row(0)[1], 65535);
        assert_eq!(q.row(0)[2], 0);
    }
}
