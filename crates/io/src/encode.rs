//! Encoding to JPEG/PNG files and byte buffers.

use photoforge_core::PixelBuffer;
use std::io::Write;

use crate::IoError;

/// Encodes RGBA8 to JPEG (alpha is dropped; quality 1–100).
pub fn encode_jpeg_bytes(buf: &PixelBuffer, quality: u8) -> Result<Vec<u8>, IoError> {
    let mut rgb = Vec::with_capacity(buf.width() as usize * buf.height() as usize * 3);
    for px in buf.as_raw().chunks_exact(4) {
        rgb.extend_from_slice(&px[0..3]);
    }
    let mut out = Vec::new();
    let encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    image::ImageEncoder::write_image(
        encoder,
        &rgb,
        buf.width(),
        buf.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(out)
}

/// Encodes RGBA8 to PNG bytes.
pub fn encode_png_bytes(buf: &PixelBuffer) -> Result<Vec<u8>, IoError> {
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    image::ImageEncoder::write_image(
        encoder,
        buf.as_raw(),
        buf.width(),
        buf.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

/// Encodes and writes to a file, creating parent directories. Format is
/// chosen by extension (`.jpg`/`.jpeg` → JPEG, anything else → PNG).
pub fn encode_to_file(
    buf: &PixelBuffer,
    path: &std::path::Path,
    quality: u8,
) -> Result<(), IoError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let bytes = match ext.as_str() {
        "jpg" | "jpeg" => encode_jpeg_bytes(buf, quality)?,
        _ => encode_png_bytes(buf)?,
    };
    let mut file = std::fs::File::create(path)?;
    file.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_roundtrip_dimensions_and_quality() {
        let img = photoforge_fixtures::radial(40, 30);
        let q90 = encode_jpeg_bytes(&img, 90).unwrap();
        let q30 = encode_jpeg_bytes(&img, 30).unwrap();
        assert!(q90.len() > q30.len(), "higher quality must be bigger");
        let decoded = crate::decode_bytes(&q90).unwrap();
        assert_eq!((decoded.buffer.width(), decoded.buffer.height()), (40, 30));
    }

    #[test]
    fn png_roundtrip_is_lossless() {
        let img = photoforge_fixtures::noise(23, 11, 77);
        let png = encode_png_bytes(&img).unwrap();
        let decoded = crate::decode_bytes(&png).unwrap();
        assert_eq!(decoded.buffer, img);
    }

    #[test]
    fn encode_to_file_by_extension() {
        let dir = std::env::temp_dir().join(format!("pfg_enc_test_{}", std::process::id()));
        let img = photoforge_fixtures::gradient_v(12, 34);
        encode_to_file(&img, &dir.join("a.jpg"), 85).unwrap();
        encode_to_file(&img, &dir.join("b.png"), 85).unwrap();
        assert!(dir.join("a.jpg").exists());
        assert_eq!(crate::decode_file(&dir.join("b.png")).unwrap().buffer, img);
        // Nested directory creation.
        encode_to_file(&img, &dir.join("deep/nest/c.png"), 85).unwrap();
        assert!(dir.join("deep/nest/c.png").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
