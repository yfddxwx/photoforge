//! `PixelBuffer`: the canonical 8-bit storage format.
//!
//! Packed RGBA8 (4 bytes per pixel, row-major, no padding), sRGB-encoded.
//! This is the interchange format with codecs, the GUI texture uploader and
//! the export path. All computation happens in [`crate`]'s linear-light f32
//! space (see ADR-0002); this buffer is storage, not a compute space.

/// An 8-bit RGBA pixel buffer with packed row-major layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelBuffer {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl PixelBuffer {
    /// Allocates a zeroed (transparent black) buffer.
    pub fn new(width: u32, height: u32) -> Self {
        let len = Self::checked_len(width, height);
        Self {
            width,
            height,
            data: vec![0; len],
        }
    }

    /// Adopts an existing RGBA8 vector. Errors if the length does not match
    /// `width * height * 4`.
    pub fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<Self, ShapeError> {
        let expected = Self::checked_len(width, height);
        if data.len() != expected {
            return Err(ShapeError {
                width,
                height,
                expected,
                len: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Allocates a buffer filled with one solid color.
    pub fn solid(width: u32, height: u32, color: crate::Rgba8) -> Self {
        let mut buf = Self::new(width, height);
        buf.fill_solid(color);
        buf
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Number of bytes: `width * height * 4`.
    pub fn len_bytes(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn as_raw(&self) -> &[u8] {
        &self.data
    }

    pub fn as_raw_mut(&mut self) -> &mut Vec<u8> {
        &mut self.data
    }

    /// The row `y` as 4-byte pixel groups.
    pub fn row(&self, y: u32) -> &[u8] {
        let y = y as usize;
        let stride = self.width as usize * 4;
        &self.data[y * stride..(y + 1) * stride]
    }

    /// The row `y` as a mutable slice of 4-byte pixel groups.
    pub fn row_mut(&mut self, y: u32) -> &mut [u8] {
        let y = y as usize;
        let stride = self.width as usize * 4;
        &mut self.data[y * stride..(y + 1) * stride]
    }

    /// Reads one pixel. Panics if out of bounds (programmer error; the
    /// processing pipeline never indexes dynamically user-controlled
    /// coordinates).
    pub fn pixel(&self, x: u32, y: u32) -> crate::Rgba8 {
        assert!(
            x < self.width && y < self.height,
            "pixel ({x},{y}) out of bounds {}x{}",
            self.width,
            self.height
        );
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    /// Writes one pixel. Panics if out of bounds.
    pub fn set_pixel(&mut self, x: u32, y: u32, px: crate::Rgba8) {
        assert!(
            x < self.width && y < self.height,
            "pixel ({x},{y}) out of bounds {}x{}",
            self.width,
            self.height
        );
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.data[i..i + 4].copy_from_slice(&px);
    }

    /// Fills every pixel with one color.
    pub fn fill_solid(&mut self, color: crate::Rgba8) {
        for px in self.data.chunks_exact_mut(4) {
            px.copy_from_slice(&color);
        }
    }

    /// Copies the sub-rectangle `(x, y, w, h)`, clamped to the buffer bounds.
    /// Returns an empty buffer if the intersection is empty.
    pub fn region(&self, x: u32, y: u32, w: u32, h: u32) -> PixelBuffer {
        let x0 = x.min(self.width);
        let y0 = y.min(self.height);
        let x1 = x.saturating_add(w).min(self.width);
        let y1 = y.saturating_add(h).min(self.height);
        let out_w = x1.saturating_sub(x0);
        let out_h = y1.saturating_sub(y0);
        let mut out = PixelBuffer::new(out_w, out_h);
        if out_w == 0 || out_h == 0 {
            return out;
        }
        let src_stride = self.width as usize * 4;
        let dst_stride = out_w as usize * 4;
        for row in 0..out_h as usize {
            let s = (y0 as usize + row) * src_stride + x0 as usize * 4;
            let d = row * dst_stride;
            out.data[d..d + dst_stride].copy_from_slice(&self.data[s..s + dst_stride]);
        }
        out
    }

    /// Pastes `other` at `(x, y)`, clipped to bounds.
    pub fn paste(&mut self, other: &PixelBuffer, x: i64, y: i64) {
        for oy in 0..other.height as i64 {
            let dy = y + oy;
            if dy < 0 || dy >= self.height as i64 {
                continue;
            }
            for ox in 0..other.width as i64 {
                let dx = x + ox;
                if dx < 0 || dx >= self.width as i64 {
                    continue;
                }
                let src = (oy as usize * other.width as usize + ox as usize) * 4;
                let dst = (dy as usize * self.width as usize + dx as usize) * 4;
                self.data[dst..dst + 4].copy_from_slice(&other.data[src..src + 4]);
            }
        }
    }

    fn checked_len(width: u32, height: u32) -> usize {
        (width as usize)
            .checked_mul(height as usize)
            .and_then(|p| p.checked_mul(4))
            .expect("buffer dimensions overflow usize")
    }
}

/// Length mismatch when adopting a raw byte vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("shape mismatch: {width}x{height} RGBA needs {expected} bytes, got {len}")]
pub struct ShapeError {
    pub width: u32,
    pub height: u32,
    pub expected: usize,
    pub len: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_zeroed_and_solid_fills() {
        let mut b = PixelBuffer::new(3, 2);
        assert_eq!(b.len_bytes(), 24);
        assert_eq!(b.pixel(0, 0), [0, 0, 0, 0]);
        b.fill_solid([1, 2, 3, 255]);
        assert_eq!(b.pixel(2, 1), [1, 2, 3, 255]);
    }

    #[test]
    fn from_raw_validates_length() {
        assert!(PixelBuffer::from_raw(2, 2, vec![0; 16]).is_ok());
        assert!(PixelBuffer::from_raw(2, 2, vec![0; 15]).is_err());
    }

    #[test]
    fn roundtrip_pixel_set_get() {
        let mut b = PixelBuffer::new(4, 4);
        b.set_pixel(3, 2, [9, 8, 7, 6]);
        assert_eq!(b.pixel(3, 2), [9, 8, 7, 6]);
        assert_eq!(b.pixel(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn region_clamps_and_copies() {
        let mut b = PixelBuffer::solid(4, 4, [10, 0, 0, 255]);
        b.set_pixel(1, 1, [99, 0, 0, 255]);
        let r = b.region(1, 1, 2, 2);
        assert_eq!((r.width(), r.height()), (2, 2));
        assert_eq!(r.pixel(0, 0), [99, 0, 0, 255]);
        // Fully out of bounds -> empty.
        let e = b.region(10, 10, 2, 2);
        assert_eq!((e.width(), e.height()), (0, 0));
        // Overhanging rectangle is clamped.
        let c = b.region(3, 3, 5, 5);
        assert_eq!((c.width(), c.height()), (1, 1));
    }

    #[test]
    fn paste_clips_to_bounds() {
        let mut dst = PixelBuffer::solid(4, 4, [0, 0, 0, 255]);
        let src = PixelBuffer::solid(2, 2, [7, 7, 7, 255]);
        dst.paste(&src, 3, 3);
        assert_eq!(dst.pixel(3, 3), [7, 7, 7, 255]);
        assert_eq!(dst.pixel(0, 0), [0, 0, 0, 255]);
        // Fully outside: no-op, no panic.
        dst.paste(&src, 100, 100);
        dst.paste(&src, -100, -100);
        assert_eq!(dst.pixel(0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn row_slices_match_layout() {
        let mut b = PixelBuffer::new(3, 2);
        b.set_pixel(2, 1, [5, 5, 5, 5]);
        assert_eq!(&b.row(1)[8..12], &[5, 5, 5, 5]);
        b.row_mut(0)[0..4].copy_from_slice(&[1, 1, 1, 1]);
        assert_eq!(b.pixel(0, 0), [1, 1, 1, 1]);
    }
}
