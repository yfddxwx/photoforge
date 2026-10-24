//! `LinearImage`: the f32 linear-light working space (ADR-0002).
//!
//! Storage is packed RGBA f32, row-major, nominal range [0, 1] (ops may
//! produce out-of-range intermediates; [`LinearImage::to_pixel_buffer`]
//! clamps on the way out). Alpha is carried through conversions and left
//! untouched by pointwise ops.

use crate::buffer::PixelBuffer;
use crate::color::srgb;
use crate::Rgba32F;

/// A linear-light f32 RGBA image.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
}

impl LinearImage {
    /// Allocates a zeroed image.
    pub fn new(width: u32, height: u32) -> Self {
        let len = LinearImage::checked_len(width, height);
        Self {
            width,
            height,
            data: vec![0.0; len],
        }
    }

    /// Allocates a solid-color image (alpha = 1).
    pub fn solid(width: u32, height: u32, color: [f32; 3]) -> Self {
        let mut img = Self::new(width, height);
        for px in img.data.chunks_exact_mut(4) {
            px[0] = color[0];
            px[1] = color[1];
            px[2] = color[2];
            px[3] = 1.0;
        }
        img
    }

    /// Builds from a per-pixel closure.
    pub fn from_fn(width: u32, height: u32, mut f: impl FnMut(u32, u32) -> Rgba32F) -> Self {
        let mut img = Self::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let px = f(x, y);
                let i = (y as usize * width as usize + x as usize) * 4;
                img.data[i..i + 4].copy_from_slice(&px);
            }
        }
        img
    }

    /// Converts from 8-bit sRGB storage (exact 256-entry decode table).
    pub fn from_pixel_buffer(src: &PixelBuffer) -> Self {
        let mut img = Self::new(src.width(), src.height());
        for (dst, px) in img
            .data
            .chunks_exact_mut(4)
            .zip(src.as_raw().chunks_exact(4))
        {
            dst[0] = srgb::decode_u8(px[0]);
            dst[1] = srgb::decode_u8(px[1]);
            dst[2] = srgb::decode_u8(px[2]);
            dst[3] = f32::from(px[3]) / 255.0;
        }
        img
    }

    /// Converts to 8-bit sRGB storage: clamp to [0, 1], encode, round. The
    /// round-trip from a `PixelBuffer` is bit-exact (tested).
    pub fn to_pixel_buffer(&self) -> PixelBuffer {
        let mut out = vec![0u8; self.data.len()];
        for (dst, px) in out.chunks_exact_mut(4).zip(self.data.chunks_exact(4)) {
            dst[0] = srgb::encode_u8(px[0]);
            dst[1] = srgb::encode_u8(px[1]);
            dst[2] = srgb::encode_u8(px[2]);
            dst[3] = (px[3] * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
        // Unwrap is safe: lengths match by construction.
        PixelBuffer::from_raw(self.width, self.height, out)
            .expect("linear→u8 conversion produced a mismatched buffer")
    }

    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> Rgba32F {
        debug_assert!(x < self.width && y < self.height);
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, px: Rgba32F) {
        debug_assert!(x < self.width && y < self.height);
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.data[i..i + 4].copy_from_slice(&px);
    }

    /// The row `y` as a flat f32 slice (4 floats per pixel).
    #[inline]
    pub fn row(&self, y: u32) -> &[f32] {
        let stride = self.width as usize * 4;
        &self.data[y as usize * stride..(y as usize + 1) * stride]
    }

    /// The row `y` as a mutable flat f32 slice.
    #[inline]
    pub fn row_mut(&mut self, y: u32) -> &mut [f32] {
        let stride = self.width as usize * 4;
        let y = y as usize;
        &mut self.data[y * stride..(y + 1) * stride]
    }

    /// Rows as an iterator of flat slices.
    pub fn rows(&self) -> impl Iterator<Item = &[f32]> {
        let stride = self.width as usize * 4;
        self.data.chunks_exact(stride)
    }

    /// Rows as a mutable iterator of flat slices.
    pub fn rows_mut(&mut self) -> impl Iterator<Item = &mut [f32]> {
        let stride = self.width as usize * 4;
        self.data.chunks_exact_mut(stride)
    }

    /// Rec. 709 luma of the pixel at (x, y).
    #[inline]
    pub fn luma_at(&self, x: u32, y: u32) -> f32 {
        srgb::luma(self.pixel(x, y))
    }

    pub fn size_bytes(&self) -> usize {
        self.data.len() * std::mem::size_of::<f32>()
    }

    fn checked_len(width: u32, height: u32) -> usize {
        (width as usize)
            .checked_mul(height as usize)
            .and_then(|p| p.checked_mul(4))
            .expect("linear image dimensions overflow usize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::XorShift64;

    #[test]
    fn u8_roundtrip_bit_exact() {
        let mut rng = XorShift64::new(99);
        let mut pb = PixelBuffer::new(37, 23); // prime-ish dims
        for px in pb.as_raw_mut().chunks_exact_mut(4) {
            px[0] = rng.next_u8();
            px[1] = rng.next_u8();
            px[2] = rng.next_u8();
            px[3] = 255;
        }
        let lin = LinearImage::from_pixel_buffer(&pb);
        let back = lin.to_pixel_buffer();
        assert_eq!(pb, back);
    }

    #[test]
    fn solid_and_from_fn() {
        let img = LinearImage::solid(4, 4, [0.5, 0.25, 0.125]);
        assert_eq!(img.pixel(3, 3), [0.5, 0.25, 0.125, 1.0]);
        let g = LinearImage::from_fn(4, 2, |x, _y| [x as f32 / 3.0, 0.0, 0.0, 1.0]);
        assert_eq!(g.pixel(3, 1)[0], 1.0);
    }

    #[test]
    fn row_accessors() {
        let mut img = LinearImage::new(4, 3);
        img.set_pixel(2, 1, [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(&img.row(1)[8..12], &[1.0, 0.5, 0.25, 1.0]);
        let rows: Vec<&[f32]> = img.rows().collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].len(), 16);
        for r in img.rows_mut() {
            r[4] = 2.0; // pixel (1, y) red channel
        }
        assert_eq!(img.pixel(1, 2)[0], 2.0);
    }

    #[test]
    fn out_of_range_clamps_on_encode() {
        let img = LinearImage::from_fn(2, 1, |x, _| match x {
            0 => [-0.5, 0.7, 1.5, 1.0],
            _ => [f32::INFINITY, f32::NEG_INFINITY, f32::NAN, 1.0],
        });
        let pb = img.to_pixel_buffer();
        assert_eq!(pb.pixel(0, 0), [0, 218, 255, 255]); // encode(0.7) ≈ 218
        assert_eq!(pb.pixel(1, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn zero_dim_and_large_dims_ok() {
        let img = LinearImage::new(0, 0);
        assert_eq!(img.to_pixel_buffer().len_bytes(), 0);
        let img = LinearImage::new(1, 1);
        assert_eq!(img.data.len(), 4);
    }
}
