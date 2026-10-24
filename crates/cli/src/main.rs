//! Headless batch processing.
//!
//! `photoforge-cli ./photos/ --preset film.json -o out/` applies a preset
//! (and, when present, the per-image `.pfg` sidecar — the sidecar is applied
//! after the preset so per-image edits win) to a set of images and exports at
//! full resolution with correct EXIF-orientation handling. Files are
//! processed in parallel (rayon).

use std::path::{Path, PathBuf};

use clap::Parser;
use photoforge_pipeline::{EditState, Quality, Renderer};

#[derive(Parser, Debug)]
#[command(
    name = "photoforge-cli",
    version,
    about = "Batch-process photos with photoforge's non-destructive pipeline"
)]
struct Args {
    /// Input image files and/or directories (recursed).
    inputs: Vec<PathBuf>,
    /// Preset JSON to apply (after any per-image sidecar).
    #[arg(short, long)]
    preset: Option<PathBuf>,
    /// Output directory (default: ./photoforge-out).
    #[arg(short, long, default_value = "photoforge-out")]
    out: PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value = "jpeg")]
    format: Format,
    /// JPEG quality (1-100).
    #[arg(long, default_value_t = 92)]
    quality: u8,
    /// Resize so the longest edge is N pixels (Lanczos3; optional).
    #[arg(long)]
    resize: Option<u32>,
    /// Also write a `.pfg` sidecar next to each output.
    #[arg(long)]
    sidecar_out: bool,
    /// Parallel jobs (default: all cores).
    #[arg(long)]
    jobs: Option<usize>,
    /// Skip images whose output already exists.
    #[arg(long)]
    skip_existing: bool,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum Format {
    Jpeg,
    Png,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    if args.inputs.is_empty() {
        eprintln!("no inputs given — pass files or directories");
        std::process::exit(2);
    }
    if let Some(jobs) = args.jobs {
        rayon::ThreadPoolBuilder::new()
            .num_threads(jobs.max(1))
            .build_global()
            .ok();
    }

    let preset = args.preset.as_deref().map(|p| {
        photoforge_presets::Preset::from_file(p).unwrap_or_else(|e| {
            eprintln!("preset load failed: {e}");
            std::process::exit(2);
        })
    });

    let mut files = Vec::new();
    for input in &args.inputs {
        collect_images(input, &mut files);
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        eprintln!("no image files found in the given inputs");
        std::process::exit(2);
    }
    log::info!("{} image(s) to process", files.len());
    std::fs::create_dir_all(&args.out).expect("create output dir");

    let results: Vec<Result<String, String>> = {
        use rayon::prelude::*;
        files
            .par_iter()
            .map(|file| process_one(file, &args, preset.as_ref()))
            .collect()
    };
    let mut ok = 0usize;
    let mut failed = 0usize;
    for (file, result) in files.iter().zip(&results) {
        match result {
            Ok(msg) => {
                ok += 1;
                log::info!("{msg}");
            }
            Err(e) => {
                failed += 1;
                log::error!("{} failed: {e}", file.display());
            }
        }
    }
    log::info!("done: {ok} exported, {failed} failed");
    if failed > 0 {
        std::process::exit(1);
    }
}

fn collect_images(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                collect_images(&entry, out);
            } else if is_image(&entry) {
                out.push(entry);
            }
        }
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "tif" | "tiff" | "bmp"
            )
        })
        .unwrap_or(false)
}

fn process_one(
    file: &Path,
    args: &Args,
    preset: Option<&photoforge_presets::Preset>,
) -> Result<String, String> {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".into());
    let ext = match args.format {
        Format::Jpeg => "jpg",
        Format::Png => "png",
    };
    let out_path = args.out.join(format!("{stem}.{ext}"));
    if args.skip_existing && out_path.exists() {
        return Ok(format!("{} skipped (exists)", file.display()));
    }

    let decoded = photoforge_io::decode_file(file).map_err(|e| e.to_string())?;
    // State: sidecar if present (per-image edits), else fresh; then the
    // preset overrides the global stack.
    let mut state = match photoforge_presets::load_sidecar(file) {
        Ok(Some(s)) => s,
        _ => EditState::with_global_stack(),
    };
    if let Some(preset) = preset {
        preset.apply_to(&mut state);
    }

    let pyramid = std::sync::Arc::new(photoforge_io::ImagePyramid::build(std::sync::Arc::new(
        decoded.buffer,
    )));
    let mut renderer = Renderer::new(state.clone());
    let mut rendered = renderer.render_full(&pyramid, Quality::Final);

    if let Some(max_edge) = args.resize {
        let (w, h) = (rendered.width(), rendered.height());
        let longest = w.max(h) as f32;
        if longest > max_edge as f32 {
            let scale = max_edge as f32 / longest;
            let dw = ((w as f32 * scale).round() as u32).max(1);
            let dh = ((h as f32 * scale).round() as u32).max(1);
            rendered = photoforge_ops::geometry::resize(
                &rendered,
                dw,
                dh,
                photoforge_ops::geometry::Resample::Lanczos3,
            );
        }
    }

    photoforge_io::encode_to_file(&rendered, &out_path, args.quality).map_err(|e| e.to_string())?;
    if args.sidecar_out {
        let _ = photoforge_presets::save_sidecar(&out_path, &state);
    }
    Ok(format!(
        "{} -> {} ({}×{})",
        file.display(),
        out_path.display(),
        rendered.width(),
        rendered.height()
    ))
}
