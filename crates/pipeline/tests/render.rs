//! Pipeline engine tests: identity, known values, cache invalidation, mask
//! blending, geometry, fusion equivalence and no-panic properties.

use std::sync::Arc;

use photoforge_core::PixelBuffer;
use photoforge_fixtures::{gradient_h, radial, solid};
use photoforge_io::ImagePyramid;
use photoforge_ops::OpParams;
use photoforge_pipeline::mask::{Mask, Stroke};
use photoforge_pipeline::{CropRect, EditState, Geometry, Quality, Renderer};

fn pyramid_of(buf: PixelBuffer) -> ImagePyramid {
    ImagePyramid::build(Arc::new(buf))
}

fn identity_state() -> EditState {
    EditState::with_global_stack()
}

#[test]
fn identity_state_returns_base_level_exactly() {
    let base = gradient_h(64, 40);
    let pyr = pyramid_of(base.clone());
    let mut r = Renderer::new(identity_state());
    let out = r.render_level(&pyr, 0, Quality::Final);
    assert_eq!(*out, base);
}

#[test]
fn exposure_matches_direct_math() {
    let base = gradient_h(64, 40);
    let pyr = pyramid_of(base);
    let mut state = identity_state();
    // Exposure is adjustment index 1 in the global order.
    state.adjustments[1].op = OpParams::Exposure { stops: 1.0 };
    let mut r = Renderer::new(state);
    let out = r.render_level(&pyr, 0, Quality::Final);

    // Expected: Q16 roundtrip through the exposure op via the PREPARED path
    // (the pipeline runs prepared fns; apply_pixel is the slow reference).
    let mut expected = photoforge_core::LinearImageQ16::from_pixel_buffer(&pyr.level(0));
    let mut tmp = expected.to_linear();
    let f = photoforge_ops::OpParams::Exposure { stops: 1.0 }.prepare_pixel_fn();
    for px in tmp.data.chunks_exact_mut(4) {
        let q = f([px[0], px[1], px[2], px[3]]);
        px.copy_from_slice(&q);
    }
    expected = photoforge_core::LinearImageQ16::from_linear(&tmp);
    assert_eq!(*out, expected.to_pixel_buffer());
}

#[test]
fn plus_one_ev_doubles_through_pipeline() {
    // A 25% linear gray (encoded ≈ 137) becomes 50% linear (encoded ≈ 188).
    let lin = 0.25f32;
    let enc = photoforge_core::color::srgb::encode_f32(lin);
    let base = solid(
        4,
        4,
        [
            (enc * 255.0) as u8,
            (enc * 255.0) as u8,
            (enc * 255.0) as u8,
            255,
        ],
    );
    let pyr = pyramid_of(base);
    let mut state = identity_state();
    state.adjustments[1].op = OpParams::Exposure { stops: 1.0 };
    let mut r = Renderer::new(state);
    let out = r.render_level(&pyr, 0, Quality::Final);
    let px = out.pixel(2, 2);
    let expected = photoforge_core::color::srgb::encode_u8(0.5);
    assert_eq!(px[0], expected);
}

#[test]
fn final_cache_hits_when_unchanged() {
    let pyr = pyramid_of(gradient_h(32, 32));
    let mut r = Renderer::new(identity_state());
    let a = r.render_level(&pyr, 1, Quality::Final);
    let b = r.render_level(&pyr, 1, Quality::Final);
    assert!(
        Arc::ptr_eq(&a, &b),
        "second render must hit the final cache"
    );
    assert_eq!(r.stats.final_hits, 1);
    assert_eq!(r.stats.final_misses, 1);
}

/// Colored gradient (R along x, B along y) so chroma ops are observable.
fn color_gradient(w: u32, h: u32) -> PixelBuffer {
    photoforge_core::LinearImage::from_fn(w, h, |x, y| {
        [
            x as f32 / (w - 1).max(1) as f32,
            0.3,
            y as f32 / (h - 1).max(1) as f32,
            1.0,
        ]
    })
    .to_pixel_buffer()
}

#[test]
fn editing_last_op_reuses_earlier_stages() {
    let pyr = pyramid_of(color_gradient(48, 32));
    let mut state = identity_state();
    state.adjustments[1].op = OpParams::Exposure { stops: 0.5 };
    let mut r = Renderer::new(state.clone());
    r.render_level(&pyr, 0, Quality::Final);

    // Edit the LAST global op (saturation) only: stages 0..=8 remain valid.
    let mut edited = state.clone();
    edited.adjustments[11].op = OpParams::Saturation { amount: 0.6 };
    let misses_before = r.stats.final_misses;
    r.sync_state(&edited);
    let out = r.render_level(&pyr, 0, Quality::Final);

    // Must equal a fresh renderer over the edited state (correctness), while
    // having missed the final cache only once more.
    let mut fresh = Renderer::new(edited);
    let expected = fresh.render_level(&pyr, 0, Quality::Final);
    assert_eq!(*out, *expected);
    assert_eq!(r.stats.final_misses, misses_before + 1);
}

