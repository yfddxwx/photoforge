//! Color space conversions.
//!
//! - [`srgb`]: the sRGB transfer functions and the u8 ↔ linear-light bridges
//!   (the storage boundary, ADR-0002).
//! - [`hsv`]: classic hue/sat/value on encoded RGB.
//! - [`oklab`]: OKLab (Ottosson 2020), the perceptual space for chroma work.

pub mod hsv;
pub mod oklab;
pub mod srgb;
