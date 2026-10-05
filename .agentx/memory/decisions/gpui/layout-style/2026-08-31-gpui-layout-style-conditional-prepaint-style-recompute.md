---
agentx:
  kind: decision
  scope: decisions/gpui/layout-style
  status: current
  created: 2026-08-31T11:17:34+02:00
  updated: 2026-08-31T11:17:34+02:00
  topics:
    - gpux
    - prepaint
    - sticky-positioning
    - performance
  supersedes: []
  superseded_by: []
---

## Decision: Reuse pre-resolution styles unless sticky publication can change them

### Context: `Interactivity::prepaint` computed a complete style before resolving positioned bounds, published the resulting sticky state, and then unconditionally computed the same style again. Ordinary nodes have no post-resolution refinement, while sticky-state listeners and applicable `when_stuck(...)` styles are the only current paths that can make the second result differ.

### Alternatives considered: Keep unconditional recomputation; directly refine the first style for every stuck element; cache computed styles across lifecycle phases or frames.

### Reasoning: Retaining the first style and recomputing only for an applicable stuck refinement or a sticky-state listener removes one full default-and-refine pass from ordinary prepaint without adding cache ownership, invalidation, or cross-phase state. Conservative listener recomputation preserves synchronous callback effects. Focused sticky tests, all gpux and WGPU tests, native checks, and six headed WebGPU cases preserved behavior. Three-process medians were neutral-to-positive overall, and fresh profiles reduced `compute_style_internal` self time by 29.4% at DPR 1 and 10.6% at DPR 2.

### Trade-offs accepted: Elements with a sticky-state listener conservatively retain the second computation even when the phase does not change; this avoids needing callback-effect tracking. DPR 1 acknowledged throughput moved -0.61% within process variance while paced p95 improved 0.95%; DPR 2 throughput and p95 improved 1.51% and 0.62%.

### Supersedes: None.
