//! Core foundation types for photoforge.

pub mod buffer;
pub mod color;
pub mod histogram;
pub mod linear;
pub mod math;
pub mod q16;
pub mod rng;
pub mod tile;

pub use buffer::PixelBuffer;
pub use linear::LinearImage;
pub use q16::LinearImageQ16;

/// An 8-bit sRGB-encoded RGBA pixel (straight alpha; the pipeline treats
/// images as opaque).
pub type Rgba8 = [u8; 4];

/// An f32 RGBA pixel in linear light (the working space; see ADR-0002).
pub type Rgba32F = [f32; 4];
