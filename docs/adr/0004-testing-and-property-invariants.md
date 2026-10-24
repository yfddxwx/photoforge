# 0004 — Testing policy: analytic values + property invariants

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Requirement: every op is a pure function with unit tests; no panics on any
input image; property tests must cover output range, NaN-freedom and
constant-image invariance; test images are synthesized in code.

## Decision

Testing rests on four layers:

1. **Analytic unit tests.** Ops are verified against hand-computed values:
   +1 EV multiplies linear values by exactly 2; saturation −100% yields
   R = G = B; the sRGB u8 round-trip is exact for all 256 values; the
   monotone cubic spline passes through its control points and preserves
   monotonicity; Lanczos3 impulse response equals the windowed sinc.
2. **Property loops** (deterministic xorshift RNG — no external property-test
   dependency): for every op, run over randomized buffers *and* randomized
   parameter sets, asserting:
   - output shape equals input shape;
   - all channels within [0, 1] after the op (where documented);
   - no NaN/Inf;
   - purity: the input buffer is bit-identical after the call;
   - default/identity parameters reproduce the input exactly (bit-equal).
3. **Constant-image invariance:** constant images stay constant (within a
   documented float epsilon) through blur, sharpening, clarity and denoise;
   grayscale images stay neutral through saturation ops.
4. **No-panic fuzzing:** random byte buffers into the decode layer must yield
   `Err`, never panic; the full pipeline must accept arbitrary
   buffer/parameter combinations without panicking (property loop).

Sizes include degenerate cases: 1×1, 1×N, N×1, prime dimensions, and radius
larger than the image.

## Consequences

- Identity-at-defaults is enforced *bit-exactly*, which forces ops to be
  written so that neutral parameters short-circuit exactly (e.g. exposure
  factor 1.0 multiplies by 1.0, no accumulation error).
- Property loops are seeded deterministically, so failures reproduce.
- The test suite uses the same generator library as the `photoforge-fixtures`
  binary; nothing is downloaded.
