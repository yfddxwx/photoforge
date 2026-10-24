//! Reusable parameter slider row: drag = live (interactive) update,
//! double-click = reset to default.

pub struct ParamRow<'a> {
    pub label: &'a str,
    pub value: &'a mut f32,
    pub default: f32,
    pub min: f32,
    pub max: f32,
    /// Formats the value (defaults to 2-decimals).
    pub format: Option<fn(f32) -> String>,
}

impl ParamRow<'_> {
    /// Draws the row. Returns `(changed_interactively, committed)`.
    pub fn show(self, ui: &mut egui::Ui) -> (bool, bool) {
        let Self {
            label,
            value,
            default,
            min,
            max,
            format,
        } = self;
        let mut changed = false;
        let mut committed = false;

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(12.5)
                    .color(egui::Color32::from_rgb(190, 190, 196)),
            );
            let slider = egui::Slider::new(value, min..=max)
                .show_value(false)
                .smart_aim(false);
            let resp = ui.add_sized(ui.available_size(), slider);
            if resp.changed() {
                changed = true;
            }
            if resp.drag_stopped() {
                committed = true;
            }
            if resp.double_clicked() && *value != default {
                *value = default;
                changed = true;
                committed = true;
            }
            let text = match format {
                Some(f) => f(*value),
                None => format!("{value:.2}"),
            };
            ui.monospace(
                egui::RichText::new(text)
                    .size(11.5)
                    .color(egui::Color32::from_rgb(150, 150, 158)),
            );
        });
        (changed, committed)
    }
}

/// Convenience formats.
pub fn fmt_stops(v: f32) -> String {
    format!("{v:+.2} EV")
}

pub fn fmt_signed(v: f32) -> String {
    format!("{v:+.2}")
}
