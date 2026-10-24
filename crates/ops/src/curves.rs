//! Monotone cubic splines (Fritsch–Carlson) for tone curves.
//!
//! Reference: F. N. Fritsch & R. E. Carlson, "Monotone Piecewise Cubic
//! Interpolation", SIAM J. Numer. Anal. 17 (1980). Interior tangents use the
//! weighted-harmonic-mean variant with the standard sign constraint
//! (non-positive slope product ⇒ zero tangent), which preserves monotonicity
//! of the data — the property a tone curve editor relies on.
//!
//! Curve application happens on the sRGB-encoded domain (ADR-0002): each
//! channel is encoded continuously, mapped through its curve, and the luma
//! curve scales all channels by `f(Y′)/Y′` so white stays white.

use photoforge_core::color::srgb;
use photoforge_core::Rgba32F;

pub use self::spline::MonotoneSpline;

mod spline {
    /// A monotone piecewise-cubic (Hermite) interpolant over sorted control
    /// points.
    #[derive(Clone, Debug, PartialEq)]
    pub struct MonotoneSpline {
        xs: Vec<f32>,
        ys: Vec<f32>,
        ms: Vec<f32>,
    }

    impl MonotoneSpline {
        /// Builds from control points. Requires at least 2 points with
        /// distinct x after normalization/dedup; returns `None` otherwise.
        pub fn new(mut points: Vec<[f32; 2]>) -> Option<Self> {
            if points.len() < 2 {
                return None;
            }
            points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
            let mut xs: Vec<f32> = Vec::with_capacity(points.len());
            let mut ys: Vec<f32> = Vec::with_capacity(points.len());
            for p in points {
                let p = [p[0].clamp(0.0, 1.0), p[1].clamp(0.0, 1.0)];
                if let Some(&last) = xs.last() {
                    if p[0] - last < 1e-4 {
                        *ys.last_mut().expect("just pushed") = p[1];
                        continue;
                    }
                }
                xs.push(p[0]);
                ys.push(p[1]);
            }
            if xs.len() < 2 {
                return None;
            }
            let n = xs.len();
            let mut h = vec![0.0f32; n - 1];
            let mut delta = vec![0.0f32; n - 1];
            for i in 0..n - 1 {
                h[i] = xs[i + 1] - xs[i];
                delta[i] = (ys[i + 1] - ys[i]) / h[i];
            }
            let mut ms = vec![0.0f32; n];
            ms[0] = delta[0];
            ms[n - 1] = delta[n - 2];
            for i in 1..n - 1 {
                if delta[i - 1] * delta[i] <= 0.0 {
                    ms[i] = 0.0;
                } else {
                    // Weighted harmonic mean (Fritsch–Carlson).
                    let w1 = 2.0 * h[i] + h[i - 1];
                    let w2 = h[i] + 2.0 * h[i - 1];
                    ms[i] = (w1 + w2) / (w1 / delta[i - 1] + w2 / delta[i]);
                }
            }
            Some(Self { xs, ys, ms })
        }

        /// Identity spline (diagonal through (0,0) and (1,1)).
        pub fn identity() -> Self {
            Self {
                xs: vec![0.0, 1.0],
                ys: vec![0.0, 1.0],
                ms: vec![1.0, 1.0],
            }
        }

        /// True when the point list is exactly the two-corner identity.
        pub fn is_identity_points(points: &[[f32; 2]]) -> bool {
            points.len() == 2 && points[0] == [0.0, 0.0] && points[1] == [1.0, 1.0]
        }

