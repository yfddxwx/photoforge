//! EXIF metadata parsing (via `kamadak-exif`, ADR-0001).
//!
//! Parsing failures degrade gracefully: metadata is optional information;
//! orientation defaults to `Normal` and the summary to empty strings.

use photoforge_core::PixelBuffer;

use crate::orient::Orientation;

/// Human-readable subset of EXIF metadata shown in the GUI.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExifSummary {
    pub camera_make: String,
    pub camera_model: String,
    pub lens_model: String,
    pub iso: String,
    pub exposure_time: String,
    pub f_number: String,
    pub focal_length: String,
    pub datetime_original: String,
}

/// Reads EXIF from an image container (JPEG/TIFF/PNG/WebP bytes).
pub fn parse_exif(bytes: &[u8]) -> Option<::exif::Exif> {
    let mut cursor = std::io::Cursor::new(bytes);
    ::exif::Reader::new().read_from_container(&mut cursor).ok()
}

/// Reads EXIF from a file.
pub fn parse_exif_file(path: &std::path::Path) -> Option<::exif::Exif> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    ::exif::Reader::new().read_from_container(&mut reader).ok()
}

/// The raw `Orientation` tag value (1..8), defaulting to 1.
pub fn orientation_of(exif: &::exif::Exif) -> Orientation {
    Orientation::from_tag(raw_tag_u16(exif, ::exif::Tag::Orientation).unwrap_or(1))
}

fn raw_tag_u16(exif: &::exif::Exif, tag: ::exif::Tag) -> Option<u16> {
    let field = exif.get_field(tag, ::exif::In::PRIMARY)?;
    let v = field.value.get_uint(0)?;
    u16::try_from(v).ok()
}

/// Extracts the display summary from parsed EXIF.
pub fn summarize(exif: &::exif::Exif) -> ExifSummary {
    use ::exif::Tag;
    let text = |tag: Tag| -> String {
        exif.get_field(tag, ::exif::In::PRIMARY)
            .map(|f| f.display_value().to_string())
            .unwrap_or_default()
            .trim_matches('"')
            .trim_end_matches('\0')
            .to_string()
    };
    ExifSummary {
        camera_make: text(Tag::Make),
        camera_model: text(Tag::Model),
        lens_model: text(Tag::LensModel),
        iso: text(Tag::PhotographicSensitivity),
        exposure_time: text(Tag::ExposureTime),
        f_number: text(Tag::FNumber),
        focal_length: text(Tag::FocalLength),
        datetime_original: text(Tag::DateTimeOriginal),
    }
}

/// Builds a minimal valid EXIF TIFF blob carrying only an Orientation tag —
/// used by tests to exercise real metadata paths without external files.
pub fn build_exif_tiff(orientation: u16) -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II\x2A\x00"); // little-endian TIFF magic
    tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at offset 8
    tiff.extend_from_slice(&1u16.to_le_bytes()); // one entry
    tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation tag
    tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    tiff.extend_from_slice(&1u32.to_le_bytes()); // count
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&0u16.to_le_bytes()); // value padding
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    tiff
}

/// Wraps an EXIF TIFF blob into an APP1 segment and splices it after the SOI
/// of a JPEG byte stream (test helper).
pub fn splice_exif_into_jpeg(jpeg: &[u8], exif_tiff: &[u8]) -> Vec<u8> {
    assert!(
        jpeg.len() >= 2 && jpeg[0] == 0xFF && jpeg[1] == 0xD8,
        "not a JPEG"
    );
    let mut out = Vec::with_capacity(jpeg.len() + exif_tiff.len() + 10);
    out.extend_from_slice(&jpeg[0..2]); // SOI
    out.extend_from_slice(&[0xFF, 0xE1]); // APP1 marker
    let seg_len: u16 = (2 + 6 + exif_tiff.len()) as u16; // includes its own 2 bytes
    out.extend_from_slice(&seg_len.to_be_bytes());
    out.extend_from_slice(b"Exif\x00\x00");
    out.extend_from_slice(exif_tiff);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// Convenience: re-encode `img` as a baseline JPEG with an EXIF orientation
/// segment (test helper).
pub fn jpeg_with_orientation(img: &PixelBuffer, orientation: u16) -> Vec<u8> {
    let mut rgb_bytes = Vec::with_capacity(img.width() as usize * img.height() as usize * 3);
    for px in img.as_raw().chunks_exact(4) {
        rgb_bytes.extend_from_slice(&px[0..3]);
    }
    let rgb = image::RgbImage::from_raw(img.width(), img.height(), rgb_bytes)
        .expect("buffer size mismatch");
    let mut jpeg = Vec::new();
    {
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95);
        image::ImageEncoder::write_image(
            encoder,
            rgb.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgb8,
        )
        .expect("jpeg encode failed");
    }
    splice_exif_into_jpeg(&jpeg, &build_exif_tiff(orientation))
}
