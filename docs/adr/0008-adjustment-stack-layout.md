# 0008 — Adjustment stack: fixed global order + local instances

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Lightroom-style UIs show a fixed panel of adjustments (each present with a
neutral default), while "local adjustments" need an unbounded number of
masked instances. The data model must serve both the GUI panel and the
ordered-stack requirement (parameters can be changed at any time).

## Decision

1. **The state is a plain ordered list** (`EditState.adjustments`), executed
   front to back.
2. **The GUI's global adjustments are a fixed canonical set** in a fixed
   order (`ops::params::GLOBAL_OP_ORDER`): white balance, exposure,
   contrast, highlights, shadows, whites, blacks, vibrance, saturation (the
   list grows with M5/M6 ops). They are always present, identity by default;
   disabling is equivalent to identity.
3. **Local adjustments are user-added entries appended after the global
   stack**, each carrying an optional mask (ADR-0007). Users can add/remove
   instances (e.g. three brushed exposure tweaks); reordering the local list
   is permitted but not required by any milestone.
4. **Presets capture op parameters of the global stack** (and, explicitly,
   nothing image-specific: no masks, no geometry, no crop) — see the preset
   format decision in the M8 milestone.

## Consequences

- The pipeline has no special case for "global vs local": everything is an
  ordered list entry; the distinction is purely UI-side.
- Cache prefix hashing (ADR-0006) handles both fixed and dynamic entries
  uniformly.
- Panel sections map 1:1 onto the canonical list, giving Lightroom-like UX
  without slider-visibility bookkeeping.
