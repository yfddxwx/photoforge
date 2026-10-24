//! Mask representation and rasterization (ADR-0007).
//!
//! Masks are stored in normalized image coordinates ([0, 1] relative to the
//! display-oriented frame) so they survive resizes; they are rasterized to a
//! per-pixel f32 opacity map at render resolution and cached there.
//! Brush radius is stored as a fraction of the frame diagonal.

use serde::{Deserialize, Serialize};

/// A local adjustment mask.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Mask {
    /// Painted strokes (add on top, erase removes).
    Brush { strokes: Vec<Stroke> },
    /// Linear gradient from `start` (effect 0) to `end` (effect 1) with a
    /// smooth (smoothstep) transition along the whole span.
    LinearGradient { start: [f32; 2], end: [f32; 2] },
    /// Radial gradient: full effect inside `radius`, falling off over
    /// `feather` (fraction of radius) to zero outside.
    RadialGradient {
        center: [f32; 2],
        /// Radii as fractions of frame width/height (elliptical).
        radius: [f32; 2],
        /// 0..1: portion of the radius used for the falloff.
        feather: f32,
    },
}

/// One brushed polyline segment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    /// Polyline vertices in normalized coordinates.
    pub points: Vec<[f32; 2]>,
    /// Brush radius as a fraction of the frame diagonal.
    pub radius: f32,
    /// 0 (soft) .. 1 (hard edge).
    pub hardness: f32,
    /// Max opacity deposited per pass, 0..1.
    pub flow: f32,
    /// When true the stroke removes mask coverage instead of adding it.
    pub erase: bool,
}

impl Mask {
    /// Rasterizes to `w × h` f32 opacities in [0, 1], row-major.
    pub fn rasterize(&self, w: u32, h: u32) -> Vec<f32> {
        let mut out = vec![0.0f32; (w as usize) * (h as usize)];
        if w == 0 || h == 0 {
            return out;
        }
        match self {
            Mask::Brush { strokes } => {
                let diag = ((w as f32).hypot(h as f32)).max(1.0);
                for stroke in strokes {
                    rasterize_stroke(stroke, w, h, diag, &mut out);
                }
            }
            Mask::LinearGradient { start, end } => {
                rasterize_linear_gradient(*start, *end, w, h, &mut out);
            }
            Mask::RadialGradient {
                center,
                radius,
                feather,
            } => {
                rasterize_radial(*center, *radius, *feather, w, h, &mut out);
            }
        }
        out
    }
}

fn rasterize_linear_gradient(start: [f32; 2], end: [f32; 2], w: u32, h: u32, out: &mut [f32]) {
    let dx = (end[0] - start[0]) * w as f32;
    let dy = (end[1] - start[1]) * h as f32;
    let len_sq = dx * dx + dy * dy;
    for y in 0..h {
        let row_y = y as f32 + 0.5;
        for x in 0..w {
            let px = x as f32 + 0.5;
            let t = if len_sq < f32::EPSILON {
                // Degenerate gradient: uniform half effect.
                0.5
            } else {
                (((px - start[0] * w as f32) * dx + (row_y - start[1] * h as f32) * dy) / len_sq)
                    .clamp(0.0, 1.0)
            };
            out[y as usize * w as usize + x as usize] = t * t * (3.0 - 2.0 * t);
        }
    }
}

fn rasterize_radial(
    center: [f32; 2],
    radius: [f32; 2],
    feather: f32,
    w: u32,
    h: u32,
    out: &mut [f32],
) {
    let cx = center[0] * w as f32;
    let cy = center[1] * h as f32;
    let rx = (radius[0] * w as f32).max(1.0);
    let ry = (radius[1] * h as f32).max(1.0);
    let feather = feather.clamp(0.0, 1.0);
    for y in 0..h {
        let ny = (y as f32 + 0.5 - cy) / ry;
        for x in 0..w {
            let nx = (x as f32 + 0.5 - cx) / rx;
            let r = (nx * nx + ny * ny).sqrt(); // 1.0 at the ellipse edge
                                                // Solid inside (1 − feather), smooth falloff to the edge.
            let t = ((1.0 - r) / feather.max(1e-3)).clamp(0.0, 1.0);
            out[y as usize * w as usize + x as usize] = t * t * (3.0 - 2.0 * t);
        }
    }
}

fn rasterize_stroke(stroke: &Stroke, w: u32, h: u32, diag: f32, out: &mut [f32]) {
    let radius_px = (stroke.radius.max(1e-4) * diag).max(0.5);
    let hardness = stroke.hardness.clamp(0.0, 1.0);
    let flow = stroke.flow.clamp(0.0, 1.0);
    let step = radius_px * 0.3;
    // Splat discs along each segment.
    for seg in stroke.points.windows(2) {
        let [ax, ay] = [seg[0][0] * w as f32, seg[0][1] * h as f32];
        let [bx, by] = [seg[1][0] * w as f32, seg[1][1] * h as f32];
        let dist = ((bx - ax).hypot(by - ay)).max(step);
        let n = (dist / step).ceil() as usize;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            splat_disc(
                ax + (bx - ax) * t,
                ay + (by - ay) * t,
                radius_px,
                hardness,
                flow,
                stroke.erase,
                w,
                h,
                out,
            );
        }
    }
    // Single-point stroke.
    if stroke.points.len() == 1 {
        let [px, py] = [
            stroke.points[0][0] * w as f32,
            stroke.points[0][1] * h as f32,
        ];
        splat_disc(px, py, radius_px, hardness, flow, stroke.erase, w, h, out);
    }
}

