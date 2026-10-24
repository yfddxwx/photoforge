# ADR Index

Architecture Decision Records (Nygård format: status / context / decision /
consequences). Decisions are immutable; superseding ones link forward.

| # | Title |
|---|-------|
| [0001](0001-dependency-policy.md) | Dependency policy: which external crates are allowed |
| [0002](0002-color-pipeline.md) | Color pipeline: linear working space and op domains |
| [0003](0003-parallelism-and-fusion.md) | Parallelism model: row-band tiling + pointwise op fusion |
| [0004](0004-testing-and-property-invariants.md) | Testing policy: analytic values + property invariants |
| [0005](0005-geometry-order-and-coordinates.md) | Geometry order, coordinate conventions and resampling |
| [0006](0006-caching-and-quantization.md) | Pipeline caching: prefix hashes, Q16 stages, LRU budget |
| [0007](0007-mask-representation.md) | Mask representation: normalized shapes, per-pixel lerp |
| [0008](0008-adjustment-stack-layout.md) | Adjustment stack: fixed global order + local instances |
