---
agentx:
  kind: decision
  scope: decisions/gpui/positioning-scroll
  status: superseded
  created: 2026-07-31T03:30:00+02:00
  updated: 2026-07-31T19:15:00+02:00
  topics:
    - gpux
    - fixed-sticky-positioning
    - keyed-sticky-stacking
  supersedes: []
  superseded_by:
    - 2026-07-31-gpui-positioning-scroll-universal-sticky-push-out.md
---

## Decision: Use one canonical relation for ordinary and virtualized sticky stacking

### Context: Ordinary sticky siblings, RayX virtual rows, and gpux List/UniformList candidates needed the same keyed stacking behavior without overlays or per-surface key-only APIs. Correct ordinary geometry also requires every candidate in a scroll context to be known before any keyed candidate commits visual bounds.

### Alternatives considered: Keep ordinary sticky siblings independent; add a RayX-only keyed policy; add separate key vectors to each virtualizer; infer grouping from item or element identity; add a global sticky manager; use one gpux relation carried through each surface's existing candidate API.

### Reasoning: `StickyStacking::{Level, StackId}` gives explicit hierarchy and contiguous keyed runs one shared model. `Window` can plan complete ordinary candidate sequences where scroll contexts and positioned bounds are owned, while virtualizers can retain the same relation for offscreen rows. Looking up the next contiguous run also lets every surface apply one overlap displacement to the complete outgoing run, retaining its rows for an iOS-style push-out instead of painting the new run over them. A predecessor index stores one record per candidate and reconstructs only the active depth, meeting the linear-memory and bounded-lookup contract.

### Trade-offs accepted: Authors must provide stack ids separately from item identity. Keyed behavior is limited to supported sticky start edges, virtualizers still own offscreen candidate discovery, and low-level List/UniformList must shift the outgoing rows' local sticky scroll context while they leave the viewport. Diagnostics remain feature-gated and Test Suite-only.

### Supersedes: None
