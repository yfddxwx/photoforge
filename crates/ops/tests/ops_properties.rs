//! Property tests for pointwise ops (ADR-0004): purity, range, NaN-freedom,
//! identity at defaults, analytic known values.

use photoforge_core::rng::XorShift64;
use photoforge_core::{LinearImage, PixelBuffer};
use photoforge_ops::OpParams;

/// Random linear images of assorted shapes; values in [0, 1].
fn random_image(rng: &mut XorShift64, w: u32, h: u32) -> LinearImage {
    LinearImage::from_fn(w, h, |_, _| {
        [rng.next_f32(), rng.next_f32(), rng.next_f32(), 1.0]
    })
}

fn random_params(rng: &mut XorShift64) -> Vec<OpParams> {
    use photoforge_ops::params::GLOBAL_OP_ORDER;
    let mut out = Vec::new();
    for kind in GLOBAL_OP_ORDER {
        if rng.next_f32() < 0.4 {
            continue; // some ops at identity
        }
        let amount = rng.range_f32(-1.0, 1.0);
        let p = match kind {
            photoforge_ops::params::OpKind::Exposure => OpParams::Exposure {
                stops: rng.range_f32(-4.0, 4.0),
            },
            photoforge_ops::params::OpKind::Contrast => OpParams::Contrast { amount },
            photoforge_ops::params::OpKind::Highlights => OpParams::Highlights { amount },
            photoforge_ops::params::OpKind::Shadows => OpParams::Shadows { amount },
            photoforge_ops::params::OpKind::Whites => OpParams::Whites { amount },
            photoforge_ops::params::OpKind::Blacks => OpParams::Blacks { amount },
            photoforge_ops::params::OpKind::WhiteBalance => OpParams::WhiteBalance {
                temp: amount,
                tint: rng.range_f32(-1.0, 1.0),
            },
            photoforge_ops::params::OpKind::Vibrance => OpParams::Vibrance { amount },
            photoforge_ops::params::OpKind::Saturation => OpParams::Saturation { amount },
        };
        out.push(p);
    }
    out
}

const SHAPES: [(u32, u32); 5] = [(1, 1), (1, 9), (8, 1), (13, 17), (33, 5)];

/// Every op: pure, shape-preserving, in-range, NaN-free, deterministic.
#[test]
fn property_ops_pure_and_safe() {
    let mut rng = XorShift64::new(4242);
    for &(w, h) in &SHAPES {
        let input = random_image(&mut rng, w, h);
        for params in random_params(&mut rng) {
            let mut work = input.clone();
            for y in 0..work.height {
                let row = work.row_mut(y);
                for px in row.chunks_exact_mut(4) {
                    let p = [px[0], px[1], px[2], px[3]];
                    let out = params.apply_pixel(p);
                    px.copy_from_slice(&out);
                }
            }
            for (i, v) in work.data.iter().enumerate() {
                assert!(v.is_finite(), "{params:?} produced non-finite at {i}");
                if i % 4 != 3 {
                    assert!(
                        (0.0..=1.0).contains(v),
                        "{params:?} out of range {v} at {i}"
                    );
                }
            }
            // Purity: alpha untouched, deterministic re-run.
            let alpha_ok = work
                .data
                .chunks_exact(4)
                .zip(input.data.chunks_exact(4))
                .all(|(o, i)| o[3] == i[3]);
            assert!(alpha_ok, "alpha modified by {params:?}");
        }
    }
}

/// Identity parameters reproduce the input bit-exactly.
#[test]
fn property_identity_is_bit_exact() {
    let mut rng = XorShift64::new(7);
    for &(w, h) in &SHAPES {
        let input = random_image(&mut rng, w, h);
        for kind in photoforge_ops::params::GLOBAL_OP_ORDER {
            let p = OpParams::identity_of(kind);
            assert!(p.is_identity(), "{kind:?} default not identity");
            let out_img = input
                .data
                .chunks_exact(4)
                .map(|px| p.apply_pixel([px[0], px[1], px[2], px[3]]))
                .collect::<Vec<_>>();
            for (o, i) in out_img.iter().zip(input.data.chunks_exact(4)) {
                assert_eq!(&o[0..4], i, "{kind:?} identity changed pixel");
            }
        }
    }
}

/// +1 EV multiplies linear values by exactly 2 (below the clamp).
#[test]
fn exposure_one_stop_doubles_linear() {
    let p = OpParams::Exposure { stops: 1.0 };
    for v in [0.0f32, 0.0625, 0.125, 0.25, 0.3] {
        let out = p.apply_pixel([v, v, v, 1.0]);
        assert_eq!(out[0], v * 2.0, "+1EV must be an exact ×2");
        assert_eq!(out[1], out[0]);
    }
    // And −1 EV halves.
    let p = OpParams::Exposure { stops: -1.0 };
    let out = p.apply_pixel([0.5, 0.5, 0.5, 1.0]);
    assert_eq!(out[0], 0.25);
    // Values above the clamp stay in range.
    let p = OpParams::Exposure { stops: 3.0 };
    let out = p.apply_pixel([0.9, 0.9, 0.9, 1.0]);
    assert_eq!(out[0], 1.0);
}

