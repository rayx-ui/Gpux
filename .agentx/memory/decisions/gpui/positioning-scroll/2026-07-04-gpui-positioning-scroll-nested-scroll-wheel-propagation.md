---
agentx:
  kind: decision
  scope: decisions/gpui/positioning-scroll
  status: current
  created: 2026-07-04T21:42:53+02:00
  updated: 2026-07-04T21:42:53+02:00
  topics:
    - gpux
    - nested-scroll
    - wheel-routing
    - fixed-sticky-positioning
  supersedes: []
  superseded_by: []
---

## Decision: Clamp and consume nested scroll wheel input in the active scrollport

### Context: Nested sticky headers in the Lab fixed/sticky story passed through because a wheel event inside the inner scrollport could also move the parent scrollport in the same dispatch path, changing the sticky containing context before the inner sticky state was visually stable.

### Alternatives considered: Make the Lab nested panel use a hidden wrapper or proxy scroll surface; special-case sticky descendants; stop all wheel propagation from any scrollable hitbox even when it is already at an edge; clamp scroll offsets only during the next layout pass; clamp and consume only when the current scrollport actually moves.

### Reasoning: GPUI scroll ownership should be determined by the scrollport under the pointer. Computing the scroll max during listener registration and clamping the candidate offset before notifying lets the current scrollport consume wheel input only while it can move. This keeps nested sticky descendants anchored to the inner scroll context without blocking expected parent scrolling once the inner scrollport reaches an edge.

### Trade-offs accepted: This keeps GPUI's existing wheel dispatch model but adds edge-aware propagation semantics. If a platform later needs true residual-delta handoff, that should be added deliberately instead of reintroducing unconditional propagation for consumed inner scroll input.

### Supersedes: None
