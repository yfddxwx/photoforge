//! Geometry application (ADR-0005 order: rotate90/flip → straighten →
//! perspective → crop) over 8-bit storage, after the adjustment chain.

use photoforge_ops::geometry::{self, EdgeFill, Resample, WarpOptions};
use serde::{Deserialize, Serialize};

use crate::state::CropRect;

/// Display geometry applied after adjustments.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    /// 0..3 quarter turns clockwise.
    #[serde(default)]
    pub rotate_quarter: u8,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
    /// Arbitrary-angle rotation in degrees (clockwise positive).
    #[serde(default)]
    pub straighten_deg: f32,
    /// Perspective quad (TL, TR, BR, BL) in pixel-center coordinates of the
    /// rotated/straightened frame; mapped to fill the output.
    #[serde(default)]
    pub perspective: Option<[[f32; 2]; 4]>,
    /// Final crop in display-oriented pixel coordinates.
    #[serde(default)]
    pub crop: Option<CropRect>,
}

impl Geometry {
    /// True when applying geometry is an exact no-op.
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// Applies the geometry stack. `quality` selects the warp resample.
    pub fn apply(
        &self,
        img: &photoforge_core::PixelBuffer,
        quality: Quality,
    ) -> photoforge_core::PixelBuffer {
        let mut cur = img.clone();
        if self.rotate_quarter % 4 != 0 {
            cur = geometry::rotate90(&cur, u32::from(self.rotate_quarter % 4));
        }
        if self.flip_h {
            cur = geometry::flip_h(&cur);
        }
        if self.flip_v {
            cur = geometry::flip_v(&cur);
        }
        let opts = WarpOptions {
            resample: match quality {
                Quality::Draft => Resample::Bilinear,
                Quality::Final => Resample::Lanczos3,
            },
            fill: EdgeFill::ClampEdge,
        };
        if self.straighten_deg != 0.0 {
            cur = geometry::straighten(&cur, self.straighten_deg, opts);
        }
        if let Some(quad) = self.perspective {
            cur = geometry::perspective_correct(&cur, quad, opts);
        }
        if let Some(c) = self.crop {
            cur = geometry::crop(&cur, c.x, c.y, c.w, c.h);
        }
        cur
    }
}

/// Render quality selector: drags use `Draft`, committed frames and exports
/// use `Final` (Lanczos warps).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Quality {
    Draft,
    Final,
}
