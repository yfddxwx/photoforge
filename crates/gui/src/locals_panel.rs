//! M7: geometry (crop session toolbar) and local-adjustments sections.

use photoforge_ops::params::OpKind;
use photoforge_ops::OpParams;

use crate::app::PhotoForgeApp;
use crate::panels::{binding, param_row};
use crate::slider::{fmt_signed, ParamRow};

pub fn geometry_section(app: &mut PhotoForgeApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let cropping = app.crop_session.is_some();
        let label = if cropping {
            "Crop: editing…"
        } else {
            "Crop (C)"
        };
        if ui
            .add_enabled(app.image().is_some(), egui::Button::new(label))
            .clicked()
        {
            let (w, h) = app
                .image()
                .map(|im| (im.width, im.height))
                .unwrap_or((0, 0));
            let straighten = app.state().geometry.straighten_deg;
            app.crop_session = Some(crate::canvas::CropSession::full(w, h, straighten));
            app.bump_rev();
            app.send_render(ui.ctx());
        }
        if cropping {
            if ui.button("Apply (Enter)").clicked() {
                let ctx = ui.ctx().clone();
                if let Some(session) = app.crop_session.take() {
                    let (x, y, w, h) = session.rect_i();
                    app.tool = crate::canvas::Tool::Navigate;
                    app.edit_state(&ctx, false, |s| {
                        s.geometry.straighten_deg = session.straighten;
                        s.geometry.crop = Some(photoforge_pipeline::CropRect { x, y, w, h });
                    });
                    app.set_status_pub(format!("Crop applied ({w}×{h})"));
                }
            }
            if ui.button("Cancel (Esc)").clicked() {
                let ctx = ui.ctx().clone();
                app.crop_session = None;
                app.edit_state(&ctx, false, |_| {});
                app.set_status_pub("Crop cancelled");
            }
        }
    });
    if let Some(session) = app.crop_session.as_mut() {
        ui.horizontal(|ui| {
            ui.label("Aspect:");
            let aspects = [
                ("Free", None),
                ("1:1", Some(1.0)),
                ("4:3", Some(4.0 / 3.0)),
                ("3:2", Some(3.0 / 2.0)),
                ("16:9", Some(16.0 / 9.0)),
            ];
            for (label, a) in aspects {
                if ui.selectable_label(session.aspect == a, label).clicked() {
                    session.aspect = a;
                }
            }
        });
        let mut deg = session.straighten;
        let row = ParamRow {
            label: "Straighten",
            value: &mut deg,
            default: 0.0,
            min: -15.0,
            max: 15.0,
            format: Some(|v| format!("{v:+.1}°")),
        };
        let (changed, committed) = row.show(ui);
        if changed {
            session.straighten = deg;
            app.state_changed(ui.ctx(), true);
        }
        if committed {
            app.state_changed(ui.ctx(), false);
        }
        ui.weak("Enter applies · Esc cancels · drag inside to move, edges to resize");
    } else {
        ui.weak(format!(
            "crop: {:?} · straighten: {:+.1}°",
            app.state()
                .geometry
                .crop
                .map(|c| (c.w, c.h))
                .unwrap_or((0, 0)),
            app.state().geometry.straighten_deg
        ));
    }
}

