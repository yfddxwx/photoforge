# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

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
