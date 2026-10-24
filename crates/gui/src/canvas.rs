//! Canvas widget: image display with zoom/pan, mask tools, the crop overlay
//! and before/after comparison.

use egui::{Color32, Rect, Sense, Vec2};

/// Active canvas tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Navigate,
    Brush,
    LinearGrad,
    RadialGrad,
}

/// Brush parameters for the mask painting tool.
#[derive(Clone, Copy, Debug)]
pub struct BrushParams {
    /// Fraction of the frame diagonal.
    pub radius: f32,
    /// 0 soft .. 1 hard.
    pub hardness: f32,
    /// Max opacity per pass.
    pub flow: f32,
    pub erase: bool,
}

impl Default for BrushParams {
    fn default() -> Self {
        Self {
            radius: 0.08,
            hardness: 0.6,
            flow: 0.9,
            erase: false,
        }
    }
}

/// Interactive crop state (image-pixel coordinates, pre-crop frame).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CropSession {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Aspect ratio (w/h); None = free.
    pub aspect: Option<f32>,
    /// Live straighten angle while in the crop tool.
    pub straighten: f32,
}

impl CropSession {
    pub fn full(w: u32, h: u32, straighten: f32) -> Self {
        Self {
            x0: 0.0,
            y0: 0.0,
            x1: w as f32,
            y1: h as f32,
            aspect: None,
            straighten,
        }
    }

    pub fn rect_i(&self) -> (u32, u32, u32, u32) {
        let x = self.x0.round().clamp(0.0, f32::MAX) as u32;
        let y = self.y0.round().clamp(0.0, f32::MAX) as u32;
        let w = (self.x1 - self.x0).round().max(1.0) as u32;
        let h = (self.y1 - self.y0).round().max(1.0) as u32;
        (x, y, w, h)
    }
}

/// Before/after mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Compare {
    Off,
    /// Show the original while a key/button is held.
    Hold,
    /// Vertical split at fraction t (left = original).
    Split(f32),
}

pub enum CanvasEvent {
    /// The view changed in a way that requires re-rendering at a new level.
    ViewChanged {
        interacting: bool,
    },
    Open,
    /// Brush stroke sample at normalized image coords.
    StrokePoint([f32; 2]),
    /// Brush drag released.
    StrokeEnded,
    /// Gradient drag finished.
    GradientCommitted {
        linear: bool,
        start: [f32; 2],
        end: [f32; 2],
    },
    /// The crop rect changed (already updated in the session).
    CropEdited,
    /// Split divider moved (new t).
    SplitMoved(f32),
}

pub struct CanvasInput<'a> {
    pub image_size: Option<[u32; 2]>,
    pub file_name: &'a str,
    pub tool: Tool,
    /// In-progress gradient drag start (owned by the app).
    pub gradient_drag: &'a mut Option<[f32; 2]>,
    pub crop: Option<&'a mut CropSession>,
    /// Original-image texture for compare modes.
    pub before_texture: Option<&'a egui::TextureHandle>,
    pub compare: Compare,
    /// Red-tint overlay of the active mask.
    pub mask_overlay: Option<&'a egui::TextureHandle>,
}

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

/// Which crop handle (if any) is under a point. Coordinates in crop space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CropHandle {
    Inside,
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

/// Hit-tests a crop rect (x0,y0,x1,y1) at a point; margin in the same units.
pub fn crop_hit(rect: [f32; 4], p: [f32; 2], margin: f32) -> Option<CropHandle> {
    let [x0, y0, x1, y1] = rect;
    let (px, py) = (p[0], p[1]);
    let near = |a: f32, b: f32| (a - b).abs() <= margin;
    let in_range = |v: f32, a: f32, b: f32| v >= a - margin && v <= b + margin;
    if near(x0, px) && near(y0, py) {
        Some(CropHandle::TopLeft)
    } else if near(x1, px) && near(y0, py) {
        Some(CropHandle::TopRight)
    } else if near(x1, px) && near(y1, py) {
        Some(CropHandle::BottomRight)
    } else if near(x0, px) && near(y1, py) {
        Some(CropHandle::BottomLeft)
    } else if near(x0, px) && in_range(py, y0, y1) {
        Some(CropHandle::Left)
    } else if near(x1, px) && in_range(py, y0, y1) {
        Some(CropHandle::Right)
    } else if near(y0, py) && in_range(px, x0, x1) {
        Some(CropHandle::Top)
    } else if near(y1, py) && in_range(px, x0, x1) {
        Some(CropHandle::Bottom)
    } else if px > x0 && px < x1 && py > y0 && py < y1 {
        Some(CropHandle::Inside)
    } else {
        None
    }
}