type LocalKind = (&'static str, fn() -> OpParams);

const LOCAL_KINDS: [LocalKind; 6] = [
    ("Exposure", || OpParams::Exposure { stops: 0.0 }),
    ("Contrast", || OpParams::Contrast { amount: 0.0 }),
    ("White Balance", || OpParams::WhiteBalance {
        temp: 0.0,
        tint: 0.0,
    }),
    ("Saturation", || OpParams::Saturation { amount: 0.0 }),
    ("Clarity", || OpParams::Clarity { amount: 0.0 }),
    ("Shadows", || OpParams::Shadows { amount: 0.0 }),
];

pub fn local_adjustments_section(app: &mut PhotoForgeApp, ui: &mut egui::Ui) {
    let n_global = photoforge_ops::params::GLOBAL_OP_ORDER.len();
    let n_locals = app.state().adjustments.len().saturating_sub(n_global);

    ui.horizontal(|ui| {
        ui.menu_button("+ Add local", |ui| {
            for (name, make) in LOCAL_KINDS {
                if ui.button(name).clicked() {
                    ui.close_menu();
                    app.add_local_adjustment(ui.ctx(), make());
                }
            }
        });
        if n_locals > 0 {
            ui.weak(format!("{n_locals} local"));
        }
    });

    for li in 0..n_locals {
        let idx = n_global + li;
        let (name, enabled) = {
            let adj = &app.state().adjustments[idx];
            (adj.op.label(), adj.enabled)
        };
        let selected = app.active_local == Some(idx);
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    selected,
                    format!("{} {}", if enabled { "●" } else { "○" }, name),
                )
                .clicked()
            {
                app.active_local = if selected { None } else { Some(idx) };
            }
            if ui.small_button("✕").clicked() {
                app.remove_local_adjustment(ui.ctx(), idx);
            }
        });
    }

    if let Some(idx) = app.active_local {
        if idx < app.state().adjustments.len() {
            ui.separator();
            ui.strong("Mask");
            ui.horizontal(|ui| {
                let t = app.tool;
                if ui
                    .selectable_label(t == crate::canvas::Tool::Brush, "Brush (B)")
                    .clicked()
                {
                    app.tool = crate::canvas::Tool::Brush;
                }
                if ui
                    .selectable_label(t == crate::canvas::Tool::LinearGrad, "Linear (G)")
                    .clicked()
                {
                    app.tool = crate::canvas::Tool::LinearGrad;
                }
                if ui
                    .selectable_label(t == crate::canvas::Tool::RadialGrad, "Radial (R)")
                    .clicked()
                {
                    app.tool = crate::canvas::Tool::RadialGrad;
                }
                if ui.button("Clear mask").clicked() {
                    app.edit_state(ui.ctx(), false, |s| {
                        if let Some(adj) = s.adjustments.get_mut(idx) {
                            adj.mask = Some(photoforge_pipeline::Mask::Brush {
                                strokes: Vec::new(),
                            });
                        }
                    });
                }
            });
            if app.tool == crate::canvas::Tool::Brush {
                let mut radius = app.brush.radius;
                let mut hardness = app.brush.hardness;
                let mut flow = app.brush.flow;
                let mut erase = app.brush.erase;
                ui.horizontal(|ui| {
                    ui.label("Size:");
                    ui.add(egui::Slider::new(&mut radius, 0.01..=0.4).show_value(false));
                });
                ui.horizontal(|ui| {
                    ui.label("Hardness:");
                    ui.add(egui::Slider::new(&mut hardness, 0.0..=1.0).show_value(false));
                });
                ui.horizontal(|ui| {
                    ui.label("Flow:");
                    ui.add(egui::Slider::new(&mut flow, 0.05..=1.0).show_value(false));
                });
                if ui.checkbox(&mut erase, "Erase").changed() {
                    app.brush.erase = erase;
                }
                app.brush.radius = radius;
                app.brush.hardness = hardness;
                app.brush.flow = flow;
            }
            ui.separator();
            ui.strong("Parameters");
            local_param_rows(app, ui, idx);
        }
    } else {
        ui.weak("Add a local adjustment, then paint a mask with B / G / R tools.");
    }
}

type StateGet = Box<dyn Fn(&photoforge_pipeline::EditState) -> f32>;
type StateSet = Box<dyn Fn(&mut photoforge_pipeline::EditState, f32)>;

fn local_param_rows(app: &mut PhotoForgeApp, ui: &mut egui::Ui, idx: usize) {
    let kind = app
        .state()
        .adjustments
        .get(idx)
        .map(|a| a.op.kind())
        .unwrap_or(OpKind::Exposure);
    let get: StateGet = match kind {
        OpKind::Exposure => Box::new(move |s: &photoforge_pipeline::EditState| {
            s.adjustments.get(idx).map_or(0.0, |adj| match &adj.op {
                OpParams::Exposure { stops } => *stops,
                _ => 0.0,
            })
        }),
        OpKind::WhiteBalance => Box::new(move |s: &photoforge_pipeline::EditState| {
            s.adjustments.get(idx).map_or(0.0, |adj| match &adj.op {
                OpParams::WhiteBalance { temp, .. } => *temp,
                _ => 0.0,
            })
        }),
        OpKind::Saturation => Box::new(move |s: &photoforge_pipeline::EditState| {
            s.adjustments.get(idx).map_or(0.0, |adj| match &adj.op {
                OpParams::Saturation { amount } => *amount,
                _ => 0.0,
            })
        }),
        _ => Box::new(move |s: &photoforge_pipeline::EditState| {
            s.adjustments.get(idx).map_or(0.0, |adj| match &adj.op {
                OpParams::Contrast { amount }
                | OpParams::Clarity { amount }
                | OpParams::Shadows { amount } => *amount,
                _ => 0.0,
            })
        }),
    };
    let set: StateSet = match kind {
        OpKind::Exposure => Box::new(move |s: &mut photoforge_pipeline::EditState, v: f32| {
            if let Some(adj) = s.adjustments.get_mut(idx) {
                if let OpParams::Exposure { stops } = &mut adj.op {
                    *stops = v;
                }
            }
        }),
        OpKind::WhiteBalance => Box::new(move |s: &mut photoforge_pipeline::EditState, v: f32| {
            if let Some(adj) = s.adjustments.get_mut(idx) {
                if let OpParams::WhiteBalance { temp, .. } = &mut adj.op {
                    *temp = v;
                }
            }
        }),
        OpKind::Saturation => Box::new(move |s: &mut photoforge_pipeline::EditState, v: f32| {
            if let Some(adj) = s.adjustments.get_mut(idx) {
                if let OpParams::Saturation { amount } = &mut adj.op {
                    *amount = v;
                }
            }
        }),
        _ => Box::new(move |s: &mut photoforge_pipeline::EditState, v: f32| {
            if let Some(adj) = s.adjustments.get_mut(idx) {
                match &mut adj.op {
                    OpParams::Contrast { amount }
                    | OpParams::Clarity { amount }
                    | OpParams::Shadows { amount } => *amount = v,
                    _ => {}
                }
            }
        }),
    };
    let (min, max) = if kind == OpKind::Exposure {
        (-4.0, 4.0)
    } else {
        (-1.0, 1.0)
    };
    param_row(app, ui, binding("Amount", min, max, fmt_signed, get, set));
}
