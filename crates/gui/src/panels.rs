//! Panels: top bar, adjustment panel (+ histogram), canvas host, export
//! dialog, status bar.

use photoforge_ops::params::GLOBAL_OP_ORDER;
use photoforge_ops::OpParams;

use crate::app::PhotoForgeApp;
use crate::canvas::{self, CanvasInput, CanvasResponse};
use crate::slider::{fmt_signed, fmt_stops, ParamRow};

pub fn top_bar(app: &mut PhotoForgeApp, ctx: &egui::Context) {
    let mut open_clicked = false;
    egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading(egui::RichText::new("photoforge").size(16.0));
            ui.separator();
            if ui.button("Open… (Ctrl+O)").clicked() {
                open_clicked = true;
            }
            ui.separator();
            if ui.button("Fit (F)").clicked() {
                app.view().fit = true;
            }
            if ui.button("100% (1)").clicked() {
                app.view().zoom = 1.0;
                app.view().fit = false;
            }
            ui.separator();
            if ui
                .add_enabled(app.image().is_some(), egui::Button::new("Export…"))
                .clicked()
            {
                *app.export_ui().open = true;
            }
            ui.checkbox(app.show_exif(), "EXIF");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("render {:.1} ms", app.render_ms()));
            });
        });
        ui.add_space(2.0);
    });
    if open_clicked {
        open_dialog(app);
    }
}

pub fn open_dialog(app: &mut PhotoForgeApp) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter(
            "Images",
            &["jpg", "jpeg", "png", "webp", "tif", "tiff", "bmp"],
        )
        .pick_file()
    {
        app.request_open_pub(path);
    }
}

pub fn adjustment_panel(app: &mut PhotoForgeApp, ctx: &egui::Context) {
    egui::SidePanel::right("adjustments")
        .resizable(true)
        .default_width(300.0)
        .show(ctx, |ui| {
            histogram_widget(app, ui);
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                ui.strong("Light");
                ui.separator();
                // White balance (adjustment 0): temp + tint.
                wb_rows(app, ui);
                exposure_row(app, ui);
                tone_row(app, ui, 2, "Contrast");
                tone_row(app, ui, 3, "Highlights");
                tone_row(app, ui, 4, "Shadows");
                tone_row(app, ui, 5, "Whites");
                tone_row(app, ui, 6, "Blacks");

                ui.add_space(6.0);
                ui.strong("Color");
                ui.separator();
                chroma_row(app, ui, 7, "Vibrance");
                chroma_row(app, ui, 8, "Saturation");

                ui.add_space(8.0);
                ui.weak(format!(
                    "{} global adjustments — curves/levels, filters and masks land in M5–M7",
                    GLOBAL_OP_ORDER.len()
                ));
            });
        });
}

type StateGetter = Box<dyn Fn(&photoforge_pipeline::EditState) -> f32>;
type StateSetter = Box<dyn Fn(&mut photoforge_pipeline::EditState, f32)>;

/// A bound slider row: where to read the value, how to write it, and its
/// slider range. Interactive while dragging; a refined render on release.
struct ParamBinding {
    label: &'static str,
    min: f32,
    max: f32,
    default: f32,
    format: fn(f32) -> String,
    get: StateGetter,
    set: StateSetter,
}

fn param_row(app: &mut PhotoForgeApp, ui: &mut egui::Ui, b: ParamBinding) {
    let mut value = (b.get)(app.state());
    let row = ParamRow {
        label: b.label,
        value: &mut value,
        default: b.default,
        min: b.min,
        max: b.max,
        format: Some(b.format),
    };
    let (changed, committed) = row.show(ui);
    if changed {
        app.edit_state(ui.ctx(), true, |s| (b.set)(s, value));
    }
    if committed {
        // State is already correct; ask for the refined frame.
        app.edit_state(ui.ctx(), false, |_| {});
    }
}

/// Builds a [`ParamBinding`] with the usual 0 default.
fn binding(
    label: &'static str,
    min: f32,
    max: f32,
    format: fn(f32) -> String,
    get: impl Fn(&photoforge_pipeline::EditState) -> f32 + 'static,
    set: impl Fn(&mut photoforge_pipeline::EditState, f32) + 'static,
) -> ParamBinding {
    ParamBinding {
        label,
        min,
        max,
        default: 0.0,
        format,
        get: Box::new(get),
        set: Box::new(set),
    }
}

