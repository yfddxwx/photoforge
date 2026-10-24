# 0001 — Dependency policy

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

photoforge's stated requirement is that all image *processing* algorithms are
implemented from scratch: no OpenCV, no libvips, no C bindings, no CV
frameworks. We still need container/codec support and metadata parsing —
reimplementing JPEG/TIFF entropy coding is out of scope and would not improve
the project.

## Decision

Exactly two external image-related crates are allowed:

1. `image` — container/codec layer only (JPEG, PNG, WebP, TIFF, BMP decode
   and encode). Its resampling/processing helpers are **not** used.
2. `kamadak-exif` — EXIF parsing (pure Rust).

Everything else — color space math, histograms, resampling (Lanczos3, bilinear),
blurs (gaussian, box), sharpening, median/bilateral denoise, noise/grain,
monotone spline curves, homography/perspective, pyramid downsampling — is
implemented in this repository and unit-tested against analytic values.

Non-image dependencies are allowed where they are not image algorithms:
`rayon` (data parallelism), `serde`/`serde_json` (edit-state serialization),
`clap` (CLI), `eframe`/`egui` (GUI toolkit), `rfd` (native file dialogs),
`dirs`, `uuid`, `log`/`env_logger`, `thiserror`.

## Consequences

- All quality/perf characteristics of the processing chain are ours to build,
  benchmark and fix (criterion benchmarks are part of the acceptance criteria).
- Codec bugs are out of our control; decode failures must surface as errors,
  never panics (property-tested with random bytes).