        /// Evaluates at `x`, clamped to the end tangents outside the range.
        pub fn eval(&self, x: f32) -> f32 {
            let n = self.xs.len();
            if x <= self.xs[0] {
                return (self.ys[0] + self.ms[0] * (x - self.xs[0])).clamp(0.0, 1.0);
            }
            if x >= self.xs[n - 1] {
                return (self.ys[n - 1] + self.ms[n - 1] * (x - self.xs[n - 1])).clamp(0.0, 1.0);
            }
            let mut lo = 0usize;
            let mut hi = n - 1;
            while hi - lo > 1 {
                let mid = (lo + hi) / 2;
                if self.xs[mid] <= x {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let h = self.xs[lo + 1] - self.xs[lo];
            let t = (x - self.xs[lo]) / h;
            let t2 = t * t;
            let t3 = t2 * t;
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            h00 * self.ys[lo]
                + h10 * h * self.ms[lo]
                + h01 * self.ys[lo + 1]
                + h11 * h * self.ms[lo + 1]
        }
    }
}

/// True when none of the four curves has any effect.
pub fn curves_are_identity(r: &[[f32; 2]], g: &[[f32; 2]], b: &[[f32; 2]], l: &[[f32; 2]]) -> bool {
    [r, g, b, l]
        .iter()
        .all(|pts| pts.is_empty() || MonotoneSpline::is_identity_points(pts))
}

/// Applies the four curves to one linear-light pixel (slow path: builds the
/// splines per call — the pipeline uses `OpParams::prepare_pixel_fn`).
pub fn curve_pixel(
    px: Rgba32F,
    r: &[[f32; 2]],
    g: &[[f32; 2]],
    b: &[[f32; 2]],
    l: &[[f32; 2]],
) -> Rgba32F {
    if curves_are_identity(r, g, b, l) {
        return px;
    }
    let mut e = [
        srgb::encode_continuous(px[0]),
        srgb::encode_continuous(px[1]),
        srgb::encode_continuous(px[2]),
    ];
    for (i, pts) in [r, g, b].iter().enumerate() {
        if !pts.is_empty() {
            if let Some(spline) = MonotoneSpline::new((*pts).to_vec()) {
                e[i] = spline.eval(e[i]);
            }
        }
    }
    if !l.is_empty() {
        if let Some(spline) = MonotoneSpline::new(l.to_vec()) {
            let y = 0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2];
            let y2 = spline.eval(y);
            let factor = y2 / y.max(1e-3);
            e = [
                (e[0] * factor).clamp(0.0, 1.0),
                (e[1] * factor).clamp(0.0, 1.0),
                (e[2] * factor).clamp(0.0, 1.0),
            ];
        }
    }
    [
        srgb::decode(e[0]),
        srgb::decode(e[1]),
        srgb::decode(e[2]),
        px[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_control_points() {
        let s = MonotoneSpline::new(vec![[0.0, 0.0], [0.3, 0.2], [0.7, 0.8], [1.0, 1.0]])
            .expect("valid points");
        for p in [[0.0, 0.0], [0.3, 0.2], [0.7, 0.8], [1.0, 1.0]] {
            assert!(
                (s.eval(p[0]) - p[1]).abs() < 1e-5,
                "{p:?} -> {}",
                s.eval(p[0])
            );
        }
    }

    #[test]
    fn preserves_monotonicity() {
        let s = MonotoneSpline::new(vec![
            [0.0, 0.0],
            [0.25, 0.1],
            [0.5, 0.15],
            [0.75, 0.85],
            [1.0, 1.0],
        ])
        .expect("valid");
        let mut prev = -1.0;
        for i in 0..=512 {
            let v = s.eval(i as f32 / 512.0);
            assert!(v >= prev - 1e-6, "non-monotone at {i}: {prev} -> {v}");
            prev = v;
        }
    }

    #[test]
    fn flat_regions_stay_flat() {
        let s = MonotoneSpline::new(vec![[0.0, 0.0], [0.5, 0.5], [0.6, 0.5], [1.0, 1.0]])
            .expect("valid");
        for x in [0.55f32, 0.58] {
            assert!(
                (s.eval(x) - 0.5).abs() < 1e-3,
                "flat region moved: {}",
                s.eval(x)
            );
        }
    }

    #[test]
    fn identity_spline_is_identity() {
        let s = MonotoneSpline::identity();
        for i in 0..=64 {
            let x = i as f32 / 64.0;
            assert!((s.eval(x) - x).abs() < 1e-6);
        }
        assert!(MonotoneSpline::is_identity_points(&[
            [0.0, 0.0],
            [1.0, 1.0]
        ]));
        assert!(!MonotoneSpline::is_identity_points(&[
            [0.0, 0.0],
            [0.5, 0.4],
            [1.0, 1.0]
        ]));
    }

    #[test]
    fn clamps_out_of_range() {
        // Tone curves only ever see x in [0, 1]; outside, evaluation
        // extrapolates linearly with the end tangent and clamps to [0, 1].
        let s = MonotoneSpline::new(vec![[0.0, 0.05], [1.0, 0.95]]).expect("valid");
        assert_eq!(s.eval(-1.0), 0.0);
        assert_eq!(s.eval(2.0), 1.0);
        assert_eq!(s.eval(-1e-9), 0.05);
        assert_eq!(s.eval(1.0 + 1e-9), 0.95);
    }

    #[test]
    fn degenerate_inputs() {
        assert!(MonotoneSpline::new(vec![]).is_none());
        assert!(MonotoneSpline::new(vec![[0.0, 0.0]]).is_none());
        // Duplicate x collapses to a valid two-point spline.
        let s = MonotoneSpline::new(vec![[0.0, 0.0], [0.5, 0.5], [0.5, 0.5], [1.0, 1.0]]);
        assert!(s.is_some());
        // NaN input never panics.
        let s = MonotoneSpline::new(vec![[0.0, 0.0], [1.0, 1.0]]).expect("valid");
        let v = s.eval(f32::NAN);
        assert!(v.is_finite() || v.is_nan()); // no panic; NaN clamps via comparison
    }
}
