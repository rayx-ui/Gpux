---
agentx:
  kind: decision
  scope: decisions/gpui/layout-style
  status: current
  created: 2026-08-30T23:18:00+02:00
  updated: 2026-08-30T23:18:00+02:00
  topics:
    - gpux
    - taffy
    - layout
    - performance
    - dependencies
  supersedes: []
  superseded_by: []
---

## Decision: Use registry Taffy 0.14.0 for gpux layout

### Context: Returning Taffy 0.12.2 to crates.io eliminated 23,970 tracked vendor lines but also removed a measured two-slot child-storage patch. Taffy 0.14.0 is the current published release and includes more correct caching, a second-chance measure cache, skipped measurement for absolutely positioned children with known dimensions, and fewer unnecessary flexbox min-content measurements.

### Alternatives considered: Keep registry 0.12.2; recreate the inline patch in a maintained exact-revision fork; restore full vendoring; upgrade to registry 0.14.0 with the minimum required gpux API adaptation.

### Reasoning: Three fresh headed Test Suite WebGPU processes measured 20.34% and 21.24% higher median acknowledged throughput at DPR 1 and 2 versus registry 0.12.2. Paced p95 improved 0.43% and 4.60%. The result slightly beat the removed inline patch on all four medians, while all six browser runs preserved exact layout, renderer, traversal, final-scroll, and terminal-error counters. All 400 gpux tests and native/WASM static gates passed.

### Trade-offs accepted: gpux now adapts Taffy 0.14's `LayoutInput`/`LayoutOutput` measure API through `compute_leaf_layout`, and classifies the new `FlowRoot` display with block containers in diagnostics. The exact dependency pin deliberately delays later Taffy releases until they receive the same compatibility and performance proof.

### Supersedes: None.
