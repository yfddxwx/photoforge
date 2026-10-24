# 0006 — Pipeline caching and quantization

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

The non-destructive pipeline re-renders pixels from the decoded original on
every change. Editing one slider out of fifteen must not recompute the whole
stack, and preview-vs-export must not diverge visibly. A first
implementation cached every stage as 8-bit sRGB: quantization error from
dark-region sRGB codes was amplified by subsequent gains, drifting up to
8 LSB between the staged (preview) and fused (export) paths.

## Decision

1. **Validity = FNV-1a prefix hash.** Each stage cache entry stores the hash
   of the `(id, revision)` sequence of the *enabled* adjustments that
   produced it. A stage is reusable iff the current prefix hash matches —
   edits, enables/disables, adds and removals upstream all invalidate
   downstream stages automatically. Every stage is keyed per pyramid level.
2. **Stage buffers are 16-bit linear** (`core::LinearImageQ16`, 2× u8
   memory): quantization error (~1.5e-5 linear) amplified through a dozen
   ops stays ≤ 2 LSB on the 8-bit display grid (property-tested), so preview
   and export agree visually.
3. **Final-frame cache.** The post-geometry 8-bit result is cached per
   (level, quality) with the full prefix hash + geometry revision; unchanged
   state renders cost nothing (the common idle case).
4. **Mask rasters** are cached per (adjustment id, resolution), keyed by an
   FNV hash of the mask geometry.
5. **Export path** (`render_full`) replays the stack in one fused pass per
   band at full resolution — no per-stage quantization at all — then applies
   geometry. Equivalence with the staged path is property-tested (≤ 2 LSB).
6. **Memory budget.** The cache holds a byte budget (512 MiB default) and
   evicts least-recently-used entries; entries larger than the budget are
   not cached.
7. **Masked ops do not break fusion chains**: a masked pointwise op blends
   per pixel inside the chain (`out = lerp(in, op(in), mask)`), so the
   unmasked common case stays maximally fused.

## Consequences

- Editing adjustment *k* recomputes only stages ≥ *k*; the idle case hits
  the final cache.
- Preview staging cost: 2× memory vs u8, bounded by the LRU budget.
- Cache correctness is defined by the hash, not by mutation bookkeeping —
  one mechanism covers param edits, structural changes and quality switches.
