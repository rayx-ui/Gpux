---
agentx:
  kind: decision
  scope: decisions/gpui/positioning-scroll
  status: superseded
  created: 2026-07-29T01:30:00+02:00
  updated: 2026-10-05T23:30:00+02:00
  topics:
    - AD-041
    - gpux
    - scroll-activity
    - programmatic-scroll
  supersedes: []
  superseded_by:
    - ../input/2026-10-05-gpui-input-upstream-input-model.md
---

## Decision: AD-041 share ScrollActivity between touch and programmatic scrolling

### Context: why this came up

The unified pointer epic now owns GPUI touch-release activity. The older motion epic still planned an independent `ScrollAnimationState` plus another pass through Div, UniformList, ListState, and RayX owners.

### Alternatives considered: what else was on the table

Keep a separate programmatic animation state machine, or let each scroll owner implement its own smooth-scroll lifecycle.

### Reasoning: why this option won

One policy-neutral `ScrollActivity` has the correct frame ownership, bounds, cancellation, and retargeting boundary for both drivers. A Programmatic driver can supply explicit smooth behavior while direct touch remains immediate and touch release remains the touch driver.

### Trade-offs accepted: what you gave up

The motion epic must depend on the shared activity and cannot independently alter owners already migrated by the pointer epic. Its remaining work is limited to explicit programmatic policy resolution and focused validation.

### Supersedes: link to prior decision, if replacing

None.
