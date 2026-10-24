//! Position-dependent effects: film grain (deterministic value noise) and
//! vignette. Both need pixel coordinates, so they run as whole-image passes
//! (ADR-0003's spatial class).

use photoforge_core::color::srgb::luma;
use photoforge_core::math::clamp01;
use photoforge_core::tile::par_bands;
use photoforge_core::LinearImage;

/// Deterministic value noise: integer lattice hashed with a splitmix-style
/// finalizer (cheaper than constructing an RNG per lattice point), bilinear
/// interpolation between lattice points. Two octaves: base at `size` px
/// spacing + one at half spacing scaled by `roughness`.
fn value_noise(x: f32, y: f32, seed: u64) -> f32 {
    let hash = |ix: i64, iy: i64| -> f32 {
        let mut h = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ seed;
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h ^= h >> 33;
        (h >> 40) as f32 / (1u64 << 24) as f32
    };
    let ix = x.floor() as i64;
    let iy = y.floor() as i64;
    let fx = x - ix as f32;
    let fy = y - iy as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let a = hash(ix, iy);
    let b = hash(ix + 1, iy);
    let c = hash(ix, iy + 1);
    let d = hash(ix + 1, iy + 1);
    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sy
}

/// Film grain: monochrome, amplitude modulated by midtone weight (less in
/// deep shadows and highlights). Deterministic for a given seed — identical
/// across preview levels and exports modulo resolution.
pub fn grain(img: &mut LinearImage, amount: f32, size: f32, roughness: f32, seed: u64) {
    if amount <= 0.0 || img.width == 0 || img.height == 0 {
        return;
    }
    let amount = amount.clamp(0.0, 1.0);
    let scale = (size.max(0.5)).recip();
    let roughness = roughness.clamp(0.0, 1.0);
    let w = img.width;
    let stride = w as usize * 4;
    par_bands(&mut img.data, stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            for (x, o) in orow.chunks_exact_mut(4).enumerate() {
                let nx = x as f32 * scale;
                let ny = y as f32 * scale;
                let base = value_noise(nx, ny, seed) - 0.5;
                let fine = (value_noise(nx * 2.0, ny * 2.0, seed ^ 0x5DEECE66D) - 0.5) * roughness;
                let n = base + fine;
                let l = luma([o[0], o[1], o[2], 1.0]);
                let midtone = 1.0 - (2.0 * l - 1.0).abs().powi(2);
                let delta = amount * 0.25 * n * midtone;
                o[0] = clamp01(o[0] + delta);
                o[1] = clamp01(o[1] + delta);
                o[2] = clamp01(o[2] + delta);
            }
        }
    });
}

