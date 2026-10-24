//! photoforge-ops
//!
//! Every image adjustment as a pure function over `LinearImage` (linear-light
//! f32 working space) and every geometry operation (crop, rotate, flip,
//! straighten, perspective, Lanczos3 resize) over `PixelBuffer`.
//!
//! Invariants enforced by tests (ADR-0004): pure functions, identity at
//! default parameters, output within [0, 1] where documented, no NaN, no
//! panics on any input.

pub mod bw;
pub mod color;
pub mod curves;
pub mod denoise;
pub mod effects;
pub mod filters;
pub mod geometry;
pub mod hsl;
pub mod levels;
pub mod lut;
pub mod params;
pub mod tone;

pub use params::{fuse_run, OpParams, Prepared};
