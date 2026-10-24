//! Tonal adjustments (linear-light pointwise ops).
//!
//! Conventions (ADR-0002): all math on linear RGB; region weights derive from
//! Rec. 709 luma; outputs clamp to [0, 1]; identity parameters are bit-exact.
//!
//! Region model — chosen so every op is *provably monotone* on grayscale
//! ramps (no tone inversion) and pins the black/white points:
//! - highlights / whites compress (or expand) the *distance to white*:
//!   `out = 1 − (1 − c) · g(L)`; white stays white.
//! - shadows / blacks scale *from black*: `out = c · g(L)`; black stays
//!   black. (Consequence: absolute black cannot be lifted by these sliders —
//!   use exposure or the tone curve for that.)
//! - windows are smoothsteps of luma whose derivative is zero at both ends;
//!   for such windows `c · w′(c) ≤ 0.75` at every point, so gains with
//!   `|k| ≤ 1.33` keep `d(out)/dc ≥ 0`. All coefficients here are ≤ 0.8.

use photoforge_core::color::srgb::luma;
use photoforge_core::math::{clamp01, lerp, smoothstep};
use photoforge_core::Rgba32F;

/// Exposure in stops: +1 EV multiplies linear values by exactly 2.
#[inline]
pub fn exposure_pixel(px: Rgba32F, stops: f32) -> Rgba32F {
    if stops == 0.0 {
        return px;
    }
    let gain = f32::exp2(stops);
    [
        clamp01(px[0] * gain),
        clamp01(px[1] * gain),
        clamp01(px[2] * gain),
        px[3],
    ]
}

/// Contrast: power curve anchored at 18% linear gray.
/// `out = pivot · (in / pivot)^k` with `k = 2^(1.5 · amount)`.
/// Monotone, exact identity at the pivot, progressive clipping at extremes.
#[inline]
pub fn contrast_pixel(px: Rgba32F, amount: f32) -> Rgba32F {
    if amount == 0.0 {
        return px;
    }
    const PIVOT: f32 = 0.18;
    let k = f32::exp2(amount * 1.5);
    [
        clamp01(pivot_contrast(px[0], PIVOT, k)),
        clamp01(pivot_contrast(px[1], PIVOT, k)),
        clamp01(pivot_contrast(px[2], PIVOT, k)),
        px[3],
    ]
}

#[inline]
fn pivot_contrast(c: f32, pivot: f32, k: f32) -> f32 {
    // powf of a negative base is NaN — clamp the (already sanitized) input.
    let base = (c.max(0.0) / pivot).max(f32::MIN_POSITIVE);
    pivot * base.powf(k)
}

/// Highlights: positive = recovery (compress bright values toward white,
/// darkening them); negative = boost. White point pinned.
#[inline]
pub fn highlights_pixel(px: Rgba32F, amount: f32) -> Rgba32F {
    if amount == 0.0 {
        return px;
    }
    let w = smoothstep(0.45, 1.0, luma(px));
    let g = 1.0 + amount * 0.8 * w;
    [
        clamp01(1.0 - (1.0 - px[0]) * g),
        clamp01(1.0 - (1.0 - px[1]) * g),
        clamp01(1.0 - (1.0 - px[2]) * g),
        px[3],
    ]
}

/// Shadows: positive = lift (scale up from black), negative = crush.
/// Black point pinned.
#[inline]
pub fn shadows_pixel(px: Rgba32F, amount: f32) -> Rgba32F {
    if amount == 0.0 {
        return px;
    }
    let w = 1.0 - smoothstep(0.0, 0.55, luma(px));
    let g = 1.0 + amount * 0.8 * w;
    [
        clamp01(px[0] * g),
        clamp01(px[1] * g),
        clamp01(px[2] * g),
        px[3],
    ]
}

/// Whites: positive brightens the top of the range (shrinks distance to
/// white), negative dims it. White point pinned.
#[inline]
pub fn whites_pixel(px: Rgba32F, amount: f32) -> Rgba32F {
    if amount == 0.0 {
        return px;
    }
    let w = smoothstep(0.6, 1.0, luma(px));
    let g = 1.0 - amount * 0.8 * w;
    [
        clamp01(1.0 - (1.0 - px[0]) * g),
        clamp01(1.0 - (1.0 - px[1]) * g),
        clamp01(1.0 - (1.0 - px[2]) * g),
        px[3],
    ]
}

