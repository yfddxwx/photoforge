//! Application state and the egui update loop.

use std::path::PathBuf;
use std::sync::Arc;

use photoforge_core::histogram::Histogram;
use photoforge_io::{ExifSummary, ImagePyramid};
use photoforge_pipeline::EditState;

use crate::canvas::{CanvasResponse, ViewState};
use crate::worker::{Frame, Request, Worker};

/// A rendered preview ready for display.
struct Preview {
    rev: u64,
    texture: egui::TextureHandle,
    width: usize,
    height: usize,
    ms: f32,
    level: usize,
}

pub struct OpenImage {
    pub path: PathBuf,
    pub file_name: String,
    pub exif: ExifSummary,
    pub width: u32,
    pub height: u32,
    pub pyramid: Arc<ImagePyramid>,
}

pub struct PhotoForgeApp {
    worker: Worker,
    /// UI-side source of truth; cloned into each render request.
    state: Arc<EditState>,
    state_rev: u64,
    inflight_rev: u64,
    interacting: bool,

    image: Option<OpenImage>,
    view: ViewState,
    preview: Option<Preview>,
    hist: Option<Histogram>,
    render_ms: f32,

    status: String,
    export_open: bool,
    export_path: String,
    export_quality: u8,
    export_running: bool,
    export_progress: f32,
    show_exif: bool,
}

impl PhotoForgeApp {
    pub fn new(cc: &eframe::CreationContext<'_>, image_path: Option<PathBuf>) -> Self {
        let mut app = Self {
            worker: Worker::spawn(),
            state: Arc::new(EditState::with_global_stack()),
            state_rev: 1,
            inflight_rev: 0,
            interacting: false,
            image: None,
            view: ViewState::default(),
            preview: None,
            hist: None,
            render_ms: 0.0,
            status: "Open an image (Ctrl+O) to start editing.".into(),
            export_open: false,
            export_path: String::new(),
            export_quality: 90,
            export_running: false,
            export_progress: 0.0,
            show_exif: false,
        };
        if let Some(path) = image_path {
            app.request_open(path);
        }
        // Keep ticking even without user input so worker frames arrive.
        cc.egui_ctx
            .request_repaint_after(std::time::Duration::from_millis(33));
        app
    }

    fn request_open(&mut self, path: PathBuf) {
        if self.worker.tx.send(Request::OpenPath(path)).is_err() {
            self.status = "Worker died (bug)".into();
        }
    }

    pub fn set_status_pub(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    /// Public entry for panel-initiated opens.
    pub fn request_open_pub(&mut self, path: PathBuf) {
        self.request_open(path);
    }

    /// Bumps the state revision and schedules a render.
    fn state_changed(&mut self, ctx: &egui::Context, interacting: bool) {
        self.state_rev += 1;
        self.interacting = interacting;
        self.send_render(ctx);
    }

    fn send_render(&mut self, ctx: &egui::Context) {
        let target_px = self.view.target_px_hint().max(64.0) as u32;
        self.inflight_rev = self.state_rev;
        let _ = self.worker.tx.send(Request::Render {
            rev: self.state_rev,
            state: self.state.clone(),
            target_px,
            interacting: self.interacting,
        });
        ctx.request_repaint();
    }

    fn poll_worker(&mut self, ctx: &egui::Context) {
        loop {
            match self.worker.rx.try_recv() {
                Ok(Frame::ImageLoaded(payload)) => {
                    let (path, exif, width, height, pyramid) = (
                        payload.path,
                        payload.exif,
                        payload.width,
                        payload.height,
                        payload.pyramid,
                    );
                    self.image = Some(OpenImage {
                        file_name: path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        path,
                        exif,
                        width,
                        height,
                        pyramid,
                    });
                    self.view = ViewState::default();
                    self.status = format!(
                        "Loaded {} ({}×{})",
                        self.image.as_ref().expect("just set").path.display(),
                        width,
                        height
                    );
                    self.export_path = std::env::current_dir()
                        .unwrap_or_default()
                        .join("export.jpg")
                        .to_string_lossy()
                        .to_string();
                    self.send_render(ctx);
                }
                Ok(Frame::LoadFailed(msg)) => {
                    self.status = format!("Failed to open image: {msg}");
                }
                Ok(Frame::Rendered {
                    rev,
                    buf,
                    hist,
                    level,
                    ms,
                    interacting,
                }) => {
                    self.render_ms = ms;
                    self.hist = Some(*hist);
                    if rev >= self.preview.as_ref().map_or(0, |p| p.rev) {
                        let texture = ctx.load_texture(
                            "preview",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [buf.width() as usize, buf.height() as usize],
                                buf.as_raw(),
                            ),
                            egui::TextureOptions::LINEAR,
                        );
                        self.preview = Some(Preview {
                            rev,
                            texture,
                            width: buf.width() as usize,
                            height: buf.height() as usize,
                            ms,
                            level,
                        });
                    }
                    // When an interaction frame lands and dragging is over,
                    // request the refined frame.
                    if interacting && !self.interacting {
                        self.send_render(ctx);
                    }
                    ctx.request_repaint();
                }
                Ok(Frame::ExportProgress(p)) => {
                    self.export_progress = p;
                    ctx.request_repaint();
                }
                Ok(Frame::ExportDone(result)) => {
                    self.export_running = false;
                    match result {
                        Ok(p) => self.status = format!("Exported {}", p.display()),
                        Err(e) => self.status = format!("Export failed: {e}"),
                    }
                    ctx.request_repaint();
                }
                Err(_) => break,
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }

    fn open_file_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Images",
                &["jpg", "jpeg", "png", "webp", "tif", "tiff", "bmp"],
            )
            .pick_file()
        {
            self.request_open(path);
        }
    }
}

