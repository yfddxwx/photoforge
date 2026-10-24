# 0005 — Geometry pipeline order and coordinate conventions

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Geometry has interacting parts: EXIF orientation, user rotations/flips,
straightening (arbitrary-angle rotation with edge fill), perspective
correction and crop. Resampling must be gamma-correct, and both the pipeline
and the GUI need one consistent coordinate convention.

## Decision

1. **Order (applied by the pipeline after the adjustment chain):**
   EXIF orientation (at decode time, see below) → rotate90/flip → straighten →
   perspective correction → crop.
   Rationale: EXIF orientation belongs to the *file*, applied once at decode so
   everything downstream (including sidecar coordinates) lives in display
   orientation. Straighten rotates the full frame; perspective is authored on
   the straightened frame; crop is the final viewport cut.
2. **Coordinate convention:** pixel *centers* are at integer coordinates.
   Quads passed to `homography_for_quad` are in pixel-center units, and the
   output size of a quad mapping is `round(span) + 1` pixels. EXIF rotations
   and flips are exact permutations (no resampling).
3. **Resampling:** geometry warps and resizes happen in linear light
   (convert → warp → convert back). `resize` widens the filter support when
   downscaling (proper low-pass; support = 3·scale for Lanczos3). Warps
   (straighten/perspective) use fixed support — they assume mild scale
   changes; final image *resizing* must go through `resize`.
4. **Edge fill:** straighten/perspective fill out-of-frame areas by edge
   replication (`ClampEdge`, default) or black; selectable per operation.
5. **Quality switching:** interactive drags may request `Resample::Bilinear`
   for latency; committed geometry and exports use `Resample::Lanczos3`.

## Consequences

- `straighten(0°)` and an identity-homography perspective warp are exact
  identities (tested), so committing geometry never degrades untouched
  images.
- A 90° straighten equals `rotate90` only for square frames (rotation about
  the center of a non-square frame is not a grid permutation) — documented
  and reflected in tests.