#[allow(clippy::too_many_arguments)]
fn splat_disc(
    cx: f32,
    cy: f32,
    radius: f32,
    hardness: f32,
    flow: f32,
    erase: bool,
    w: u32,
    h: u32,
    out: &mut [f32],
) {
    let x0 = ((cx - radius).floor()).max(0.0) as i64;
    let y0 = ((cy - radius).floor()).max(0.0) as i64;
    let x1 = ((cx + radius).ceil() as i64).min(w as i64 - 1);
    let y1 = ((cy + radius).ceil() as i64).min(h as i64 - 1);
    if x1 < x0 || y1 < y0 {
        return;
    }
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ((x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy)) / radius;
            if d >= 1.0 {
                continue;
            }
            // Falloff: flat to `hardness`, smooth to the rim.
            let t = ((d - hardness) / (1.0 - hardness).max(1e-3)).clamp(0.0, 1.0);
            let profile = 1.0 - t * t * (3.0 - 2.0 * t);
            let paint = profile * flow;
            let idx = y as usize * w as usize + x as usize;
            if erase {
                out[idx] *= 1.0 - paint;
            } else {
                out[idx] = out[idx].max(paint);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(map: &[f32], w: u32, x: u32, y: u32) -> f32 {
        map[y as usize * w as usize + x as usize]
    }

    #[test]
    fn linear_gradient_endpoints() {
        let m = Mask::LinearGradient {
            start: [0.0, 0.5],
            end: [1.0, 0.5],
        };
        let (w, h) = (100u32, 50u32);
        let map = m.rasterize(w, h);
        assert!(sample(&map, w, 2, 25) < 0.02, "start must be ~0");
        assert!(sample(&map, w, 97, 25) > 0.98, "end must be ~1");
        let mid = sample(&map, w, 50, 25);
        assert!(
            (mid - 0.5).abs() < 0.05,
            "midpoint ~0.5 (smoothstep), got {mid}"
        );
        // Uniform along the perpendicular axis.
        assert!((sample(&map, w, 50, 5) - mid).abs() < 0.01);
    }

    #[test]
    fn radial_gradient_shape() {
        let m = Mask::RadialGradient {
            center: [0.5, 0.5],
            radius: [0.4, 0.4],
            feather: 0.5,
        };
        let (w, h) = (200u32, 200u32);
        let map = m.rasterize(w, h);
        assert!(sample(&map, w, 100, 100) > 0.99, "center solid");
        assert!(sample(&map, w, 100, 130) > 0.99, "inner solid");
        let edge = sample(&map, w, 100, 180); // r = 1.0
        assert!(edge < 0.01, "outside must be 0, got {edge}");
        let mid = sample(&map, w, 100, 150); // r = 0.75
        assert!(mid > 0.05 && mid < 0.95, "mid falloff {mid}");
    }

    #[test]
    fn brush_paint_and_erase() {
        let paint = Mask::Brush {
            strokes: vec![Stroke {
                points: vec![[0.5, 0.5], [0.5, 0.5]],
                radius: 0.1,
                hardness: 1.0,
                flow: 1.0,
                erase: false,
            }],
        };
        let (w, h) = (100u32, 100u32);
        let map = paint.rasterize(w, h);
        assert!(sample(&map, w, 50, 50) > 0.99, "brush center painted");
        assert_eq!(sample(&map, w, 10, 10), 0.0, "outside untouched");

        let erase = Mask::Brush {
            strokes: vec![Stroke {
                points: vec![[0.5, 0.5], [0.5, 0.5]],
                radius: 0.2,
                hardness: 1.0,
                flow: 1.0,
                erase: true,
            }],
        };
        let erased = erase.rasterize(w, h);
        assert_eq!(sample(&erased, w, 50, 50), 0.0, "erase removes");
        // Erase cannot go below zero on empty map.
        assert!(erased.iter().all(|&v| v >= 0.0));
    }

    #[test]
    fn brush_polyline_covers_path() {
        let stroke = Mask::Brush {
            strokes: vec![Stroke {
                points: vec![[0.1, 0.5], [0.9, 0.5]],
                radius: 0.05,
                hardness: 0.8,
                flow: 1.0,
                erase: false,
            }],
        };
        let (w, h) = (200u32, 100u32);
        let map = stroke.rasterize(w, h);
        for x in [25u32, 60, 100, 140, 175] {
            assert!(sample(&map, w, x, 50) > 0.5, "path coverage at {x}");
        }
        assert_eq!(sample(&map, w, 100, 10), 0.0);
    }

    #[test]
    fn soft_brush_falls_off() {
        let soft = Mask::Brush {
            strokes: vec![Stroke {
                points: vec![[0.5, 0.5], [0.5, 0.5]],
                radius: 0.2,
                hardness: 0.0,
                flow: 1.0,
                erase: false,
            }],
        };
        let (w, h) = (200u32, 200u32);
        let map = soft.rasterize(w, h);
        let center = sample(&map, w, 100, 100);
        let rim = sample(&map, w, 170, 100); // near r = 1
        assert!(center > 0.9);
        assert!(rim < 0.2, "soft rim {rim}");
    }

    #[test]
    fn degenerate_inputs() {
        assert!(Mask::LinearGradient {
            start: [0.5, 0.5],
            end: [0.5, 0.5]
        }
        .rasterize(10, 10)
        .iter()
        .all(|&v| (v - 0.5).abs() < 1e-6));
        assert_eq!(Mask::Brush { strokes: vec![] }.rasterize(0, 0).len(), 0);
        // Zero radius / out-of-frame strokes must not panic.
        let wild = Mask::Brush {
            strokes: vec![Stroke {
                points: vec![[-5.0, -5.0], [5.0, 5.0]],
                radius: 0.0,
                hardness: 0.5,
                flow: 0.5,
                erase: false,
            }],
        };
        let _ = wild.rasterize(32, 32);
    }
}
