//! Panels: top bar, adjustment panel (+ histogram), canvas host, export
//! dialog, status bar.

use photoforge_ops::params::GLOBAL_OP_ORDER;
use photoforge_ops::OpParams;

use crate::app::PhotoForgeApp;
use crate::canvas::{self, CanvasEvent, CanvasInput};
use crate::curve_editor::{self, CurveChannel};
use crate::slider::{fmt_plain, fmt_signed, fmt_stops, ParamRow};

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
                .add_enabled(app.can_undo(), egui::Button::new("Undo (Ctrl+Z)"))
                .clicked()
            {
                app.undo(ctx);
            }
            if ui
                .add_enabled(app.can_redo(), egui::Button::new("Redo (Ctrl+Y)"))
                .clicked()
            {
                app.redo(ctx);
            }
            if ui
                .add_enabled(
                    app.image().is_some(),
                    egui::Button::new("Save .pfg (Ctrl+S)"),
                )
                .clicked()
            {
                app.save_sidecar();
            }
            if ui.button("Open Folder…").clicked() {
                app.open_folder();
            }
            ui.separator();
            ui.menu_button("Preset", |ui| {
                if ui.button("Apply from file…").clicked() {
                    apply_preset_dialog(app, ctx);
                }
                if ui
                    .add_enabled(
                        app.image().is_some(),
                        egui::Button::new("Save global stack…"),
                    )
                    .clicked()
                {
                    save_preset_dialog(app, ctx);
                }
            });
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
                ui.strong("Tone Curve");
                ui.separator();
                curve_section(app, ui);

                ui.add_space(6.0);
                ui.strong("Levels");
                ui.separator();
                levels_section(app, ui);

                ui.add_space(6.0);
                ui.strong("Color Mixer (HSL)");
                ui.separator();
                hsl_section(app, ui);

                ui.add_space(6.0);
                ui.strong("Color");
                ui.separator();
                chroma_row(app, ui, 10, "Vibrance");
                chroma_row(app, ui, 11, "Saturation");

                ui.add_space(6.0);
                ui.strong("LUT");
                ui.separator();
                lut_section(app, ui);

                ui.add_space(6.0);
                ui.strong("Detail");
                ui.separator();
                crate::effects_panel::effect_section(app, ui, crate::effects_panel::SHARPEN);
                crate::effects_panel::effect_section(app, ui, crate::effects_panel::CLARITY);
                crate::effects_panel::effect_section(app, ui, crate::effects_panel::DENOISE);

                ui.add_space(6.0);
                ui.strong("Effects");
                ui.separator();
                crate::effects_panel::effect_section(app, ui, crate::effects_panel::BLUR);
                crate::effects_panel::effect_section(app, ui, crate::effects_panel::GRAIN);
                crate::effects_panel::effect_section(app, ui, crate::effects_panel::VIGNETTE);
                crate::effects_panel::effect_section(app, ui, crate::effects_panel::BLACK_WHITE);
                crate::effects_panel::effect_section(app, ui, crate::effects_panel::DUOTONE);

                ui.add_space(6.0);
                ui.strong("Geometry");
                ui.separator();
                crate::locals_panel::geometry_section(app, ui);

                ui.add_space(6.0);
                ui.strong("Local Adjustments");
                ui.separator();
                crate::locals_panel::local_adjustments_section(app, ui);

                ui.add_space(8.0);
                ui.weak(format!(
                    "{} global adjustments \u{2014} masks and crop land in M7",
                    GLOBAL_OP_ORDER.len()
                ));
            });
        });
}

pub type StateGetter = Box<dyn Fn(&photoforge_pipeline::EditState) -> f32>;
pub type StateSetter = Box<dyn Fn(&mut photoforge_pipeline::EditState, f32)>;

/// A bound slider row: where to read the value, how to write it, and its
/// slider range. Interactive while dragging; a refined render on release.
pub struct ParamBinding {
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub format: fn(f32) -> String,
    pub get: StateGetter,
    pub set: StateSetter,
}

