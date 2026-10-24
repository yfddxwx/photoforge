//! Color histograms (256 bins per channel) over 8-bit storage.
//!
//! R, G, B bins count the stored (sRGB-encoded) channel values; the `luma`
//! bins use Rec. 709 luma of the *linear* values, re-encoded to the 8-bit
//! display grid (ADR-0002). Computation is row-parallel via rayon.

use crate::buffer::PixelBuffer;
use crate::color::srgb;

/// RGB + encoded-luma histogram with 256 bins per channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Histogram {
    pub r: [u32; 256],
    pub g: [u32; 256],
    pub b: [u32; 256],
    pub luma: [u32; 256],
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            r: [0; 256],
            g: [0; 256],
            b: [0; 256],
            luma: [0; 256],
        }
    }
}

impl Histogram {
    /// Computes the histogram of a buffer.
    pub fn compute(buf: &PixelBuffer) -> Self {
        use rayon::prelude::*;
        let stride = buf.width() as usize * 4;
        if stride == 0 {
            return Self::default();
        }
        let rows = buf.as_raw().par_chunks_exact(stride);

        let zero = || [0u32; 256];
        let acc = rows.fold(
            || (zero(), zero(), zero(), zero()),
            |(mut hr, mut hg, mut hb, mut hl), row| {
                for px in row.chunks_exact(4) {
                    hr[px[0] as usize] += 1;
                    hg[px[1] as usize] += 1;
                    hb[px[2] as usize] += 1;
                    let l = srgb::encode_u8(srgb::luma([
                        srgb::decode_u8(px[0]),
                        srgb::decode_u8(px[1]),
                        srgb::decode_u8(px[2]),
                        1.0,
                    ]));
                    hl[l as usize] += 1;
                }
                (hr, hg, hb, hl)
            },
        );
        let reduced = acc.reduce(
            || (zero(), zero(), zero(), zero()),
            |mut a, b| {
                for i in 0..256 {
                    a.0[i] += b.0[i];
                    a.1[i] += b.1[i];
                    a.2[i] += b.2[i];
                    a.3[i] += b.3[i];
                }
                a
            },
        );
        Self {
            r: reduced.0,
            g: reduced.1,
            b: reduced.2,
            luma: reduced.3,
        }
    }

    /// Total counted pixels (sum over any channel).
    pub fn total(&self) -> u64 {
        self.r.iter().map(|&c| c as u64).sum()
    }

    /// Highest non-zero bin across channels (for scaling a display).
    pub fn max_bin(&self) -> u32 {
        self.r
            .iter()
            .chain(self.g.iter())
            .chain(self.b.iter())
            .chain(self.luma.iter())
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// The smallest luma bin holding at least `frac` of the cumulative
    /// distribution (e.g. 0.005 for a "black point" auto-suggestion).
    pub fn luma_percentile(&self, frac: f32) -> u8 {
        let total = self.total();
        if total == 0 {
            return 0;
        }
        let target = (total as f32 * frac.clamp(0.0, 1.0)) as u64;
        let mut acc = 0u64;
        for (i, &c) in self.luma.iter().enumerate() {
            acc += c as u64;
            if acc >= target {
                return i as u8;
            }
        }
        255
    }

    /// True when the histogram has no data at all.
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_color_histogram() {
        let buf = PixelBuffer::solid(10, 10, [0, 128, 255, 255]);
        let h = Histogram::compute(&buf);
        assert_eq!(h.total(), 100);
        assert_eq!(h.r[0], 100);
        assert_eq!(h.g[128], 100);
        assert_eq!(h.b[255], 100);
        // Encoded luma of (0, 0.216, 1.0)-ish: strictly between 0 and 255.
        let luma_bin = h.luma.iter().position(|&c| c == 100).unwrap();
        assert!(luma_bin > 0 && luma_bin < 255);
    }

    #[test]
    fn gradient_fills_all_bins_once() {
        // Inline gray gradient (avoid a dev-dependency cycle with fixtures).
        let mut buf = PixelBuffer::new(256, 4);
        for y in 0..4 {
            let row = buf.row_mut(y);
            for (x, px) in row.chunks_exact_mut(4).enumerate() {
                px[0] = x as u8;
                px[1] = x as u8;
                px[2] = x as u8;
                px[3] = 255;
            }
        }
        let h = Histogram::compute(&buf);
        assert_eq!(h.total(), 1024);
        for i in 0..256 {
            assert_eq!(h.r[i], 4, "bin {i}");
            assert_eq!(h.g[i], 4);
            assert_eq!(h.b[i], 4);
        }
        // Gray gradient: luma equals the channel value.
        assert_eq!(h.luma[3], 4);
        assert_eq!(h.luma[250], 4);
    }

    #[test]
    fn max_bin_and_percentile() {
        let mut buf = PixelBuffer::solid(8, 8, [10, 10, 10, 255]);
        for y in 0..2 {
            for x in 0..4 {
                buf.set_pixel(x, y, [250, 250, 250, 255]);
            }
        }
        let h = Histogram::compute(&buf);
        assert_eq!(h.max_bin(), 56);
        // 56 of 64 pixels sit at luma bin 10: the 20th and 5th percentiles
        // are both 10; only past 87.5% do we reach the 250 bin.
        assert_eq!(h.luma_percentile(0.2), 10);
        assert_eq!(h.luma_percentile(0.05), 10);
        assert_eq!(h.luma_percentile(0.9), 250);
        assert_eq!(Histogram::default().luma_percentile(0.5), 0);
    }

    #[test]
    fn empty_buffer() {
        let buf = PixelBuffer::new(0, 0);
        let h = Histogram::compute(&buf);
        assert!(h.is_empty());
        assert_eq!(h.max_bin(), 0);
    }
}
