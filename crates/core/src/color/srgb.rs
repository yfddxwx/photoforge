//! sRGB transfer functions and the u8 ↔ linear-light boundary.
//!
//! IEC 61966-2-1 sRGB EOTF (decode, encoded → linear):
//! `v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055)^2.4`
//! and its inverse (encode):
//! `v <= 0.0031308 ? v * 12.92 : 1.055 * v^(1/2.4) − 0.055`.
//!
//! Hot paths use lookup tables instead of `powf`:
//! - `TO_LINEAR_U8`: the exact 256-entry decode table (inputs are u8).
//! - `DECODE_LUT` / `ENCODE_LUT`: 65536-entry tables over [0, 1] for
//!   arbitrary f32 inputs. Their worst-case error is far below half an 8-bit
//!   code (tested against the formulas), which keeps u8 round-trips
//!   bit-exact.
//!
//! Domain conventions (ADR-0002): non-finite inputs are sanitized — NaN maps
//! to 0 (black), +Inf to 1, −Inf to 0.

use std::sync::LazyLock;

/// Rec. 709 luma coefficients applied to *linear* RGB (ADR-0002).
pub const LUMA_R: f32 = 0.2126;
pub const LUMA_G: f32 = 0.7152;
pub const LUMA_B: f32 = 0.0722;

const CUTOFF: f32 = 0.04045;
const PHI: f32 = 12.92;
const POW: f32 = 2.4;
const DENOM: f32 = 1.055;
const OFFSET: f32 = 0.055;

const LUT_BITS: u32 = 16;
const LUT_SIZE: usize = 1 << LUT_BITS;
const LUT_MAX: f32 = (LUT_SIZE - 1) as f32;

/// sRGB decode (encoded → linear) from the IEC formula. Reference
/// implementation; the tables below must match it.
pub fn decode_formula(x: f32) -> f32 {
    if x <= CUTOFF {
        x / PHI
    } else {
        ((x + OFFSET) / DENOM).powf(POW)
    }
}

/// sRGB encode (linear → encoded) from the IEC formula. Reference
/// implementation; the tables below must match it.
pub fn encode_formula(x: f32) -> f32 {
    if x <= 0.003_130_8 {
        x * PHI
    } else {
        DENOM * x.powf(1.0 / POW) - OFFSET
    }
}

static TO_LINEAR_U8: LazyLock<[f32; 256]> = LazyLock::new(|| {
    let mut t = [0.0f32; 256];
    for (i, v) in t.iter_mut().enumerate() {
        *v = decode_formula(i as f32 / 255.0);
    }
    t
});

/// Decoded linear light for every possible u8 value, exact by construction.
static DECODE_LUT: LazyLock<Vec<f32>> = LazyLock::new(|| {
    let mut t = Vec::with_capacity(LUT_SIZE);
    for i in 0..LUT_SIZE {
        t.push(decode_formula(i as f32 / LUT_MAX));
    }
    t
});

static ENCODE_LUT: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let mut t = Vec::with_capacity(LUT_SIZE);
    for i in 0..LUT_SIZE {
        let lin = i as f32 / LUT_MAX;
        t.push(
            (encode_formula(lin) * 255.0 + 0.5)
                .floor()
                .clamp(0.0, 255.0) as u8,
        );
    }
    t
});

/// Decodes an 8-bit sRGB value to linear light (exact table lookup).
#[inline]
pub fn decode_u8(c: u8) -> f32 {
    TO_LINEAR_U8[c as usize]
}

/// Decodes an arbitrary sRGB-encoded value in [0, 1] to linear light.
/// Non-finite input maps to 0.
#[inline]
pub fn decode(x: f32) -> f32 {
    if x.is_nan() || x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    DECODE_LUT[(x * LUT_MAX) as usize]
}

/// Encodes linear light in [0, 1] to an 8-bit sRGB value. Non-finite input
/// maps to 0 (NaN) or 255 (+Inf).
#[inline]
pub fn encode_u8(x: f32) -> u8 {
    if x.is_nan() || x <= 0.0 {
        return 0;
    }
    if x >= 1.0 {
        return 255; // Includes +Inf.
    }
    ENCODE_LUT[(x * LUT_MAX) as usize]
}

/// Encodes linear light in [0, 1] to an sRGB-encoded f32 (u8-grid value / 255).
#[inline]
pub fn encode_f32(x: f32) -> f32 {
    encode_u8(x) as f32 / 255.0
}

