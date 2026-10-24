//! Geometry op tests: exact remaps, analytic resampling values, homography
//! correctness, identity-at-zero and no-panic degenerate inputs (ADR-0004).

use photoforge_core::PixelBuffer;
use photoforge_ops::geometry::*;

/// 3×2 image with four distinct corner colors + distinct top-center.
fn marker_image() -> PixelBuffer {
    let mut img = PixelBuffer::new(3, 2);
    img.set_pixel(0, 0, [255, 0, 0, 255]); // TL red
    img.set_pixel(2, 0, [0, 255, 0, 255]); // TR green
    img.set_pixel(0, 1, [0, 0, 255, 255]); // BL blue
    img.set_pixel(2, 1, [255, 255, 0, 255]); // BR yellow
    img.set_pixel(1, 0, [10, 10, 10, 255]); // top-center
    img
}

#[test]
fn rotate90_quarters_are_exact() {
    let m = marker_image();
    // CW 90: stored TL -> displayed TR.
    let r1 = rotate90(&m, 1);
    assert_eq!((r1.width(), r1.height()), (2, 3));
    assert_eq!(r1.pixel(1, 0), [255, 0, 0, 255]); // TR = red
    assert_eq!(r1.pixel(1, 2), [0, 255, 0, 255]); // BR = green
    assert_eq!(r1.pixel(0, 0), [0, 0, 255, 255]); // TL = blue
    assert_eq!(r1.pixel(0, 2), [255, 255, 0, 255]); // BL = yellow
    assert_eq!(r1.pixel(1, 1)[0], 10); // top-center stays on top
                                       // Four CW = identity; single CCW == three CW.
    assert_eq!(rotate90(&rotate90(&rotate90(&rotate90(&m, 1), 1), 1), 1), m);
    assert_eq!(rotate90(&m, 3), rotate90(&rotate90(&m, 1), 2));
    // 180: TL <-> BR.
    let r2 = rotate90(&m, 2);
    assert_eq!(r2.pixel(2, 1), [255, 0, 0, 255]);
    assert_eq!(r2.pixel(0, 0), [255, 255, 0, 255]);
}

#[test]
fn flips_are_exact() {
    let m = marker_image();
    let fh = flip_h(&m);
    assert_eq!(fh.pixel(0, 0), [0, 255, 0, 255]);
    assert_eq!(fh.pixel(2, 0), [255, 0, 0, 255]);
    assert_eq!(flip_h(&fh), m);
    let fv = flip_v(&m);
    assert_eq!(fv.pixel(0, 0), [0, 0, 255, 255]);
    assert_eq!(fv.pixel(0, 1), [255, 0, 0, 255]);
    assert_eq!(flip_v(&fv), m);
}

#[test]
fn crop_clamps() {
    let m = marker_image();
    assert_eq!(crop(&m, 1, 0, 2, 2).width(), 2);
    assert_eq!(crop(&m, 10, 10, 2, 2).len_bytes(), 0);
    assert_eq!(crop(&m, 0, 0, 99, 99), m);
}

#[test]
fn resize_solid_stays_solid() {
    let solid = PixelBuffer::solid(20, 10, [100, 150, 200, 255]);
    for resample in [Resample::Bilinear, Resample::Lanczos3] {
        let up = resize(&solid, 40, 30, resample);
        assert_eq!(up.pixel(17, 23), [100, 150, 200, 255]);
        let down = resize(&solid, 5, 3, resample);
        assert_eq!(down.pixel(2, 1), [100, 150, 200, 255]);
    }
}

#[test]
fn resize_downscale_averages() {
    // 2x2 with three bright quarters -> 1x1 somewhere strictly between.
    let mut img = PixelBuffer::new(2, 2);
    img.set_pixel(0, 0, [0, 0, 0, 255]);
    for p in [(1, 0), (0, 1), (1, 1)] {
        img.set_pixel(p.0, p.1, [100, 0, 0, 255]);
    }
    let out = resize(&img, 1, 1, Resample::Lanczos3);
    let v = out.pixel(0, 0)[0];
    // The average happens in linear light, not byte space — it must sit
    // strictly between the dark and bright byte values.
    assert!(v > 30 && v < 100, "averaged value {v}");
}

#[test]
fn resize_upscale_preserves_corners() {
    let m = marker_image();
    let up = resize(&m, 6, 4, Resample::Bilinear);
    assert_eq!((up.width(), up.height()), (6, 4));
    assert_eq!(up.pixel(0, 0), [255, 0, 0, 255]);
    assert_eq!(up.pixel(5, 3), [255, 255, 0, 255]);
}

#[test]
fn straighten_zero_degrees_is_identity() {
    let m = marker_image();
    for resample in [Resample::Bilinear, Resample::Lanczos3] {
        let out = straighten(
            &m,
            0.0,
            WarpOptions {
                resample,
                ..Default::default()
            },
        );
        assert_eq!(out, m, "0° straighten must be exact ({resample:?})");
    }
}