/// Draws the central canvas; returns events for the app.
pub fn show(
    ui: &mut egui::Ui,
    view: &mut ViewState,
    texture: Option<&egui::TextureHandle>,
    _texture_px: [usize; 2],
    input: &mut CanvasInput<'_>,
) -> Vec<CanvasEvent> {
    let mut events = Vec::new();
    let (canvas_rect, response) =
        ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());

    let painter = ui.painter_at(canvas_rect);
    painter.rect_filled(canvas_rect, 0.0, Color32::from_rgb(12, 12, 14));

    if let (Some(tex), Some([iw, ih])) = (
        texture,
        input.image_size.map(|s| [s[0] as usize, s[1] as usize]),
    ) {
        let canvas = canvas_rect.size();
        if view.fit {
            view.zoom = ViewState::fit_zoom(canvas, Vec2::new(iw as f32, ih as f32));
        }
        view.clamp_pan(canvas, Vec2::new(iw as f32, ih as f32));

        let image_rect = Rect::from_min_size(
            canvas_rect.min + view.offset,
            Vec2::new(iw as f32, ih as f32) * view.zoom,
        );
        let to_screen = |p: [f32; 2]| {
            egui::pos2(
                canvas_rect.min.x + view.offset.x + p[0] * view.zoom,
                canvas_rect.min.y + view.offset.y + p[1] * view.zoom,
            )
        };
        let to_image = |pos: egui::Pos2| -> [f32; 2] {
            [
                (pos.x - canvas_rect.min.x - view.offset.x) / view.zoom,
                (pos.y - canvas_rect.min.y - view.offset.y) / view.zoom,
            ]
        };

        // Main texture (or split: before on the left).
        match (input.compare, input.before_texture) {
            (Compare::Hold, Some(before)) => {
                painter.image(before.id(), image_rect, full_uv(), Color32::WHITE);
            }
            (Compare::Split(t), Some(before)) => {
                let xr = image_rect.left() + image_rect.width() * t;
                if xr > image_rect.left() {
                    painter.image(
                        before.id(),
                        Rect::from_min_max(image_rect.min, egui::pos2(xr, image_rect.bottom())),
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(t, 1.0)),
                        Color32::WHITE,
                    );
                }
                if xr < image_rect.right() {
                    painter.image(
                        tex.id(),
                        Rect::from_min_max(egui::pos2(xr, image_rect.top()), image_rect.max),
                        Rect::from_min_max(egui::pos2(t, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                // Divider.
                painter.line_segment(
                    [
                        egui::pos2(xr, image_rect.top()),
                        egui::pos2(xr, image_rect.bottom()),
                    ],
                    egui::Stroke::new(2.0_f32, Color32::from_rgb(240, 240, 245)),
                );
                // Divider drag.
                if response.dragged() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        if (pos.x - xr).abs() < 12.0 {
                            let t2 =
                                ((pos.x - image_rect.left()) / image_rect.width()).clamp(0.0, 1.0);
                            events.push(CanvasEvent::SplitMoved(t2));
                        }
                    }
                }
            }
            _ => {
                painter.image(tex.id(), image_rect, full_uv(), Color32::WHITE);
            }
        }

        // Mask overlay (red tint).
        if let Some(overlay) = input.mask_overlay {
            painter.image(overlay.id(), image_rect, full_uv(), Color32::WHITE);
        }

        // Gradient drag visualization.
        if let (Some(start), Some(pos)) = (*input.gradient_drag, response.interact_pointer_pos()) {
            let a = to_screen(start);
            painter.line_segment(
                [a, pos],
                egui::Stroke::new(1.5_f32, Color32::from_rgb(255, 240, 200)),
            );
            painter.circle_filled(a, 4.0, Color32::from_rgb(255, 240, 200));
            painter.circle_filled(pos, 4.0, Color32::from_rgb(255, 240, 200));
        }

        // Crop overlay.
        if let Some(crop) = input.crop.as_deref_mut() {
            let r =
                Rect::from_min_max(to_screen([crop.x0, crop.y0]), to_screen([crop.x1, crop.y1]));
            // Dim outside.
            for band in [
                Rect::from_min_max(canvas_rect.min, egui::pos2(canvas_rect.max.x, r.top())),
                Rect::from_min_max(egui::pos2(canvas_rect.min.x, r.bottom()), canvas_rect.max),
                Rect::from_min_max(
                    egui::pos2(canvas_rect.min.x, r.top()),
                    egui::pos2(r.left(), r.bottom()),
                ),
                Rect::from_min_max(
                    egui::pos2(r.right(), r.top()),
                    egui::pos2(canvas_rect.max.x, r.bottom()),
                ),
            ] {
                let band = band.intersect(canvas_rect);
                if band.is_positive() {
                    painter.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 130));
                }
            }
            // Thirds.
            let grid =
                egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 90));
            for i in 1..3 {
                let t = i as f32 / 3.0;
                let x = egui::lerp(r.left()..=r.right(), t);
                let y = egui::lerp(r.top()..=r.bottom(), t);
                painter.line_segment([egui::pos2(x, r.top()), egui::pos2(x, r.bottom())], grid);
                painter.line_segment([egui::pos2(r.left(), y), egui::pos2(r.right(), y)], grid);
            }
            // Frame + handles.
            painter.rect_stroke(
                r,
                0.0,
                egui::Stroke::new(1.5_f32, Color32::from_rgb(250, 250, 250)),
            );
            let hs = 5.0;
            for p in [
                r.left_top(),
                egui::pos2(r.center().x, r.top()),
                r.right_top(),
                egui::pos2(r.right(), r.center().y),
                r.right_bottom(),
                egui::pos2(r.center().x, r.bottom()),
                r.left_bottom(),
                egui::pos2(r.left(), r.center().y),
            ] {
                painter.rect_filled(
                    Rect::from_center_size(p, Vec2::splat(hs * 2.0)),
                    1.0,
                    Color32::from_rgb(250, 250, 250),
                );
            }

            // Crop interaction.
            if response.dragged() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let img_pt = to_image(pos);
                    let margin_img = 8.0 / view.zoom;
                    let hit = crop_hit([crop.x0, crop.y0, crop.x1, crop.y1], img_pt, margin_img);
                    let delta = response.drag_delta();
                    let dx = delta.x / view.zoom;
                    let dy = delta.y / view.zoom;
                    match hit {
                        Some(CropHandle::Inside) => {
                            let w = crop.x1 - crop.x0;
                            let h = crop.y1 - crop.y0;
                            let (iw, ih) = (iw as f32, ih as f32);
                            let mut nx0 = (crop.x0 + dx).clamp(0.0, iw - w);
                            let ny0 = (crop.y0 + dy).clamp(0.0, ih - h);
                            if let Some(a) = crop.aspect {
                                // Keep aspect while panning: clamp inside bounds.
                                let max_x0 = (iw - w).min(iw - w);
                                nx0 = nx0.clamp(0.0, max_x0);
                                let _ = a;
                            }
                            crop.x0 = nx0;
                            crop.y0 = ny0;
                            crop.x1 = nx0 + w;
                            crop.y1 = ny0 + h;
                            events.push(CanvasEvent::CropEdited);
                        }
                        Some(handle) => {
                            resize_crop(crop, handle, img_pt, iw as f32, ih as f32);
                            events.push(CanvasEvent::CropEdited);
                        }
                        None => {}
                    }
                }
            }
        }

        // Tool interactions (suppressed while cropping).
        if input.crop.is_none() {
            match input.tool {
                Tool::Navigate => {
                    if response.dragged() {
                        let delta = response.drag_delta();
                        view.offset += delta;
                        view.fit = false;
                        view.clamp_pan(canvas, Vec2::new(iw as f32, ih as f32));
                    }
                }
                Tool::Brush => {
                    if response.dragged() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let p = to_image(pos);
                            let n = [
                                (p[0] / iw as f32).clamp(0.0, 1.0),
                                (p[1] / ih as f32).clamp(0.0, 1.0),
                            ];
                            events.push(CanvasEvent::StrokePoint(n));
                        }
                    }
                    if response.drag_stopped() {
                        events.push(CanvasEvent::StrokeEnded);
                    }
                }
                Tool::LinearGrad | Tool::RadialGrad => {
                    let linear = input.tool == Tool::LinearGrad;
                    if response.drag_started() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let p = to_image(pos);
                            *input.gradient_drag = Some([
                                (p[0] / iw as f32).clamp(0.0, 1.0),
                                (p[1] / ih as f32).clamp(0.0, 1.0),
                            ]);
                        }
                    }
                    if let (Some(start), true) = (*input.gradient_drag, response.drag_stopped()) {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let p = to_image(pos);
                            let end = [
                                (p[0] / iw as f32).clamp(0.0, 1.0),
                                (p[1] / ih as f32).clamp(0.0, 1.0),
                            ];
                            events.push(CanvasEvent::GradientCommitted { linear, start, end });
                        }
                        *input.gradient_drag = None;
                    }
                    if response.drag_stopped() {
                        *input.gradient_drag = None;
                    }
                }
            }

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
                        let p = (cursor - canvas_rect.min - view.offset) / view.zoom;
                        view.zoom = new_zoom;
                        view.fit = false;
                        view.offset = cursor - canvas_rect.min - p * view.zoom;
                        view.clamp_pan(canvas, Vec2::new(iw as f32, ih as f32));
                        events.push(CanvasEvent::ViewChanged { interacting: false });
                    }
                }
            }
        }

        // Overlay text.
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
            events.push(CanvasEvent::Open);
        }
    }

    events
}

