//! Background worker: owns the image pyramid and the pipeline `Renderer`.
//!
//! All pixel work happens here — the UI thread only paints. Render requests
//! are coalesced (the queue is drained and only the newest request runs);
//! during interaction a coarser pyramid level keeps latency low.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use photoforge_core::histogram::Histogram;
use photoforge_core::PixelBuffer;
use photoforge_io::{encode_to_file, ExifSummary, ImagePyramid};
use photoforge_pipeline::{EditState, Quality, Renderer};

/// Messages INTO the worker.
pub enum Request {
    /// Decode + orient + build pyramid; resets the renderer.
    OpenPath(PathBuf),
    /// Render the current state (coalesced).
    Render {
        rev: u64,
        state: Arc<EditState>,
        /// Viewport width in image pixels (pyramid level selector).
        target_px: u32,
        /// Interaction: prefer latency over quality.
        interacting: bool,
    },
    /// Export full-resolution to a file (runs on its own thread).
    Export {
        state: Arc<EditState>,
        pyramid: Arc<ImagePyramid>,
        path: PathBuf,
        quality_jpeg: u8,
    },
}

/// Payload of [`Frame::ImageLoaded`] (boxed to keep the enum small).
pub struct ImageLoadedPayload {
    pub path: PathBuf,
    pub exif: ExifSummary,
    pub width: u32,
    pub height: u32,
    pub pyramid: Arc<ImagePyramid>,
}

/// Messages OUT of the worker.
pub enum Frame {
    ImageLoaded(Box<ImageLoadedPayload>),
    LoadFailed(String),
    Rendered {
        rev: u64,
        buf: Arc<PixelBuffer>,
        hist: Box<Histogram>,
        level: usize,
        ms: f32,
        interacting: bool,
    },
    ExportProgress(f32),
    ExportDone(Result<PathBuf, String>),
}

pub struct Worker {
    pub tx: Sender<Request>,
    pub rx: Receiver<Frame>,
}

impl Worker {
    /// Spawns the worker thread.
    pub fn spawn() -> Self {
        let (req_tx, req_rx) = std::sync::mpsc::channel::<Request>();
        let (frame_tx, frame_rx) = std::sync::mpsc::channel::<Frame>();
        std::thread::Builder::new()
            .name("photoforge-render".into())
            .spawn(move || run(req_rx, frame_tx))
            .expect("failed to spawn render worker");
        Self {
            tx: req_tx,
            rx: frame_rx,
        }
    }
}

fn run(rx: Receiver<Request>, tx: Sender<Frame>) {
    let mut pyramid: Option<Arc<ImagePyramid>> = None;
    let mut renderer: Option<Renderer> = None;

    loop {
        let req = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // Coalesce: only the newest request matters; an Export carries
        // everything it needs and runs on its own thread.
        let mut newest = Some(req);
        while let Ok(next) = rx.try_recv() {
            newest = Some(next);
        }
        match newest {
            Some(Request::Render {
                rev,
                state,
                target_px,
                interacting,
            }) => {
                if let (Some(pyr), Some(rnd)) = (&pyramid, renderer.as_mut()) {
                    rnd.sync_state(&state);
                    let target = if interacting {
                        (target_px / 2).max(64)
                    } else {
                        target_px
                    };
                    let level = pyr.level_for_target(target.max(1));
                    let quality = if interacting {
                        Quality::Draft
                    } else {
                        Quality::Final
                    };
                    let t0 = Instant::now();
                    let buf = rnd.render_level(pyr, level, quality);
                    let ms = t0.elapsed().as_secs_f32() * 1000.0;
                    let hist = Box::new(Histogram::compute(&buf));
                    let _ = tx.send(Frame::Rendered {
                        rev,
                        buf,
                        hist,
                        level,
                        ms,
                        interacting,
                    });
                }
            }
            Some(Request::OpenPath(path)) => {
                open_path(&path, &tx, &mut pyramid, &mut renderer);
            }
            Some(Request::Export {
                state,
                pyramid: pyr,
                path,
                quality_jpeg,
            }) => {
                let frame_tx = tx.clone();
                std::thread::Builder::new()
                    .name("photoforge-export".into())
                    .spawn(move || run_export(state, pyr, path, quality_jpeg, frame_tx))
                    .ok();
            }
            None => {}
        }
    }
}

fn open_path(
    path: &std::path::Path,
    tx: &Sender<Frame>,
    pyramid: &mut Option<Arc<ImagePyramid>>,
    renderer: &mut Option<Renderer>,
) {
    log::info!("opening {}", path.display());
    *renderer = None;
    *pyramid = None;
    match photoforge_io::decode_file(path) {
        Ok(decoded) => {
            let pyr = Arc::new(ImagePyramid::build(Arc::new(decoded.buffer)));
            let state = EditState::with_global_stack();
            *renderer = Some(Renderer::new(state));
            *pyramid = Some(pyr.clone());
            let _ = tx.send(Frame::ImageLoaded(Box::new(ImageLoadedPayload {
                path: path.to_path_buf(),
                exif: decoded.exif,
                width: pyr.level(0).width(),
                height: pyr.level(0).height(),
                pyramid: pyr,
            })));
        }
        Err(e) => {
            let _ = tx.send(Frame::LoadFailed(format!("{e}")));
        }
    }
}

fn run_export(
    state: Arc<EditState>,
    pyramid: Arc<ImagePyramid>,
    path: PathBuf,
    quality_jpeg: u8,
    tx: Sender<Frame>,
) {
    let _ = tx.send(Frame::ExportProgress(0.05));
    // One-shot export: a fresh renderer (no cache bookkeeping needed).
    let mut renderer = Renderer::new((*state).clone());
    let _ = tx.send(Frame::ExportProgress(0.15));
    let rendered = renderer.render_full(&pyramid, Quality::Final);
    let _ = tx.send(Frame::ExportProgress(0.75));
    match encode_to_file(&rendered, &path, quality_jpeg) {
        Ok(()) => {
            log::info!("exported {}", path.display());
            let _ = tx.send(Frame::ExportDone(Ok(path)));
        }
        Err(e) => {
            let _ = tx.send(Frame::ExportDone(Err(format!("{e}"))));
        }
    }
}
