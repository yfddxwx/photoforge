# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- M0: cargo workspace with crates `core`, `io`, `ops`, `pipeline`, `history`,
  `gui`, `cli`, `presets`, `fixtures`.
- M0: CI workflow (Windows/Linux/macOS matrix: rustfmt, clippy `-D warnings`,
  tests, release build; MSRV 1.80 job).
- M0: synthetic test-image fixture generator (`photoforge-fixtures`):
  horizontal/vertical/radial gradients, solid colors, deterministic noise,
  grayscale step wedges, resolution wedge, zone plate and a synthetic color
  test chart.
- M0: `docs/ARCHITECTURE.md`, ADRs 0001–0004, `docs/shortcuts.md`.
