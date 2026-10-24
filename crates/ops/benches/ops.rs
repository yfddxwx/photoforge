//! Ops benchmarks. Run with `cargo bench -p photoforge-ops`.
//!
//! Key numbers for the performance budget (user requirement): a fit-view
//! preview frame (15+ tonal adjustments on a ~2 MP level) must stay under
//! 16 ms, and full-resolution exports under 3 s (see the pipeline bench).

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use photoforge_core::rng::XorShift64;
use photoforge_core::LinearImage;
use photoforge_ops::OpParams;

fn random_image(w: u32, h: u32, seed: u64) -> LinearImage {
    let mut rng = XorShift64::new(seed);
    LinearImage::from_fn(w, h, |_, _| {
        [rng.next_f32(), rng.next_f32(), rng.next_f32(), 1.0]
    })
}

fn bench_pointwise(c: &mut Criterion) {
    let mut group = c.benchmark_group("pointwise_1mp");
    let img = random_image(1000, 1000, 1);
    let ops: Vec<(&str, OpParams)> = vec![
        ("exposure", OpParams::Exposure { stops: 0.3 }),
        (
            "white_balance",
            OpParams::WhiteBalance {
                temp: 0.2,
                tint: -0.1,
            },
        ),
        ("contrast", OpParams::Contrast { amount: 0.3 }),
        ("vibrance", OpParams::Vibrance { amount: 0.4 }),
    ];
    for (name, op) in ops {
        group.bench_with_input(BenchmarkId::new("apply", name), &op, |b, op| {
            // Use the prepared path — the same one the pipeline runs.
            let f = op.prepare_pixel_fn();
            b.iter(|| {
                let mut work = img.clone();
                for y in 0..work.height {
                    let row = work.row_mut(y);
                    for px in row.chunks_exact_mut(4) {
                        let p = [px[0], px[1], px[2], px[3]];
                        let q = f(p);
                        px.copy_from_slice(&q);
                    }
                }
                black_box(&work);
            });
        });
    }
    // A fused 15-op tonal chain (the common preview case).
    let chain: Vec<OpParams> = vec![
        OpParams::WhiteBalance {
            temp: 0.1,
            tint: 0.05,
        },
        OpParams::Exposure { stops: 0.2 },
        OpParams::Contrast { amount: 0.15 },
        OpParams::Highlights { amount: -0.3 },
        OpParams::Shadows { amount: 0.25 },
        OpParams::Whites { amount: 0.1 },
        OpParams::Blacks { amount: -0.1 },
        OpParams::ToneCurve {
            r: vec![[0.0, 0.0], [0.5, 0.45], [1.0, 1.0]],
            g: vec![[0.0, 0.0], [0.5, 0.45], [1.0, 1.0]],
            b: vec![[0.0, 0.0], [0.5, 0.45], [1.0, 1.0]],
            l: Vec::new(),
        },
        OpParams::Levels {
            black: 0.02,
            gamma: 1.1,
            white: 0.98,
        },
        OpParams::Hsl {
            zones: [
                [0.0, 0.1, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, -0.1, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
            ],
        },
        OpParams::Vibrance { amount: 0.3 },
        OpParams::Saturation { amount: 0.1 },
        OpParams::Grain {
            amount: 0.3,
            size: 2.0,
            roughness: 0.3,
            seed: 7,
        },
        OpParams::Vignette {
            amount: -0.2,
            midpoint: 0.5,
            feather: 0.5,
        },
    ];
    let prepared: Vec<photoforge_ops::params::PixelFn> =
        chain.iter().map(|op| op.prepare_pixel_fn()).collect();
    group.bench_function("fused_15op_chain", |b| {
        b.iter(|| {
            let mut work = img.clone();
            for y in 0..work.height {
                let row = work.row_mut(y);
                for px in row.chunks_exact_mut(4) {
                    let mut q = [px[0], px[1], px[2], px[3]];
                    for f in &prepared {
                        let p = q;
                        q = f(p);
                    }
                    px.copy_from_slice(&q);
                }
            }
            black_box(&work);
        });
    });
    group.finish();
}

fn bench_spatial(c: &mut Criterion) {
    let mut group = c.benchmark_group("spatial_1mp");
    let img = random_image(1000, 1000, 2);
    group.bench_function("gaussian_r8", |b| {
        b.iter_batched(
            || img.clone(),
            |mut w| {
                photoforge_ops::filters::gaussian_blur(&mut w, 8.0);
                black_box(w.data.len())
            },
            criterion::BatchSize::LargeInput,
        );
    });
    group.bench_function("box_r40", |b| {
        b.iter_batched(
            || img.clone(),
            |mut w| {
                photoforge_ops::filters::box_blur(&mut w, 40);
                black_box(w.data.len())
            },
            criterion::BatchSize::LargeInput,
        );
    });
    group.bench_function("unsharp", |b| {
        b.iter_batched(
            || img.clone(),
            |mut w| {
                photoforge_ops::filters::unsharp_mask(&mut w, 0.8, 1.0, 0.003);
                black_box(w.data.len())
            },
            criterion::BatchSize::LargeInput,
        );
    });
    group.bench_function("clarity", |b| {
        b.iter_batched(
            || img.clone(),
            |mut w| {
                photoforge_ops::filters::clarity(&mut w, 0.4);
                black_box(w.data.len())
            },
            criterion::BatchSize::LargeInput,
        );
    });
    group.bench_function("denoise", |b| {
        b.iter_batched(
            || img.clone(),
            |mut w| {
                photoforge_ops::denoise::denoise(&mut w, 0.5);
                black_box(w.data.len())
            },
            criterion::BatchSize::LargeInput,
        );
    });
    group.finish();
}

fn bench_resize(c: &mut Criterion) {
    let mut group = c.benchmark_group("resize");
    group.sample_size(10);
    let img = random_image(2400, 1600, 3);
    group.bench_function("lanczos3_half", |b| {
        b.iter(|| {
            let out = photoforge_ops::geometry::resize(
                &img.to_pixel_buffer(),
                1200,
                800,
                photoforge_ops::geometry::Resample::Lanczos3,
            );
            black_box(&out);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_pointwise, bench_spatial, bench_resize);
criterion_main!(benches);
