//! Canvas widget: image display with zoom/pan.

use egui::{Color32, Rect, Sense, Vec2};

/// View transform: `zoom` is screen pixels per image pixel; `offset` is the
/// image-space position at the canvas top-left (in image pixels).
#[derive(Clone, Copy, Debug)]
pub struct ViewState {
    pub zoom: f32,
    pub offset: Vec2,
    /// Fit mode: recompute zoom to fill the canvas every frame.
    pub fit: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            offset: Vec2::ZERO,
            fit: true,
        }
    }
}

impl ViewState {
    /// Pyramid-level selector hint: the viewport's image-pixel width.
    pub fn target_px_hint(&self) -> f32 {
        // The canvas size isn't known here; the renderer picks a level from
        // this width. A 1600px-wide viewport is a good default for typical
        // windows; zooming in shrinks the hint (deeper level).
        (1600.0 / self.zoom.max(0.01)).clamp(64.0, 8000.0)
    }

    fn fit_zoom(canvas: Vec2, image: Vec2) -> f32 {
        if image.x <= 0.0 || image.y <= 0.0 {
            return 1.0;
        }
        (canvas.x / image.x).min(canvas.y / image.y).min(1.0)
    }

    fn clamp_pan(&mut self, canvas: Vec2, image: Vec2) {
        let shown = image * self.zoom;
        if shown.x <= canvas.x {
            self.offset.x = (canvas.x - shown.x) / 2.0;
        } else {
            self.offset.x = self.offset.x.clamp(canvas.x - shown.x, 0.0);
        }
        if shown.y <= canvas.y {
            self.offset.y = (canvas.y - shown.y) / 2.0;
        } else {
            self.offset.y = self.offset.y.clamp(canvas.y - shown.y, 0.0);
        }
    }
}

pub enum CanvasResponse {
    /// The view changed in a way that requires re-rendering at a new level.
    ViewChanged {
        interacting: bool,
    },
    Open,
}

pub struct CanvasInput<'a> {
    pub image_size: Option<[u32; 2]>,
    pub file_name: &'a str,
}

/// Draws the central canvas; handles wheel-zoom and drag-pan.
pub fn show(
    ui: &mut egui::Ui,
    view: &mut ViewState,
    texture: Option<&egui::TextureHandle>,
    texture_px: [usize; 2],
    input: &CanvasInput<'_>,
) -> Option<CanvasResponse> {
    let mut response_kind: Option<CanvasResponse> = None;
    let (canvas_rect, mut response) =
        ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());

    let painter = ui.painter_at(canvas_rect);
    painter.rect_filled(canvas_rect, 0.0, Color32::from_rgb(12, 12, 14));

    if let (Some(tex), [_tw, _th], Some([iw, ih])) = (
        texture,
        texture_px,
        input.image_size.map(|s| [s[0] as usize, s[1] as usize]),
    ) {
        let canvas = canvas_rect.size();
        // Fit-mode bookkeeping.
        if view.fit {
            view.zoom = ViewState::fit_zoom(canvas, Vec2::new(iw as f32, ih as f32));
        }
        view.clamp_pan(canvas, Vec2::new(iw as f32, ih as f32));

        let image_rect = Rect::from_min_size(
            canvas_rect.min + view.offset,
            Vec2::new(iw as f32, ih as f32) * view.zoom,
        );
        // Clip to the canvas.
        painter.image(
            tex.id(),
            image_rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        // Wheel zoom at cursor.
        if response.hovered() {
            let scroll = ui.ctx().input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.01 {
                let cursor = response
                    .interact_pointer_pos()
                    .unwrap_or_else(|| canvas_rect.center());
                let factor = (scroll / 300.0).exp();
                let new_zoom = (view.zoom * factor).clamp(0.02, 16.0);
                if (new_zoom - view.zoom).abs() > f32::EPSILON {
                    // Keep the image point under the cursor stationary.
                    let p = (cursor - canvas_rect.min - view.offset) / view.zoom;
                    view.zoom = new_zoom;
                    view.fit = false;
                    view.offset = cursor - canvas_rect.min - p * view.zoom;
                    view.clamp_pan(canvas, Vec2::new(iw as f32, ih as f32));
                    response_kind = Some(CanvasResponse::ViewChanged { interacting: false });
                }
            }
        }

        // Drag to pan.
        if response.dragged() {
            let delta = response.drag_delta();
            view.offset += delta;
            view.fit = false;
            view.clamp_pan(canvas, Vec2::new(iw as f32, ih as f32));
        }

        // Overlay: zoom level + filename (top-left).
        painter.text(
            canvas_rect.min + egui::vec2(10.0, 8.0),
            egui::Align2::LEFT_TOP,
            format!(
                "{}  |  {}%  |  {}×{}",
                input.file_name,
                (view.zoom * 100.0).round() as i32,
                input.image_size.map_or(0, |s| s[0]),
                input.image_size.map_or(0, |s| s[1])
            ),
            egui::FontId::proportional(13.0),
            Color32::from_rgba_unmultiplied(220, 220, 224, 190),
        );
    } else {
        // Empty state.
        let center = canvas_rect.center();
        painter.text(
            center + egui::vec2(0.0, -20.0),
            egui::Align2::CENTER_CENTER,
            "No image open",
            egui::FontId::proportional(22.0),
            Color32::from_rgb(120, 120, 126),
        );
        if ui
            .put(
                Rect::from_center_size(center + egui::vec2(0.0, 24.0), Vec2::new(160.0, 32.0)),
                egui::Button::new("Open image…"),
            )
            .clicked()
        {
            response_kind = Some(CanvasResponse::Open);
        }
    }

    let _ = &mut response;
    response_kind
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_zoom_and_pan_clamp() {
        // Landscape image in landscape canvas fits by width.
        let z = ViewState::fit_zoom(Vec2::new(1000.0, 500.0), Vec2::new(2000.0, 500.0));
        assert!((z - 0.5).abs() < 1e-5);
        let mut v = ViewState {
            zoom: 2.0,
            offset: Vec2::ZERO,
            fit: false,
        };
        // Shown smaller than canvas → centered (shown = 400×200 in 1000×500).
        v.clamp_pan(Vec2::new(1000.0, 500.0), Vec2::new(200.0, 100.0));
        assert!((v.offset.x - 300.0).abs() < 1e-3);
        assert!((v.offset.y - 150.0).abs() < 1e-3);
        // Shown larger → pannable, never showing outside.
        v.offset = Vec2::new(100.0, 100.0);
        v.clamp_pan(Vec2::new(1000.0, 500.0), Vec2::new(2000.0, 500.0));
        assert_eq!(v.offset, Vec2::ZERO);
        v.offset = Vec2::new(-10_000.0, -10_000.0);
        v.clamp_pan(Vec2::new(1000.0, 500.0), Vec2::new(2000.0, 500.0));
        // shown = 4000×1000 → x ∈ [−3000, 0], y ∈ [−500, 0].
        assert_eq!(v.offset, Vec2::new(-3000.0, -500.0));
    }

    #[test]
    fn target_hint_scales_with_zoom() {
        let v = ViewState {
            zoom: 0.25,
            ..ViewState::default()
        };
        assert!(v.target_px_hint() > 4000.0);
        let v = ViewState {
            zoom: 4.0,
            ..ViewState::default()
        };
        assert!(v.target_px_hint() < 800.0);
    }
}