fn full_uv() -> Rect {
    Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
}

/// Resizes the crop rect from a handle drag, clamped to the frame and
/// respecting the aspect lock.
fn resize_crop(crop: &mut CropSession, handle: CropHandle, p: [f32; 2], iw: f32, ih: f32) {
    let clamp_pt = |p: [f32; 2]| [p[0].clamp(0.0, iw), p[1].clamp(0.0, ih)];
    let p = clamp_pt(p);
    match handle {
        CropHandle::TopLeft => {
            crop.x0 = p[0];
            crop.y0 = p[1];
        }
        CropHandle::Top => crop.y0 = p[1],
        CropHandle::TopRight => {
            crop.x1 = p[0];
            crop.y0 = p[1];
        }
        CropHandle::Right => crop.x1 = p[0],
        CropHandle::BottomRight => {
            crop.x1 = p[0];
            crop.y1 = p[1];
        }
        CropHandle::Bottom => crop.y1 = p[1],
        CropHandle::BottomLeft => {
            crop.x0 = p[0];
            crop.y1 = p[1];
        }
        CropHandle::Left => crop.x0 = p[0],
        CropHandle::Inside => {}
    }
    // Enforce orientation + aspect.
    if crop.x1 < crop.x0 {
        std::mem::swap(&mut crop.x0, &mut crop.x1);
    }
    if crop.y1 < crop.y0 {
        std::mem::swap(&mut crop.y0, &mut crop.y1);
    }
    if let Some(a) = crop.aspect {
        // Match height to width.
        let w = crop.x1 - crop.x0;
        let h = w / a;
        if handle == CropHandle::Top || handle == CropHandle::Bottom {
            let w = h * a;
            let cx = (crop.x0 + crop.x1) / 2.0;
            crop.x0 = (cx - w / 2.0).clamp(0.0, iw);
            crop.x1 = (crop.x0 + w).min(iw);
        }
        crop.y1 = (crop.y0 + h).min(ih);
        // If we clipped, re-derive width from the actual height.
        let h2 = crop.y1 - crop.y0;
        let w2 = (h2 * a).min(iw - crop.x0);
        crop.x1 = crop.x0 + w2;
    }
    // Minimum size.
    if crop.x1 - crop.x0 < 8.0 {
        crop.x1 = (crop.x0 + 8.0).min(iw);
    }
    if crop.y1 - crop.y0 < 8.0 {
        crop.y1 = (crop.y0 + 8.0).min(ih);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_zoom_and_pan_clamp() {
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

    #[test]
    fn crop_hit_test_regions() {
        let r = [10.0, 10.0, 50.0, 30.0];
        assert_eq!(crop_hit(r, [10.0, 10.0], 0.5), Some(CropHandle::TopLeft));
        assert_eq!(
            crop_hit(r, [50.0, 30.0], 0.5),
            Some(CropHandle::BottomRight)
        );
        assert_eq!(crop_hit(r, [30.0, 10.0], 0.5), Some(CropHandle::Top));
        assert_eq!(crop_hit(r, [50.0, 20.0], 0.5), Some(CropHandle::Right));
        assert_eq!(crop_hit(r, [30.0, 20.0], 0.5), Some(CropHandle::Inside));
        assert_eq!(crop_hit(r, [80.0, 20.0], 0.5), None);
    }

    #[test]
    fn resize_respects_aspect_and_bounds() {
        let mut c = CropSession {
            x0: 0.0,
            y0: 0.0,
            x1: 100.0,
            y1: 100.0,
            aspect: Some(1.0),
            straighten: 0.0,
        };
        resize_crop(&mut c, CropHandle::BottomRight, [80.0, 40.0], 200.0, 200.0);
        // Height clamps to 40 → width becomes 40 as well.
        assert!(((c.x1 - c.x0) - (c.y1 - c.y0)).abs() < 1e-4);
        // Never leaves the frame.
        assert!(c.x1 <= 200.0 && c.y1 <= 200.0 && c.x0 >= 0.0 && c.y0 >= 0.0);
    }
}
