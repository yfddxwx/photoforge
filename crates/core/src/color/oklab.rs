//! OKLab perceptual color space (Björn Ottosson, 2020,
//! <https://bottosson.github.io/posts/oklab/>).
//!
//! Operates on *linear* sRGB triples. Used for vibrance/saturation (chroma
//! scaling without hue shifts) and perceptual masks. Matrices are the ones
//! published with the reference implementation (public domain / CC0).

// The published matrix constants are kept at full written precision.
#![allow(clippy::excessive_precision)]

/// A color in OKLab: `l` (lightness, 0..1 for sRGB gamut), `a`, `b` (chroma,
/// roughly −0.4..0.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

impl Oklab {
    /// Chroma magnitude √(a² + b²).
    #[inline]
    pub fn chroma(self) -> f32 {
        (self.a * self.a + self.b * self.b).sqrt()
    }

    /// Hue angle in degrees [0, 360); 0 for achromatic.
    #[inline]
    pub fn hue_deg(self) -> f32 {
        if self.a == 0.0 && self.b == 0.0 {
            return 0.0;
        }
        self.b.atan2(self.a).to_degrees().rem_euclid(360.0)
    }
}

#[inline]
fn cbrt_signed(x: f32) -> f32 {
    // f32::cbrt is sign-preserving for negative inputs, which is what the
    // OKLab pipeline needs for slightly out-of-gamut intermediates.
    x.cbrt()
}

/// Converts a linear-light sRGB triple to OKLab.
pub fn linear_srgb_to_oklab(rgb: [f32; 3]) -> Oklab {
    let [r, g, b] = rgb;

    let l = 0.412_221_47_f32.mul_add(r, 0.536_332_55_f32.mul_add(g, 0.051_445_993_f32 * b));
    let m = 0.211_903_5_f32.mul_add(r, 0.680_699_55_f32.mul_add(g, 0.107_396_96_f32 * b));
    let s = 0.088_302_46_f32.mul_add(r, 0.281_718_85_f32.mul_add(g, 0.629_978_7_f32 * b));

    let l_ = cbrt_signed(l);
    let m_ = cbrt_signed(m);
    let s_ = cbrt_signed(s);

    Oklab {
        l: 0.210_454_26_f32.mul_add(l_, 0.793_617_8_f32.mul_add(m_, -0.004_072_047_f32 * s_)),
        a: 1.977_998_5_f32.mul_add(l_, (-2.428_592_2_f32).mul_add(m_, 0.450_593_7_f32 * s_)),
        b: 0.025_904_037_f32.mul_add(l_, 0.782_771_77_f32.mul_add(m_, -0.808_675_77_f32 * s_)),
    }
}

/// Converts OKLab back to a linear-light sRGB triple.
pub fn oklab_to_linear_srgb(c: Oklab) -> [f32; 3] {
    let l_ = c.l + 0.396_337_78_f32 * c.a + 0.215_803_76_f32 * c.b;
    let m_ = c.l - 0.105_561_346_f32 * c.a - 0.063_854_17_f32 * c.b;
    let s_ = c.l - 0.089_484_18_f32 * c.a - 1.291_485_5_f32 * c.b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    [
        4.076_741_7_f32.mul_add(l, (-3.307_711_6_f32).mul_add(m, 0.230_969_93_f32 * s)),
        (-1.268_438_f32).mul_add(l, 2.609_757_4_f32.mul_add(m, -0.341_319_4_f32 * s)),
        (-0.004_196_086_3_f32).mul_add(l, (-0.703_418_6_f32).mul_add(m, 1.707_614_7_f32 * s)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn white_maps_to_l_one_zero_chroma() {
        let c = linear_srgb_to_oklab([1.0, 1.0, 1.0]);
        assert!(close(c.l, 1.0, 1e-5), "l = {}", c.l);
        assert!(close(c.a, 0.0, 1e-5), "a = {}", c.a);
        assert!(close(c.b, 0.0, 1e-5), "b = {}", c.b);
        let black = linear_srgb_to_oklab([0.0, 0.0, 0.0]);
        assert_eq!(black.l, 0.0);
        assert_eq!(black.a, 0.0);
        assert_eq!(black.b, 0.0);
    }

    #[test]
    fn grays_have_zero_chroma() {
        for i in 1..100 {
            let v = i as f32 / 100.0;
            let c = linear_srgb_to_oklab([v, v, v]);
            assert!(c.chroma() < 1e-5, "gray {v} has chroma {}", c.chroma());
        }
    }

    #[test]
    fn roundtrip_is_tight() {
        let mut x = 0.31f32;
        for _ in 0..2000 {
            x = (x * 1.31 + 0.07).fract();
            let y = (x * 5.7).fract();
            let z = (x * 2.9 + y).fract();
            let rgb = [x, y, z];
            let lab = linear_srgb_to_oklab(rgb);
            let back = oklab_to_linear_srgb(lab);
            for i in 0..3 {
                assert!(
                    close(back[i], rgb[i], 1e-4),
                    "roundtrip {rgb:?} -> {back:?}"
                );
            }
        }
    }

    #[test]
    fn red_is_positive_a_axis() {
        let c = linear_srgb_to_oklab([1.0, 0.0, 0.0]);
        assert!(c.a > 0.15, "red a = {}", c.a);
        let blue = linear_srgb_to_oklab([0.0, 0.0, 1.0]);
        assert!(blue.b < -0.05, "blue b = {}", blue.b);
    }

    #[test]
    fn out_of_gamut_stays_finite() {
        // Negative and >1 intermediates must not produce NaN.
        for rgb in [[-0.5f32, 1.5, 0.2], [2.0, -2.0, 2.0]] {
            let lab = linear_srgb_to_oklab(rgb);
            let back = oklab_to_linear_srgb(lab);
            assert!(lab.l.is_finite() && lab.a.is_finite() && lab.b.is_finite());
            assert!(back.iter().all(|v| v.is_finite()), "{rgb:?}");
        }
        // NaN input propagates in the pure math layer (no panic); the ops
        // layer guarantees sanitized inputs upstream (ADR-0002).
        let lab = linear_srgb_to_oklab([f32::NAN, 0.0, 0.0]);
        assert!(lab.l.is_nan()); // propagation, not a panic
    }
}