#[test]
fn editing_first_op_invalidates_everything_but_result_is_right() {
    let pyr = pyramid_of(gradient_h(48, 32));
    let mut state = identity_state();
    let mut r = Renderer::new(state.clone());
    r.render_level(&pyr, 0, Quality::Final);
    state.adjustments[0].op = OpParams::WhiteBalance {
        temp: 0.4,
        tint: 0.0,
    };
    r.sync_state(&state);
    let out = r.render_level(&pyr, 0, Quality::Final);
    let mut fresh = Renderer::new(state);
    let expected = fresh.render_level(&pyr, 0, Quality::Final);
    assert_eq!(*out, *expected);
}

#[test]
fn masked_gradient_exposure_blends() {
    let base = solid(
        100,
        20,
        [
            photoforge_core::color::srgb::encode_u8(0.25),
            photoforge_core::color::srgb::encode_u8(0.25),
            photoforge_core::color::srgb::encode_u8(0.25),
            255,
        ],
    );
    let pyr = pyramid_of(base);
    let mut state = identity_state();
    // Replace saturation (last) with a masked exposure +2.
    state.adjustments[11] = photoforge_pipeline::Adjustment::masked(
        OpParams::Exposure { stops: 2.0 },
        Mask::LinearGradient {
            start: [0.0, 0.5],
            end: [1.0, 0.5],
        },
    );
    let mut r = Renderer::new(state);
    let out = r.render_level(&pyr, 0, Quality::Final);
    // Left edge (mask ≈ 0): unchanged 25% gray. Right edge (mask ≈ 1): 25% × 4 = 100%.
    let left = photoforge_core::color::srgb::encode_u8(0.25);
    let right = photoforge_core::color::srgb::encode_u8(1.0);
    assert_eq!(out.pixel(1, 10)[0], left, "mask=0 must be a no-op");
    assert_eq!(out.pixel(98, 10)[0], right, "mask=1 must apply fully");
    // Midpoint: smoothstep(0.5) = 0.5 → 0.25 → 0.25 + 0.5·(1−0.25) = 0.625.
    let mid_expected = photoforge_core::color::srgb::encode_u8(0.625);
    let mid = out.pixel(50, 10)[0];
    assert!(
        (i16::from(mid) - i16::from(mid_expected)).abs() <= 2,
        "mid {mid} vs {mid_expected}"
    );
}

#[test]
fn radial_mask_center_only() {
    let base = solid(80, 80, [128, 128, 128, 255]);
    let pyr = pyramid_of(base);
    let mut state = identity_state();
    state.adjustments[11] = photoforge_pipeline::Adjustment::masked(
        OpParams::Exposure { stops: 3.0 },
        Mask::RadialGradient {
            center: [0.5, 0.5],
            radius: [0.3, 0.3],
            feather: 0.5,
        },
    );
    let mut r = Renderer::new(state);
    let out = r.render_level(&pyr, 0, Quality::Final);
    assert_eq!(
        out.pixel(10, 10),
        [128, 128, 128, 255],
        "outside mask untouched"
    );
    assert!(out.pixel(40, 40)[0] > 128, "center brightened");
}

#[test]
fn disabled_ops_are_skipped() {
    let base = gradient_h(32, 32);
    let pyr = pyramid_of(base.clone());
    let mut state = identity_state();
    state.adjustments[1].op = OpParams::Exposure { stops: 2.0 };
    state.adjustments[1].enabled = false;
    let mut r = Renderer::new(state);
    let out = r.render_level(&pyr, 0, Quality::Final);
    assert_eq!(*out, base, "disabled adjustment must not alter output");
}

#[test]
fn geometry_crop_rotate_and_identity() {
    let base = gradient_h(64, 48);
    let pyr = pyramid_of(base);
    let mut r = Renderer::new(identity_state());

    let mut state = identity_state();
    state.geometry.crop = Some(CropRect {
        x: 4,
        y: 6,
        w: 20,
        h: 10,
    });
    r.sync_state(&state);
    let out = r.render_level(&pyr, 0, Quality::Final);
    assert_eq!((out.width(), out.height()), (20, 10));

    state.geometry = Geometry {
        rotate_quarter: 1,
        ..Geometry::default()
    };
    r.sync_state(&state);
    let out = r.render_level(&pyr, 0, Quality::Final);
    assert_eq!((out.width(), out.height()), (48, 64));

    // Zero straighten must be an exact identity on top of the chain.
    state.geometry.straighten_deg = 0.0;
    r.sync_state(&state);
    let out = r.render_level(&pyr, 0, Quality::Final);
    assert_eq!((out.width(), out.height()), (48, 64));
}

