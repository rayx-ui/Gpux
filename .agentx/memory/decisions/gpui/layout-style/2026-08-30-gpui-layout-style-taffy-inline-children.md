---
agentx:
  kind: decision
  scope: decisions/gpui/layout-style
  status: superseded
  created: 2026-08-30T22:32:00+02:00
  updated: 2026-08-30T22:32:00+02:00
  topics:
    - gpux
    - taffy
    - allocation
    - layout
  supersedes: []
  superseded_by:
    - ../../repository/packaging/2026-08-30-repository-packaging-upstream-dependency-sources.md
---

## Decision: Vendor Taffy with two inline child slots

### Context: The warmed wide DataTable rebuilds 782 Taffy containers and their child vectors every frame. Diagnostics measured 726 containers (92.84%) at no more than two children, while maximum fanout remains 25.

### Alternatives considered: Retain crates.io Taffy unchanged; use one, four, or eight inline slots; replace Taffy; wait for an upstream representation change; vendor Taffy with two inline slots and heap overflow.

### Reasoning: Two slots are the smallest measured capacity exceeding 90% coverage and preserve arbitrary fanout. Three fresh headed WebGPU processes measured 19.92% and 17.43% higher median acknowledged throughput at DPR 1 and 2, with slightly better paced p95, exact layout/renderer counters, and zero terminal error. Taffy's full library suite and its alloc-only configuration passed.

### Trade-offs accepted: RayX owns an approximately one-megabyte vendored dependency and must replay or retire the patch during Taffy upgrades. Every child-storage entry carries inline capacity even for leaves, and the 56 containers above two children still allocate. The public `children()` method materializes a `Vec` to preserve Taffy's API.

### Supersedes: None