/// Continuous (non-u8-quantized) encode table: 65536 entries over [0, 1].
static ENCODE_F32_LUT: LazyLock<Vec<f32>> = LazyLock::new(|| {
    let mut t = Vec::with_capacity(LUT_SIZE);
    for i in 0..LUT_SIZE {
        t.push(encode_formula(i as f32 / LUT_MAX).clamp(0.0, 1.0));
    }
    t
});

/// Encodes linear light to a *continuous* sRGB-encoded f32 (no u8
/// quantization) — used by encoded-domain ops (curves, levels, HSL, LUT) so
/// adjustments stay smooth below the 8-bit grid.
#[inline]
pub fn encode_continuous(x: f32) -> f32 {
    if x.is_nan() || x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    ENCODE_F32_LUT[(x * LUT_MAX) as usize]
}

/// Rec. 709 luma of a linear-light RGBA pixel.
#[inline]
pub fn luma(px: crate::Rgba32F) -> f32 {
    LUMA_R * px[0] + LUMA_G * px[1] + LUMA_B * px[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All 256 u8 values survive the round trip exactly, through both the
    /// exact table path and the f32 LUT path.
    #[test]
    fn u8_roundtrip_exact() {
        for c in 0..=255u8 {
            let lin = decode_u8(c);
            assert_eq!(encode_u8(lin), c, "u8 roundtrip failed at {c}");
            let lin2 = decode(c as f32 / 255.0);
            assert_eq!(encode_u8(lin2), c, "f32-LUT roundtrip failed at {c}");
        }
    }

    /// The 16-bit LUTs track the formulas far below half an 8-bit code.
    #[test]
    fn luts_match_formulas() {
        for i in (0..LUT_SIZE).step_by(97) {
            let x = i as f32 / LUT_MAX;
            let dec_err = (decode(x) - decode_formula(x)).abs();
            let enc_err = (encode_f32(x) - encode_formula(x).clamp(0.0, 1.0)).abs();
            assert!(dec_err < 2e-4, "decode LUT error {dec_err} at {x}");
            // encode_f32 returns the u8-grid value; compare on that grid.
            assert!(
                enc_err <= 0.5 / 255.0 + 1e-4,
                "encode LUT error {enc_err} at {x}"
            );
        }
    }

    #[test]
    fn boundary_values() {
        assert_eq!(decode_u8(0), 0.0);
        assert_eq!(decode_u8(255), 1.0);
        assert_eq!(decode(0.0), 0.0);
        assert_eq!(decode(1.0), 1.0);
        assert_eq!(encode_u8(0.0), 0);
        assert_eq!(encode_u8(1.0), 255);
        // Out-of-range and non-finite inputs are sanitized, never NaN.
        assert_eq!(decode(-0.5), 0.0);
        assert_eq!(decode(f32::NAN), 0.0);
        assert_eq!(decode(f32::INFINITY), 1.0);
        assert_eq!(encode_u8(-0.5), 0);
        assert_eq!(encode_u8(f32::NAN), 0);
        assert_eq!(encode_u8(f32::INFINITY), 255);
        assert_eq!(encode_u8(2.0), 255);
    }

    #[test]
    fn mid_gray_is_188() {
        // Linear 0.5 encodes to 188/255 (well-known sRGB value).
        assert_eq!(encode_u8(0.5), 188);
        // decode(188/255) is linear ~0.504 (the curve is asymmetric); what
        // must hold is the round-trip through the grid value.
        let lin = decode_u8(188);
        assert!(lin > 0.5 && lin < 0.51, "decode(188) = {lin}");
        assert_eq!(encode_u8(lin), 188);
    }

    #[test]
    fn luma_coefficients_sum_to_one() {
        let white = luma([1.0, 1.0, 1.0, 1.0]);
        assert!((white - 1.0).abs() < 1e-6);
        let black = luma([0.0, 0.0, 0.0, 1.0]);
        assert_eq!(black, 0.0);
    }

    #[test]
    fn decode_monotone() {
        let mut prev = -1.0f32;
        for i in 0..=1000 {
            let x = i as f32 / 1000.0;
            let d = decode(x);
            assert!(d >= prev, "decode not monotone at {x}");
            prev = d;
        }
    }
}