#[test]
fn straighten_90_matches_rotate90() {
    // Square image: a 90° rotation about the center is an exact grid
    // permutation, so bilinear sampling must reproduce rotate90 bit-exactly.
    let mut m = PixelBuffer::new(4, 4);
    m.set_pixel(0, 0, [255, 0, 0, 255]);
    m.set_pixel(3, 0, [0, 255, 0, 255]);
    m.set_pixel(0, 3, [0, 0, 255, 255]);
    m.set_pixel(3, 3, [255, 255, 0, 255]);
    let out = straighten(
        &m,
        90.0,
        WarpOptions {
            resample: Resample::Bilinear,
            fill: EdgeFill::ClampEdge,
        },
    );
    let expected = rotate90(&m, 1);
    assert_eq!(out.pixel(3, 0), expected.pixel(3, 0)); // red -> TR
    assert_eq!(out.pixel(3, 3), expected.pixel(3, 3)); // green -> BR
    assert_eq!(out.pixel(0, 0), expected.pixel(0, 0)); // blue -> TL
    assert_eq!(out.pixel(0, 3), expected.pixel(0, 3)); // yellow -> BL
}

#[test]
fn straighten_small_angle_keeps_center() {
    let mut img = PixelBuffer::solid(21, 21, [50, 60, 70, 255]);
    img.set_pixel(10, 10, [200, 0, 0, 255]);
    let out = straighten(
        &img,
        5.0,
        WarpOptions {
            resample: Resample::Bilinear,
            fill: EdgeFill::ClampEdge,
        },
    );
    assert_eq!(out.pixel(10, 10), [200, 0, 0, 255]);
}

#[test]
fn homography_maps_quad_corners_exactly() {
    let quad = [[10.0, 5.0], [200.0, 30.0], [190.0, 150.0], [20.0, 140.0]];
    let (h, w, hgt) = homography_for_quad(quad);
    assert!(w > 0 && hgt > 0);
    let dst_corners = [
        [0.0, 0.0],
        [w as f32 - 1.0, 0.0],
        [w as f32 - 1.0, hgt as f32 - 1.0],
        [0.0, hgt as f32 - 1.0],
    ];
    for (d, q) in dst_corners.iter().zip(quad.iter()) {
        let p = apply_homography(&h, *d);
        assert!((p[0] - q[0]).abs() < 0.05, "{d:?} -> {p:?} != {q:?}");
        assert!((p[1] - q[1]).abs() < 0.05);
    }
}

#[test]
fn homography_identity_for_axis_aligned_quad() {
    let quad = [[0.0, 0.0], [30.0, 0.0], [30.0, 20.0], [0.0, 20.0]];
    let (h, w, hgt) = homography_for_quad(quad);
    assert_eq!((w, hgt), (31, 21)); // pixel-center convention: span + 1
    let p = apply_homography(&h, [10.0, 7.0]);
    assert!((p[0] - 10.0).abs() < 1e-3 && (p[1] - 7.0).abs() < 1e-3);
}

#[test]
fn perspective_output_size_from_quad() {
    let m = marker_image();
    // Axis-aligned quad spanning 3x2 exactly.
    let quad = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]];
    let out = perspective_correct(&m, quad, WarpOptions::default());
    assert_eq!((out.width(), out.height()), (3, 2));
    // Content matches the original (identity mapping).
    assert_eq!(out, m);
}

#[test]
fn perspective_keeps_contrast_through_warp() {
    let mut img = PixelBuffer::new(60, 60);
    for y in 0..60 {
        for x in 0..60 {
            let v = if (x / 10) % 2 == 0 { 255u8 } else { 0 };
            img.set_pixel(x, y, [v, v, v, 255]);
        }
    }

    let quad = [[5.0, 5.0], [55.0, 15.0], [50.0, 55.0], [8.0, 45.0]];
    let out = perspective_correct(
        &img,
        quad,
        WarpOptions {
            resample: Resample::Bilinear,
            fill: EdgeFill::ClampEdge,
        },
    );
    assert!(out.width() > 0 && out.height() > 0);
    for y in 0..out.height() {
        let mut min = 255u8;
        let mut max = 0u8;
        for x in 0..out.width() {
            let v = out.pixel(x, y)[0];
            min = min.min(v);
            max = max.max(v);
        }
        assert!(max - min > 100, "row {y} lost contrast");
    }
}

#[test]
fn warp_edge_fill_modes() {
    let img = PixelBuffer::solid(4, 4, [10, 20, 30, 255]);
    let shift = [[1.0, 0.0, -10.0], [0.0, 1.0, -10.0], [0.0, 0.0, 1.0]];
    let clamped = warp(
        &img,
        &shift,
        4,
        4,
        WarpOptions {
            resample: Resample::Bilinear,
            fill: EdgeFill::ClampEdge,
        },
    );
    assert_eq!(clamped.pixel(0, 0), [10, 20, 30, 255]); // replicated edge
    let black = warp(
        &img,
        &shift,
        4,
        4,
        WarpOptions {
            resample: Resample::Bilinear,
            fill: EdgeFill::Black,
        },
    );
    assert_eq!(black.pixel(0, 0), [0, 0, 0, 255]);
}

#[test]
fn degenerate_inputs_no_panic() {
    let empty = PixelBuffer::new(0, 0);
    // An explicitly requested 4x4 output of an empty input is a black 4x4.
    assert_eq!(
        resize(&empty, 4, 4, Resample::Lanczos3).len_bytes(),
        4 * 4 * 4
    );
    assert_eq!(
        straighten(&empty, 33.0, WarpOptions::default()).len_bytes(),
        0
    );
    let one = PixelBuffer::solid(1, 1, [9, 9, 9, 255]);
    let up = resize(&one, 5, 7, Resample::Lanczos3);
    assert_eq!(up.pixel(3, 4), [9, 9, 9, 255]);
    let _ = straighten(&one, 179.0, WarpOptions::default());
    let _ = warp(&one, &[[0.0; 3]; 3], 3, 3, WarpOptions::default());
}
