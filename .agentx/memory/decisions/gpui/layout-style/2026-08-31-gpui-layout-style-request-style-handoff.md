---
agentx:
  kind: decision
  scope: decisions/gpui/layout-style
  status: current
  created: 2026-08-31T15:11:16+02:00
  updated: 2026-08-31T15:11:16+02:00
  topics:
    - gpux
    - request-layout
    - prepaint
    - performance
    - ownership
  supersedes: []
  superseded_by: []
---

## Decision: Handoff request styles only through gpux-owned element paths

### Context: Request-to-prepaint telemetry proved that 716 warmed candidates belong to gpux-owned Div, Img, Svg, and UniformList implementations, while 13 belong to external custom `Interactivity` consumers whose public callback consumes `Style` by value.

### Alternatives considered: Change the public callback to borrow style; clone the request style for every caller; retain styles for all callers through a second public API; keep recomputing all prepaint styles; add a cross-frame style cache.

### Reasoning: Private borrowed Window/Taffy adapters let the four gpux-owned element implementations finish layout without consuming the request value. The value can then move into the existing window-owned draw-local slot, move back out during prepaint, and be replaced in that same slot with the final style for paint. Structural dynamic-style gates remain authoritative, and external callers retain their existing public consuming contract. The warmed workload proves exactly 716 actual request-style reuses with all 13 external candidates unchanged, while all runtime invariants and package validation pass.

### Trade-offs accepted: The optimization deliberately leaves 13 eligible external prepaint computations in place to avoid a public API migration or a clone. Median paced p95 moved +3.24%/+1.61% at DPR 1/2 within process variance, while acknowledged throughput moved -0.13%/+0.25%; the accepted claim is exact removal of 716 repeated computations without material benchmark regression.

### Supersedes: None. This extends [frame-bounded prepaint style handoff](2026-08-31-gpui-layout-style-frame-bounded-prepaint-style-handoff.md) backward to the gpux-owned request phase without changing its draw-local lifetime.
