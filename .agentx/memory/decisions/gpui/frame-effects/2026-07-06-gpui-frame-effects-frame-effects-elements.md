---
agentx:
  kind: decision
  scope: decisions/gpui/frame-effects
  status: current
  created: 2026-07-06T12:00:00+02:00
  updated: 2026-07-06T12:00:00+02:00
  topics:
    - runtime
    - gpux-foundation
    - shader-effects
    - elements
    - clipping
  supersedes: []
  superseded_by: []
---

## Decision: Split lightweight frame geometry from shader effects

### Context: The shader-effects branch initially exposed element shader effects through broad `Styled`/`div()` APIs. On review, `div()` already carries text, styling, interactivity, flex/grid, child collections, and other behavior, so it is too heavy and too semantically broad for effect ownership. The agreed epic is recorded in `.agentx/tasks/frame-effects-navigation-transitions/`.

### Alternatives considered: Keep shader effects on `div()`/`Styled`; add a `box_()` element; add a hidden wrapper that forwards old div effect APIs; create a lightweight container with multi-child `ParentElement` behavior; implement clipping only for effect quads.

### Reasoning: `frame()` is the clearest short name for a super-light geometry container. Keeping it single-child with `.child(...)` only avoids accidentally recreating div's flex/grid/collection surface. `effects()` can reuse the same geometry model while owning shader effects and screen effect regions explicitly. Rounded clipping must be scene/renderer-level inherited clip state because children can emit arbitrary primitives and effects.

### Trade-offs accepted: Callers must wrap content in `effects().child(...)` instead of attaching effects directly to `div()`. The migration requires removing old public styled effect APIs rather than forwarding them. Rounded radius clipping has real renderer cost and cannot be treated as a local element convenience.

### Supersedes: None
