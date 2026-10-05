---
agentx:
  kind: decision
  scope: decisions/gpui/positioning-scroll
  status: current
  created: 2026-07-31T19:15:00+02:00
  updated: 2026-07-31T19:15:00+02:00
  topics:
    - gpux
    - fixed-sticky-positioning
    - sticky-push-out
    - keyed-sticky-stacking
  supersedes:
    - 2026-07-31-gpui-positioning-scroll-keyed-sticky-stacking.md
  superseded_by: []
---

## Decision: Apply one push-out replacement model to every start-edge sticky relation

### Context: Keyed sticky runs already used an iOS-style handoff, but flat and hierarchical level candidates still needed the same continuous replacement across ordinary Div, VirtualList/ListView/TreeView, gpux List, and UniformList. Hierarchical replacement also had to preserve shared ancestors while the outgoing suffix left the viewport.

### Alternatives considered: Keep push-out keyed-only; make each renderer infer its own transition; abruptly remove replaced level candidates; overlay incoming rows above outgoing rows; use the canonical relation index to identify the next replacement and the retained ancestor depth.

### Reasoning: `StickyRelationIndex::next_replacement` can describe every relation transition with the incoming candidate and retained depth. A flat or root replacement retains depth zero, a same-level replacement retains lower-level ancestors, and a keyed run change replaces the complete run. Applying one overlap displacement to only the outgoing suffix preserves continuity, clipping, hit testing, and linear relation storage. Ordinary unkeyed Divs derive lanes from resolved start insets, while virtualizers keep their explicit `StickyStacking` metadata.

### Trade-offs accepted: Automatic ordinary hierarchy is expressed through distinct start insets rather than a new public level API. Only supported start edges participate in stacking; height drives vertical transitions and width drives horizontal transitions. Virtualizers remain responsible for retaining offscreen candidates, and low-level List/UniformList still shift outgoing rows' local sticky scroll context during release.

### Supersedes: [Canonical keyed sticky relation](2026-07-31-gpui-positioning-scroll-keyed-sticky-stacking.md)
