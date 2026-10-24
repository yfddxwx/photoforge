//! Small scalar helpers shared by color and processing code.

/// Clamps to [0, 1]. NaN maps to 0.
#[inline]
pub fn clamp01(x: f32) -> f32 {
    if x.is_nan() || x <= 0.0 {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}

/// Linear interpolation.
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Smooth Hermite step between `edge0` and `edge1` (GLSL semantics).
/// Returns 0 for `x <= edge0`, 1 for `x >= edge1`; if the edges are equal,
/// the step is an abrupt threshold. Non-finite edges yield 0.
#[inline]
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = if (edge1 - edge0).abs() < f32::EPSILON {
        if x < edge0 {
            0.0
        } else {
            1.0
        }
    } else {
        let t = (x - edge0) / (edge1 - edge0);
        if t.is_nan() {
            0.0
        } else {
            t.clamp(0.0, 1.0)
        }
    };
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp01_handles_edges() {
        assert_eq!(clamp01(-1.0), 0.0);
        assert_eq!(clamp01(0.5), 0.5);
        assert_eq!(clamp01(2.0), 1.0);
        assert_eq!(clamp01(f32::NAN), 0.0);
        assert_eq!(clamp01(f32::INFINITY), 1.0);
    }

    #[test]
    fn lerp_endpoints() {
        assert_eq!(lerp(1.0, 3.0, 0.0), 1.0);
        assert_eq!(lerp(1.0, 3.0, 1.0), 3.0);
        assert!((lerp(1.0, 3.0, 0.5) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn smoothstep_shape() {
        assert_eq!(smoothstep(0.0, 1.0, -0.5), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 1.0), 1.0);
        assert_eq!(smoothstep(0.0, 1.0, 1.5), 1.0);
        assert!((smoothstep(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
        // Monotone.
        let mut prev = -1.0;
        for i in 0..=100 {
            let v = smoothstep(0.2, 0.8, i as f32 / 100.0);
            assert!(v >= prev);
            prev = v;
        }
        // Degenerate edges: abrupt threshold, never NaN.
        assert_eq!(smoothstep(0.5, 0.5, 0.4), 0.0);
        assert_eq!(smoothstep(0.5, 0.5, 0.6), 1.0);
        assert!(!smoothstep(f32::NAN, 1.0, 0.5).is_nan());
    }
}
