//! Levels: black point, gamma (midpoint), white point on the sRGB-encoded
//! domain (ADR-0002).
//!
//! `out = ((e − black) / (white − black))^(1/gamma)`, clamped — the classic
//! Photoshop-style levels mapping.

use photoforge_core::color::srgb;
use photoforge_core::Rgba32F;

/// Evaluates levels on an encoded value in [0, 1].
#[inline]
pub fn levels_encoded(e: f32, black: f32, gamma: f32, white: f32) -> f32 {
    let span = white - black;
    if span.abs() < 1e-4 {
        return e; // degenerate: leave unchanged rather than blow up
    }
    let t = ((e - black) / span).clamp(0.0, 1.0);
    if (gamma - 1.0).abs() < 1e-6 {
        return t;
    }
    t.powf(1.0 / gamma)
}

/// Applies master levels to a linear-light pixel (encode → map → decode).
#[inline]
pub fn levels_pixel(px: Rgba32F, black: f32, gamma: f32, white: f32) -> Rgba32F {
    if black == 0.0 && gamma == 1.0 && white == 1.0 {
        return px;
    }
    let e = [
        srgb::encode_continuous(px[0]),
        srgb::encode_continuous(px[1]),
        srgb::encode_continuous(px[2]),
    ];
    [
        srgb::decode(levels_encoded(e[0], black, gamma, white)),
        srgb::decode(levels_encoded(e[1], black, gamma, white)),
        srgb::decode(levels_encoded(e[2], black, gamma, white)),
        px[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_at_defaults() {
        for i in 0..=100 {
            let e = i as f32 / 100.0;
            assert_eq!(levels_encoded(e, 0.0, 1.0, 1.0), e);
        }
    }

    #[test]
    fn black_point_clips_to_zero() {
        assert_eq!(levels_encoded(0.05, 0.1, 1.0, 1.0), 0.0);
        assert!((levels_encoded(0.1, 0.1, 1.0, 1.0)).abs() < 1e-6);
        assert!(levels_encoded(0.5, 0.1, 1.0, 1.0) > (0.5 - 0.1) / 0.9 - 1e-5);
    }

    #[test]
    fn white_point_clips_to_one() {
        assert_eq!(levels_encoded(0.95, 0.0, 1.0, 0.9), 1.0);
        assert!((levels_encoded(0.9, 0.0, 1.0, 0.9) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn gamma_is_a_midpoint_move() {
        // gamma 2 lifts encoded 0.25 to 0.5.
        assert!((levels_encoded(0.25, 0.0, 2.0, 1.0) - 0.5).abs() < 1e-5);
        // gamma 0.5 pushes 0.5 down to 0.25.
        assert!((levels_encoded(0.5, 0.0, 0.5, 1.0) - 0.25).abs() < 1e-5);
        // Gamma lifts the midtone: 0.5 with gamma 3 → 0.5^(1/3) ≈ 0.794.
        assert!((levels_encoded(0.5, 0.0, 3.0, 1.0) - 0.5f32.powf(1.0 / 3.0)).abs() < 1e-5);
    }

    #[test]
    fn monotone() {
        let mut prev = -1.0;
        for i in 0..=256 {
            let v = levels_encoded(i as f32 / 255.0, 0.08, 1.4, 0.94);
            assert!(v >= prev - 1e-6, "levels inverted at {i}");
            prev = v;
        }
    }

    #[test]
    fn degenerate_span_leaves_input() {
        assert_eq!(levels_encoded(0.3, 0.5, 1.0, 0.5), 0.3);
    }
}
