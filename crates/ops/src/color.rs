//! Chroma adjustments in OKLab (vibrance, saturation).

use photoforge_core::color::oklab::{linear_srgb_to_oklab, oklab_to_linear_srgb};
use photoforge_core::math::clamp01;
use photoforge_core::Rgba32F;

/// Saturation: scales OKLab chroma by `1 + amount`. At −1 the result is an
/// exact neutral gray (a = b = 0); at 0 bit-exact identity.
#[inline]
pub fn saturation_pixel(px: Rgba32F, amount: f32) -> Rgba32F {
    if amount == 0.0 {
        return px;
    }
    let lab = linear_srgb_to_oklab([px[0], px[1], px[2]]);
    let out = oklab_to_linear_srgb(photoforge_core::color::oklab::Oklab {
        l: lab.l,
        a: lab.a * (1.0 + amount),
        b: lab.b * (1.0 + amount),
    });
    [clamp01(out[0]), clamp01(out[1]), clamp01(out[2]), px[3]]
}

/// Vibrance: chroma boost weighted inversely by existing chroma, so muted
/// colors move more and saturated ones are protected. Grays stay exact.
#[inline]
pub fn vibrance_pixel(px: Rgba32F, amount: f32) -> Rgba32F {
    if amount == 0.0 {
        return px;
    }
    let lab = linear_srgb_to_oklab([px[0], px[1], px[2]]);
    let chroma = lab.chroma();
    // Protection: full effect for chroma 0, none at/above ~0.35.
    let weight = (1.0 - chroma / 0.35).clamp(0.0, 1.0);
    let factor = 1.0 + amount * 1.2 * weight;
    let out = oklab_to_linear_srgb(photoforge_core::color::oklab::Oklab {
        l: lab.l,
        a: lab.a * factor,
        b: lab.b * factor,
    });
    [clamp01(out[0]), clamp01(out[1]), clamp01(out[2]), px[3]]
}