fn wb_rows(app: &mut PhotoForgeApp, ui: &mut egui::Ui) {
    let temp = binding(
        "Temp",
        -1.0,
        1.0,
        fmt_signed,
        |s| {
            s.adjustments
                .first()
                .and_then(|a| match &a.op {
                    OpParams::WhiteBalance { temp, .. } => Some(*temp),
                    _ => None,
                })
                .unwrap_or(0.0)
        },
        |s, v| {
            if let Some(adj) = s.adjustments.get_mut(0) {
                if let OpParams::WhiteBalance { temp, .. } = &mut adj.op {
                    *temp = v;
                }
            }
        },
    );
    param_row(app, ui, temp);
    let tint = binding(
        "Tint",
        -1.0,
        1.0,
        fmt_signed,
        |s| {
            s.adjustments
                .first()
                .and_then(|a| match &a.op {
                    OpParams::WhiteBalance { tint, .. } => Some(*tint),
                    _ => None,
                })
                .unwrap_or(0.0)
        },
        |s, v| {
            if let Some(adj) = s.adjustments.get_mut(0) {
                if let OpParams::WhiteBalance { tint, .. } = &mut adj.op {
                    *tint = v;
                }
            }
        },
    );
    param_row(app, ui, tint);
}

fn exposure_row(app: &mut PhotoForgeApp, ui: &mut egui::Ui) {
    let row = binding(
        "Exposure",
        -4.0,
        4.0,
        fmt_stops,
        |s| {
            s.adjustments
                .get(1)
                .and_then(|a| match &a.op {
                    OpParams::Exposure { stops } => Some(*stops),
                    _ => None,
                })
                .unwrap_or(0.0)
        },
        |s, v| {
            if let Some(adj) = s.adjustments.get_mut(1) {
                if let OpParams::Exposure { stops } = &mut adj.op {
                    *stops = v;
                }
            }
        },
    );
    param_row(app, ui, row);
}

fn tone_row(app: &mut PhotoForgeApp, ui: &mut egui::Ui, idx: usize, label: &'static str) {
    let get = move |s: &photoforge_pipeline::EditState| {
        s.adjustments
            .get(idx)
            .and_then(|adj| match &adj.op {
                OpParams::Contrast { amount }
                | OpParams::Highlights { amount }
                | OpParams::Shadows { amount }
                | OpParams::Whites { amount }
                | OpParams::Blacks { amount } => Some(*amount),
                _ => None,
            })
            .unwrap_or(0.0)
    };
    let set = move |s: &mut photoforge_pipeline::EditState, v: f32| {
        if let Some(adj) = s.adjustments.get_mut(idx) {
            if let OpParams::Contrast { amount }
            | OpParams::Highlights { amount }
            | OpParams::Shadows { amount }
            | OpParams::Whites { amount }
            | OpParams::Blacks { amount } = &mut adj.op
            {
                *amount = v;
            }
        }
    };
    param_row(app, ui, binding(label, -1.0, 1.0, fmt_signed, get, set));
}

fn chroma_row(app: &mut PhotoForgeApp, ui: &mut egui::Ui, idx: usize, label: &'static str) {
    let get = move |s: &photoforge_pipeline::EditState| {
        s.adjustments
            .get(idx)
            .and_then(|adj| match &adj.op {
                OpParams::Vibrance { amount } | OpParams::Saturation { amount } => Some(*amount),
                _ => None,
            })
            .unwrap_or(0.0)
    };
    let set = move |s: &mut photoforge_pipeline::EditState, v: f32| {
        if let Some(adj) = s.adjustments.get_mut(idx) {
            match &mut adj.op {
                OpParams::Vibrance { amount } | OpParams::Saturation { amount } => *amount = v,
                _ => {}
            }
        }
    };
    param_row(app, ui, binding(label, -1.0, 1.0, fmt_signed, get, set));
}

