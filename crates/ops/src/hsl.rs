//! Eight-zone HSL color adjustment.
//!
//! Zones sit on the classic hue wheel (red 0°, orange 30°, yellow 60°,
//! green 120°, aqua 180°, blue 240°, purple 285°, magenta 315°) with
//! raised-cosine weights of half-width 45° (wrapped around red). Hue/sat
//! operate on the HSV model over sRGB-encoded values; luminance scales the
//! encoded value. All sliders at zero = exact identity.

use photoforge_core::color::hsv::{hsv_to_rgb, rgb_to_hsv};
use photoforge_core::color::srgb;
use photoforge_core::math::clamp01;
use photoforge_core::Rgba32F;

/// Zone centers in degrees, in display order.
pub const ZONE_CENTERS: [f32; 8] = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 285.0, 315.0];

pub const ZONE_NAMES: [&str; 8] = [
    "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
];

const HALF_WIDTH: f32 = 45.0;

/// Per-zone adjustments (hue in degrees, sat/lum in −1..1) applied to a
/// linear-light pixel: encode → HSV zone math → decode.
#[inline]
pub fn hsl_pixel_rgb(px: Rgba32F, zones: &[[f32; 3]; 8]) -> Rgba32F {
    let any_effect = zones
        .iter()
        .any(|z| z[0] != 0.0 || z[1] != 0.0 || z[2] != 0.0);
    if !any_effect {
        return px;
    }
    let e = [
        srgb::encode_continuous(px[0]),
        srgb::encode_continuous(px[1]),
        srgb::encode_continuous(px[2]),
    ];
    let out = hsl_encoded(e, zones);
    [
        srgb::decode(out[0]),
        srgb::decode(out[1]),
        srgb::decode(out[2]),
        px[3],
    ]
}

/// Zone math on encoded values (public for tests).
#[inline]
pub fn hsl_encoded(rgb_encoded: [f32; 3], zones: &[[f32; 3]; 8]) -> [f32; 3] {
    let (mut h, mut s, mut v) = rgb_to_hsv(rgb_encoded);
    if s <= 0.0 {
        return rgb_encoded; // grays are unaffected
    }
    let mut hue_shift = 0.0f32;
    let mut sat_mult = 0.0f32;
    let mut lum_mult = 0.0f32;
    for (i, z) in zones.iter().enumerate() {
        let d = (h - ZONE_CENTERS[i])
            .abs()
            .min(360.0 - (h - ZONE_CENTERS[i]).abs());
        if d >= HALF_WIDTH {
            continue;
        }
        // Raised cosine weight: 1 at the center, 0 at ±half-width.
        let t = 0.5 * (1.0 + (std::f32::consts::PI * d / HALF_WIDTH).cos());
        hue_shift += z[0] * t;
        sat_mult += z[1] * t;
        lum_mult += z[2] * t;
    }
    h = (h + hue_shift).rem_euclid(360.0);
    s = clamp01(s * (1.0 + sat_mult));
    v = clamp01(v * (1.0 + lum_mult * 0.6));
    hsv_to_rgb(h, s, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zones_all(v: [f32; 3]) -> [[f32; 3]; 8] {
        [[v[0], v[1], v[2]]; 8]
    }

    #[test]
    fn identity_when_all_zero() {
        // Encoded-domain roundtrip through HSV drifts ~1e-6; the pipeline
        // guarantees bit-exact identity via the early returns in
        // hsl_pixel_rgb and OpParams::is_identity.
        for rgb in [[1.0, 0.0, 0.0], [0.2, 0.7, 0.4], [0.5, 0.5, 0.5]] {
            let out = hsl_encoded(rgb, &zones_all([0.0, 0.0, 0.0]));
            for c in 0..3 {
                assert!((out[c] - rgb[c]).abs() < 1e-5, "{rgb:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn red_zone_boosts_red_saturation() {
        // Pure red (hue 0): red-zone sat +0.5 increases saturation — but it
        // is already 1.0; use a desaturated red instead.
        let rgb = [0.8f32, 0.3, 0.3];
        let mut zones = zones_all([0.0, 0.0, 0.0]);
        zones[0] = [0.0, 0.8, 0.0];
        let out = hsl_encoded(rgb, &zones);
        // Saturation growth: the dominant channel stays, the others sink.
        assert!(out[1] < rgb[1] && out[2] < rgb[2], "{rgb:?} -> {out:?}");
        assert!(rgb_to_hsv(out).1 > rgb_to_hsv(rgb).1);
        // Green zone must not touch red.
        let mut zones = zones_all([0.0, 0.0, 0.0]);
        zones[3] = [0.0, 0.9, 0.0];
        assert_eq!(hsl_encoded(rgb, &zones), rgb);
    }

    #[test]
    fn hue_shift_wraps_around_red() {
        // Shift red all the way to magenta (−60° lands at 300°).
        let red = [1.0f32, 0.0, 0.0];
        let mut zones = zones_all([0.0, 0.0, 0.0]);
        zones[0] = [-60.0, 0.0, 0.0];
        let out = hsl_encoded(red, &zones);
        let (h, s, _v) = rgb_to_hsv(out);
        assert!((s - 1.0).abs() < 1e-4);
        assert!((h - 300.0).abs() < 1.0, "expected ~300°, got {h}");
    }

    #[test]
    fn weight_overlaps_between_zones() {
        // Hue 15° sits between red and orange: both zones act.
        let rgb = hsv_to_rgb(15.0, 0.8, 0.9);
        let mut zones = zones_all([0.0, 0.0, 0.0]);
        zones[0] = [0.0, -1.0, 0.0]; // red desaturate
        let out_red_only = hsl_encoded(rgb, &zones);
        zones[1] = [0.0, -1.0, 0.0]; // add orange
        let out_both = hsl_encoded(rgb, &zones);
        let s_red = rgb_to_hsv(out_red_only).1;
        let s_both = rgb_to_hsv(out_both).1;
        assert!(s_both < s_red, "overlapping zones should compound");
    }

    #[test]
    fn grays_untouched() {
        let mut zones = zones_all([30.0, 0.9, 0.9]);
        zones[3] = [-90.0, -0.5, -0.5];
        assert_eq!(hsl_encoded([0.4, 0.4, 0.4], &zones), [0.4, 0.4, 0.4]);
    }

    #[test]
    fn luminance_scales_value() {
        let rgb = hsv_to_rgb(30.0, 0.5, 0.8);
        let mut zones = zones_all([0.0, 0.0, 0.0]);
        zones[1] = [0.0, 0.0, 1.0]; // orange lum +
        let out = hsl_encoded(rgb, &zones);
        assert!(rgb_to_hsv(out).2 > 0.8, "v {}", rgb_to_hsv(out).2);
        zones[1] = [0.0, 0.0, -1.0];
        let out = hsl_encoded(rgb, &zones);
        assert!(rgb_to_hsv(out).2 < 0.8);
    }
}
