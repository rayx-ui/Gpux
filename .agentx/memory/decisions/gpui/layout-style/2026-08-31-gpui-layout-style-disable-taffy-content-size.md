---
agentx:
  kind: decision
  scope: decisions/gpui/layout-style
  status: current
  created: 2026-08-31T06:13:59+02:00
  updated: 2026-08-31T06:13:59+02:00
  topics:
    - gpux
    - taffy
    - layout
    - scrolling
    - performance
    - dependencies
  supersedes:
    - 2026-08-31-gpui-layout-style-minimum-taffy-features.md
  superseded_by: []
---

## Decision: Let gpux own scroll extents without Taffy content-size tracking

### Context: Taffy's `content_size` feature computes, propagates, stores, and rounds each node's scrollable-overflow rectangle. Gpux never reads that feature-gated field; `Div::prepaint` computes the visible scroll content extent from the actual child layout bounds it already needs.

### Alternatives considered: Keep Taffy's duplicate overflow tracking for potential future callers; expose and adopt Taffy's overflow rectangle throughout GPUI; disable `content_size` while retaining Taffy's flex, grid, block, tree, and standard-library capabilities.

### Reasoning: Removing only `content_size` passed all 411 gpux unit tests, every gpux integration suite including scroll-content mutation coverage, all 52 serialized WGPU tests, and six headed Test Suite WebGPU cases. Every case retained the exact -351402.6875 final offset and established Taffy, renderer, identity, and batching counters. Relative to the accepted six-feature build, three-process median acknowledged throughput improved 0.15%/0.38% at DPR 1/2, paced p95 improved 1.21%/1.58%, and the repeated WASM atomic prefix shrank by 256 bytes.

### Trade-offs accepted: Gpux cannot read Taffy's scrollable-overflow rectangle unless it deliberately re-enables `content_size`. Any future consumer must first reconcile that rectangle's CSS overflow semantics with GPUI's child-bounds, transforms, clipping, and scrolling ownership rather than silently restoring duplicate work.

### Supersedes: [Explicit minimum Taffy feature set](2026-08-31-gpui-layout-style-minimum-taffy-features.md)
