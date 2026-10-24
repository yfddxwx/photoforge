//! Synthetic test-image generators.
//!
//! All test imagery in photoforge is produced by this crate (ADR-0004: no
//! external downloads). The library API is used by the test suites; the
//! `photoforge-fixtures` binary writes the same images to disk for manual
//! inspection and manual GUI testing.

use photoforge_core::PixelBuffer;

/// Horizontal linear gradient black→white (R=G=B), with alpha 255.
pub fn gradient_h(w: u32, h: u32) -> PixelBuffer {
    let mut buf = PixelBuffer::new(w, h);
    for y in 0..h {
        let row = buf.row_mut(y);
        for x in 0..w as usize {
            let v = if w <= 1 {
                0
            } else {
                (x * 255 / (w as usize - 1)) as u8
            };
            row[x * 4] = v;
            row[x * 4 + 1] = v;
            row[x * 4 + 2] = v;
            row[x * 4 + 3] = 255;
        }
    }
    buf
}

/// Vertical linear gradient black→white.
pub fn gradient_v(w: u32, h: u32) -> PixelBuffer {
    let mut buf = PixelBuffer::new(w, h);
    for y in 0..h {
        let v = if h <= 1 {
            0
        } else {
            (y as usize * 255 / (h as usize - 1)) as u8
        };
        let row = buf.row_mut(y);
        for px in row.chunks_exact_mut(4) {
            px[0] = v;
            px[1] = v;
            px[2] = v;
            px[3] = 255;
        }
    }
    buf
}

