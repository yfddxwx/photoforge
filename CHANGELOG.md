# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- M8: `photoforge-history` — generic command-pattern undo/redo with a byte
  budget, JSON disk spill with transparent reload, and a snapshot ring
  (ADR-0009).
- M8: `photoforge-presets` — version-checked JSON presets (global stack
  only) and `.pfg` sidecars stored next to originals (ADR-0010).
- M8: GUI history wiring — labeled undo/redo (Ctrl+Z/Y), sidecar save
  (Ctrl+S), crash-recovery session autosave/restore, preset save/apply
  dialogs, filmstrip folder view with background thumbnails and per-image
  state caching.
- M8: `photoforge-cli` — headless batch export (preset + sidecar merge,
  rayon-parallel, EXIF-correct, optional resize/sidecar-out/skip-existing),
  covered by end-to-end tests.
- M8: criterion benchmarks for ops and the pipeline (fit-preview and 24 MP
  export scenarios) with measured-numbers documentation and a gap analysis
  against the CPU-only budgets (ARCHITECTURE.md).
- M8: performance work — `Prepared` domain-tagged op functions with
  `fuse_run` batched encode/decode, 3-D LUTs for OKLab/HSL ops, segment-level
  preview caching, fast integer-hash grain noise.
- M8: cargo-dist release configuration (workspace metadata + release
  workflow) for three-platform artifacts.

- M5: tone curves — Fritsch-Carlson monotone cubic splines, per-channel
  (R/G/B) plus luma channel, applied in the sRGB-encoded domain with a
  prepared 4096-entry LUT in the pipeline; draggable curve editor widget
  (add on click, right-click removes, per-channel colors).
- M5: levels (black/gamma/white, encoded domain) with histogram-percentile
  auto button.
- M5: eight-zone HSL color mixer (raised-cosine zone weights on the hue
  wheel, tabbed per-zone UI).
- M5: .cube 3D LUT import (parser, trilinear sampler, display-domain
  application) with file dialog.
- M5: continuous sRGB encode table (16-bit) for sub-8-bit-precision
  encoded-domain ops.

- M3: `ops` pointwise adjustments — exposure (+1 EV = exact ×2), contrast
  (18% pivot power curve), highlights/shadows/whites/blacks (provably
  monotone region ops pinning black/white points), white balance (warm/cool
  anchor gains + tint), vibrance/saturation in OKLab. Full property suite:
  purity, range, NaN-freedom, identity-at-defaults, tone monotonicity,
  region selectivity.
- M3: `photoforge-pipeline` — non-destructive renderer with fixed global
  stack + masked local adjustments, Q16 per-stage caches keyed by FNV prefix
  hashes (LRU byte budget), fused full-resolution export replay, mask
  rasterization (brush / linear / radial gradients), geometry application.
- M3: `core::LinearImageQ16` 16-bit linear stage storage (staged-vs-fused
  drift ≤ 2 LSB, tested).
- M3: ADRs 0006 (caching), 0007 (masks), 0008 (stack layout).

- M2: `ops::geometry` — crop, rotate90, flip, straighten (arbitrary angle,
  edge-fill), perspective correction (4-point DLT homography), Lanczos3 and
  bilinear resize with proper downscale kernel widening, all resampling in
  linear light.
- M2: `photoforge-io` — decode/encode (JPEG/PNG/WebP/TIFF/BMP), EXIF parsing
  and automatic orientation correction for all 8 orientations, mip image
  pyramids, thumbnails, metadata summary; garbage input returns errors,
  never panics (property-tested).
- M2: ADR-0005 (geometry order, pixel-center convention, resampling policy).

- M0: cargo workspace with crates `core`, `io`, `ops`, `pipeline`, `history`,
  `gui`, `cli`, `presets`, `fixtures`.
- M0: CI workflow (Windows/Linux/macOS matrix: rustfmt, clippy `-D warnings`,
  tests, release build; MSRV 1.80 job).
- M0: synthetic test-image fixture generator (`photoforge-fixtures`):
  horizontal/vertical/radial gradients, solid colors, deterministic noise,
  grayscale step wedges, resolution wedge, zone plate and a synthetic color
  test chart.
- M0: `docs/ARCHITECTURE.md`, ADRs 0001–0004, `docs/shortcuts.md`.
