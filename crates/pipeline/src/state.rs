//! The persistent edit state: adjustments + geometry. This (as JSON) is the
//! source of truth — pixels are always re-derived (see ARCHITECTURE.md).

use photoforge_ops::OpParams;
use serde::{Deserialize, Serialize};

use crate::geometry::Geometry;
use crate::mask::Mask;

/// Full non-destructive edit description for one image.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EditState {
    /// Schema version for forward migration.
    #[serde(default)]
    pub version: u32,
    /// Ordered adjustment stack (ADR-0008): global adjustments first
    /// (fixed order), user-added local (masked) adjustments after.
    #[serde(default)]
    pub adjustments: Vec<Adjustment>,
    #[serde(default)]
    pub geometry: Geometry,
}

impl EditState {
    pub fn new() -> Self {
        Self {
            version: 1,
            adjustments: Vec::new(),
            geometry: Geometry::default(),
        }
    }

    /// Builds the canonical global stack: one identity adjustment per
    /// kind in [`photoforge_ops::params::GLOBAL_OP_ORDER`].
    pub fn with_global_stack() -> Self {
        let mut state = Self::new();
        for kind in photoforge_ops::params::GLOBAL_OP_ORDER {
            state
                .adjustments
                .push(Adjustment::new(OpParams::identity_of(kind)));
        }
        state
    }

    /// Serializes to pretty JSON (sidecars are user-inspectable).
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

/// One entry in the adjustment stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Adjustment {
    /// Stable id (uuid) — cache keys and history commands reference it.
    pub id: String,
    pub op: OpParams,
    /// Local adjustment mask; `None` = global (applies everywhere).
    #[serde(default)]
    pub mask: Option<Mask>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Adjustment {
    pub fn new(op: OpParams) -> Self {
        Self {
            id: crate::new_adjustment_id(),
            op,
            mask: None,
            enabled: true,
        }
    }

    pub fn masked(op: OpParams, mask: Mask) -> Self {
        Self {
            id: crate::new_adjustment_id(),
            op,
            mask: Some(mask),
            enabled: true,
        }
    }
}

/// Crop rectangle in display-oriented pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use photoforge_ops::params::GLOBAL_OP_ORDER;

    #[test]
    fn global_stack_shape() {
        let s = EditState::with_global_stack();
        assert_eq!(s.adjustments.len(), GLOBAL_OP_ORDER.len());
        for adj in &s.adjustments {
            assert!(adj.enabled);
            assert!(adj.mask.is_none());
            assert!(adj.op.is_identity());
        }
        // Order matches the canonical order.
        for (adj, kind) in s.adjustments.iter().zip(GLOBAL_OP_ORDER) {
            assert_eq!(adj.op.kind(), kind);
        }
    }

    #[test]
    fn json_roundtrip() {
        let mut s = EditState::with_global_stack();
        s.adjustments[1].op = OpParams::Exposure { stops: 1.5 };
        s.adjustments[2].enabled = false;
        s.geometry.crop = Some(CropRect {
            x: 2,
            y: 3,
            w: 10,
            h: 11,
        });
        s.geometry.straighten_deg = -3.25;
        let json = s.to_json().unwrap();
        let back = EditState::from_json(&json).unwrap();
        assert_eq!(back, s);
        // Tagged op format is stable and human-readable.
        assert!(json.contains("\"type\": \"exposure\""));
    }

    #[test]
    fn missing_fields_get_defaults() {
        // Forward compatibility: unknown/missing fields fall back.
        let json =
            r#"{"version":1,"adjustments":[{"id":"x","op":{"type":"exposure","stops":1.0}}]}"#;
        let s = EditState::from_json(json).unwrap();
        assert_eq!(s.adjustments.len(), 1);
        assert!(s.adjustments[0].enabled);
        assert!(s.adjustments[0].mask.is_none());
    }
}
