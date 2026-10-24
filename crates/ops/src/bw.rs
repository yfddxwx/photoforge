//! Black & white conversion (channel mixer) and duotone mapping.

use photoforge_core::color::srgb;
use photoforge_core::math::{clamp01, lerp};
use photoforge_core::Rgba32F;

/// Rec. 709 defaults for the B&W channel mixer (ADR-0002 luma weights).
pub const BW_DEFAULTS: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Channel-mixer black & white: gray = normalized weighted mix of linear
/// channels. `weights` are raw mixer values (any sign, like Lightroom's
/// sliders); they are normalized by their sum (guarded against 0).
#[inline]
pub fn black_white_pixel(px: Rgba32F, weights: [f32; 3]) -> Rgba32F {
    let sum = weights[0] + weights[1] + weights[2];
    let w = if sum.abs() < 1e-4 {
        BW_DEFAULTS
    } else {
        [weights[0] / sum, weights[1] / sum, weights[2] / sum]
    };
    let gray = clamp01(w[0] * px[0] + w[1] * px[1] + w[2] * px[2]);
    [gray, gray, gray, px[3]]
}

/// Duotone: maps luma to a gradient between two colors (given in linear
/// light; `0..1` per channel). Black/white endpoints reproduce plain B&W.
#[inline]
pub fn duotone_pixel(px: Rgba32F, shadow: [f32; 3], highlight: [f32; 3]) -> Rgba32F {
    let t = clamp01(srgb::luma(px));
    [
        clamp01(lerp(shadow[0], highlight[0], t)),
        clamp01(lerp(shadow[1], highlight[1], t)),
        clamp01(lerp(shadow[2], highlight[2], t)),
        px[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bw_defaults_match_luma_on_grays() {
        for v in [0.1f32, 0.35, 0.7] {
            let px = [v, v, v, 1.0];
            let out = black_white_pixel(px, BW_DEFAULTS);
            // The coefficient sum is 1.0 up to one f32 ulp.
            assert!((out[0] - v).abs() < 1e-6, "{v} -> {out:?}");
            assert_eq!(out[1], out[0]);
            assert_eq!(out[2], out[0]);
        }
    }

    #[test]
    fn bw_weights_normalize() {
        // Doubling all weights must not change the result.
        let px = [0.6f32, 0.3, 0.1, 1.0];
        let a = black_white_pixel(px, [0.5, 0.4, 0.1]);
        let b = black_white_pixel(px, [1.0, 0.8, 0.2]);
        assert_eq!(a[0], b[0]);
        // Gray input stays gray under any weights.
        let g = black_white_pixel([0.5, 0.5, 0.5, 1.0], [2.0, -1.0, 0.3]);
        assert_eq!(g[0], g[1]);
        assert_eq!(g[1], g[2]);
    }

    #[test]
    fn bw_zero_weights_fall_back_to_luma() {
        let px = [0.2f32, 0.4, 0.6, 1.0];
        let out = black_white_pixel(px, [0.0, 0.0, 0.0]);
        assert_eq!(out[0], out[1]);
        assert_eq!(out[1], out[2]);
        assert!(out[0] > 0.0 && out[0] < 1.0);
    }

    #[test]
    fn duotone_black_white_is_grayscale() {
        let px = [0.7f32, 0.25, 0.4, 1.0];
        let out = duotone_pixel(px, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        assert_eq!(out[0], out[1]);
        assert_eq!(out[1], out[2]);
        // Equals the luma-mapped gray.
        let t = clamp01(srgb::luma(px));
        assert!((out[0] - t).abs() < 1e-6);
    }

    #[test]
    fn duotone_tinting() {
        let px = [0.5f32, 0.5, 0.5, 1.0];
        let sepia = duotone_pixel(px, [0.05, 0.03, 0.0], [1.0, 0.9, 0.7]);
        assert!(sepia[0] > sepia[2], "sepia should be warm");
        // Endpoints pin the colors.
        let black = duotone_pixel([0.0, 0.0, 0.0, 1.0], [0.1, 0.2, 0.3], [1.0, 1.0, 1.0]);
        assert_eq!(black[0], 0.1);
        let white = duotone_pixel([1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 0.0], [0.9, 0.85, 0.8]);
        assert_eq!(white[2], 0.8);
    }
}
