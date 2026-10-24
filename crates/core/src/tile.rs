//! Tile-parallel framework (ADR-0003).
//!
//! The workhorse is the *row band*: the image is split into horizontal bands;
//! each band owns a disjoint mutable slice of the output and may read source
//! rows extended by a `halo` margin. Because every kernel in photoforge is
//! either separable (gaussian/box/Lanczos: row pass + row-parallel vertical
//! pass reading an immutable temp) or row-local (median, bilateral), bands
//! with halo cover all cases with no unsafe code and double-buffered I/O.

use rayon::prelude::*;

/// A rectangle of pixels: position, size and the halo margin needed by a
/// kernel. Used by the pipeline's region-of-interest rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// Extra rows/columns of context required around the tile.
    pub halo: u32,
}

impl TileRect {
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            halo: 0,
        }
    }

    pub fn with_halo(mut self, halo: u32) -> Self {
        self.halo = halo;
        self
    }

    /// The rectangle extended by halo, clamped to `width x height`.
    pub fn expanded(&self, width: u32, height: u32) -> TileRect {
        let x0 = self.x.saturating_sub(self.halo);
        let y0 = self.y.saturating_sub(self.halo);
        let x1 = self.x.saturating_add(self.w + self.halo).min(width);
        let y1 = self.y.saturating_add(self.h + self.halo).min(height);
        TileRect {
            x: x0,
            y: y0,
            w: x1.saturating_sub(x0),
            h: y1.saturating_sub(y0),
            halo: self.halo,
        }
    }
}

/// Splits `height` rows into bands of at most `band_h` rows.
/// Returns `(y0, y1)` pairs covering `0..height` exactly.
pub fn band_ranges(height: u32, band_h: u32) -> Vec<(u32, u32)> {
    let band_h = band_h.max(1);
    let mut v = Vec::new();
    let mut y = 0;
    while y < height {
        let y1 = (y + band_h).min(height);
        v.push((y, y1));
        y = y1;
    }
    v
}

/// Runs `f` in parallel over disjoint mutable bands of `out` (row-major
/// elements, `stride` elements per row). `f(y0, y1, band_slice)` receives the
/// element range `[y0 * stride .. y1 * stride]`.
pub fn par_bands<T: Send>(
    out: &mut [T],
    stride: usize,
    band_h: u32,
    f: impl Fn(u32, u32, &mut [T]) + Sync,
) {
    if stride == 0 || out.is_empty() {
        return;
    }
    let band_elems = band_h.max(1) as usize * stride;
    out.par_chunks_mut(band_elems)
        .enumerate()
        .for_each(|(band, chunk)| {
            let y0 = band as u32 * band_h.max(1);
            let y1 = y0 + (chunk.len() / stride) as u32;
            f(y0, y1, chunk);
        });
}

/// Like [`par_bands`], but each band worker also gets read access to source
/// rows `[y0 − halo .. y1 + halo]` (clamped to the buffer) from the immutable
/// `src` slice. This is the halo-tiling primitive for windowed kernels.
pub fn par_bands_with_src<T: Send + Sync>(
    src: &[T],
    out: &mut [T],
    stride: usize,
    height: u32,
    band_h: u32,
    halo: u32,
    f: impl Fn(u32, u32, &[T], &mut [T]) + Sync,
) {
    let band_h = band_h.max(1);
    if stride == 0 || out.is_empty() {
        return;
    }
    let band_elems = band_h as usize * stride;
    out.par_chunks_mut(band_elems)
        .enumerate()
        .for_each(|(band, chunk)| {
            let y0 = band as u32 * band_h;
            let y1 = y0 + (chunk.len() / stride) as u32;
            let s0 = (y0.saturating_sub(halo)) as usize * stride;
            let s1 = (y1 + halo).min(height) as usize * stride;
            f(y0, y1, &src[s0..s1], chunk);
        });
}

/// Simple row-parallel map over an image's rows.
pub fn par_rows(height: u32, f: impl Fn(u32) + Sync + Send) {
    (0..height).into_par_iter().for_each(f);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_ranges_cover_exactly() {
        assert_eq!(band_ranges(10, 4), vec![(0, 4), (4, 8), (8, 10)]);
        assert_eq!(band_ranges(0, 4), Vec::<(u32, u32)>::new());
        assert_eq!(
            band_ranges(5, 0),
            vec![(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]
        );
        assert_eq!(band_ranges(3, 10), vec![(0, 3)]);
    }

    #[test]
    fn par_bands_disjoint_and_covering() {
        let mut data = vec![0u32; 12 * 4]; // 12 rows, stride 4
        par_bands(&mut data, 4, 5, |y0, y1, band| {
            for row in band.chunks_exact_mut(4) {
                for v in row {
                    *v = y0 * 1000 + y1;
                }
            }
        });
        // Bands: (0,5) rows 0-4, (5,10) rows 5-9, (10,12) rows 10-11.
        assert_eq!(&data[0..4], &[5; 4]);
        assert_eq!(&data[4 * 4..5 * 4], &[5; 4]);
        assert_eq!(&data[5 * 4..6 * 4], &[5010; 4]);
        assert_eq!(&data[9 * 4..10 * 4], &[5010; 4]);
        assert_eq!(&data[10 * 4..11 * 4], &[10_012; 4]);
    }

    #[test]
    fn par_bands_with_src_halo_reads_neighbors() {
        // src rows are 0..8 filled with the row index; each output band
        // writes the max source row index it could see (y1 + halo - 1).
        let src: Vec<u32> = (0..8).flat_map(|y| [y; 4]).collect();
        let mut out = vec![0u32; 32];
        par_bands_with_src(&src, &mut out, 4, 8, 2, 2, |y0, y1, sband, oband| {
            let seen_max = sband.chunks_exact(4).map(|r| r[0]).max().unwrap();
            for row in oband.chunks_exact_mut(4) {
                for v in row {
                    *v = seen_max;
                }
            }
            assert!(y1 > y0);
        });
        // Band (0,2) with halo reads rows 0..4 → max 3. Band (2,4) reads 0..6 → 5.
        assert_eq!(out[0], 3);
        assert_eq!(out[2 * 4], 5);
        // Last band (6,8) clamps at the bottom: reads rows 4..8 → max 7.
        assert_eq!(out[6 * 4], 7);
    }

    #[test]
    fn tile_rect_expansion_clamps() {
        let t = TileRect::new(10, 10, 20, 20).with_halo(4);
        let e = t.expanded(40, 30);
        assert_eq!((e.x, e.y, e.w, e.h), (6, 6, 28, 24));
        let edge = TileRect::new(0, 0, 10, 10).with_halo(8).expanded(12, 12);
        assert_eq!((edge.x, edge.y, edge.w, edge.h), (0, 0, 12, 12));
    }

    #[test]
    fn par_rows_runs_all() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let count = AtomicU32::new(0);
        par_rows(100, |_| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(count.load(Ordering::Relaxed), 100);
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        let mut empty: Vec<u8> = Vec::new();
        par_bands(&mut empty, 4, 8, |_, _, _| panic!("must not be called"));
        par_bands_with_src::<u8>(&[], &mut empty, 0, 0, 8, 2, |_, _, _, _| {
            panic!("must not be called")
        });
    }
}
