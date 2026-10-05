---
agentx:
  kind: decision
  scope: decisions/gpui/positioning-scroll
  status: current
  created: 2026-07-31T19:00:00+02:00
  updated: 2026-07-31T19:00:00+02:00
  topics:
    - gpux
    - fixed-sticky-positioning
    - sticky-push-out
    - paint-order
    - hit-testing
  supersedes: []
  superseded_by: []
---

## Decision: Order active sticky hierarchies through element traversal

### Context: During suffix push-out, a deeper sticky child can geometrically overlap a retained shallower parent. Ordinary Div, low-level List, and UniformList traversed active candidates parent-first, so the later child received higher positioned scene and hitbox order. ListView and TreeView already used the required inverse hierarchy order.

### Alternatives considered: Add arbitrary ranked orders to the global positioned scene and hitbox trees; introduce a sticky-only z-index API; clip child content against parent bounds; or traverse active hierarchy candidates deepest-first and preserve incoming candidates last.

### Reasoning: GPUI positioned layers already preserve correct primitive order inside each element and give later positioned elements higher scene and hitbox order. Reordering whole candidate elements at their owning traversal boundary reuses that contract: deeper active children are inserted first, retained parents are inserted afterward, and incoming replacements remain last. This solves visuals and hit testing together without changing global scene semantics or flattening a sticky element's internal background and child order.

### Trade-offs accepted: The rule is relational rather than a general `z-index` system. Ordinary Div computes the order from the active sticky plan for direct candidates, while List and UniformList apply it to their retained virtual candidates. Unrelated children keep their existing relative order.

### Supersedes: None
