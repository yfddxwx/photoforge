//! The adjustment parameter model: one serde-friendly enum consumed by the
//! pipeline, the GUI, presets and `.pfg` sidecars.
//!
//! Every variant has neutral ("identity") defaults — an identity parameter
//! set reproduces the input bit-exactly (ADR-0004), which the property tests
//! enforce per variant.

use serde::{Deserialize, Serialize};

use crate::color;
use crate::tone;
use photoforge_core::Rgba32F;

/// Parameters of one adjustment. Serialized with an internally tagged
/// `"type"` field (stable format for sidecars/presets).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpParams {
    Exposure {
        /// Stops of exposure. +1 stop multiplies linear light by exactly 2.
        #[serde(default)]
        stops: f32,
    },
    Contrast {
        /// −1..1; positive increases contrast around 18% linear gray.
        #[serde(default)]
        amount: f32,
    },
    Highlights {
        /// −1..1; positive recovers (darkens) bright regions.
        #[serde(default)]
        amount: f32,
    },
    Shadows {
        /// −1..1; positive lifts dark regions.
        #[serde(default)]
        amount: f32,
    },
    Whites {
        /// −1..1; positive brightens the white end.
        #[serde(default)]
        amount: f32,
    },
    Blacks {
        /// −1..1; positive lifts the black point.
        #[serde(default)]
        amount: f32,
    },
    WhiteBalance {
        /// −1 (cool) .. 1 (warm).
        #[serde(default)]
        temp: f32,
        /// −1 (green) .. 1 (magenta).
        #[serde(default)]
        tint: f32,
    },
    Vibrance {
        /// −1..1; boosts low-chroma colors more (OKLab).
        #[serde(default)]
        amount: f32,
    },
    Saturation {
        /// −1..1; −1 = exact grayscale, +1 = doubled chroma (OKLab).
        #[serde(default)]
        amount: f32,
    },
}

impl OpParams {
    /// Neutral parameters for the op kind named by `kind`.
    pub fn identity_of(kind: OpKind) -> Self {
        match kind {
            OpKind::Exposure => Self::Exposure { stops: 0.0 },
            OpKind::Contrast => Self::Contrast { amount: 0.0 },
            OpKind::Highlights => Self::Highlights { amount: 0.0 },
            OpKind::Shadows => Self::Shadows { amount: 0.0 },
            OpKind::Whites => Self::Whites { amount: 0.0 },
            OpKind::Blacks => Self::Blacks { amount: 0.0 },
            OpKind::WhiteBalance => Self::WhiteBalance {
                temp: 0.0,
                tint: 0.0,
            },
            OpKind::Vibrance => Self::Vibrance { amount: 0.0 },
            OpKind::Saturation => Self::Saturation { amount: 0.0 },
        }
    }

    /// The op kind this variant represents.
    pub fn kind(&self) -> OpKind {
        match self {
            Self::Exposure { .. } => OpKind::Exposure,
            Self::Contrast { .. } => OpKind::Contrast,
            Self::Highlights { .. } => OpKind::Highlights,
            Self::Shadows { .. } => OpKind::Shadows,
            Self::Whites { .. } => OpKind::Whites,
            Self::Blacks { .. } => OpKind::Blacks,
            Self::WhiteBalance { .. } => OpKind::WhiteBalance,
            Self::Vibrance { .. } => OpKind::Vibrance,
            Self::Saturation { .. } => OpKind::Saturation,
        }
    }

    /// True when applying this op is a bit-exact no-op.
    pub fn is_identity(&self) -> bool {
        match self {
            Self::Exposure { stops } => *stops == 0.0,
            Self::Contrast { amount }
            | Self::Highlights { amount }
            | Self::Shadows { amount }
            | Self::Whites { amount }
            | Self::Blacks { amount }
            | Self::Vibrance { amount }
            | Self::Saturation { amount } => *amount == 0.0,
            Self::WhiteBalance { temp, tint } => *temp == 0.0 && *tint == 0.0,
        }
    }

    /// UI label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Exposure { .. } => "Exposure",
            Self::Contrast { .. } => "Contrast",
            Self::Highlights { .. } => "Highlights",
            Self::Shadows { .. } => "Shadows",
            Self::Whites { .. } => "Whites",
            Self::Blacks { .. } => "Blacks",
            Self::WhiteBalance { .. } => "White Balance",
            Self::Vibrance { .. } => "Vibrance",
            Self::Saturation { .. } => "Saturation",
        }
    }

    /// All ops in this milestone are pointwise (fusable); spatial ops land in
    /// M6 and will extend this enum's behavior (ADR-0003).
    pub fn is_pointwise(&self) -> bool {
        true
    }

    /// Applies the op to one linear-light RGBA pixel (alpha passes through).
    #[inline]
    pub fn apply_pixel(&self, px: Rgba32F) -> Rgba32F {
        match self {
            Self::Exposure { stops } => tone::exposure_pixel(px, *stops),
            Self::Contrast { amount } => tone::contrast_pixel(px, *amount),
            Self::Highlights { amount } => tone::highlights_pixel(px, *amount),
            Self::Shadows { amount } => tone::shadows_pixel(px, *amount),
            Self::Whites { amount } => tone::whites_pixel(px, *amount),
            Self::Blacks { amount } => tone::blacks_pixel(px, *amount),
            Self::WhiteBalance { temp, tint } => tone::white_balance_pixel(px, *temp, *tint),
            Self::Vibrance { amount } => color::vibrance_pixel(px, *amount),
            Self::Saturation { amount } => color::saturation_pixel(px, *amount),
        }
    }
}

/// The kinds of adjustments (panel sections are built from this list).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpKind {
    Exposure,
    Contrast,
    Highlights,
    Shadows,
    Whites,
    Blacks,
    WhiteBalance,
    Vibrance,
    Saturation,
}

/// The canonical global adjustment order (Lightroom-like; ADR-0008).
pub const GLOBAL_OP_ORDER: [OpKind; 9] = [
    OpKind::WhiteBalance,
    OpKind::Exposure,
    OpKind::Contrast,
    OpKind::Highlights,
    OpKind::Shadows,
    OpKind::Whites,
    OpKind::Blacks,
    OpKind::Vibrance,
    OpKind::Saturation,
];
