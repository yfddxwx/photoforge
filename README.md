# photoforge

A non-destructive desktop photo editor written in Rust, targeting the core
Lightroom workflow: a non-destructive adjustment stack, masked local
adjustments, presets, batch export, undo/redo and crash-recovery sessions.
All image processing algorithms — color spaces, resampling, blurs, splines,
histograms — are implemented from scratch in this repository (see
[ADR-0001](docs/adr/0001-dependency-policy.md)); the only image I/O crates
used are `image` (decoding/encoding) and `kamadak-exif` (metadata).

## Features

- **Non-destructive pipeline** — 21 fixed global adjustments (white balance,
  exposure, contrast, highlight/shadow/white/black points, tone curves,
  levels, 8-zone HSL, vibrance, saturation, LUT, sharpening, clarity,
  denoise, blur, B&W mixer, duotone, grain, vignette) plus any number of
  masked local adjustments. Edits live in a `.pfg` sidecar next to the
  original; pixels are always re-derived.
- **Masked local adjustments** — brush (size/hardness/flow/erase), linear and
  radial gradients with red-tint overlay visualization.
- **Tone curves** — Fritsch–Carlson monotone splines, per-channel + luma,
  draggable editor.
- **Color** — linear-light processing throughout (ADR-0002), OKLab for
  chroma ops, `.cube` 3D LUT import.
- **Geometry** — crop with thirds/aspect-lock overlay, straighten, rotate,
  flip, perspective correction, Lanczos3 resize; EXIF orientation applied
  automatically on decode.
- **History** — unlimited-depth undo/redo with a 64 MiB budget and disk
  spill, labeled commands, crash-recovery session restore.
- **Library** — filmstrip folder view with background thumbnails; per-image
  state preserved when switching; before/after hold and split compare.
- **Presets** — save/apply JSON presets; `photoforge-cli` batch-applies them
  headlessly with EXIF-correct output.
- **Darkroom UI** — egui/eframe, dark theme, live histogram, zoom/pan,
  draft-then-refine interactive rendering.

## Building

Requires Rust (MSRV 1.80, edition 2021):

```sh
cargo build --release
cargo run --release -p photoforge-gui              # GUI editor (pass an image path to open it)
cargo run --release -p photoforge-cli -- ./photos/ --preset film.json -o out/
cargo test --workspace                             # all tests (180+)
cargo bench -p photoforge-pipeline                 # performance benchmarks
```

### CLI batch export

```sh
photoforge-cli ./photos/ --preset film.json -o out/     [--format jpeg|png] [--quality 90] [--resize 2048]     [--sidecar-out] [--skip-existing] [--jobs N]
```

Per-image `.pfg` sidecars (if present) are applied after the preset;
processing is parallel (rayon) and EXIF orientation is applied on decode.

## Testing

All test images are synthesized by code (no external downloads): gradients,
solids, noise fields and a synthetic test chart. Run the fixture generator to
write them to disk for manual GUI testing:

```sh
cargo run -p photoforge-fixtures -- --out testdata
```

The suite covers analytic op values (+1 EV = exact ×2), property invariants
(purity, range, NaN-freedom, identity at defaults, tone monotonicity),
cache-invalidation behavior, EXIF orientation end-to-end (hand-built EXIF
in JPEG), no-panic fuzzing of decode inputs, history spill round-trips and
CLI end-to-end runs.

## Performance

The chain is built for throughput: fused pointwise passes, prepared LUTs
(1-D and 3-D), prefix-hash segment caches, band-parallel filters. Measured
numbers on the development machine (a shared sandboxed VM — see the
honest discussion in [ARCHITECTURE.md](docs/ARCHITECTURE.md)) and the gap
to the CPU-only budgets are documented there; closing the remaining gap is
the SIMD/GPU acceleration track (`wgpu`), which the architecture is shaped
for.

## Documentation

- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — crates, color model, execution
  model, caching, threading, measured performance.
- [docs/adr/](docs/adr/README.md) — architecture decision records (0001–0010).
- [docs/shortcuts.md](docs/shortcuts.md) — keyboard shortcuts.

## Status

Milestones M0–M8 complete. GPU compute (wgpu) and RAW decoding (`rawler`)
remain the documented follow-up tracks.
