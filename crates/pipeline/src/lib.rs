//! photoforge-pipeline
//!
//! The non-destructive engine: an ordered adjustment stack (a fixed global
//! set plus user-added masked local adjustments — ADR-0008), mask
//! rasterization and blending (ADR-0007), stage caches keyed by prefix
//! hashes (ADR-0006), pyramid-level preview rendering and full-resolution
//! fused export replay.

pub mod geometry;
pub mod mask;
pub mod render;
pub mod state;

pub use geometry::{Geometry, Quality};
pub use mask::{Mask, Stroke};
pub use render::Renderer;
pub use state::{Adjustment, CropRect, EditState};

/// Creates a fresh adjustment id.
pub fn new_adjustment_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