pub fn param_row(app: &mut PhotoForgeApp, ui: &mut egui::Ui, b: ParamBinding) {
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
pub fn binding(
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

// --- M5 sections ---

fn curve_points_channel(s: &photoforge_pipeline::EditState, ch: CurveChannel) -> Vec<[f32; 2]> {
    s.adjustments
        .get(7)
        .and_then(|a| match &a.op {
            OpParams::ToneCurve { r, g, b, l } => Some(match ch {
                CurveChannel::R => r.clone(),
                CurveChannel::G => g.clone(),
                CurveChannel::B => b.clone(),
                CurveChannel::L => l.clone(),
            }),
            _ => None,
        })
        .unwrap_or_default()
}

fn set_curve_points_channel(
    s: &mut photoforge_pipeline::EditState,
    ch: CurveChannel,
    pts: Vec<[f32; 2]>,
) {
    if let Some(adj) = s.adjustments.get_mut(7) {
        if let OpParams::ToneCurve { r, g, b, l } = &mut adj.op {
            match ch {
                CurveChannel::R => *r = pts,
                CurveChannel::G => *g = pts,
                CurveChannel::B => *b = pts,
                CurveChannel::L => *l = pts,
            }
        }
    }
}

fn curve_section(app: &mut PhotoForgeApp, ui: &mut egui::Ui) {
    let channel_id = egui::Id::new("curve_channel");
    let mut channel = ui.ctx().data_mut(|d| {
        d.get_temp::<CurveChannel>(channel_id)
            .unwrap_or(CurveChannel::L)
    });
    ui.horizontal(|ui| {
        for ch in CurveChannel::ALL {
            // ch: CurveChannel (Copy)
            if ui
                .selectable_label(
                    channel == ch,
                    egui::RichText::new(ch.label()).color(ch.color()),
                )
                .clicked()
            {
                channel = ch;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Reset").clicked() {
                let mut empty = Vec::new();
                app.edit_state(ui.ctx(), false, |s| {
                    set_curve_points_channel(s, channel, std::mem::take(&mut empty));
                });
            }
        });
    });
    ui.ctx().data_mut(|d| d.insert_temp(channel_id, channel));

    let mut points = curve_points_channel(app.state(), channel);
    let edit = curve_editor::show(ui, &mut points, channel);
    if edit.changed {
        app.edit_state(ui.ctx(), true, |s| {
            set_curve_points_channel(s, channel, points.clone());
        });
    }
    if edit.committed {
        app.edit_state(ui.ctx(), false, |_| {});
    }
}

fn levels_section(app: &mut PhotoForgeApp, ui: &mut egui::Ui) {
    let levels_binding = |label: &'static str,
                          min: f32,
                          max: f32,
                          get: fn(&OpParams) -> f32,
                          set: fn(&mut OpParams, f32)| {
        binding(
            label,
            min,
            max,
            fmt_plain,
            move |s: &photoforge_pipeline::EditState| {
                s.adjustments
                    .get(8)
                    .and_then(|a| match &a.op {
                        OpParams::Levels { .. } => Some(get(&a.op)),
                        _ => None,
                    })
                    .unwrap_or(0.0)
            },
            move |s: &mut photoforge_pipeline::EditState, v: f32| {
                if let Some(adj) = s.adjustments.get_mut(8) {
                    set(&mut adj.op, v);
                }
            },
        )
    };
    let black = levels_binding(
        "Black",
        0.0,
        0.4,
        |p| match p {
            OpParams::Levels { black, .. } => *black,
            _ => 0.0,
        },
        |p, v| {
            if let OpParams::Levels { black, .. } = p {
                *black = v;
            }
        },
    );
    param_row(app, ui, black);
    let gamma = levels_binding(
        "Gamma",
        0.25,
        4.0,
        |p| match p {
            OpParams::Levels { gamma, .. } => *gamma,
            _ => 1.0,
        },
        |p, v| {
            if let OpParams::Levels { gamma, .. } = p {
                *gamma = v;
            }
        },
    );
    param_row(app, ui, gamma);
    let white = levels_binding(
        "White",
        0.6,
        1.0,
        |p| match p {
            OpParams::Levels { white, .. } => *white,
            _ => 1.0,
        },
        |p, v| {
            if let OpParams::Levels { white, .. } = p {
                *white = v;
            }
        },
    );
    param_row(app, ui, white);

    ui.horizontal(|ui| {
        if ui
            .add_enabled(app.histogram().is_some(), egui::Button::new("Auto"))
            .clicked()
        {
            if let Some(hist) = app.histogram() {
                let b = f32::from(hist.luma_percentile(0.001)) / 255.0;
                let w = f32::from(hist.luma_percentile(0.999)) / 255.0;
                let (b, w) = if w - b < 0.05 { (0.0, 1.0) } else { (b, w) };
                app.edit_state(ui.ctx(), false, |s| {
                    if let Some(adj) = s.adjustments.get_mut(8) {
                        if let OpParams::Levels { black, white, .. } = &mut adj.op {
                            *black = b;
                            *white = w;
                        }
                    }
                });
            }
        }
        if ui.small_button("Reset").clicked() {
            app.edit_state(ui.ctx(), false, |s| {
                if let Some(adj) = s.adjustments.get_mut(8) {
                    adj.op = OpParams::Levels {
                        black: 0.0,
                        gamma: 1.0,
                        white: 1.0,
                    };
                }
            });
        }
    });
}

fn hsl_section(app: &mut PhotoForgeApp, ui: &mut egui::Ui) {
    let zone_id = egui::Id::new("hsl_zone");
    let mut zone = ui
        .ctx()
        .data_mut(|d| d.get_temp::<u8>(zone_id).unwrap_or(0)) as usize;
    ui.horizontal_wrapped(|ui| {
        for (i, name) in photoforge_ops::hsl::ZONE_NAMES.iter().enumerate() {
            if ui.selectable_label(zone == i, *name).clicked() {
                zone = i;
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(zone_id, zone as u8));

    let zone_copy = zone;
    let zone_get = move |s: &photoforge_pipeline::EditState, which: usize| {
        s.adjustments
            .get(9)
            .and_then(|a| match &a.op {
                OpParams::Hsl { zones } => Some(zones[zone_copy][which]),
                _ => None,
            })
            .unwrap_or(0.0)
    };
    let zone_set = move |s: &mut photoforge_pipeline::EditState, which: usize, v: f32| {
        if let Some(adj) = s.adjustments.get_mut(9) {
            if let OpParams::Hsl { zones } = &mut adj.op {
                zones[zone_copy][which] = v;
            }
        }
    };

    let hue = binding(
        "Hue",
        -30.0,
        30.0,
        fmt_signed,
        move |s| zone_get(s, 0),
        move |s, v| zone_set(s, 0, v),
    );
    param_row(app, ui, hue);
    let sat = binding(
        "Sat",
        -1.0,
        1.0,
        fmt_signed,
        move |s| zone_get(s, 1),
        move |s, v| zone_set(s, 1, v),
    );
    param_row(app, ui, sat);
    let lum = binding(
        "Lum",
        -1.0,
        1.0,
        fmt_signed,
        move |s| zone_get(s, 2),
        move |s, v| zone_set(s, 2, v),
    );
    param_row(app, ui, lum);
}

fn lut_section(app: &mut PhotoForgeApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.button("Load .cube…").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Cube LUT", &["cube"])
                .pick_file()
            {
                match photoforge_ops::lut::CubeLut::from_file(&path) {
                    Ok(lut) => {
                        let name = if lut.name.is_empty() {
                            path.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default()
                        } else {
                            lut.name.clone()
                        };
                        let (size, table) = (lut.size, lut.table);
                        app.edit_state(ui.ctx(), false, |s| {
                            if let Some(adj) = s.adjustments.get_mut(12) {
                                adj.op = OpParams::Lut { name, size, table };
                            }
                        });
                    }
                    Err(e) => {
                        app.set_status_pub(format!("LUT load failed: {e}"));
                    }
                }
            }
        }
        let name = app
            .state()
            .adjustments
            .get(12)
            .and_then(|a| match &a.op {
                OpParams::Lut { name, table, .. } if !table.is_empty() => Some(name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "(none)".into());
        ui.weak(name);
        if ui.small_button("Clear").clicked() {
            app.edit_state(ui.ctx(), false, |s| {
                if let Some(adj) = s.adjustments.get_mut(12) {
                    adj.op = OpParams::Lut {
                        name: String::new(),
                        size: 0,
                        table: Vec::new(),
                    };
                }
            });
        }
    });
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

pub fn central_canvas(app: &mut PhotoForgeApp, ctx: &egui::Context) -> Vec<CanvasEvent> {
    // Snapshot Copy-able state to keep borrows disjoint across the closure.
    let (texture, tw, th) = match app.preview() {
        Some(p) => (Some(p.0.clone()), p.1, p.2),
        None => (None, 0, 0),
    };
    let before = app.before_texture().cloned();
    let mask_overlay = app.mask_overlay.clone();
    let image_size = app.image().map(|i| [i.width, i.height]);
    let file_name = app
        .image()
        .map(|i| i.file_name.clone())
        .unwrap_or_else(|| "no image".to_string());
    let mut view = *app.view();
    let mut input = CanvasInput {
        image_size,
        file_name: &file_name,
        tool: app.tool,
        gradient_drag: &mut app.gradient_drag,
        crop: app.crop_session.as_mut(),
        before_texture: before.as_ref(),
        compare: app.compare,
        mask_overlay: mask_overlay.as_ref(),
    };

    let events = egui::CentralPanel::default()
        .show(ctx, |ui| {
            canvas::show(ui, &mut view, texture.as_ref(), [tw, th], &mut input)
        })
        .inner;
    *app.view() = view;
    events
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
                ui.weak("non-destructive: edits live in the .pfg sidecar · Ctrl+S saves");
            });
        });
    });
}

