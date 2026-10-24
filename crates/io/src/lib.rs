//! photoforge-io
//!
//! Decoding/encoding (JPEG, PNG, WebP, TIFF, BMP via the `image` crate —
//! see ADR-0001), EXIF parsing with automatic orientation correction,
//! thumbnail generation and mip-level image pyramids used for preview
//! rendering.

pub mod decode;
pub mod encode;
pub mod exif;
pub mod orient;
pub mod pyramid;

pub use decode::{decode_bytes, decode_file, DecodedImage};
pub use exif::ExifSummary;
pub use orient::{apply_orientation, Orientation};
pub use pyramid::{thumbnail, ImagePyramid};

/// I/O layer errors. Decode failures are values, never panics (ADR-0004).
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image decode/encode error: {0}")]
    Image(#[from] image::ImageError),
    #[error("buffer shape mismatch: {0}")]
    Shape(#[from] photoforge_core::buffer::ShapeError),
    #[error("unsupported or corrupt image data")]
    Unsupported,
}
