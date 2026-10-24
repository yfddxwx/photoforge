//! Toggleable effect sections (M6): sharpen/clarity/denoise, blur, grain,
//! vignette, black & white mixer, duotone presets.

use photoforge_ops::OpParams;

use crate::app::PhotoForgeApp;
use crate::panels::param_row;
use crate::slider::{fmt_plain, fmt_signed};

type StateGet = Box<dyn Fn(&photoforge_pipeline::EditState) -> f32>;
type StateSet = Box<dyn Fn(&mut photoforge_pipeline::EditState, f32)>;

pub struct EffectSlider {
    label: &'static str,
    min: f32,
    max: f32,
    default: f32,
    get: fn(&OpParams) -> f32,
    set: fn(&mut OpParams, f32),
}

pub struct EffectDef {
    idx: usize,
    name: &'static str,
    sliders: &'static [EffectSlider],
}

macro_rules! amt {
    ($variant:ident) => {
        EffectSlider {
            label: "Amount",
            min: -1.0,
            max: 1.0,
            default: 0.0,
            get: |p| match p {
                OpParams::$variant { amount, .. } => *amount,
                _ => 0.0,
            },
            set: |p, v| {
                if let OpParams::$variant { amount, .. } = p {
                    *amount = v;
                }
            },
        }
    };
}

impl EffectDef {
    const SHARPEN: Self = Self {
        idx: 13,
        name: "Sharpening",
        sliders: &[
            amt!(Sharpen),
            EffectSlider {
                label: "Radius",
                min: 0.3,
                max: 6.0,
                default: 1.0,
                get: |p| match p {
                    OpParams::Sharpen { radius, .. } => *radius,
                    _ => 1.0,
                },
                set: |p, v| {
                    if let OpParams::Sharpen { radius, .. } = p {
                        *radius = v;
                    }
                },
            },
            EffectSlider {
                label: "Threshold",
                min: 0.0,
                max: 0.1,
                default: 0.0,
                get: |p| match p {
                    OpParams::Sharpen { threshold, .. } => *threshold,
                    _ => 0.0,
                },
                set: |p, v| {
                    if let OpParams::Sharpen { threshold, .. } = p {
                        *threshold = v;
                    }
                },
            },
        ],
    };
    const CLARITY: Self = Self {
        idx: 14,
        name: "Clarity",
        sliders: &[amt!(Clarity)],
    };
    const DENOISE: Self = Self {
        idx: 15,
        name: "Noise Reduction",
        sliders: &[EffectSlider {
            label: "Strength",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            get: |p| match p {
                OpParams::Denoise { strength } => *strength,
                _ => 0.0,
            },
            set: |p, v| {
                if let OpParams::Denoise { strength } = p {
                    *strength = v;
                }
            },
        }],
    };
    const BLUR: Self = Self {
        idx: 16,
        name: "Blur",
        sliders: &[EffectSlider {
            label: "Radius (px)",
            min: 0.0,
            max: 40.0,
            default: 0.0,
            get: |p| match p {
                OpParams::Blur { radius } => *radius,
                _ => 0.0,
            },
            set: |p, v| {
                if let OpParams::Blur { radius } = p {
                    *radius = v;
                }
            },
        }],
    };
    const BLACK_WHITE: Self = Self {
        idx: 17,
        name: "Black && White",
        sliders: &[
            EffectSlider {
                label: "Red",
                min: -1.0,
                max: 2.0,
                default: photoforge_ops::bw::BW_DEFAULTS[0],
                get: |p| match p {
                    OpParams::BlackWhite { r, .. } => *r,
                    _ => 0.0,
                },
                set: |p, v| {
                    if let OpParams::BlackWhite { r, .. } = p {
                        *r = v;
                    }
                },
            },
            EffectSlider {
                label: "Green",
                min: -1.0,
                max: 2.0,
                default: photoforge_ops::bw::BW_DEFAULTS[1],
                get: |p| match p {
                    OpParams::BlackWhite { g, .. } => *g,
                    _ => 0.0,
                },
                set: |p, v| {
                    if let OpParams::BlackWhite { g, .. } = p {
                        *g = v;
                    }
                },
            },
            EffectSlider {
                label: "Blue",
                min: -1.0,
                max: 2.0,
                default: photoforge_ops::bw::BW_DEFAULTS[2],
                get: |p| match p {
                    OpParams::BlackWhite { b, .. } => *b,
                    _ => 0.0,
                },
                set: |p, v| {
                    if let OpParams::BlackWhite { b, .. } = p {
                        *b = v;
                    }
                },
            },
        ],
    };
    const DUOTONE: Self = Self {
        idx: 18,
        name: "Duotone",
        sliders: &[],
    };
    const GRAIN: Self = Self {
        idx: 19,
        name: "Grain",
        sliders: &[
            EffectSlider {
                label: "Amount",
                min: 0.0,
                max: 1.0,
                default: 0.0,
                get: |p| match p {
                    OpParams::Grain { amount, .. } => *amount,
                    _ => 0.0,
                },
                set: |p, v| {
                    if let OpParams::Grain { amount, .. } = p {
                        *amount = v;
                    }
                },
            },
            EffectSlider {
                label: "Size",
                min: 0.5,
                max: 8.0,
                default: 2.0,
                get: |p| match p {
                    OpParams::Grain { size, .. } => *size,
                    _ => 2.0,
                },
                set: |p, v| {
                    if let OpParams::Grain { size, .. } = p {
                        *size = v;
                    }
                },
            },
            EffectSlider {
                label: "Roughness",
                min: 0.0,
                max: 1.0,
                default: 0.0,
                get: |p| match p {
                    OpParams::Grain { roughness, .. } => *roughness,
                    _ => 0.0,
                },
                set: |p, v| {
                    if let OpParams::Grain { roughness, .. } = p {
                        *roughness = v;
                    }
                },
            },
        ],
    };
    const VIGNETTE: Self = Self {
        idx: 20,
        name: "Vignette",
        sliders: &[
            amt!(Vignette),
            EffectSlider {
                label: "Midpoint",
                min: 0.0,
                max: 1.0,
                default: 0.5,
                get: |p| match p {
                    OpParams::Vignette { midpoint, .. } => *midpoint,
                    _ => 0.5,
                },
                set: |p, v| {
                    if let OpParams::Vignette { midpoint, .. } = p {
                        *midpoint = v;
                    }
                },
            },
            EffectSlider {
                label: "Feather",
                min: 0.05,
                max: 1.0,
                default: 0.5,
                get: |p| match p {
                    OpParams::Vignette { feather, .. } => *feather,
                    _ => 0.5,
                },
                set: |p, v| {
                    if let OpParams::Vignette { feather, .. } = p {
                        *feather = v;
                    }
                },
            },
        ],
    };
}

