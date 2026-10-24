//! The adjustment parameter model: one serde-friendly enum consumed by the
//! pipeline, the GUI, presets and `.pfg` sidecars.
//!
//! Every variant has neutral ("identity") defaults — an identity parameter
//! set reproduces the input bit-exactly (ADR-0004), which the property tests
//! enforce per variant.

use serde::{Deserialize, Serialize};

use crate::{color, curves, hsl, levels, lut, tone};
use photoforge_core::color::srgb;
use photoforge_core::math::lerp;
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
    /// Tone curves over sRGB-encoded values (ADR-0002): per channel plus a
    /// luma channel. Points are in [0, 1]²; default two-corner points are
    /// the identity.
    ToneCurve {
        #[serde(default)]
        r: Vec<[f32; 2]>,
        #[serde(default)]
        g: Vec<[f32; 2]>,
        #[serde(default)]
        b: Vec<[f32; 2]>,
        #[serde(default)]
        l: Vec<[f32; 2]>,
    },
    /// Master levels: black point, gamma, white point (encoded domain).
    Levels {
        #[serde(default)]
        black: f32,
        #[serde(default = "default_gamma")]
        gamma: f32,
        #[serde(default = "default_white")]
        white: f32,
    },
    /// Eight-zone HSL: per zone `[hue_degrees, sat, lum]`, all 0 = neutral.
    Hsl {
        #[serde(default)]
        zones: [[f32; 3]; 8],
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
    /// Imported `.cube` LUT (display-domain application). Empty table =
    /// identity (no LUT loaded).
    Lut {
        #[serde(default)]
        name: String,
        #[serde(default)]
        size: u32,
        /// `size³` entries, red fastest (see `ops::lut`).
        #[serde(default)]
        table: Vec<[f32; 3]>,
    },
}

fn default_gamma() -> f32 {
    1.0
}

fn default_white() -> f32 {
    1.0
}

/// A prepared per-pixel function (curve/levels ops precompute LUTs here).
pub type PixelFn = Box<dyn Fn(Rgba32F) -> Rgba32F + Send + Sync>;

