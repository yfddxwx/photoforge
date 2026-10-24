# 0010 — Preset and sidecar formats

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

The non-destructive workflow needs two JSON artifacts: `.pfg` sidecars
(the per-image edit state, stored next to the original) and preset files
(shareable looks applied to many images, consumed by the GUI and
`photoforge-cli`).

## Decision

1. **Sidecar** — `<full original file name>.pfg` (e.g. `IMG_0001.JPG.pfg`,
   unambiguous across same-stem files):
   `{ "format": "photoforge-sidecar", "version": 1, "state": EditState }`.
   The state serializes with internally-tagged ops (`"type": "exposure"`)
   and normalized mask coordinates (ADR-0007) — resolution-independent.
2. **Preset** — `{ "version": 1, "name", "adjustments": [{op, enabled}, …] }`
   capturing **only the global adjustment stack in canonical order**
   (ADR-0008). Masks, local adjustments, geometry and crops are
   image-specific and never serialized into presets. `apply_to` overwrites
   the target's global stack and leaves everything else untouched.
3. **Versioning.** Both formats carry a version; loading a file with a
   different major version is an error (`PresetError::Version`). Unknown
   fields are tolerated (serde defaults) for forward compatibility.
4. **Batch semantics.** For `photoforge-cli`, the state resolution order is:
   sidecar if present → else fresh global stack → then the preset overwrites
   the global stack. Per-image sidecar edits therefore win over the preset
   for shared ops; the preset still provides ops the sidecar's global stack
   also covers (last writer wins per op, order documented).

## Consequences

- Sidecars travel with the images (rename-safe as a pair) and are
  human-inspectable; presets are diffable and scriptable.
- Applying a preset through the CLI is deterministic and sidecar-compatible,
  which the batch acceptance test exercises end-to-end.