#[test]
fn full_export_equals_level0_render() {
    // Fused full-res path and staged per-op path must agree within a couple
    // of LSB on the 8-bit grid (Q16 staging drift, ADR-0006).
    let base = radial(150, 100);
    let pyr = pyramid_of(base);
    let mut state = identity_state();
    state.adjustments[1].op = OpParams::Exposure { stops: 0.7 };
    state.adjustments[3].op = OpParams::Highlights { amount: -0.6 };
    state.adjustments[4].op = OpParams::Shadows { amount: 0.4 };
    state.adjustments[10].op = OpParams::Vibrance { amount: 0.5 };
    state.adjustments[11].op = OpParams::Saturation { amount: -0.2 };

    let mut staged = Renderer::new(state.clone());
    let a = staged.render_level(&pyr, 0, Quality::Final);
    let mut fused = Renderer::new(state);
    let b = fused.render_full(&pyr, Quality::Final);

    let mut max_diff = 0i32;
    for (pa, pb) in a.as_raw().iter().zip(b.as_raw()) {
        max_diff = max_diff.max(i32::from((i16::from(*pa) - i16::from(*pb)).abs()));
    }
    assert!(max_diff <= 2, "staged vs fused drift {max_diff} LSB");
}

#[test]
fn render_full_applies_masks_and_geometry() {
    let base = solid(60, 40, [128, 128, 128, 255]);
    let pyr = pyramid_of(base);
    let mut state = identity_state();
    state.adjustments[1].op = OpParams::Exposure { stops: 2.0 };
    // Mask runs over the PRE-crop frame: fully applied from x = 30 onward.
    state.adjustments[11] = photoforge_pipeline::Adjustment::masked(
        OpParams::Exposure { stops: -2.0 },
        Mask::LinearGradient {
            start: [0.0, 0.5],
            end: [0.5, 0.5],
        },
    );
    state.geometry.crop = Some(CropRect {
        x: 0,
        y: 0,
        w: 30,
        h: 20,
    });
    let mut r = Renderer::new(state);
    let out = r.render_full(&pyr, Quality::Final);
    assert_eq!((out.width(), out.height()), (30, 20));
    // x=28: mask ≈ 0.99 → +2EV then −2EV → back to ≈128. Left: +2EV only.
    let right = i16::from(out.pixel(28, 10)[0]);
    assert!(
        (right - 128).abs() <= 3,
        "right side {right} should cancel to 128"
    );
    assert!(out.pixel(1, 10)[0] > 128);
}

#[test]
fn mask_rasters_are_cached() {
    let pyr = pyramid_of(gradient_h(32, 32));
    let mut state = identity_state();
    state.adjustments[11] = photoforge_pipeline::Adjustment::masked(
        OpParams::Exposure { stops: 1.0 },
        Mask::Brush {
            strokes: vec![Stroke {
                points: vec![[0.5, 0.5]],
                radius: 0.2,
                hardness: 0.7,
                flow: 1.0,
                erase: false,
            }],
        },
    );
    let mut r = Renderer::new(state);
    r.render_level(&pyr, 0, Quality::Final);
    let len_after_first = r.cache_len();
    r.render_level(&pyr, 0, Quality::Final);
    assert_eq!(
        r.cache_len(),
        len_after_first,
        "no cache growth on re-render"
    );
}

