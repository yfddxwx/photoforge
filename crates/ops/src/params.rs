//! The adjustment parameter model: one serde-friendly enum consumed by the
//! pipeline, the GUI, presets and `.pfg` sidecars.
//!
//! Every variant has neutral ("identity") defaults — an identity parameter
//! set reproduces the input bit-exactly (ADR-0004), which the property tests
//! enforce per variant.

use serde::{Deserialize, Serialize};

use crate::{bw, color, curves, denoise, effects, filters, hsl, levels, lut, tone};
use photoforge_core::color::srgb;
use photoforge_core::math::{clamp01, lerp};
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
    // --- spatial / position-dependent ops (apply_image, ADR-0003) ---
    Sharpen {
        #[serde(default)]
        amount: f32,
        #[serde(default = "default_radius")]
        radius: f32,
        #[serde(default)]
        threshold: f32,
    },
    Clarity {
        #[serde(default)]
        amount: f32,
    },
    Denoise {
        #[serde(default)]
        strength: f32,
    },
    Blur {
        #[serde(default)]
        radius: f32,
    },
    BlackWhite {
        #[serde(default = "default_bw_r")]
        r: f32,
        #[serde(default = "default_bw_g")]
        g: f32,
        #[serde(default = "default_bw_b")]
        b: f32,
    },
    Duotone {
        /// Shadow color in linear light (0..1 per channel).
        #[serde(default)]
        shadow: [f32; 3],
        /// Highlight color in linear light.
        #[serde(default = "default_duotone_highlight")]
        highlight: [f32; 3],
    },
    Grain {
        #[serde(default)]
        amount: f32,
        #[serde(default = "default_grain_size")]
        size: f32,
        #[serde(default)]
        roughness: f32,
        #[serde(default)]
        seed: u64,
    },
    Vignette {
        #[serde(default)]
        amount: f32,
        #[serde(default = "default_midpoint")]
        midpoint: f32,
        #[serde(default = "default_feather")]
        feather: f32,
    },
}

fn default_radius() -> f32 {
    1.0
}

fn default_bw_r() -> f32 {
    bw::BW_DEFAULTS[0]
}

fn default_bw_g() -> f32 {
    bw::BW_DEFAULTS[1]
}

fn default_bw_b() -> f32 {
    bw::BW_DEFAULTS[2]
}

fn default_duotone_highlight() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

fn default_grain_size() -> f32 {
    2.0
}

fn default_midpoint() -> f32 {
    0.5
}

fn default_feather() -> f32 {
    0.5
}

fn default_gamma() -> f32 {
    1.0
}

fn default_white() -> f32 {
    1.0
}

/// A prepared per-pixel function (curve/levels ops precompute LUTs here).
pub type PixelFn = Box<dyn Fn(Rgba32F) -> Rgba32F + Send + Sync>;

/// A prepared function over sRGB-encoded RGB triples.
pub type EncodedFn = Box<dyn Fn([f32; 3]) -> [f32; 3] + Send + Sync>;

/// A prepared op, tagged with its working domain (ADR-0002): linear-domain
/// ops apply directly; encoded-domain ops are batched by [`fuse_run`] so a
/// run pays one encode/decode round-trip instead of one per op.
pub enum Prepared {
    Linear(PixelFn),
    Encoded(EncodedFn),
}