fn histogram_widget(app: &mut PhotoForgeApp, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.strong("Histogram");
    let Some(hist) = app.histogram().cloned() else {
        ui.weak("(no image)");
        return;
    };
    let log_id = egui::Id::new("hist_log");
    let mut log = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(log_id).unwrap_or(false));
    if ui.checkbox(&mut log, "log scale").changed() {
        ui.ctx().data_mut(|d| d.insert_temp(log_id, log));
    }
    let (resp, painter) =
        ui.allocate_painter(egui::vec2(ui.available_width(), 96.0), egui::Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(14, 14, 16));
    let max_bin = hist.max_bin().max(1) as f32;
    let scale = |v: f32| {
        let t = if log {
            (v / max_bin).max(1e-4).log10() / 4.0 + 0.25
        } else {
            v / max_bin
        };
        t.clamp(0.0, 1.0)
    };
    let channels: [(&[u32; 256], egui::Color32); 3] = [
        (
            &hist.r,
            egui::Color32::from_rgba_unmultiplied(220, 60, 60, 110),
        ),
        (
            &hist.g,
            egui::Color32::from_rgba_unmultiplied(70, 210, 90, 110),
        ),
        (
            &hist.b,
            egui::Color32::from_rgba_unmultiplied(80, 110, 240, 110),
        ),
    ];
    let w = rect.width() / 256.0;
    for (bins, color) in channels {
        let mut pts = Vec::with_capacity(258);
        pts.push(egui::pos2(rect.left(), rect.bottom()));
        for (i, &c) in bins.iter().enumerate() {
            let h = scale(c as f32) * rect.height();
            pts.push(egui::pos2(rect.left() + i as f32 * w, rect.bottom() - h));
        }
        pts.push(egui::pos2(rect.right(), rect.bottom()));
        painter.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
    }
    let mut luma_pts = Vec::with_capacity(256);
    for (i, &c) in hist.luma.iter().enumerate() {
        let h = scale(c as f32) * rect.height();
        luma_pts.push(egui::pos2(rect.left() + i as f32 * w, rect.bottom() - h));
    }
    painter.add(egui::Shape::line(
        luma_pts,
        egui::Stroke::new(1.2_f32, egui::Color32::from_rgb(230, 230, 235)),
    ));
}

pub fn central_canvas(app: &mut PhotoForgeApp, ctx: &egui::Context) -> Option<CanvasResponse> {
    // Snapshot Copy-able state to keep borrows disjoint across the closure.
    let (texture, tw, th) = match app.preview() {
        Some(p) => (Some(p.0.clone()), p.1, p.2),
        None => (None, 0, 0),
    };
    let image_size = app.image().map(|i| [i.width, i.height]);
    let file_name = app
        .image()
        .map(|i| i.file_name.clone())
        .unwrap_or_else(|| "no image".to_string());
    let mut view = *app.view();

    let response = egui::CentralPanel::default()
        .show(ctx, |ui| {
            let input = CanvasInput {
                image_size,
                file_name: &file_name,
            };
            canvas::show(ui, &mut view, texture.as_ref(), [tw, th], &input)
        })
        .inner;
    *app.view() = view;
    response
}

pub fn export_dialog(app: &mut PhotoForgeApp, ctx: &egui::Context) {
    // Copy out, draw with locals, copy back (avoids borrows across egui's
    // closure).
    let ui_state = app.export_ui();
    if !*ui_state.open {
        return;
    }
    let mut open = *ui_state.open;
    let mut path = ui_state.path.clone();
    let mut quality = *ui_state.quality;
    let running = *ui_state.running;
    let progress = ui_state.progress;
    let _ = &ui_state; // end the borrow

    let mut do_export = false;
    egui::Window::new("Export")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Output:");
                ui.add_sized(
                    [ui.available_width(), 20.0],
                    egui::TextEdit::singleline(&mut path),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("JPEG quality:");
                ui.add(egui::Slider::new(&mut quality, 50..=100).suffix("%"));
            });
            ui.add_space(6.0);
            if running {
                ui.add(
                    egui::ProgressBar::new(progress)
                        .show_percentage()
                        .animate(true),
                );
                ui.weak("Exporting full resolution…");
            } else if ui.button("Export").clicked() {
                do_export = true;
            }
        });
    {
        let ui_state = app.export_ui();
        *ui_state.open = open;
        *ui_state.path = path;
        *ui_state.quality = quality;
    }
    if do_export {
        app.start_export(ctx);
    }
}

pub fn status_bar(app: &mut PhotoForgeApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(app.status())
                    .size(12.0)
                    .color(egui::Color32::from_rgb(160, 160, 168)),
            );
            if let Some(img) = app.image() {
                ui.separator();
                let exif = &img.exif;
                let mut parts: Vec<String> = Vec::new();
                let cam = format!("{} {}", exif.camera_make, exif.camera_model)
                    .trim()
                    .to_string();
                if !cam.is_empty() {
                    parts.push(cam);
                }
                if !exif.iso.is_empty() && exif.iso != "0" {
                    parts.push(format!("ISO {}", exif.iso));
                }
                if !exif.exposure_time.is_empty() {
                    parts.push(exif.exposure_time.clone());
                }
                if !exif.f_number.is_empty() {
                    parts.push(exif.f_number.clone());
                }
                if !exif.focal_length.is_empty() {
                    parts.push(exif.focal_length.clone());
                }
                if !parts.is_empty() {
                    ui.weak(parts.join("  ·  "));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak("M4 preview — curves, filters and masks land in M5–M7");
            });
        });
    });
}