fn effect_binding(def_idx: usize, sl: &'static EffectSlider) -> crate::panels::ParamBinding {
    let (min, max, default) = (sl.min, sl.max, sl.default);
    let (get_fn, set_fn) = (sl.get, sl.set);
    let get: StateGet = Box::new(move |s: &photoforge_pipeline::EditState| {
        s.adjustments
            .get(def_idx)
            .map(|a| get_fn(&a.op))
            .unwrap_or(default)
    });
    let set: StateSet = Box::new(move |s: &mut photoforge_pipeline::EditState, v: f32| {
        if let Some(adj) = s.adjustments.get_mut(def_idx) {
            set_fn(&mut adj.op, v);
        }
    });
    let fmt: fn(f32) -> String = if (sl.max - sl.min) > 3.0 {
        fmt_plain
    } else {
        fmt_signed
    };
    crate::panels::ParamBinding {
        label: sl.label,
        min,
        max,
        default,
        format: fmt,
        get,
        set,
    }
}

pub fn effect_section(app: &mut PhotoForgeApp, ui: &mut egui::Ui, def: EffectDef) {
    let idx = def.idx;
    let enabled = app
        .state()
        .adjustments
        .get(idx)
        .map(|a| a.enabled)
        .unwrap_or(false);
    let mut toggle = enabled;
    if ui.checkbox(&mut toggle, def.name).changed() && toggle != enabled {
        app.edit_state(ui.ctx(), false, |s| {
            if let Some(adj) = s.adjustments.get_mut(idx) {
                adj.enabled = toggle;
            }
        });
    }
    if !enabled {
        return;
    }
    ui.indent(idx as u64, |ui| {
        for sl in def.sliders {
            param_row(app, ui, effect_binding(idx, sl));
        }
        if idx == 18 {
            duotone_buttons(app, ui);
        }
    });
}

fn duotone_buttons(app: &mut PhotoForgeApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.small_button("Sepia").clicked() {
            app.edit_state(ui.ctx(), false, |s| {
                if let Some(adj) = s.adjustments.get_mut(18) {
                    adj.op = OpParams::Duotone {
                        shadow: [0.06, 0.03, 0.0],
                        highlight: [1.0, 0.92, 0.78],
                    };
                }
            });
        }
        if ui.small_button("Cool").clicked() {
            app.edit_state(ui.ctx(), false, |s| {
                if let Some(adj) = s.adjustments.get_mut(18) {
                    adj.op = OpParams::Duotone {
                        shadow: [0.0, 0.02, 0.08],
                        highlight: [0.85, 0.93, 1.0],
                    };
                }
            });
        }
        if ui.small_button("B&W").clicked() {
            app.edit_state(ui.ctx(), false, |s| {
                if let Some(adj) = s.adjustments.get_mut(18) {
                    adj.op = OpParams::Duotone {
                        shadow: [0.0, 0.0, 0.0],
                        highlight: [1.0, 1.0, 1.0],
                    };
                }
            });
        }
    });
}

pub const SHARPEN: EffectDef = EffectDef::SHARPEN;
pub const CLARITY: EffectDef = EffectDef::CLARITY;
pub const DENOISE: EffectDef = EffectDef::DENOISE;
pub const BLUR: EffectDef = EffectDef::BLUR;
pub const GRAIN: EffectDef = EffectDef::GRAIN;
pub const VIGNETTE: EffectDef = EffectDef::VIGNETTE;
pub const BLACK_WHITE: EffectDef = EffectDef::BLACK_WHITE;
pub const DUOTONE: EffectDef = EffectDef::DUOTONE;
