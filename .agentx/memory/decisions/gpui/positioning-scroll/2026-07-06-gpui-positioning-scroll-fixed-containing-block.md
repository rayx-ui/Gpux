---
agentx:
  kind: decision
  scope: decisions/gpui/positioning-scroll
  status: current
  created: 2026-07-06T02:25:00+02:00
  updated: 2026-07-06T02:25:00+02:00
  topics:
    - runtime
    - gpux-foundation
    - fixed-sticky-positioning
    - fixed-containing-block
  supersedes:
    - 2026-07-04-gpui-positioning-scroll-fixed-sticky-positioning.md
  superseded_by: []
---

## Decision: Resolve fixed divs against the active content-mask containing block

### Context: The Lab Fixed And Sticky story showed a fixed toolbar authored inside the "Fixed Toolbar In Clipped Parent" scroll group rendering at the viewport bottom instead of inside that clipped group. Temporary WASM runtime tracing showed the fixed toolbar's layout bounds were computed under the clipped scrollport while the previous fixed resolver always used viewport bounds and temporarily replaced ancestor masks with the viewport mask.

### Alternatives considered: Keep viewport-only fixed positioning; work around the Lab story with an app-level wrapper; add a new app-facing fixed-in-parent API; resolve only against the nearest scroll context; resolve against the current GPUI content mask as the containing block.

### Reasoning: `Window::content_mask().bounds` already represents the active prepaint/paint clipping block and defaults to the viewport when no clipped ancestor is present. Resolving fixed insets against that mask makes ordinary viewport fixed elements keep working while making fixed descendants inside overflow-clipped parents anchor to the parent scrollport. Preserving the same mask for hitboxes, paint, and children keeps visual output and pointer behavior aligned. Intentional global overlays already have explicit top-layer APIs or `Deferred::unclipped()`, so fixed positioning does not need to double as a mask-escape mechanism.

### Trade-offs accepted: This does not implement every CSS containing-block rule, such as transform-created fixed containing blocks or full stacking-context semantics. It chooses the GPUI content-mask model as the clean framework-level rule for current RayX fixed positioning.

### Supersedes: 2026-07-04-runtime-gpux-foundation-fixed-sticky-positioning.md for the viewport-only fixed containing-block and ancestor-mask escape behavior.