impl eframe::App for PhotoForgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker(ctx);

        // Global shortcuts.
        ctx.input(|i| {
            if i.modifiers.ctrl && i.key_pressed(egui::Key::O) {
                self.open_file_dialog();
            }
        });
        if self.image.is_some() {
            ctx.input(|i| {
                if i.key_pressed(egui::Key::F) {
                    self.view.fit = true;
                }
                if i.key_pressed(egui::Key::Num1) {
                    self.view.zoom = 1.0;
                    self.view.fit = false;
                }
            });
        }

        crate::panels::top_bar(self, ctx);
        crate::panels::adjustment_panel(self, ctx);
        match crate::panels::central_canvas(self, ctx) {
            Some(CanvasResponse::ViewChanged { interacting }) => {
                self.state_changed(ctx, interacting);
            }
            Some(CanvasResponse::Open) => self.open_file_dialog(),
            None => {}
        }
        crate::panels::export_dialog(self, ctx);
        crate::panels::status_bar(self, ctx);
    }
}

// ---- Accessors used by the panel modules ----
impl PhotoForgeApp {
    pub fn state(&self) -> &EditState {
        &self.state
    }

    pub fn image(&self) -> Option<&OpenImage> {
        self.image.as_ref()
    }

    pub fn preview(&self) -> Option<(&egui::TextureHandle, usize, usize, f32, usize)> {
        self.preview
            .as_ref()
            .map(|p| (&p.texture, p.width, p.height, p.ms, p.level))
    }

    pub fn histogram(&self) -> Option<&Histogram> {
        self.hist.as_ref()
    }

    pub fn render_ms(&self) -> f32 {
        self.render_ms
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn view(&mut self) -> &mut ViewState {
        &mut self.view
    }

    pub fn show_exif(&mut self) -> &mut bool {
        &mut self.show_exif
    }

    /// Mutates the edit state through `f` and schedules a render.
    pub fn edit_state(
        &mut self,
        ctx: &egui::Context,
        interacting: bool,
        f: impl FnOnce(&mut EditState),
    ) {
        let state = Arc::make_mut(&mut self.state);
        f(state);
        self.state_changed(ctx, interacting);
    }

    pub fn export_ui(&mut self) -> ExportUi<'_> {
        ExportUi {
            open: &mut self.export_open,
            path: &mut self.export_path,
            quality: &mut self.export_quality,
            running: &mut self.export_running,
            progress: self.export_progress,
        }
    }

    pub fn start_export(&mut self, ctx: &egui::Context) {
        let Some(img) = &self.image else {
            return;
        };
        self.export_running = true;
        self.export_progress = 0.0;
        let _ = self.worker.tx.send(Request::Export {
            state: self.state.clone(),
            pyramid: img.pyramid.clone(),
            path: PathBuf::from(&self.export_path),
            quality_jpeg: self.export_quality,
        });
        ctx.request_repaint();
    }
}

pub struct ExportUi<'a> {
    pub open: &'a mut bool,
    pub path: &'a mut String,
    pub quality: &'a mut u8,
    pub running: &'a mut bool,
    pub progress: f32,
}
