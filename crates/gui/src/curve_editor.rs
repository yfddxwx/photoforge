//! Tone-curve editor widget: draggable control points over a monotone
//! spline, click to add, right-click to remove.

use photoforge_ops::curves::MonotoneSpline;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveChannel {
    R,
    G,
    B,
    L,
}

impl CurveChannel {
    pub fn label(self) -> &'static str {
        match self {
            Self::R => "R",
            Self::G => "G",
            Self::B => "B",
            Self::L => "L",
        }
    }

    pub fn color(self) -> egui::Color32 {
        match self {
            Self::R => egui::Color32::from_rgb(235, 90, 90),
            Self::G => egui::Color32::from_rgb(90, 220, 110),
            Self::B => egui::Color32::from_rgb(100, 130, 245),
            Self::L => egui::Color32::from_rgb(230, 230, 235),
        }
    }

    pub const ALL: [CurveChannel; 4] = [Self::R, Self::G, Self::B, Self::L];
}

/// What the user did this frame.
pub struct CurveEdit {
    pub changed: bool,
    pub committed: bool,
}

/// Draws the editor bound to `points` (in [0, 1]²). Mutates in place.
pub fn show(ui: &mut egui::Ui, points: &mut Vec<[f32; 2]>, channel: CurveChannel) -> CurveEdit {
    let mut edit = CurveEdit {
        changed: false,
        committed: false,
    };
    let side = ui.available_width().min(240.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_rgb(14, 14, 16));

    // Grid.
    let grid = egui::Color32::from_rgb(38, 38, 44);
    for i in 1..4 {
        let t = i as f32 / 4.0;
        let x = egui::lerp(rect.left()..=rect.right(), t);
        let y = egui::lerp(rect.top()..=rect.bottom(), t);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.0_f32, grid),
        );
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0_f32, grid),
        );
    }
    // Diagonal.
    painter.line_segment(
        [
            egui::pos2(rect.left(), rect.bottom()),
            egui::pos2(rect.right(), rect.top()),
        ],
        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(60, 60, 68)),
    );

    let to_screen = |p: [f32; 2]| {
        egui::pos2(
            egui::lerp(rect.left()..=rect.right(), p[0]),
            egui::lerp(rect.bottom()..=rect.top(), p[1]),
        )
    };
    let to_curve = |pos: egui::Pos2| -> [f32; 2] {
        [
            ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0),
        ]
    };

    // Curve polyline.
    if points.len() >= 2 {
        if let Some(spline) = MonotoneSpline::new(points.clone()) {
            let pts: Vec<egui::Pos2> = (0..=128)
                .map(|i| {
                    let t = i as f32 / 128.0;
                    to_screen([t, spline.eval(t)])
                })
                .collect();
            painter.add(egui::Shape::line(
                pts,
                egui::Stroke::new(1.8_f32, channel.color()),
            ));
        }
    }

    // Control points.
    for p in points.iter() {
        painter.circle_filled(to_screen(*p), 4.0, channel.color());
        painter.circle_stroke(
            to_screen(*p),
            4.0,
            egui::Stroke::new(1.0_f32, egui::Color32::BLACK),
        );
    }

    // --- interaction ---
    let pointer = response.interact_pointer_pos();
    // Hit-test nearest point.
    let hit = pointer.and_then(|pos| {
        points
            .iter()
            .enumerate()
            .min_by(|a, b| {
                let da = to_screen(*a.1).distance_sq(pos);
                let db = to_screen(*b.1).distance_sq(pos);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .filter(|(_, p)| to_screen(**p).distance(pos) < 12.0)
            .map(|(i, _)| i)
    });

    if response.dragged() {
        if let (Some(pos), Some(idx)) = (
            pointer,
            hit.or_else(|| {
                // Start dragging the closest point on drag-begin.
                points
                    .iter()
                    .enumerate()
                    .min_by(|a, b| {
                        let da = to_screen(*a.1).distance_sq(pointer.unwrap());
                        let db = to_screen(*b.1).distance_sq(pointer.unwrap());
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .filter(|(_, p)| to_screen(**p).distance(pointer.unwrap()) < 12.0)
                    .map(|(i, _)| i)
            }),
        ) {
            let mut np = to_curve(pos);
            let min_x = if idx > 0 {
                points[idx - 1][0] + 0.02
            } else {
                0.0
            };
            let max_x = if idx + 1 < points.len() {
                points[idx + 1][0] - 0.02
            } else {
                1.0
            };
            np[0] = np[0].clamp(min_x, max_x);
            if points[idx] != np {
                points[idx] = np;
                edit.changed = true;
            }
        }
    }
    if response.drag_stopped() && edit.changed {
        edit.committed = true;
    }
    if response.clicked() && hit.is_none() {
        // Add a point where clicked (unless too close to an existing x).
        if let Some(pos) = pointer {
            let np = to_curve(pos);
            let too_close = points.iter().any(|p| (p[0] - np[0]).abs() < 0.02);
            if !too_close {
                points.push(np);
                points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
                edit.changed = true;
                edit.committed = true;
            }
        }
    }
    if response.secondary_clicked() {
        if let Some(idx) = hit {
            if points.len() > 2 {
                points.remove(idx);
                edit.changed = true;
                edit.committed = true;
            }
        }
    }

    edit
}

#[cfg(test)]
mod tests {
    use super::*;

    // The editor is interactive UI; test the math it relies on indirectly
    // (spline behavior is covered in ops). Keep a smoke test of the enum.
    #[test]
    fn channels_complete() {
        assert_eq!(CurveChannel::ALL.len(), 4);
        assert_eq!(CurveChannel::L.label(), "L");
    }
}
