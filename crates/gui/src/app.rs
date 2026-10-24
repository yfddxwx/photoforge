//! Application state and the egui update loop.

use std::path::PathBuf;
use std::sync::Arc;

use photoforge_core::histogram::Histogram;
use photoforge_io::{ExifSummary, ImagePyramid};
use photoforge_ops::OpParams;
use photoforge_pipeline::mask::{Mask, Stroke};
use photoforge_pipeline::EditState;

use crate::canvas::{BrushParams, CanvasEvent, Compare, CropSession, Tool, ViewState};
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
    state: Arc<EditState>,
    state_rev: u64,
    interacting: bool,

    image: Option<OpenImage>,
    view: ViewState,
    preview: Option<Preview>,
    hist: Option<Histogram>,
    render_ms: f32,
    before_texture: Option<egui::TextureHandle>,

    // Tools (M7).
    pub tool: Tool,
    pub brush: BrushParams,
    pub gradient_drag: Option<[f32; 2]>,
    pub compare: Compare,
    /// Index into `state.adjustments` of the local adjustment being masked.
    pub active_local: Option<usize>,
    /// Crop tool session; present while the crop tool is active.
    pub crop_session: Option<CropSession>,
    /// Mask overlay texture (rebuilt on mask edits / selection changes).
    pub mask_overlay: Option<egui::TextureHandle>,
    mask_overlay_key: (u64, [u32; 2]),

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
            interacting: false,
            image: None,
            view: ViewState::default(),
            preview: None,
            hist: None,
            render_ms: 0.0,
            before_texture: None,
            tool: Tool::Navigate,
            brush: BrushParams::default(),
            gradient_drag: None,
            compare: Compare::Off,
            active_local: None,
            crop_session: None,
            mask_overlay: None,
            mask_overlay_key: (0, [0, 0]),
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
        let _ = cc;
        app
    }

    pub fn request_open_pub(&mut self, path: PathBuf) {
        self.request_open(path);
    }

    fn request_open(&mut self, path: PathBuf) {
        if self.worker.tx.send(Request::OpenPath(path)).is_err() {
            self.status = "Worker died (bug)".into();
        }
    }

    /// Bumps the state revision and schedules a render.
    pub fn state_changed(&mut self, ctx: &egui::Context, interacting: bool) {
        self.state_rev += 1;
        self.interacting = interacting;
        self.send_render(ctx);
    }

    pub fn send_render(&mut self, ctx: &egui::Context) {
        let target_px = self.view.target_px_hint().max(64.0) as u32;
        let mut state = self.state.clone();
        if self.crop_session.is_some() {
            // The crop tool needs the pre-crop frame as the canvas backdrop.
            Arc::make_mut(&mut state).geometry.crop = None;
        }
        let _ = self.worker.tx.send(Request::Render {
            rev: self.state_rev,
            state,
            target_px,
            interacting: self.interacting,
        });
        ctx.request_repaint();
    }

    /// Adds a local adjustment of the given op kind (ADR-0008) and selects it.
    pub fn add_local_adjustment(&mut self, ctx: &egui::Context, op: OpParams) {
        let adj = photoforge_pipeline::Adjustment::masked(
            op,
            Mask::Brush {
                strokes: Vec::new(),
            },
        );
        self.edit_state(ctx, false, |s| {
            s.adjustments.push(adj);
        });
        self.active_local = Some(self.state.adjustments.len() - 1);
    }

    pub fn remove_local_adjustment(&mut self, ctx: &egui::Context, idx: usize) {
        self.edit_state(ctx, false, |s| {
            if idx >= photoforge_ops::params::GLOBAL_OP_ORDER.len() && idx < s.adjustments.len() {
                s.adjustments.remove(idx);
            }
        });
        self.active_local = None;
        self.mask_overlay = None;
    }

    /// Appends a brush stroke sample to the active local adjustment's mask.
    pub fn push_stroke_point(&mut self, pt: [f32; 2]) {
        let Some(idx) = self.active_local else {
            return;
        };
        let state = Arc::make_mut(&mut self.state);
        let Some(adj) = state.adjustments.get_mut(idx) else {
            return;
        };
        let brush = self.brush;
        let mask = adj.mask.get_or_insert_with(|| Mask::Brush {
            strokes: Vec::new(),
        });
        if let Mask::Brush { strokes } = mask {
            let need_new = match strokes.last() {
                Some(last) => last.erase != brush.erase,
                None => true,
            };
            if need_new {
                strokes.push(Stroke {
                    points: vec![pt],
                    radius: brush.radius,
                    hardness: brush.hardness,
                    flow: brush.flow,
                    erase: brush.erase,
                });
            } else if let Some(last) = strokes.last_mut() {
                last.points.push(pt);
            }
        }
    }

    /// Finishes the current stroke (commits a refined render).
    pub fn end_stroke(&mut self, ctx: &egui::Context) {
        self.state_changed(ctx, false);
    }

    /// Sets a gradient mask on the active local adjustment.
    pub fn set_gradient_mask(
        &mut self,
        ctx: &egui::Context,
        linear: bool,
        start: [f32; 2],
        end: [f32; 2],
    ) {
        let Some(idx) = self.active_local else {
            return;
        };
        self.edit_state(ctx, false, |s| {
            if let Some(adj) = s.adjustments.get_mut(idx) {
                adj.mask = Some(if linear {
                    Mask::LinearGradient { start, end }
                } else {
                    Mask::RadialGradient {
                        center: [(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0],
                        radius: [
                            ((end[0] - start[0]).abs() / 2.0).max(0.02),
                            ((end[1] - start[1]).abs() / 2.0).max(0.02),
                        ],
                        feather: 0.5,
                    }
                });
            }
        });
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
                    // Before-compare texture from a fit-level of the original.
                    let level = pyramid.level_for_target(1600);
                    let buf = photoforge_io::ImagePyramid::level(&pyramid, level);
                    self.before_texture = Some(ctx.load_texture(
                        "before",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [buf.width() as usize, buf.height() as usize],
                            buf.as_raw(),
                        ),
                        egui::TextureOptions::LINEAR,
                    ));
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
                    self.crop_session = None;
                    self.active_local = None;
                    self.mask_overlay = None;
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

    /// Rebuilds the red mask overlay for the active local adjustment at half
    /// the preview size (cheap UI-thread rasterization, keyed by state rev).
    fn refresh_mask_overlay(&mut self, ctx: &egui::Context) {
        let Some(size) = self
            .preview
            .as_ref()
            .map(|p| [(p.width / 2).max(1) as u32, (p.height / 2).max(1) as u32])
        else {
            return;
        };
        let key = (self.state_rev, size);
        if self.mask_overlay_key == key && self.mask_overlay.is_some() {
            return;
        }
        let Some(idx) = self.active_local else {
            self.mask_overlay = None;
            self.mask_overlay_key = key;
            return;
        };
        let Some(adj) = self.state.adjustments.get(idx) else {
            self.mask_overlay = None;
            self.mask_overlay_key = key;
            return;
        };
        let Some(mask) = adj.mask.as_ref() else {
            self.mask_overlay = None;
            self.mask_overlay_key = key;
            return;
        };
        let raster = mask.rasterize(size[0], size[1]);
        let mut rgba = vec![0u8; raster.len() * 4];
        for (i, &m) in raster.iter().enumerate() {
            rgba[i * 4] = 255;
            rgba[i * 4 + 1] = 40;
            rgba[i * 4 + 2] = 60;
            rgba[i * 4 + 3] = (m * 110.0) as u8;
        }
        self.mask_overlay = Some(ctx.load_texture(
            "mask_overlay",
            egui::ColorImage::from_rgba_unmultiplied([size[0] as usize, size[1] as usize], &rgba),
            egui::TextureOptions::LINEAR,
        ));
        self.mask_overlay_key = key;
    }

    fn handle_canvas_events(&mut self, ctx: &egui::Context, events: Vec<CanvasEvent>) {
        for ev in events {
            match ev {
                CanvasEvent::ViewChanged { interacting } => {
                    self.state_changed(ctx, interacting);
                }
                CanvasEvent::Open => self.open_file_dialog(),
                CanvasEvent::StrokePoint(p) => {
                    self.push_stroke_point(p);
                    self.state_changed(ctx, true);
                }
                CanvasEvent::StrokeEnded => self.end_stroke(ctx),
                CanvasEvent::GradientCommitted { linear, start, end } => {
                    self.set_gradient_mask(ctx, linear, start, end)
                }
                CanvasEvent::CropEdited => {
                    self.state_changed(ctx, true);
                }
                CanvasEvent::SplitMoved(t) => {
                    self.compare = Compare::Split(t);
                }
            }
        }
    }

    fn apply_crop(&mut self, ctx: &egui::Context) {
        if let Some(session) = self.crop_session.take() {
            let (x, y, w, h) = session.rect_i();
            self.tool = Tool::Navigate;
            self.edit_state(ctx, false, |s| {
                s.geometry.straighten_deg = session.straighten;
                s.geometry.crop = Some(photoforge_pipeline::CropRect { x, y, w, h });
            });
            self.status = format!("Crop applied ({w}×{h})");
        }
    }

    fn cancel_crop(&mut self, ctx: &egui::Context) {
        if self.crop_session.take().is_some() {
            self.tool = Tool::Navigate;
            self.edit_state(ctx, false, |_| {});
            self.status = "Crop cancelled".into();
        }
    }

    pub fn open_file_dialog(&mut self) {
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
                if i.key_pressed(egui::Key::C) && self.crop_session.is_none() {
                    let (w, h) = self
                        .image
                        .as_ref()
                        .map(|im| (im.width, im.height))
                        .unwrap_or((0, 0));
                    let straighten = self.state.geometry.straighten_deg;
                    self.crop_session = Some(CropSession::full(w, h, straighten));
                    self.tool = Tool::Navigate;
                    self.state_rev += 1;
                    self.send_render(ctx);
                }
                if i.key_pressed(egui::Key::Backslash) && self.compare == Compare::Off {
                    self.compare = Compare::Hold;
                }
                if i.key_released(egui::Key::Backslash) && self.compare == Compare::Hold {
                    self.compare = Compare::Off;
                }
                if i.key_pressed(egui::Key::P) {
                    self.compare = match self.compare {
                        Compare::Split(_) | Compare::Hold => Compare::Off,
                        Compare::Off => Compare::Split(0.5),
                    };
                }
                if i.key_pressed(egui::Key::V) {
                    self.tool = Tool::Navigate;
                }
                if i.key_pressed(egui::Key::B) {
                    self.tool = Tool::Brush;
                }
                if i.key_pressed(egui::Key::G) {
                    self.tool = Tool::LinearGrad;
                }
                if i.key_pressed(egui::Key::R) && !i.modifiers.ctrl {
                    self.tool = Tool::RadialGrad;
                }
                if i.key_pressed(egui::Key::Enter) && self.crop_session.is_some() {
                    self.apply_crop(ctx);
                }
                if i.key_pressed(egui::Key::Escape) && self.crop_session.is_some() {
                    self.cancel_crop(ctx);
                }
            });
        }

        self.refresh_mask_overlay(ctx);
        crate::panels::top_bar(self, ctx);
        crate::panels::adjustment_panel(self, ctx);
        let events = crate::panels::central_canvas(self, ctx);
        self.handle_canvas_events(ctx, events);
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

    pub fn before_texture(&self) -> Option<&egui::TextureHandle> {
        self.before_texture.as_ref()
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

    pub fn bump_rev(&mut self) {
        self.state_rev += 1;
    }

    pub fn set_status_pub(&mut self, s: impl Into<String>) {
        self.status = s.into();
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
