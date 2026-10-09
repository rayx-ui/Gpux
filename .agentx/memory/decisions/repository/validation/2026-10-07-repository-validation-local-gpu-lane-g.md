---
agentx:
  kind: decision
  scope: decisions/repository/validation
  status: current
  created: 2026-10-09T17:00:00+02:00
  updated: 2026-10-09T17:00:00+02:00
  topics:
    - gpux
    - validation
    - lane-g
    - gpu
    - performance
  supersedes: []
  superseded_by: []
---

## Decision: Real-GPU and timing proof run locally in lane G

### Context: Software adapters (WARP, lavapipe, SwiftShader) make timing meaningless, and hosted GPU runners are too complex or expensive (owner, 2026-10-07).

### Alternatives considered: Hosted GPU runners; timing thresholds in the CI lanes with wide tolerances.

### Reasoning: `gpux-testkit lane g` runs only on a developer machine and refuses when `CI` is set. It runs lane B in the `local-gpu` profile with every spec, including `@timing` and `@dpr2`; lanes C and D on the hardware adapter with relative probes only, since hardware goldens are never committed; and the `gpux-platform` benchmarks (`randomized_element_tree` and the `#[gpux::bench]` suites), summarized under `artifacts-temp/testkit/g/`. Renderer, platform or performance changes run it before a release.

### Trade-offs accepted: Performance regressions surface only when a developer runs lane G; its results depend on the machine and are not committed.

### Supersedes: None.
