---
agentx:
  kind: decision
  scope: decisions/gpui/positioning-scroll
  status: current
  created: 2026-07-04T06:38:30+02:00
  updated: 2026-07-06T02:25:00+02:00
  topics:
    - runtime
    - gpux-foundation
    - fixed-sticky-positioning
  supersedes: []
  superseded_by:
    - 2026-07-06-gpui-positioning-scroll-fixed-containing-block.md
---

## Decision: Implement fixed and sticky positioning in GPUI element lifecycle

### Context: The gpux-fixed-sticky-positioning epic needed CSS-like `div().fixed()` and `div().sticky()` behavior across ordinary GPUI trees, scroll containers, hitboxes, and renderer content masks.

### Alternatives considered: Implement the behavior as RayX-only wrappers; create overlay/proxy elements for fixed and sticky nodes; keep it in Lab/component code only; add framework-level positioned-bounds resolution in GPUI.

### Reasoning: Fixed and sticky positioning affect layout mapping, prepaint bounds, paint bounds, child offsets, content masks, hitboxes, and nested scroll context lookup. Those concerns are already owned by GPUI `Style`, `Window`, and `Div`, so a framework-level implementation gives ordinary `div()` trees and rendered collection rows the same behavior without hidden wrapper APIs.

### Trade-offs accepted: The first implementation treated the window viewport as the fixed containing block and did not implement every obscure CSS containing-block rule such as transformed ancestors. The viewport-only fixed containing-block trade-off is superseded by the 2026-07-06 fixed containing block decision; sticky elements still require a live scroll context and virtualized offscreen rows still need collection cooperation.

### Supersedes: None