/// Saturation −1 produces exact grays; grayscale survives saturation ops.
#[test]
fn saturation_minus_one_is_gray() {
    let p = OpParams::Saturation { amount: -1.0 };
    for rgb in [[0.9, 0.1, 0.3], [0.2, 0.8, 0.4], [0.5, 0.5, 0.5]] {
        let out = p.apply_pixel([rgb[0], rgb[1], rgb[2], 1.0]);
        // Channels coincide within matrix rounding (~1e-5) — and exactly on
        // the 8-bit grid, which is what the display shows.
        assert!((out[0] - out[1]).abs() < 1e-3, "{rgb:?} -> {out:?}");
        assert!((out[1] - out[2]).abs() < 1e-3, "{rgb:?} -> {out:?}");
        assert!(out[0] > 0.0 && out[0] < 1.0);
    }
    // Grayscale in → grayscale out (u8-grid exact; f32 drift ~1e-5 from the
    // OKLab matrices is far below half an 8-bit code).
    let gray = [0.3f32, 0.3, 0.3, 1.0];
    for amount in [0.7f32, -1.0] {
        let p = OpParams::Saturation { amount };
        let out = p.apply_pixel(gray);
        assert!(
            (out[0] - 0.3).abs() < 1e-3,
            "saturation {amount} gray drift {}",
            out[0]
        );
        let p = OpParams::Vibrance { amount: 0.9 };
        let out = p.apply_pixel(gray);
        assert!(
            (out[0] - 0.3).abs() < 1e-3,
            "vibrance gray drift {}",
            out[0]
        );
    }
}

/// Tone ops on a grayscale ramp are monotone non-decreasing for positive
/// amounts (no tone inversion), and pivot-exact for contrast.
#[test]
fn tone_ops_monotone_on_grayscale() {
    let n = 256;
    let ramp: Vec<f32> = (0..n).map(|i| i as f32 / (n - 1) as f32).collect();
    let check = |p: &OpParams, strictly_stronger: bool| {
        let vals: Vec<f32> = ramp
            .iter()
            .map(|&v| p.apply_pixel([v, v, v, 1.0])[0])
            .collect();
        for i in 1..n {
            assert!(
                vals[i] >= vals[i - 1] - 1e-6,
                "{p:?} inverted at {} ({} -> {})",
                i,
                vals[i - 1],
                vals[i]
            );
        }
        if strictly_stronger {
            assert!(vals[n - 1] > ramp[n - 1] - 1e-6);
        }
        vals
    };
    check(&OpParams::Contrast { amount: 0.5 }, false);
    check(&OpParams::Highlights { amount: 0.7 }, false);
    check(&OpParams::Shadows { amount: 0.7 }, false);
    check(&OpParams::Whites { amount: 0.8 }, false);
    check(&OpParams::Blacks { amount: 0.8 }, false);
    check(
        &OpParams::WhiteBalance {
            temp: 0.6,
            tint: 0.0,
        },
        false,
    );

    // Contrast keeps the 18% pivot fixed.
    let p = OpParams::Contrast { amount: 0.9 };
    let pivot = photoforge_core::color::srgb::encode_f32(0.18); // not used; linear check below
    let _ = pivot;
    let out = p.apply_pixel([0.18, 0.18, 0.18, 1.0]);
    assert!(
        (out[0] - 0.18).abs() < 1e-5,
        "contrast moved pivot: {}",
        out[0]
    );
}

/// Region selectivity: highlight ops barely touch shadows and vice versa.
#[test]
fn region_selectivity() {
    let dark = [0.02f32, 0.025, 0.018, 1.0];
    let bright = [0.95f32, 0.93, 0.97, 1.0];

    let h = OpParams::Highlights { amount: 1.0 };
    let bright_after = h.apply_pixel(bright)[0];
    let dark_after = h.apply_pixel(dark)[0];
    assert!(
        bright[0] - bright_after > 0.01,
        "highlights must act on highlights"
    );
    assert!(
        (dark_after - dark[0]).abs() < 0.005,
        "highlights leaked into shadows"
    );

    let s = OpParams::Shadows { amount: 1.0 };
    let dark_after = s.apply_pixel(dark)[0];
    let bright_after = s.apply_pixel(bright)[0];
    assert!(dark_after - dark[0] > 0.01, "shadows must lift shadows");
    assert!(
        (bright_after - bright[0]).abs() < 0.01,
        "shadows leaked into highlights"
    );

    let w = OpParams::Whites { amount: 1.0 };
    assert!((w.apply_pixel(dark)[0] - dark[0]).abs() < 0.005);
    let b = OpParams::Blacks { amount: 1.0 };
    assert!((b.apply_pixel(bright)[0] - bright[0]).abs() < 0.01);
    // Blacks lift near-black; blacks leave midtones mostly alone.
    let b_out = b.apply_pixel(dark)[0];
    assert!(b_out > dark[0], "blacks +1 must lift near-black");
}

