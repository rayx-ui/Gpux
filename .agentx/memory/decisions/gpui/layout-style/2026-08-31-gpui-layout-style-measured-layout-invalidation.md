---
agentx:
  kind: decision
  scope: decisions/gpui/layout-style
  status: current
  created: 2026-08-31T06:32:39+02:00
  updated: 2026-08-31T06:32:39+02:00
  topics:
    - gpux
    - taffy
    - layout
    - measurement
    - performance
  supersedes: []
  superseded_by: []
---

## Decision: Require explicit invalidation before reusing measured-layout output

### Context: Diagnostics show that every one of the warmed wide DataTable's 882 measured-leaf calls at DPR 1 and DPR 2 repeats the preceding frame's numeric inputs and exact snapped output. Taffy's normal dirty propagation still invokes measurement closures because gpux replaces those closures each frame.

### Alternatives considered: Immediately reuse output whenever known dimensions and available space match; keep measuring indefinitely; expose an opt-in caller-owned generation or key and reuse only when it and the Taffy inputs match.

### Reasoning: Numeric Taffy inputs do not encode the closure's captured content, font/text-system state, or other application dependencies. Input-only reuse would therefore turn a benchmark observation into an unsound global contract. An explicit owner-supplied generation can make freshness observable while preserving exact Taffy-input matching as the layout-side guard.

### Trade-offs accepted: Gpux continues paying redundant measurement work until a separately designed and validated opt-in contract exists. The diagnostic evidence remains useful for quantifying the opportunity, but it does not authorize an implicit cache.

### Supersedes: None.
