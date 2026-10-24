//! photoforge GUI entry point.

mod app;
mod canvas;
mod curve_editor;
mod effects_panel;
mod locals_panel;
mod panels;
mod slider;
mod worker;

use app::PhotoForgeApp;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let image_path = std::env::args().nth(1).map(std::path::PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("photoforge"),
        ..Default::default()
    };
    eframe::run_native(
        "photoforge",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(darkroom_visuals());
            Ok(Box::new(PhotoForgeApp::new(cc, image_path)))
        }),
    )
}

/// The darkroom look: near-black panels, low-contrast chrome, restrained
/// accents.
fn darkroom_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = egui::Color32::from_rgb(22, 22, 25);
    v.window_fill = egui::Color32::from_rgb(26, 26, 30);
    v.extreme_bg_color = egui::Color32::from_rgb(12, 12, 14); // sliders, text edits
    v.faint_bg_color = egui::Color32::from_rgb(28, 28, 32);
    v.selection.bg_fill = egui::Color32::from_rgb(70, 110, 160);
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(40, 40, 46);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(52, 52, 60);
    v.override_text_color = Some(egui::Color32::from_rgb(210, 210, 214));
    v
}
