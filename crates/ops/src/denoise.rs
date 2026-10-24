//! Denoise: joint bilateral on luma (edge-preserving) + 3×3 median on the
//! chroma residual (strength blends both corrections additively).
//!
//! Performance notes: the 5×5 spatial weights are precomputed per kernel
//! tap; the range weight `exp(−ΔL²/2σr²)` uses a 4096-entry LUT.

use photoforge_core::color::srgb::luma;
use photoforge_core::tile::par_bands;
use photoforge_core::LinearImage;

const RADIUS: i32 = 2; // 5×5 window
const SIGMA_S: f32 = 1.5;

const RANGE_LUT_N: usize = 4096;

/// Applies denoise. `strength` 0..1 scales both σr and the correction mix.
pub fn denoise(img: &mut LinearImage, strength: f32) {
    if strength <= 0.0 || img.width == 0 || img.height == 0 {
        return;
    }
    let strength = strength.clamp(0.0, 1.0);
    let sigma_r = 0.02 + strength * 0.10;
    let (w, h) = (img.width, img.height);

    // Luma plane.
    let mut luma_plane = vec![0.0f32; (w * h) as usize];
    for (i, px) in img.data.chunks_exact(4).enumerate() {
        luma_plane[i] = luma([px[0], px[1], px[2], 1.0]);
    }
    let luma_src = luma_plane.clone();

    // Precompute spatial weights and the range LUT.
    let mut spatial = [[0.0f32; 5]; 5];
    for dy in -RADIUS..=RADIUS {
        for dx in -RADIUS..=RADIUS {
            spatial[(dy + RADIUS) as usize][(dx + RADIUS) as usize] =
                (-((dx * dx + dy * dy) as f32) / (2.0 * SIGMA_S * SIGMA_S)).exp();
        }
    }
    let mut range_lut = [0.0f32; RANGE_LUT_N];
    for (i, slot) in range_lut.iter_mut().enumerate() {
        let d2 = (i as f32 / (RANGE_LUT_N - 1) as f32).powi(2); // |ΔL|² in [0,1]
        *slot = (-d2 / (2.0 * sigma_r * sigma_r)).exp();
    }
    let range_weight = |dl: f32| -> f32 {
        let i = (dl.abs().clamp(0.0, 1.0) * (RANGE_LUT_N - 1) as f32) as usize;
        range_lut[i]
    };

    let stride = w as usize * 4;
    let src = img.data.clone();
    let luma_ref = &luma_src;
    let chroma_out = img.data.clone();

    par_bands(&mut img.data, stride, 32, |y0, y1, band| {
        for y in y0..y1 {
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            for x in 0..w as usize {
                let idx = y as usize * w as usize + x;
                let l0 = luma_ref[idx];
                // Bilateral on luma.
                let mut num = 0.0f32;
                let mut den = 0.0f32;
                for dy in -RADIUS..=RADIUS {
                    let sy = (y as i64 + dy as i64).clamp(0, h as i64 - 1) as usize;
                    for dx in -RADIUS..=RADIUS {
                        let sx = (x as i64 + dx as i64).clamp(0, w as i64 - 1) as usize;
                        let wgt = spatial[(dy + RADIUS) as usize][(dx + RADIUS) as usize]
                            * range_weight(luma_ref[sy * w as usize + sx] - l0);
                        num += wgt * luma_ref[sy * w as usize + sx];
                        den += wgt;
                    }
                }
                let l_bilateral = num / den.max(1e-6);

                // Chroma: 3×3 median of the per-channel residual (c − luma).
                let mut med = [0.0f32; 3];
                for c in 0..3 {
                    let mut vals = [0.0f32; 9];
                    let mut n = 0;
                    for dy in -1i32..=1 {
                        let sy = (y as i64 + dy as i64).clamp(0, h as i64 - 1) as usize;
                        for dx in -1i32..=1 {
                            let sx = (x as i64 + dx as i64).clamp(0, w as i64 - 1) as usize;
                            let j = sy * w as usize + sx;
                            let px = &chroma_out[j * 4..j * 4 + 4];
                            vals[n] = px[c] - luma_ref[j];
                            n += 1;
                        }
                    }
                    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    med[c] = vals[4];
                }

                // Additive correction: bilateral luma delta + medianed chroma.
                let luma_delta = strength * (l_bilateral - l0);
                let base = y as usize * stride + x * 4;
                let s = &src[base..base + 4];
                for c in 0..3 {
                    let chroma = s[c] - l0;
                    let chroma_med = med[c];
                    let v = s[c] + luma_delta + strength * (chroma_med - chroma);
                    orow[x * 4 + c] = v.clamp(0.0, 1.0);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use photoforge_core::rng::XorShift64;

    #[test]
    fn constant_image_is_stable() {
        let mut img = LinearImage::solid(23, 17, [0.4, 0.35, 0.3]);
        let before = img.clone();
        denoise(&mut img, 1.0);
        for (a, b) in img.data.iter().zip(before.data.iter()) {
            assert!((a - b).abs() < 1e-4, "{a} vs {b}");
        }
    }

    #[test]
    fn noise_is_reduced_edges_kept() {
        // Flat gray + noise, with a hard edge somewhere.
        let mut rng = XorShift64::new(9);
        let mut img = LinearImage::from_fn(64, 64, |x, _| {
            let base = if x < 32 { 0.2 } else { 0.8 };
            let n = (rng.next_f32() - 0.5) * 0.15;
            [
                (base + n).clamp(0.0, 1.0),
                (base + n).clamp(0.0, 1.0),
                (base + n).clamp(0.0, 1.0),
                1.0,
            ]
        });
        // Measure noise away from the edge (x in 8..24).
        let noise_before: f32 = (8..24)
            .map(|x| (img.pixel(x, 30)[0] - 0.2).abs())
            .sum::<f32>()
            / 16.0;
        denoise(&mut img, 0.9);
        let noise_after: f32 = (8..24)
            .map(|x| (img.pixel(x, 30)[0] - 0.2).abs())
            .sum::<f32>()
            / 16.0;
        assert!(
            noise_after < noise_before * 0.7,
            "noise {noise_before} -> {noise_after}"
        );
        // The hard edge must survive (no smearing across it at x=31 vs 32).
        let left = img.pixel(29, 30)[0];
        let right = img.pixel(34, 30)[0];
        assert!(right - left > 0.4, "edge survived: {left} {right}");
    }

    #[test]
    fn strength_zero_is_exact_identity() {
        let mut img = LinearImage::from_fn(11, 5, |x, y| {
            [(x as f32 / 11.0), (y as f32 / 5.0), 0.5, 1.0]
        });
        let before = img.clone();
        denoise(&mut img, 0.0);
        assert_eq!(img, before);
    }

    #[test]
    fn degenerate_no_panic() {
        let mut one = LinearImage::solid(1, 1, [0.5, 0.5, 0.5]);
        denoise(&mut one, 1.0);
        let mut empty = LinearImage::new(0, 0);
        denoise(&mut empty, 1.0);
    }
}