/// LUT resolution for prepared encoded-domain ops.
const PREPARED_LUT_N: usize = 4096;

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
            OpKind::ToneCurve => Self::ToneCurve {
                r: Vec::new(),
                g: Vec::new(),
                b: Vec::new(),
                l: Vec::new(),
            },
            OpKind::Levels => Self::Levels {
                black: 0.0,
                gamma: 1.0,
                white: 1.0,
            },
            OpKind::Hsl => Self::Hsl {
                zones: [[0.0; 3]; 8],
            },
            OpKind::Vibrance => Self::Vibrance { amount: 0.0 },
            OpKind::Saturation => Self::Saturation { amount: 0.0 },
            OpKind::Lut => Self::Lut {
                name: String::new(),
                size: 0,
                table: Vec::new(),
            },
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
            Self::ToneCurve { .. } => OpKind::ToneCurve,
            Self::Levels { .. } => OpKind::Levels,
            Self::Hsl { .. } => OpKind::Hsl,
            Self::Vibrance { .. } => OpKind::Vibrance,
            Self::Saturation { .. } => OpKind::Saturation,
            Self::Lut { .. } => OpKind::Lut,
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
            Self::ToneCurve { r, g, b, l } => [r, g, b, l]
                .iter()
                .all(|pts| pts.is_empty() || curves::MonotoneSpline::is_identity_points(pts)),
            Self::Levels {
                black,
                gamma,
                white,
            } => *black == 0.0 && *gamma == 1.0 && *white == 1.0,
            Self::Hsl { zones } => zones
                .iter()
                .all(|z| z[0] == 0.0 && z[1] == 0.0 && z[2] == 0.0),
            Self::Lut { table, .. } => table.is_empty(),
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
            Self::ToneCurve { .. } => "Tone Curve",
            Self::Levels { .. } => "Levels",
            Self::Hsl { .. } => "Color Mixer (HSL)",
            Self::Vibrance { .. } => "Vibrance",
            Self::Saturation { .. } => "Saturation",
            Self::Lut { .. } => "LUT",
        }
    }

    /// All ops in this milestone are pointwise (fusable); spatial ops land in
    /// M6 and will extend this enum's behavior (ADR-0003).
    pub fn is_pointwise(&self) -> bool {
        true
    }

    /// Applies the op to one linear-light RGBA pixel (alpha passes through).
    /// Hot paths should prefer [`OpParams::prepare_pixel_fn`], which
    /// precomputes curve/levels LUTs.
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
            Self::ToneCurve { r, g, b, l } => curves::curve_pixel(px, r, g, b, l),
            Self::Levels {
                black,
                gamma,
                white,
            } => levels::levels_pixel(px, *black, *gamma, *white),
            Self::Hsl { zones } => hsl::hsl_pixel_rgb(px, zones),
            Self::Vibrance { amount } => color::vibrance_pixel(px, *amount),
            Self::Saturation { amount } => color::saturation_pixel(px, *amount),
            Self::Lut { table, size, .. } => {
                if *size >= 2 && table.len() == (*size as usize).pow(3) {
                    lut::lut_pixel_table(px, table, *size)
                } else {
                    px
                }
            }
        }
    }

    /// Builds an optimized per-pixel function for this op. Curve and levels
    /// ops precompute a 4096-entry table over the encoded domain; everything
    /// else wraps [`OpParams::apply_pixel`].
    pub fn prepare_pixel_fn(&self) -> PixelFn {
        match self {
            Self::ToneCurve { r, g, b, l } => {
                let mut tables: [Option<[f32; PREPARED_LUT_N]>; 4] = [None, None, None, None];
                for (i, pts) in [r, g, b, l].iter().enumerate() {
                    if pts.is_empty() || curves::MonotoneSpline::is_identity_points(pts) {
                        continue;
                    }
                    if let Some(spline) = curves::MonotoneSpline::new(pts.to_vec()) {
                        let mut t = [0.0f32; PREPARED_LUT_N];
                        for (j, slot) in t.iter_mut().enumerate() {
                            *slot = spline.eval(j as f32 / (PREPARED_LUT_N - 1) as f32);
                        }
                        tables[i] = Some(t);
                    }
                }
                if tables.iter().all(|t| t.is_none()) {
                    return Box::new(|px| px);
                }
                Box::new(move |px: Rgba32F| {
                    // Encode to the (continuous) sRGB domain, apply per-channel
                    // curves and the luma curve, decode back.
                    let e = [
                        srgb::encode_continuous(px[0]),
                        srgb::encode_continuous(px[1]),
                        srgb::encode_continuous(px[2]),
                    ];
                    let mut e = [
                        apply_table(e[0], &tables[0]),
                        apply_table(e[1], &tables[1]),
                        apply_table(e[2], &tables[2]),
                    ];
                    if let Some(lt) = tables[3].as_ref() {
                        let y = 0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2];
                        let y2 = apply_table_opt(y, Some(lt));
                        let factor = y2 / y.max(1e-3);
                        e = [
                            (e[0] * factor).clamp(0.0, 1.0),
                            (e[1] * factor).clamp(0.0, 1.0),
                            (e[2] * factor).clamp(0.0, 1.0),
                        ];
                    }
                    [
                        srgb::decode(e[0]),
                        srgb::decode(e[1]),
                        srgb::decode(e[2]),
                        px[3],
                    ]
                })
            }
            Self::Levels {
                black,
                gamma,
                white,
            } => {
                if self.is_identity() {
                    return Box::new(|px| px);
                }
                let (black, gamma, white) = (*black, *gamma, *white);
                let mut t = [0.0f32; PREPARED_LUT_N];
                for (j, slot) in t.iter_mut().enumerate() {
                    *slot = levels::levels_encoded(
                        j as f32 / (PREPARED_LUT_N - 1) as f32,
                        black,
                        gamma,
                        white,
                    );
                }
                Box::new(move |px: Rgba32F| {
                    let t = Some(t);
                    let e = [
                        apply_table(srgb::encode_continuous(px[0]), &t),
                        apply_table(srgb::encode_continuous(px[1]), &t),
                        apply_table(srgb::encode_continuous(px[2]), &t),
                    ];
                    [
                        srgb::decode(e[0]),
                        srgb::decode(e[1]),
                        srgb::decode(e[2]),
                        px[3],
                    ]
                })
            }
            other => {
                let op = other.clone();
                Box::new(move |px: Rgba32F| op.apply_pixel(px))
            }
        }
    }
}

#[inline]
fn apply_table(e: f32, table: &Option<[f32; PREPARED_LUT_N]>) -> f32 {
    apply_table_opt(e, table.as_ref())
}

#[inline]
fn apply_table_opt(e: f32, table: Option<&[f32; PREPARED_LUT_N]>) -> f32 {
    let Some(t) = table else {
        return e;
    };
    let x = e.clamp(0.0, 1.0) * (PREPARED_LUT_N - 1) as f32;
    let i = x.floor() as usize;
    let frac = x - i as f32;
    let a = t[i.min(PREPARED_LUT_N - 1)];
    let b = t[(i + 1).min(PREPARED_LUT_N - 1)];
    lerp(a, b, frac)
}

/// The kinds of adjustments (panel sections are built from this list).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpKind {
    WhiteBalance,
    Exposure,
    Contrast,
    Highlights,
    Shadows,
    Whites,
    Blacks,
    ToneCurve,
    Levels,
    Hsl,
    Vibrance,
    Saturation,
    Lut,
}

/// The canonical global adjustment order (Lightroom-like; ADR-0008).
pub const GLOBAL_OP_ORDER: [OpKind; 13] = [
    OpKind::WhiteBalance,
    OpKind::Exposure,
    OpKind::Contrast,
    OpKind::Highlights,
    OpKind::Shadows,
    OpKind::Whites,
    OpKind::Blacks,
    OpKind::ToneCurve,
    OpKind::Levels,
    OpKind::Hsl,
    OpKind::Vibrance,
    OpKind::Saturation,
    OpKind::Lut,
];
