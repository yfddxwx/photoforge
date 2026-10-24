# 0007 — Mask representation

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Local adjustments (brush, linear gradient, radial gradient) must survive
resizes and crops of the working image, serialize into `.pfg` sidecars, and
rasterize efficiently at arbitrary render resolutions.

## Decision

1. **Normalized coordinates.** All mask geometry lives in [0, 1] relative to
   the *display-oriented* frame (post-EXIF, pre-user-crop). Brush radius is
   a fraction of the frame diagonal.
2. **Three primitive shapes**, internally tagged in JSON:
   - `brush`: a list of strokes (polyline, radius, hardness 0..1, flow 0..1,
     erase flag). Rasterization splats discs every `radius·0.3` along each
     segment; add strokes take a pixel-wise `max` with the existing coverage,
     erase strokes multiply it down. Falloff is flat to `hardness`, then a
     smoothstep to the rim.
   - `linear_gradient`: projection onto the start→end axis, smoothstepped
     over the whole span (0 at start, 1 at end).
   - `radial_gradient`: normalized elliptical distance with a solid core of
     `1 − feather` and a smoothstep falloff to the rim.
3. **Opacity semantics.** The adjustment applies as
   `out = lerp(in, op(in), mask)` per pixel — mask 0 is a bit-exact no-op,
   mask 1 applies the op fully. Rasters are f32 in [0, 1].
4. **Resolution independence.** Rasters are produced on demand at the render
   resolution and cached (ADR-0006); the same stored mask serves previews,
   1:1 zoom and full-res exports.

## Consequences

- Sidecars stay resolution-independent and human-readable.
- Mask painting coordinates from the GUI need only a single normalized
  transform; no per-level bookkeeping.
- Erase-in-brush is multiplicative, so erasing cannot restore coverage the
  way "subtract" would — matching how editors behave in practice.
