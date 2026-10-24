# 0003 — Parallelism model: row-band tiling + pointwise op fusion

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

The performance budget is a fit-view preview frame under 16 ms for ~15 stacked
adjustments on a 24 MP image (rendered at a pyramid level matching screen
size) and a full-resolution 20-adjustment export under 3 s on 8 cores.
Processing must be parallel (rayon, per the requirements) and memory-safe.

## Decision

1. **Tile framework = horizontal row bands with halo.** `core::tile` splits
   an image into horizontal bands; a band worker receives its output rows
   (exclusive) and may read source rows `y0 − halo ..= y1 + halo`. All kernels
   used here are separable (gaussian, box, Lanczos) or row-local windows
   (median, bilateral), so bands + halo cover every case without unsafe code:
   input and output are distinct buffers (double buffering).
2. **Column passes** of separable filters run over *column groups* (chunks of
   64 columns processed vertically) to keep parallel granularity while
   staying cache-friendly.
3. **Pointwise op fusion.** The pipeline classifies each adjustment:
   - *pointwise* ops (exposure, contrast, tones, WB, curves, levels, HSL,
     saturation, vibrance, grain, vignette, LUT, …) implement
     `apply_pixel([f32;4]) -> [f32;4]`;
   - *spatial* ops (blur, sharpen, clarity, denoise) implement
     `apply_image(&mut LinearImage)`.
   Consecutive unmasked pointwise adjustments execute as one fused loop
   (single decode/encode round-trip per segment). A masked pointwise
   adjustment becomes its own segment (needs the pre-op pixel for blending);
   spatial ops always end the current segment.
4. **Segment execution** is row-parallel (`(0..h).into_par_iter()` over rows),
   which is enough granularity at every size and keeps the code free of
   hand-rolled scheduling.

## Consequences

- 15 tonal adjustments cost ~1 memory pass instead of ~15; the 16 ms budget
  becomes achievable without GPU.
- Fusion is invisible at the op level: ops stay pure and independently
  testable; the pipeline decides execution strategy.
- A masked spatial adjustment still runs the spatial kernel over the full
  (level) buffer and blends afterwards — correct, occasionally more work than
  strictly needed; acceptable at preview sizes and unchanged at export.
