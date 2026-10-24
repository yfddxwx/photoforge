//! Pipeline benchmarks: the two budget numbers from the requirements.
//!
//! - `preview_fit_20ops`: a fit-view preview frame (≈1600×1050 render of a
//!   24 MP original through a 20-adjustment stack) — budget < 16 ms/frame.
//! - `export_full_24mp_20ops`: full-resolution replay + geometry — budget
//!   < 3 s on 8 cores.
//!
//! Run with `cargo bench -p photoforge-pipeline`.

use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use photoforge_core::rng::XorShift64;
use photoforge_io::ImagePyramid;
use photoforge_ops::OpParams;
use photoforge_pipeline::{EditState, Quality, Renderer};

/// A 24 MP synthetic "photo": smooth gradients + noise + an edge, 6000×4000.
fn synthetic_photo(w: u32, h: u32, seed: u64) -> photoforge_core::PixelBuffer {
    use rayon::prelude::*;
    let mut rng = XorShift64::new(seed);
    let mut buf = photoforge_core::PixelBuffer::new(w, h);
    let noise: Vec<u8> = (0..(w as usize * h as usize * 3))
        .map(|_| rng.next_u8())
        .collect();
    buf.as_raw_mut()
        .par_chunks_exact_mut(w as usize * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, px) in row.chunks_exact_mut(4).enumerate() {
                let i = (y * w as usize + x) * 3;
                let fx = x as f32 / w as f32;
                let fy = y as f32 / h as f32;
                let base = 0.25 + 0.5 * fx * (1.0 - fy);
                let n = ((noise[i] as f32 / 255.0) - 0.5) * 0.06;
                for (c, k) in [1.0f32, 0.98, 0.95].iter().enumerate() {
                    px[c] = photoforge_core::color::srgb::encode_u8((base * k + n).clamp(0.0, 1.0));
                }
                px[3] = 255;
            }
        });
    buf
}

/// A representative 20-adjustment stack (the acceptance scenario).
fn twenty_op_state() -> EditState {
    let mut state = EditState::with_global_stack();
    state.adjustments[0].op = OpParams::WhiteBalance {
        temp: 0.15,
        tint: -0.05,
    };
    state.adjustments[1].op = OpParams::Exposure { stops: 0.35 };
    state.adjustments[2].op = OpParams::Contrast { amount: 0.2 };
    state.adjustments[3].op = OpParams::Highlights { amount: -0.4 };
    state.adjustments[4].op = OpParams::Shadows { amount: 0.3 };
    state.adjustments[5].op = OpParams::Whites { amount: 0.15 };
    state.adjustments[6].op = OpParams::Blacks { amount: -0.1 };
    state.adjustments[7].op = OpParams::ToneCurve {
        r: vec![[0.0, 0.0], [0.25, 0.2], [0.75, 0.8], [1.0, 1.0]],
        g: vec![[0.0, 0.0], [0.25, 0.2], [0.75, 0.8], [1.0, 1.0]],
        b: vec![[0.0, 0.0], [0.25, 0.2], [0.75, 0.8], [1.0, 1.0]],
        l: Vec::new(),
    };
    state.adjustments[8].op = OpParams::Levels {
        black: 0.02,
        gamma: 1.05,
        white: 0.98,
    };
    state.adjustments[9].op = OpParams::Hsl {
        zones: [
            [0.0, 0.1, 0.0],
            [30.0, 0.05, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, -0.1, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.05, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        ],
    };
    state.adjustments[10].op = OpParams::Vibrance { amount: 0.25 };
    state.adjustments[11].op = OpParams::Saturation { amount: 0.05 };
    state.adjustments[13].op = OpParams::Sharpen {
        amount: 0.5,
        radius: 1.0,
        threshold: 0.003,
    };
    state.adjustments[13].enabled = true;
    state.adjustments[14].op = OpParams::Clarity { amount: 0.15 };
    state.adjustments[14].enabled = true;
    state.adjustments[19].op = OpParams::Grain {
        amount: 0.2,
        size: 2.0,
        roughness: 0.2,
        seed: 11,
    };
    state.adjustments[19].enabled = true;
    state.adjustments[20].op = OpParams::Vignette {
        amount: -0.25,
        midpoint: 0.5,
        feather: 0.6,
    };
    state.adjustments[20].enabled = true;
    state
}

fn bench_preview(c: &mut Criterion) {
    let mut group = c.benchmark_group("preview");
    group.sample_size(20);
    // 24 MP base (6000x4000), fit-view level ≈ 1600 px wide.
    let base = synthetic_photo(6000, 4000, 42);
    let pyramid = ImagePyramid::build(Arc::new(base));
    let state = twenty_op_state();
    let mut state_b = twenty_op_state();
    if let photoforge_ops::OpParams::Exposure { stops } = &mut state_b.adjustments[1].op {
        *stops += 0.001; // alternate states so the final cache always misses
    }
    let mut renderer = Renderer::new(state.clone());
    let level = pyramid.level_for_target(1600);
    let mut flip = false;
    group.bench_function("fit_20ops", |b| {
        b.iter(|| {
            flip = !flip;
            let cur = if flip { state_b.clone() } else { state.clone() };
            renderer.sync_state(&cur);
            let frame = renderer.render_level(&pyramid, level, Quality::Final);
            black_box(&frame);
        });
    });
    // Pointwise-only slice (no spatial effects): isolates the fused chain.
    let mut pw_state = twenty_op_state();
    for idx in [13usize, 14, 19, 20] {
        pw_state.adjustments[idx].enabled = false;
    }
    let mut pw_renderer = Renderer::new(pw_state.clone());
    let mut pw_b = twenty_op_state();
    if let photoforge_ops::OpParams::Exposure { stops } = &mut pw_b.adjustments[1].op {
        *stops += 0.001;
    }
    let mut flip2 = false;
    group.bench_function("fit_pointwise_only", |b| {
        b.iter(|| {
            flip2 = !flip2;
            let cur = if flip2 {
                pw_b.clone()
            } else {
                pw_state.clone()
            };
            pw_renderer.sync_state(&cur);
            let frame = pw_renderer.render_level(&pyramid, level, Quality::Final);
            black_box(&frame);
        });
    });
    group.finish();
}

fn bench_export(c: &mut Criterion) {
    let mut group = c.benchmark_group("export");
    group.sample_size(10);
    let base = synthetic_photo(6000, 4000, 42);
    let pyramid = ImagePyramid::build(Arc::new(base));
    let state = twenty_op_state();
    let mut renderer = Renderer::new(state.clone());
    group.bench_function("full_24mp_20ops", |b| {
        b.iter(|| {
            let out = renderer.render_full(&pyramid, Quality::Final);
            black_box(&out);
        });
    });
    // Pointwise-only slice for the export path.
    let mut pw_state = twenty_op_state();
    for idx in [13usize, 14, 19, 20] {
        pw_state.adjustments[idx].enabled = false;
    }
    let mut pw_renderer = Renderer::new(pw_state.clone());
    group.bench_function("full_pointwise_only", |b| {
        b.iter(|| {
            let out = pw_renderer.render_full(&pyramid, Quality::Final);
            black_box(&out);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_preview, bench_export);
criterion_main!(benches);