fn apply_preset_dialog(app: &mut PhotoForgeApp, ctx: &egui::Context) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("photoforge preset", &["json"])
        .pick_file()
    {
        match photoforge_presets::Preset::from_file(&path) {
            Ok(preset) => {
                let name = preset.name.clone();
                app.edit_state(ctx, false, |s| preset.apply_to(s));
                app.set_status_pub(format!("Applied preset \"{name}\""));
            }
            Err(e) => app.set_status_pub(format!("Preset load failed: {e}")),
        }
    }
}

fn save_preset_dialog(app: &mut PhotoForgeApp, _ctx: &egui::Context) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("photoforge preset", &["json"])
        .set_file_name("preset.json")
        .save_file()
    {
        let preset = photoforge_presets::Preset::from_state(app.state(), "preset");
        match preset.to_file(&path) {
            Ok(()) => app.set_status_pub(format!("Preset saved to {}", path.display())),
            Err(e) => app.set_status_pub(format!("Preset save failed: {e}")),
        }
    }
}

/// Bottom filmstrip: folder thumbnails, click to switch images.
pub fn filmstrip_panel(app: &mut PhotoForgeApp, ctx: &egui::Context) {
    if app.filmstrip.is_empty() {
        return;
    }
    egui::TopBottomPanel::bottom("filmstrip")
        .resizable(false)
        .show(ctx, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    let current = app.image().map(|i| i.path.clone());
                    let mut switch: Option<std::path::PathBuf> = None;
                    for item in &app.filmstrip {
                        let selected = current.as_ref() == Some(&item.path);
                        let resp = ui
                            .add(egui::Image::new(&item.texture).max_height(84.0))
                            .interact(egui::Sense::click());
                        if selected {
                            ui.painter().rect_stroke(
                                resp.rect,
                                2.0,
                                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(120, 170, 230)),
                            );
                        }
                        if resp.clicked() {
                            switch = Some(item.path.clone());
                        }
                        resp.on_hover_text(item.path.display().to_string());
                    }
                    if let Some(p) = switch {
                        app.switch_to_image(ctx, p);
                    }
                });
            });
        });
}
