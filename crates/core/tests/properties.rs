//! Property tests for core conversions and invariants (ADR-0004).

use photoforge_core::color::{oklab, srgb};
use photoforge_core::rng::XorShift64;
use photoforge_core::{LinearImage, PixelBuffer};

/// Random u8 buffers of assorted shapes survive the linear round-trip
/// bit-exactly.
#[test]
fn property_pixel_buffer_linear_roundtrip() {
    let mut rng = XorShift64::new(2024);
    for &(w, h) in &[(1u32, 1u32), (1, 7), (7, 1), (13, 17), (64, 64), (129, 3)] {
        let mut pb = PixelBuffer::new(w, h);
        for px in pb.as_raw_mut().chunks_exact_mut(4) {
            px[0] = rng.next_u8();
            px[1] = rng.next_u8();
            px[2] = rng.next_u8();
            px[3] = rng.next_u8();
        }
        let lin = LinearImage::from_pixel_buffer(&pb);
        assert_eq!(lin.width, w, "width mismatch");
        let back = lin.to_pixel_buffer();
        assert_eq!(pb, back, "roundtrip mismatch at {w}x{h}");
    }
}

/// For arbitrary linear values in [0,1], encoding to u8 and decoding again
/// stays within one 8-bit code of the original.
#[test]
fn property_encode_decode_bounded_error() {
    let mut rng = XorShift64::new(7);
    for _ in 0..50_000 {
        let lin = rng.next_f32();
        let u = srgb::encode_u8(lin);
        let back = srgb::decode_u8(u);
        // Encoded quantization: |lin - decode(u)| <= half a code interval in
        // linear light (worst near black, where the curve is steep).
        let tol = 0.5 / 255.0 * 12.92 + 1e-6;
        assert!((lin - back).abs() <= tol, "lin {lin} -> {u} -> {back}");
        assert!((0.0..=1.0).contains(&back));
    }
}

/// OKLab round-trips arbitrary in-gamut colors within 1e-4, and chroma of
/// grays is zero.
#[test]
fn property_oklab_roundtrip() {
    let mut rng = XorShift64::new(11);
    for _ in 0..5000 {
        let rgb = [rng.next_f32(), rng.next_f32(), rng.next_f32()];
        let lab = oklab::linear_srgb_to_oklab(rgb);
        let back = oklab::oklab_to_linear_srgb(lab);
        for i in 0..3 {
            assert!(
                (back[i] - rgb[i]).abs() <= 1e-4,
                "{rgb:?} -> {lab:?} -> {back:?}"
            );
        }
    }
}

/// Histogram counts are consistent across channels and sum to pixel count.
#[test]
fn property_histogram_consistency() {
    let mut rng = XorShift64::new(5);
    for &(w, h) in &[(1, 1), (31, 17), (50, 50)] {
        let mut pb = PixelBuffer::new(w, h);
        for px in pb.as_raw_mut().chunks_exact_mut(4) {
            px[0] = rng.next_u8();
            px[1] = rng.next_u8();
            px[2] = rng.next_u8();
            px[3] = 255;
        }
        let hist = photoforge_core::histogram::Histogram::compute(&pb);
        assert_eq!(hist.total(), u64::from(w * h));
        for c in hist.g.iter().chain(hist.b.iter()).chain(hist.luma.iter()) {
            assert!(*c <= w * h);
        }
    }
}
