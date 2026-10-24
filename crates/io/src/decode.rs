//! Image decoding with automatic EXIF orientation correction.

use photoforge_core::PixelBuffer;

use crate::exif;
use crate::orient::{apply_orientation, Orientation};
use crate::IoError;

/// A decoded image: display-oriented pixels plus its metadata.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    /// The image after EXIF orientation has been applied.
    pub buffer: PixelBuffer,
    /// The orientation that was applied (kept for sidecar round-trips).
    pub orientation: Orientation,
    /// EXIF summary (empty when the file has no usable EXIF).
    pub exif: exif::ExifSummary,
}

/// Decodes an image file (JPEG/PNG/WebP/TIFF/BMP by content sniffing).
pub fn decode_file(path: &std::path::Path) -> Result<DecodedImage, IoError> {
    let bytes = std::fs::read(path)?;
    let decoded = decode_bytes(&bytes)?;
    Ok(decoded)
}

/// Decodes an in-memory image container.
pub fn decode_bytes(bytes: &[u8]) -> Result<DecodedImage, IoError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| IoError::Io(std::io::Error::other(e)))?;
    let dynamic = reader.decode()?;
    let rgba = dynamic.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let raw = rgba.into_raw();
    let buffer = PixelBuffer::from_raw(w, h, raw)?;

    let (orientation, summary) = match exif::parse_exif(bytes) {
        Some(parsed) => (exif::orientation_of(&parsed), exif::summarize(&parsed)),
        None => (Orientation::Normal, exif::ExifSummary::default()),
    };
    let oriented = apply_orientation(&buffer, orientation);
    Ok(DecodedImage {
        buffer: oriented,
        orientation,
        exif: summary,
    })
}

/// Convenience: decode and return just the oriented pixels.
pub fn load_oriented(path: &std::path::Path) -> Result<PixelBuffer, IoError> {
    Ok(decode_file(path)?.buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_png_fixture_file() {
        let dir = std::env::temp_dir().join(format!("pfg_io_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grad.png");
        photoforge_fixtures::write_png(&path, &photoforge_fixtures::gradient_h(33, 17)).unwrap();
        let decoded = decode_file(&path).unwrap();
        assert_eq!((decoded.buffer.width(), decoded.buffer.height()), (33, 17));
        assert_eq!(decoded.orientation, Orientation::Normal);
        assert_eq!(decoded.buffer.pixel(0, 0)[0], 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn garbage_bytes_return_err_not_panic() {
        // Property-flavored loop (ADR-0004): random/truncated containers
        // must surface as errors.
        let mut rng = photoforge_core::rng::XorShift64::new(31337);
        for len in [0usize, 1, 2, 3, 7, 31, 100] {
            let bytes: Vec<u8> = (0..len).map(|_| rng.next_u8()).collect();
            assert!(decode_bytes(&bytes).is_err(), "len {len} decoded?!");
        }
        // Truncated valid headers.
        for probe in [
            &[0xFFu8, 0xD8, 0xFF][..],       // JPEG SOI
            &[0x89u8, b'P', b'N', b'G'][..], // PNG magic
            b"II*\x00\x08\x00\x00\x00",      // TIFF header
        ] {
            assert!(decode_bytes(probe).is_err());
        }
    }

    #[test]
    fn jpeg_with_exif_orientation_is_applied() {
        // Stored landscape with distinct corners; EXIF says rotate 90 CW.
        let mut stored = PixelBuffer::new(4, 2);
        stored.set_pixel(0, 0, [255, 0, 0, 255]); // TL red
        stored.set_pixel(3, 0, [0, 255, 0, 255]); // TR green
        stored.set_pixel(0, 1, [0, 0, 255, 255]); // BL blue
        stored.set_pixel(3, 1, [255, 255, 0, 255]); // BR yellow
        for o in 1..=8u16 {
            let jpeg = exif::jpeg_with_orientation(&stored, o);
            let decoded = decode_bytes(&jpeg).expect("decode failed");
            assert_eq!(decoded.orientation, Orientation::from_tag(o));
            // The displayed image's top-left must match the known mapping
            // (+/-12: baseline JPEG chroma subsampling bleeds on tiny images).
            let expect_tl: [u8; 3] = match o {
                1 => [255, 0, 0],
                2 => [0, 255, 0],
                3 => [255, 255, 0],
                4 => [0, 0, 255],
                5 => [255, 0, 0],
                6 => [0, 0, 255],
                7 => [255, 255, 0],
                _ => [0, 255, 0],
            };
            let tl = decoded.buffer.pixel(0, 0);
            for c in 0..3 {
                assert!(
                    (i16::from(tl[c]) - i16::from(expect_tl[c])).abs() <= 12,
                    "orientation {o}: TL {tl:?} vs {expect_tl:?}"
                );
            }
        }
    }

    #[test]
    fn jpeg_without_exif_defaults_to_normal() {
        let solid = photoforge_fixtures::solid(8, 8, [10, 200, 30, 255]);
        let jpeg = exif::jpeg_with_orientation(&solid, 1);
        let decoded = decode_bytes(&jpeg).unwrap();
        assert_eq!(decoded.orientation, Orientation::Normal);
        assert_eq!(decoded.buffer.width(), 8);
    }
}