/// Radial gradient: distance from center, normalized so the farthest corner
/// is white. Useful for vignette/geometry tests.
pub fn radial(w: u32, h: u32) -> PixelBuffer {
    let mut buf = PixelBuffer::new(w, h);
    let cx = (w as f32 - 1.0) / 2.0;
    let cy = (h as f32 - 1.0) / 2.0;
    let max_r = (cx * cx + cy * cy).sqrt().max(1.0);
    for y in 0..h {
        let row = buf.row_mut(y);
        for x in 0..w as usize {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let v = ((dx * dx + dy * dy).sqrt() / max_r * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            row[x * 4] = v;
            row[x * 4 + 1] = v;
            row[x * 4 + 2] = v;
            row[x * 4 + 3] = 255;
        }
    }
    buf
}

/// Solid color.
pub fn solid(w: u32, h: u32, color: photoforge_core::Rgba8) -> PixelBuffer {
    PixelBuffer::solid(w, h, color)
}

/// Uniform random noise (per-channel independent), deterministic for a seed.
pub fn noise(w: u32, h: u32, seed: u64) -> PixelBuffer {
    use photoforge_core::rng::XorShift64;
    let mut rng = XorShift64::new(seed);
    let mut buf = PixelBuffer::new(w, h);
    for y in 0..h {
        let row = buf.row_mut(y);
        for px in row.chunks_exact_mut(4) {
            px[0] = rng.next_u8();
            px[1] = rng.next_u8();
            px[2] = rng.next_u8();
            px[3] = 255;
        }
    }
    buf
}

/// Grayscale step wedge: `steps` evenly spaced levels, black first.
pub fn gray_steps(w: u32, h: u32, steps: u32) -> PixelBuffer {
    assert!(steps >= 2, "need at least 2 steps");
    let mut buf = PixelBuffer::new(w, h);
    let band = w as f32 / steps as f32;
    for y in 0..h {
        let row = buf.row_mut(y);
        for x in 0..w as usize {
            let s = ((x as f32) / band) as usize;
            let s = s.min(steps as usize - 1);
            let v = (s * 255 / (steps as usize - 1)) as u8;
            row[x * 4] = v;
            row[x * 4 + 1] = v;
            row[x * 4 + 2] = v;
            row[x * 4 + 3] = 255;
        }
    }
    buf
}

/// Resolution wedge: vertical bars whose width halves every group, white on
/// black. For eyeballing resampling sharpness/aliasing.
pub fn resolution_wedge(w: u32, h: u32) -> PixelBuffer {
    let mut buf = PixelBuffer::new(w, h);
    for y in 0..h {
        let row = buf.row_mut(y);
        for x in 0..w as usize {
            // Period doubles every 1/8 of the width: coarse left, fine right.
            let t = x as f32 / w as f32;
            let period = (2.0_f32.powf(1.0 + t * 6.0)).max(2.0);
            let v = if ((x as f32 / period) as u32) % 2 == 0 {
                255
            } else {
                0
            };
            row[x * 4] = v;
            row[x * 4 + 1] = v;
            row[x * 4 + 2] = v;
            row[x * 4 + 3] = 255;
        }
    }
    buf
}

/// Zone plate: sinusoidal chirp `(1 + cos(k·r²))/2`, the classic lens/resample
/// stress pattern (produces local frequencies up to Nyquist and beyond).
pub fn zone_plate(w: u32, h: u32) -> PixelBuffer {
    let mut buf = PixelBuffer::new(w, h);
    let cx = (w as f32) / 2.0;
    let cy = (h as f32) / 2.0;
    let k = 0.0006 * (900.0 / (w.max(h) as f32).max(1.0));
    for y in 0..h {
        let row = buf.row_mut(y);
        for x in 0..w as usize {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let v = (1.0 + (k * (dx * dx + dy * dy)).cos()) * 127.5;
            let v = v.clamp(0.0, 255.0) as u8;
            row[x * 4] = v;
            row[x * 4 + 1] = v;
            row[x * 4 + 2] = v;
            row[x * 4 + 3] = 255;
        }
    }
    buf
}

/// 24 sRGB patches loosely modeled on the classic X-Rite ColorChecker
/// (approximate values, good enough as synthetic reference colors).
const COLOR_CHECKER: [[u8; 3]; 24] = [
    [115, 82, 68],   // dark skin
    [194, 150, 130], // light skin
    [98, 122, 157],  // blue sky
    [87, 108, 67],   // foliage
    [133, 128, 177], // blue flower
    [103, 189, 170], // bluish green
    [214, 126, 44],  // orange
    [80, 91, 166],   // purplish blue
    [193, 90, 99],   // moderate red
    [94, 60, 108],   // purple
    [157, 188, 64],  // yellow green
    [224, 163, 46],  // orange yellow
    [56, 61, 150],   // blue
    [70, 148, 73],   // green
    [175, 54, 60],   // red
    [231, 199, 31],  // yellow
    [187, 86, 149],  // magenta
    [8, 133, 161],   // cyan
    [243, 243, 242], // white
    [200, 200, 200], // neutral 8
    [160, 160, 160], // neutral 6.5
    [122, 122, 121], // neutral 5
    [85, 85, 85],    // neutral 3.5
    [52, 52, 52],    // black
];

/// Synthetic test chart (1200×900):
/// - top half: 6×4 color-checker patches;
/// - bottom-left: 16-step grayscale wedge;
/// - bottom-middle: resolution wedge;
/// - bottom-right: zone plate + noise.
pub fn test_card() -> PixelBuffer {
    let mut card = PixelBuffer::solid(1200, 900, [24, 24, 28, 255]);
    // Color checker: 6 columns x 4 rows in the top 600 rows.
    let pw: u32 = 200; // 6 * 200 = 1200
    let ph: u32 = 150; // 4 * 150 = 600
    for (i, rgb) in COLOR_CHECKER.iter().enumerate() {
        let col = i % 6;
        let row = i / 6;
        let patch = PixelBuffer::solid(pw, ph, [rgb[0], rgb[1], rgb[2], 255]);
        card.paste(&patch, col as i64 * pw as i64, row as i64 * ph as i64);
    }
    // Gray wedge: 16 steps over the left bottom quadrant.
    let wedge = gray_steps(500, 300, 16);
    card.paste(&wedge, 0, 600);
    // Resolution wedge.
    let wedge = resolution_wedge(350, 300);
    card.paste(&wedge, 500, 600);
    // Zone plate.
    let zp = zone_plate(200, 300);
    card.paste(&zp, 850, 600);
    // Noise.
    let n = noise(150, 300, 0xC0FFEE);
    card.paste(&n, 1050, 600);
    card
}

/// Writes a PNG (for the fixture binary and manual inspection).
pub fn write_png(path: &std::path::Path, buf: &PixelBuffer) -> std::io::Result<()> {
    let img = image::RgbaImage::from_raw(buf.width(), buf.height(), buf.as_raw().to_vec())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad buffer"))?;
    img.save_with_format(path, image::ImageFormat::Png)
        .map_err(std::io::Error::other)?;
    Ok(())
}

/// Generates the standard fixture set into `dir`, returning written paths.
pub fn generate_all(dir: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let specs: Vec<(&str, PixelBuffer)> = vec![
        ("gradient_h_256x128.png", gradient_h(256, 128)),
        ("gradient_v_128x256.png", gradient_v(128, 256)),
        ("radial_256x256.png", radial(256, 256)),
        ("solid_red_64x64.png", solid(64, 64, [200, 30, 30, 255])),
        ("noise_256x256.png", noise(256, 256, 42)),
        ("gray_steps_512x64.png", gray_steps(512, 64, 16)),
        ("resolution_wedge_512x128.png", resolution_wedge(512, 128)),
        ("zone_plate_256x256.png", zone_plate(256, 256)),
        ("test_card_1200x900.png", test_card()),
    ];
    let mut paths = Vec::new();
    for (name, buf) in specs {
        let p = dir.join(name);
        write_png(&p, &buf)?;
        paths.push(p);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_h_endpoints_and_monotone() {
        let g = gradient_h(256, 8);
        assert_eq!(g.pixel(0, 0)[0], 0);
        assert_eq!(g.pixel(255, 0)[0], 255);
        // Non-decreasing along the row.
        let row = g.row(0);
        for x in 1..256 {
            assert!(row[x * 4] >= row[(x - 1) * 4]);
        }
    }

    #[test]
    fn gradient_v_endpoints() {
        let g = gradient_v(8, 100);
        assert_eq!(g.pixel(0, 0)[0], 0);
        assert_eq!(g.pixel(0, 99)[0], 255);
    }

    #[test]
    fn radial_center_dark_corner_bright() {
        let g = radial(101, 101);
        assert!(g.pixel(50, 50)[0] < 10);
        assert!(g.pixel(0, 0)[0] > 250);
    }

    #[test]
    fn noise_is_deterministic_and_varied() {
        let a = noise(32, 32, 7);
        let b = noise(32, 32, 7);
        let c = noise(32, 32, 8);
        assert_eq!(a.as_raw(), b.as_raw());
        assert_ne!(a.as_raw(), c.as_raw());
        // Actually noisy: many distinct values in a row.
        let mut set = std::collections::HashSet::new();
        for px in a.row(0).chunks_exact(4) {
            set.insert(px[0]);
        }
        assert!(set.len() > 10, "expected variety, got {set:?}");
    }

    #[test]
    fn gray_steps_values() {
        let g = gray_steps(400, 10, 5);
        assert_eq!(g.pixel(0, 0)[0], 0);
        assert_eq!(g.pixel(399, 0)[0], 255);
        // x=200 falls in step 2 of [0..5): 2*255/4 = 127.
        assert_eq!(g.pixel(200, 0)[0], 127);
        // Step boundaries are stable across the band.
        assert_eq!(g.pixel(0, 0)[0], g.pixel(79, 0)[0]);
        assert_eq!(g.pixel(80, 0)[0], g.pixel(159, 0)[0]);
    }

    #[test]
    fn test_card_size() {
        let c = test_card();
        assert_eq!((c.width(), c.height()), (1200, 900));
        // First checker patch is dark skin.
        assert_eq!(c.pixel(10, 10), [115, 82, 68, 255]);
    }
}