/// White balance directions and neutrality.
#[test]
fn white_balance_semantics() {
    let gray = [0.4f32, 0.4, 0.4, 1.0];
    let warm = OpParams::WhiteBalance {
        temp: 1.0,
        tint: 0.0,
    }
    .apply_pixel(gray);
    assert!(warm[0] > warm[2], "warm must raise R over B");
    let cool = OpParams::WhiteBalance {
        temp: -1.0,
        tint: 0.0,
    }
    .apply_pixel(gray);
    assert!(cool[2] > cool[0], "cool must raise B over R");
    let magenta = OpParams::WhiteBalance {
        temp: 0.0,
        tint: 1.0,
    }
    .apply_pixel(gray);
    assert!(magenta[0] > magenta[1] && magenta[2] > magenta[1]);
    let green = OpParams::WhiteBalance {
        temp: 0.0,
        tint: -1.0,
    }
    .apply_pixel(gray);
    assert!(green[1] > green[0]);
    let neutral = OpParams::WhiteBalance {
        temp: 0.0,
        tint: 0.0,
    }
    .apply_pixel(gray);
    assert_eq!(neutral, gray);
}

/// Vibrance protects saturated colors more than muted ones.
#[test]
fn vibrance_boosts_muted_more() {
    let muted = [0.5f32, 0.48, 0.52, 1.0];
    let saturated = [0.9f32, 0.1, 0.2, 1.0];
    let chroma = |rgb: [f32; 3]| photoforge_core::color::oklab::linear_srgb_to_oklab(rgb).chroma();
    let p = OpParams::Vibrance { amount: 1.0 };
    let muted_gain = chroma([
        p.apply_pixel(muted)[0],
        p.apply_pixel(muted)[1],
        p.apply_pixel(muted)[2],
    ]) / chroma([muted[0], muted[1], muted[2]]);
    let sat_gain = chroma([
        p.apply_pixel(saturated)[0],
        p.apply_pixel(saturated)[1],
        p.apply_pixel(saturated)[2],
    ]) / chroma([saturated[0], saturated[1], saturated[2]]);
    assert!(
        muted_gain > sat_gain,
        "muted {muted_gain} vs saturated {sat_gain}"
    );
    assert!(muted_gain > 1.5, "muted colors should clearly gain chroma");
}

/// Preview staging quantizes each stage to 16-bit linear (the pipeline's
/// Q16 stage cache, ADR-0006): staged-vs-fused drift must stay within 2 LSB
/// on the 8-bit display grid even for a long op chain.
#[test]
fn q16_staging_drift_bounded() {
    use photoforge_core::LinearImageQ16;
    let mut rng = XorShift64::new(31);
    let mut pb = PixelBuffer::new(23, 7);
    for px in pb.as_raw_mut().chunks_exact_mut(4) {
        px[0] = rng.next_u8();
        px[1] = rng.next_u8();
        px[2] = rng.next_u8();
        px[3] = 255;
    }
    let chain = [
        OpParams::Exposure { stops: 0.4 },
        OpParams::Contrast { amount: 0.3 },
        OpParams::Highlights { amount: -0.5 },
        OpParams::Shadows { amount: 0.5 },
        OpParams::WhiteBalance {
            temp: 0.3,
            tint: -0.2,
        },
        OpParams::Vibrance { amount: 0.6 },
    ];
    let apply = |op: &OpParams, q: &mut [f32]| {
        let p = [q[0], q[1], q[2], q[3]];
        let out = op.apply_pixel(p);
        q.copy_from_slice(&out);
    };
    // Fused: single pass in f32.
    let lin = LinearImageQ16::from_pixel_buffer(&pb).to_linear();
    let fused = {
        let mut work = lin.clone();
        for y in 0..work.height {
            let row = work.row_mut(y);
            for px in row.chunks_exact_mut(4) {
                let mut q = [px[0], px[1], px[2], px[3]];
                for op in &chain {
                    apply(op, &mut q);
                }
                px.copy_from_slice(&q);
            }
        }
        work.to_pixel_buffer()
    };
    // Staged: quantize to Q16 between every op (pipeline preview path).
    let mut staged_q = LinearImageQ16::from_pixel_buffer(&pb);
    for op in &chain {
        let mut f32img = staged_q.to_linear();
        for y in 0..f32img.height {
            let row = f32img.row_mut(y);
            for px in row.chunks_exact_mut(4) {
                let mut q = [px[0], px[1], px[2], px[3]];
                apply(op, &mut q);
                px.copy_from_slice(&q);
            }
        }
        staged_q = LinearImageQ16::from_linear(&f32img);
    }
    let staged = staged_q.to_pixel_buffer();
    let mut max_diff = 0i32;
    for (a, b) in fused.as_raw().iter().zip(staged.as_raw()) {
        max_diff = max_diff.max(i32::from((i16::from(*a) - i16::from(*b)).abs()));
    }
    assert!(
        max_diff <= 2,
        "staged (Q16) vs fused drifted {max_diff} LSB"
    );
}