/// Fuses a run of prepared ops into one per-pixel function: consecutive
/// encoded-domain ops share a single encode/decode.
pub fn fuse_run(mut run: Vec<Prepared>) -> PixelFn {
    if run.len() == 1 {
        return match run.pop().expect("len 1") {
            Prepared::Linear(f) => f,
            Prepared::Encoded(f) => Box::new(move |px: Rgba32F| {
                let e = [
                    srgb::encode_continuous(px[0]),
                    srgb::encode_continuous(px[1]),
                    srgb::encode_continuous(px[2]),
                ];
                let o = f(e);
                [
                    srgb::decode(o[0]),
                    srgb::decode(o[1]),
                    srgb::decode(o[2]),
                    px[3],
                ]
            }),
        };
    }
    Box::new(move |px: Rgba32F| {
        let mut cur = px;
        let mut i = 0usize;
        while i < run.len() {
            match &run[i] {
                Prepared::Linear(f) => {
                    cur = f(cur);
                    i += 1;
                }
                Prepared::Encoded(_) => {
                    let mut j = i;
                    while j < run.len() && matches!(run[j], Prepared::Encoded(_)) {
                        j += 1;
                    }
                    let mut e = [
                        srgb::encode_continuous(cur[0]),
                        srgb::encode_continuous(cur[1]),
                        srgb::encode_continuous(cur[2]),
                    ];
                    for f in &run[i..j] {
                        if let Prepared::Encoded(fe) = f {
                            e = fe(e);
                        }
                    }
                    cur = [
                        srgb::decode(e[0]),
                        srgb::decode(e[1]),
                        srgb::decode(e[2]),
                        cur[3],
                    ];
                    i = j;
                }
            }
        }
        cur
    })
}

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
            OpKind::Sharpen => Self::Sharpen {
                amount: 0.0,
                radius: 1.0,
                threshold: 0.0,
            },
            OpKind::Clarity => Self::Clarity { amount: 0.0 },
            OpKind::Denoise => Self::Denoise { strength: 0.0 },
            OpKind::Blur => Self::Blur { radius: 0.0 },
            OpKind::BlackWhite => Self::BlackWhite {
                r: bw::BW_DEFAULTS[0],
                g: bw::BW_DEFAULTS[1],
                b: bw::BW_DEFAULTS[2],
            },
            OpKind::Duotone => Self::Duotone {
                shadow: [0.0, 0.0, 0.0],
                highlight: [1.0, 1.0, 1.0],
            },
            OpKind::Grain => Self::Grain {
                amount: 0.0,
                size: 2.0,
                roughness: 0.0,
                seed: 0,
            },
            OpKind::Vignette => Self::Vignette {
                amount: 0.0,
                midpoint: 0.5,
                feather: 0.5,
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
            Self::Sharpen { .. } => OpKind::Sharpen,
            Self::Clarity { .. } => OpKind::Clarity,
            Self::Denoise { .. } => OpKind::Denoise,
            Self::Blur { .. } => OpKind::Blur,
            Self::BlackWhite { .. } => OpKind::BlackWhite,
            Self::Duotone { .. } => OpKind::Duotone,
            Self::Grain { .. } => OpKind::Grain,
            Self::Vignette { .. } => OpKind::Vignette,
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
            Self::Sharpen { amount, .. } | Self::Clarity { amount } => *amount == 0.0,
            Self::Denoise { strength } | Self::Blur { radius: strength } => *strength == 0.0,
            // B&W and duotone are conversions: they are "on" whenever
            // enabled — the canonical identity is the disabled state.
            Self::BlackWhite { .. } | Self::Duotone { .. } => false,
            Self::Grain { amount, .. } | Self::Vignette { amount, .. } => *amount == 0.0,
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
            Self::Sharpen { .. } => "Sharpening",
            Self::Clarity { .. } => "Clarity",
            Self::Denoise { .. } => "Noise Reduction",
            Self::Blur { .. } => "Blur",
            Self::BlackWhite { .. } => "Black && White",
            Self::Duotone { .. } => "Duotone",
            Self::Grain { .. } => "Grain",
            Self::Vignette { .. } => "Vignette",
        }
    }

    /// Pointwise ops fuse into single-pass chains; spatial/position-dependent
    /// ops (blur family, grain, vignette) run as whole-image passes and break
    /// the chain (ADR-0003).
    pub fn is_pointwise(&self) -> bool {
        !matches!(
            self,
            Self::Sharpen { .. }
                | Self::Clarity { .. }
                | Self::Denoise { .. }
                | Self::Blur { .. }
                | Self::Grain { .. }
                | Self::Vignette { .. }
        )
    }

    /// Whole-image in-place application for the spatial class. Pointwise ops
    /// fall back to a prepared per-pixel loop (used by tests and simple
    /// paths); the pipeline never routes pointwise ops here.
    pub fn apply_image(&self, img: &mut photoforge_core::LinearImage) {
        match self {
            Self::Sharpen {
                amount,
                radius,
                threshold,
            } => filters::unsharp_mask(img, *amount, *radius, *threshold),
            Self::Clarity { amount } => filters::clarity(img, *amount),
            Self::Denoise { strength } => denoise::denoise(img, *strength),
            Self::Blur { radius } => filters::gaussian_blur(img, *radius),
            Self::Grain {
                amount,
                size,
                roughness,
                seed,
            } => effects::grain(img, *amount, *size, *roughness, *seed),
            Self::Vignette {
                amount,
                midpoint,
                feather,
            } => effects::vignette(img, *amount, *midpoint, *feather),
            other => {
                let prepared = other.prepare_pixel_fn();
                for px in img.data.chunks_exact_mut(4) {
                    let p = [px[0], px[1], px[2], px[3]];
                    px.copy_from_slice(&prepared(p));
                }
            }
        }
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
            Self::BlackWhite { r, g, b } => bw::black_white_pixel(px, [*r, *g, *b]),
            Self::Duotone { shadow, highlight } => bw::duotone_pixel(px, *shadow, *highlight),
            // Spatial ops are applied via `apply_image`, not per pixel; a
            // stray per-pixel call degrades to identity rather than panicking.
            Self::Sharpen { .. }
            | Self::Clarity { .. }
            | Self::Denoise { .. }
            | Self::Blur { .. }
            | Self::Grain { .. }
            | Self::Vignette { .. } => px,
        }
    }

    /// Prepares this op tagged with its working domain: curve/levels use
    /// 4096-entry encoded-domain LUTs, exposure/WB hoist their gains, and the
    /// OKLab/HSL ops precompute 3D LUTs (sampled trilinearly) — all
    /// benchmarked against the fit-preview budget.
    pub fn prepare(&self) -> Prepared {
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
                    return Prepared::Encoded(Box::new(|e| e));
                }
                Prepared::Encoded(Box::new(move |e: [f32; 3]| {
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
                    e
                }))
            }
            Self::Levels {
                black,
                gamma,
                white,
            } => {
                if self.is_identity() {
                    return Prepared::Encoded(Box::new(|e| e));
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
                Prepared::Encoded(Box::new(move |e: [f32; 3]| {
                    [
                        apply_table(e[0], &Some(t)),
                        apply_table(e[1], &Some(t)),
                        apply_table(e[2], &Some(t)),
                    ]
                }))
            }
            Self::Exposure { stops } => {
                let gain = f32::exp2(*stops);
                if gain == 1.0 {
                    Prepared::Linear(Box::new(|px| px))
                } else {
                    Prepared::Linear(Box::new(move |px: Rgba32F| {
                        [
                            clamp01(px[0] * gain),
                            clamp01(px[1] * gain),
                            clamp01(px[2] * gain),
                            px[3],
                        ]
                    }))
                }
            }
            Self::Contrast { amount } => {
                if *amount == 0.0 {
                    Prepared::Encoded(Box::new(|e| e))
                } else {
                    let mut t = [0.0f32; PREPARED_LUT_N];
                    for (j, slot) in t.iter_mut().enumerate() {
                        *slot =
                            tone::contrast_encoded(j as f32 / (PREPARED_LUT_N - 1) as f32, *amount);
                    }
                    Prepared::Encoded(Box::new(move |e: [f32; 3]| {
                        [
                            apply_table(e[0], &Some(t)),
                            apply_table(e[1], &Some(t)),
                            apply_table(e[2], &Some(t)),
                        ]
                    }))
                }
            }
            Self::WhiteBalance { temp, tint } => {
                let (temp, tint) = (*temp, *tint);
                if temp == 0.0 && tint == 0.0 {
                    Prepared::Linear(Box::new(|px| px))
                } else {
                    let gains = tone::white_balance_gains(temp, tint);
                    Prepared::Linear(Box::new(move |px: Rgba32F| {
                        [
                            clamp01(px[0] * gains[0]),
                            clamp01(px[1] * gains[1]),
                            clamp01(px[2] * gains[2]),
                            px[3],
                        ]
                    }))
                }
            }
            Self::Vibrance { amount } | Self::Saturation { amount } => {
                // OKLab chroma ops are expensive per pixel; precompute a
                // 33³ encoded-domain LUT and sample trilinearly.
                if *amount == 0.0 {
                    Prepared::Encoded(Box::new(|e| e))
                } else {
                    let n = 33u32;
                    let op = self.clone();
                    let mut table = Vec::with_capacity((n * n * n) as usize);
                    for b in 0..n {
                        for g in 0..n {
                            for r in 0..n {
                                let e = [
                                    r as f32 / (n - 1) as f32,
                                    g as f32 / (n - 1) as f32,
                                    b as f32 / (n - 1) as f32,
                                ];
                                let lin =
                                    [srgb::decode(e[0]), srgb::decode(e[1]), srgb::decode(e[2])];
                                let out = match &op {
                                    Self::Vibrance { amount } => color::vibrance_pixel(
                                        [lin[0], lin[1], lin[2], 1.0],
                                        *amount,
                                    ),
                                    Self::Saturation { amount } => color::saturation_pixel(
                                        [lin[0], lin[1], lin[2], 1.0],
                                        *amount,
                                    ),
                                    _ => unreachable!(),
                                };
                                table.push([
                                    srgb::encode_continuous(out[0]),
                                    srgb::encode_continuous(out[1]),
                                    srgb::encode_continuous(out[2]),
                                ]);
                            }
                        }
                    }
                    Prepared::Encoded(Box::new(move |e: [f32; 3]| lut::sample_table(&table, n, e)))
                }
            }
            Self::Hsl { zones } => {
                // HSL per pixel costs a full HSV round-trip with per-zone
                // trig; precompute a 17³ encoded-domain LUT instead
                // (~1.5 ms build, trilinear sampling afterwards).
                let any_effect = zones
                    .iter()
                    .any(|z| z[0] != 0.0 || z[1] != 0.0 || z[2] != 0.0);
                if !any_effect {
                    Prepared::Encoded(Box::new(|e| e))
                } else {
                    let n = 17u32;
                    let zones = *zones;
                    let mut table = Vec::with_capacity((n * n * n) as usize);
                    for b in 0..n {
                        for g in 0..n {
                            for r in 0..n {
                                let e = [
                                    r as f32 / (n - 1) as f32,
                                    g as f32 / (n - 1) as f32,
                                    b as f32 / (n - 1) as f32,
                                ];
                                table.push(hsl::hsl_encoded(e, &zones));
                            }
                        }
                    }
                    Prepared::Encoded(Box::new(move |e: [f32; 3]| lut::sample_table(&table, n, e)))
                }
            }
            other => {
                let op = other.clone();
                Prepared::Linear(Box::new(move |px: Rgba32F| op.apply_pixel(px)))
            }
        }
    }

    /// Builds an optimized per-pixel function for this op (single-op form;
    /// pipelines fusing runs should use [`OpParams::prepare`] + [`fuse_run`]).
    pub fn prepare_pixel_fn(&self) -> PixelFn {
        fuse_run(vec![self.prepare()])
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
    Sharpen,
    Clarity,
    Denoise,
    Blur,
    BlackWhite,
    Duotone,
    Grain,
    Vignette,
}

/// The canonical global adjustment order (Lightroom-like; ADR-0008).
pub const GLOBAL_OP_ORDER: [OpKind; 21] = [
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
    OpKind::Sharpen,
    OpKind::Clarity,
    OpKind::Denoise,
    OpKind::Blur,
    OpKind::BlackWhite,
    OpKind::Duotone,
    OpKind::Grain,
    OpKind::Vignette,
];

/// Effect kinds that start disabled in a fresh global stack (they change the
/// look drastically, unlike the tonal defaults).
pub const DEFAULT_DISABLED: [OpKind; 8] = [
    OpKind::Sharpen,
    OpKind::Clarity,
    OpKind::Denoise,
    OpKind::Blur,
    OpKind::BlackWhite,
    OpKind::Duotone,
    OpKind::Grain,
    OpKind::Vignette,
];