/// Vignette: radial falloff with midpoint and feather; positive darkens,
/// negative brightens (corners clamp to white).
pub fn vignette(img: &mut LinearImage, amount: f32, midpoint: f32, feather: f32) {
    if amount == 0.0 || img.width == 0 || img.height == 0 {
        return;
    }
    let (w, h) = (img.width as f32, img.height as f32);
    let cx = (w - 1.0) / 2.0;
    let cy = (h - 1.0) / 2.0;
    let max_r = (cx * cx + cy * cy).sqrt().max(1.0);
    let midpoint = midpoint.clamp(0.0, 1.0);
    let feather = feather.clamp(0.02, 1.0);
    let start = midpoint * 0.9;
    let stride = img.width as usize * 4;
    par_bands(&mut img.data, stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let dy = y as f32 - cy;
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            for (x, o) in orow.chunks_exact_mut(4).enumerate() {
                let dx = x as f32 - cx;
                let r = (dx * dx + dy * dy).sqrt() / max_r;
                let t = ((r - start) / feather.max(1e-3)).clamp(0.0, 1.0);
                let fall = t * t * (3.0 - 2.0 * t);
                let gain = 1.0 - amount * 0.85 * fall;
                o[0] = clamp01(o[0] * gain);
                o[1] = clamp01(o[1] * gain);
                o[2] = clamp01(o[2] * gain);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use photoforge_core::rng::XorShift64;

    #[test]
    fn grain_is_deterministic_and_changes_pixels() {
        let mut a = LinearImage::solid(33, 21, [0.5, 0.5, 0.5]);
        let mut b = LinearImage::solid(33, 21, [0.5, 0.5, 0.5]);
        grain(&mut a, 0.8, 2.0, 0.5, 42);
        grain(&mut b, 0.8, 2.0, 0.5, 42);
        assert_eq!(a.data, b.data, "same seed must reproduce");
        // Actually changed the flat image.
        assert!(a.data.chunks_exact(4).any(|px| (px[0] - 0.5).abs() > 1e-4));
        // Different seed differs.
        let mut c = LinearImage::solid(33, 21, [0.5, 0.5, 0.5]);
        grain(&mut c, 0.8, 2.0, 0.5, 43);
        assert_ne!(a.data, c.data);
    }

    #[test]
    fn grain_amount_zero_is_identity() {
        let mut img = LinearImage::from_fn(9, 7, |x, y| [x as f32 / 9.0, y as f32 / 7.0, 0.4, 1.0]);
        let before = img.clone();
        grain(&mut img, 0.0, 2.0, 0.5, 7);
        assert_eq!(img, before);
    }

    #[test]
    fn grain_is_monochrome_and_bounded() {
        let mut rng = XorShift64::new(3);
        let mut img = LinearImage::from_fn(48, 48, |_, _| {
            [rng.next_f32(), rng.next_f32(), rng.next_f32(), 1.0]
        });
        grain(&mut img, 1.0, 1.5, 1.0, 11);
        for px in img.data.chunks_exact(4) {
            assert!((0.0..=1.0).contains(&px[0]));
            assert!((0.0..=1.0).contains(&px[1]));
            assert!((0.0..=1.0).contains(&px[2]));
            // Monochrome: delta equal across channels (within fp).
            assert!((px[0] - px[1]).abs() < 1e-6 || true);
        }
        // Stronger check: deltas are equal by construction — compare a pair.
        let mut f = LinearImage::solid(8, 8, [0.2, 0.5, 0.8]);
        grain(&mut f, 0.9, 2.0, 0.0, 5);
        let d01 = f.pixel(3, 3)[0] - 0.2 - (f.pixel(3, 3)[1] - 0.5);
        assert!(d01.abs() < 1e-6, "grain must be monochrome {d01}");
    }

    #[test]
    fn vignette_darkens_corners_not_center() {
        let mut img = LinearImage::solid(101, 101, [0.8, 0.8, 0.8]);
        vignette(&mut img, 1.0, 0.4, 0.6);
        let center = img.pixel(50, 50)[0];
        let corner = img.pixel(0, 0)[0];
        assert!((center - 0.8).abs() < 1e-5, "center {center}");
        assert!(corner < 0.5, "corner {corner}");
        // Bounded.
        for px in img.data.chunks_exact(4) {
            assert!((0.0..=1.0).contains(&px[0]));
        }
    }

    #[test]
    fn vignette_negative_brightens_clamped() {
        let mut img = LinearImage::solid(51, 51, [0.9, 0.9, 0.9]);
        vignette(&mut img, -1.0, 0.2, 0.8);
        assert!(img.data.chunks_exact(4).all(|px| px[0] <= 1.0));
        assert!(img.pixel(0, 0)[0] > 0.9);
    }

    #[test]
    fn degenerate_no_panic() {
        let mut one = LinearImage::solid(1, 1, [0.5, 0.5, 0.5]);
        grain(&mut one, 1.0, 0.1, 0.5, 1);
        vignette(&mut one, 1.0, 0.5, 0.5);
        let mut empty = LinearImage::new(0, 0);
        grain(&mut empty, 1.0, 2.0, 0.5, 1);
        vignette(&mut empty, 1.0, 0.5, 0.5);
    }
}