/// Random states render without panicking on every level, and outputs stay
/// in gamut (u8 storage guarantees it structurally — we check shape + no
/// NaN-driven panics through the whole stack).
#[test]
fn property_random_states_never_panic() {
    let mut rng = photoforge_core::rng::XorShift64::new(777);
    for case in 0..12 {
        let (w, h) = [(24u32, 18u32), (5, 31), (1, 1), (64, 8)][case % 4];
        let base = match case % 3 {
            0 => gradient_h(w, h),
            1 => radial(w, h),
            _ => solid(w, h, [30, 60, 90, 255]),
        };
        let pyr = pyramid_of(base);
        let mut state = identity_state();
        for adj in state.adjustments.iter_mut() {
            if rng.next_f32() < 0.5 {
                continue;
            }
            adj.op = match adj.op.kind() {
                photoforge_ops::params::OpKind::Exposure => OpParams::Exposure {
                    stops: rng.range_f32(-3.0, 3.0),
                },
                photoforge_ops::params::OpKind::WhiteBalance => OpParams::WhiteBalance {
                    temp: rng.range_f32(-1.0, 1.0),
                    tint: rng.range_f32(-1.0, 1.0),
                },
                _ => OpParams::Contrast {
                    amount: rng.range_f32(-1.0, 1.0),
                },
            };
        }
        if case % 2 == 0 {
            state.adjustments[11] = photoforge_pipeline::Adjustment::masked(
                OpParams::Exposure { stops: 1.5 },
                if case % 4 == 0 {
                    Mask::RadialGradient {
                        center: [rng.next_f32(), rng.next_f32()],
                        radius: [rng.range_f32(0.05, 0.5), rng.range_f32(0.05, 0.5)],
                        feather: rng.next_f32(),
                    }
                } else {
                    Mask::Brush {
                        strokes: vec![Stroke {
                            points: vec![[0.1, 0.1], [0.9, 0.9]],
                            radius: rng.range_f32(0.01, 0.3),
                            hardness: rng.next_f32(),
                            flow: rng.next_f32(),
                            erase: rng.next_f32() < 0.5,
                        }],
                    }
                },
            );
        }
        if case % 3 == 0 {
            state.geometry.straighten_deg = rng.range_f32(-15.0, 15.0);
        }
        let mut r = Renderer::new(state);
        for level in 0..pyr.num_levels() {
            let out = r.render_level(&pyr, level, Quality::Final);
            assert_eq!(out.width() * out.height() * 4, out.len_bytes() as u32);
        }
        let _ = r.render_full(&pyr, Quality::Final);
    }
}

#[test]
fn masked_spatial_and_crop_through_pipeline() {
    // Left half bright, right half dark; a brush-masked clarity boosts local
    // contrast only where painted; a crop cuts the output.
    let base = photoforge_core::LinearImage::from_fn(120, 60, |x, _| {
        let v = if x < 60 { 0.9 } else { 0.2 };
        [v, v, v, 1.0]
    })
    .to_pixel_buffer();
    let pyr = pyramid_of(base.clone());
    let mut state = identity_state();
    // Paint the brush centered on the brightness edge (x = 60/120 = 0.5).
    state.adjustments[14] = photoforge_pipeline::Adjustment::masked(
        OpParams::Clarity { amount: 0.8 },
        Mask::Brush {
            strokes: vec![Stroke {
                points: vec![[0.5, 0.5], [0.5, 0.5]],
                radius: 0.12,
                hardness: 0.9,
                flow: 1.0,
                erase: false,
            }],
        },
    );
    state.geometry.crop = Some(photoforge_pipeline::CropRect {
        x: 20,
        y: 10,
        w: 60,
        h: 30,
    });
    let mut r = Renderer::new(state);
    let out = r.render_level(&pyr, 0, Quality::Final);
    assert_eq!((out.width(), out.height()), (60, 30), "crop applied");

    // Mask-0 region (bright side, source x < ~45): bit-exact base values.
    let base_crop = base.region(20, 10, 60, 30);
    assert_eq!(
        out.pixel(10, 15),
        base_crop.pixel(10, 15),
        "mask-0 untouched"
    );
    assert_eq!(out.pixel(0, 0), base_crop.pixel(0, 0), "mask-0 corner");
    // Painted side (source x > 60): clarity darkens the dark flank of the
    // edge (source x=62 = crop x=42).
    let painted = out.pixel(42, 15)[0];
    let base_px = base_crop.pixel(42, 15)[0];
    assert!(
        i16::from(painted) < i16::from(base_px) - 20,
        "clarity must darken painted flank: {painted} vs {base_px}"
    );
}

#[test]
fn brush_mask_spatial_matches_unmasked_outside() {
    // Away from the painted circle, a masked blur must be a no-op.
    let base = photoforge_fixtures::radial(80, 80);
    let pyr = pyramid_of(base.clone());
    let mut state = identity_state();
    state.adjustments[16] = photoforge_pipeline::Adjustment::masked(
        OpParams::Blur { radius: 6.0 },
        Mask::Brush {
            strokes: vec![Stroke {
                points: vec![[0.5, 0.5], [0.5, 0.5]],
                radius: 0.1,
                hardness: 0.95,
                flow: 1.0,
                erase: false,
            }],
        },
    );
    let mut r = Renderer::new(state);
    let out = r.render_level(&pyr, 0, Quality::Final);
    // Corner: mask = 0 → identical to the base image.
    assert_eq!(out.pixel(2, 2), base.pixel(2, 2), "mask-0 region untouched");
    // Center: blurred (not equal, but close to the local average).
    assert_ne!(out.pixel(40, 40), base.pixel(40, 40));
}
