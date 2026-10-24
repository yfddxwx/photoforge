# photoforge

A non-destructive desktop photo editor written in Rust, targeting the core
Lightroom workflow: a non-destructive adjustment stack, masked local
adjustments, presets and batch export. All image processing algorithms —
color spaces, resampling, blurs, splines, histograms — are implemented from
scratch in this repository (see [ADR-0001](docs/adr/0001-dependency-policy.md));
the only image I/O crates used are `image` (decoding/encoding) and
`kamadak-exif` (metadata).

## Status

Work in progress, built milestone by milestone (M0–M8). See
[ARCHITECTURE.md](docs/ARCHITECTURE.md) and the
[ADR index](docs/adr/README.md).

| Milestone | Scope |
|-----------|-------|
| M0 | Workspace scaffolding, CI (Windows/Linux/macOS), synthetic test-image fixtures |
| M1 | `core`: pixel buffers, linear-light working space, sRGB/HSV/OKLab, histograms, tile parallelism |
| M2 | `io` + geometry: decode/encode, EXIF orientation, mip pyramid, Lanczos3, crop/rotate/flip/straighten/perspective |
| M3 | Basic adjustment pipeline: exposure/contrast/highlights/shadows/white balance/vibrance/saturation, non-destructive op stack with per-stage caching |
| M4 | GUI: open image, canvas pan/zoom, adjustment panel, live preview, histogram, export |
| M5 | Tone curves (monotone spline editor), levels, 8-zone HSL, `.cube` LUT import |
| M6 | Filters: gaussian blur, USM sharpening, clarity, denoise, grain, vignette, B&W/duotone |
| M7 | Local adjustments (brush / linear / radial masks), crop overlay tool, before/after |
| M8 | History (memory-capped undo/redo), session autosave, background export, filmstrip, presets, CLI batch tool, benchmarks, release builds |

## Building

Requires Rust (MSRV 1.80, edition 2021):

```sh
cargo build --release
cargo run --release -p photoforge-gui            # GUI editor
cargo run --release -p photoforge-cli -- ./photos/ --preset film.json -o out/
cargo test --workspace                           # all tests
```

## Testing

All test images are synthesized by code (no external downloads): gradients,
solid colors, noise fields and a synthetic test chart. Run the fixture
generator to write them to disk (the test suite uses the same generator
in-memory):

```sh
cargo run -p photoforge-fixtures -- --out testdata
```

You can drop your own photos into the GUI to try it on real images; JPEG, PNG,
WebP, TIFF and BMP inputs are supported.
