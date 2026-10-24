//! Classic HSV (hue/saturation/value) over any RGB triple in [0, 1].
//!
//! Used for the HSL eight-zone color adjustment's hue axis (the zone layout
//! users expect: red 0°, orange 30°, yellow 60°, green 120°, aqua 180°,
//! blue 240°, purple ~285°, magenta 315°). Pure math, no domain conversion.

use crate::math::clamp01;

/// Returns `(h, s, v)` with `h` in degrees `[0, 360)`, `s`/`v` in `[0, 1]`.
/// Achromatic input yields `h = 0`.
pub fn rgb_to_hsv(rgb: [f32; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d <= 0.0 || max <= 0.0 {
        return (0.0, 0.0, max.max(0.0));
    }
    let s = d / max;
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    // `rem_euclid(6.0)` already wraps red; clamp guards rounding at 360.
    (h.rem_euclid(360.0), s.clamp(0.0, 1.0), max.clamp(0.0, 1.0))
}

/// Inverse of [`rgb_to_hsv`]. `h` outside [0, 360) wraps; non-finite inputs
/// are sanitized to 0.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = if h.is_finite() {
        h.rem_euclid(360.0)
    } else {
        0.0
    };
    let s = clamp01(s);
    let v = clamp01(v);
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn primary_hues() {
        assert_eq!(rgb_to_hsv([1.0, 0.0, 0.0]).0, 0.0);
        assert!(close(rgb_to_hsv([0.0, 1.0, 0.0]).0, 120.0, 1e-5));
        assert!(close(rgb_to_hsv([0.0, 0.0, 1.0]).0, 240.0, 1e-5));
        assert!(close(rgb_to_hsv([0.0, 1.0, 1.0]).0, 180.0, 1e-5));
        assert!(close(rgb_to_hsv([1.0, 0.0, 1.0]).0, 300.0, 1e-5));
        assert!(close(rgb_to_hsv([1.0, 1.0, 0.0]).0, 60.0, 1e-5));
    }

    #[test]
    fn achromatic_has_zero_saturation() {
        let (h, s, v) = rgb_to_hsv([0.25, 0.25, 0.25]);
        assert_eq!((h, s), (0.0, 0.0));
        assert!(close(v, 0.25, 1e-6));
    }

    #[test]
    fn roundtrip_randomish() {
        let mut x = 0.123f32;
        for _ in 0..1000 {
            x = (x * 1.37 + 0.11).fract();
            let y = (x * 7.7).fract();
            let z = (x * 3.3 + y).fract();
            let rgb = [x, y, z];
            let (h, s, v) = rgb_to_hsv(rgb);
            let back = hsv_to_rgb(h, s, v);
            for i in 0..3 {
                assert!(
                    close(back[i], rgb[i], 1e-5),
                    "roundtrip {rgb:?} -> {back:?}"
                );
            }
        }
    }

    #[test]
    fn non_finite_is_safe() {
        let (h, s, v) = rgb_to_hsv([f32::NAN, 0.5, 0.5]);
        assert!(h.is_finite() && s.is_finite() && v.is_finite());
        let rgb = hsv_to_rgb(f32::NAN, f32::NAN, f32::NAN);
        assert!(rgb.iter().all(|c| c.is_finite()));
        // Hue wrapping.
        assert!(close(rgb_to_hsv(hsv_to_rgb(400.0, 1.0, 1.0)).0, 40.0, 1e-5));
    }
}