/// Blacks: positive lifts the bottom of the range, negative crushes it.
/// Black point pinned.
#[inline]
pub fn blacks_pixel(px: Rgba32F, amount: f32) -> Rgba32F {
    if amount == 0.0 {
        return px;
    }
    let w = 1.0 - smoothstep(0.0, 0.35, luma(px));
    let g = 1.0 + amount * 0.8 * w;
    [
        clamp01(px[0] * g),
        clamp01(px[1] * g),
        clamp01(px[2] * g),
        px[3],
    ]
}

/// White balance: channel gains interpolated between cool and warm anchor
/// illuminants, plus a green↔magenta tint axis. Neutral sliders = identity.
#[inline]
pub fn white_balance_pixel(px: Rgba32F, temp: f32, tint: f32) -> Rgba32F {
    if temp == 0.0 && tint == 0.0 {
        return px;
    }
    const WARM_R: f32 = 1.30; // ≈2500K cast
    const WARM_B: f32 = 0.72;
    const COOL_R: f32 = 0.82; // ≈7500K cast
    const COOL_B: f32 = 1.28;
    let mut gain_r = if temp > 0.0 {
        lerp(1.0, WARM_R, temp)
    } else {
        lerp(1.0, COOL_R, -temp)
    };
    let gain_g = 1.0;
    let mut gain_b = if temp > 0.0 {
        lerp(1.0, WARM_B, temp)
    } else {
        lerp(1.0, COOL_B, -temp)
    };
    // Tint: positive = magenta (less green, slightly more R/B).
    gain_r *= 1.0 + tint * 0.10;
    gain_b *= 1.0 + tint * 0.10;
    let gain_g = gain_g * (1.0 - tint * 0.25);
    [
        clamp01(px[0] * gain_r),
        clamp01(px[1] * gain_g),
        clamp01(px[2] * gain_b),
        px[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Provably-monotone spot check across the full ramp for every op at
    /// extreme settings (the property suite covers random parameters).
    #[test]
    fn extreme_amounts_stay_monotone() {
        for amount in [-1.0f32, -0.5, 0.5, 1.0] {
            type ToneFn = Box<dyn Fn(Rgba32F) -> Rgba32F>;
            let ops: Vec<(&str, ToneFn)> = vec![
                (
                    "highlights",
                    Box::new(move |px| highlights_pixel(px, amount)),
                ),
                ("shadows", Box::new(move |px| shadows_pixel(px, amount))),
                ("whites", Box::new(move |px| whites_pixel(px, amount))),
                ("blacks", Box::new(move |px| blacks_pixel(px, amount))),
                ("contrast", Box::new(move |px| contrast_pixel(px, amount))),
            ];
            for (name, f) in &ops {
                let mut prev = -1.0f32;
                for i in 0..=512 {
                    let v = i as f32 / 512.0;
                    let out = f([v, v, v, 1.0])[0];
                    assert!(
                        out >= prev - 1e-6,
                        "{name}({amount}) inverted at {v}: {prev} -> {out}"
                    );
                    prev = out;
                }
            }
        }
    }

    #[test]
    fn black_and_white_points_pinned() {
        for amount in [-1.0f32, 1.0] {
            let black = [0.0f32, 0.0, 0.0, 1.0];
            let white = [1.0f32, 1.0, 1.0, 1.0];
            assert_eq!(highlights_pixel(black, amount), black);
            assert_eq!(shadows_pixel(white, amount), white);
            assert_eq!(whites_pixel(black, amount), black);
            assert_eq!(blacks_pixel(white, amount), white);
        }
        // Highlights keep white white; shadows keep black black.
        let white = [1.0f32, 1.0, 1.0, 1.0];
        let black = [0.0f32, 0.0, 0.0, 1.0];
        assert_eq!(highlights_pixel(white, 1.0), white);
        assert_eq!(shadows_pixel(black, 1.0), black);
    }
}
