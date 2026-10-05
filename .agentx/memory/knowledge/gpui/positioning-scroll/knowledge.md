---
agentx:
  kind: knowledge
  scope: knowledge/gpui/positioning-scroll
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - fixed-positioning
    - sticky-positioning
    - scrollbar
    - wheel-routing
    - validation
  supersedes: []
  superseded_by: []
---
# Positioning And Scroll Knowledge

- Positioned scopes must allocate high draw order per inserted primitive; flattening all descendants to one order lets parent backgrounds cover child text in WebGPU output.
- Cached prepaint reuse depends on hitbox index ranges, which is why positioned hitboxes get high ids instead of a global sort of the stored vector.
- Sticky state callbacks compare only x/y phases; bounds deltas inside one phase fire nothing. `when_stuck(...)` resolves after layout from the same sticky state used for visual bounds, so it suits paint-only styling (shadow, color, opacity), not layout-affecting changes.
- WASM diagnostics observing a prepaint-time sticky callback must publish from the callback or a later confirmed render; render-start diagnostics report the previous frame's state.
- Nested sticky proof should expect a later sticky header to replace an earlier one at the same inset while the outer offset is unchanged.
- GPUI scroll listeners let every scrollable hitbox under the pointer observe wheel input; isolate the target scrollport in WASM tests whose assertions depend on global pixel position.
- Wheel consumption computes the scroll max from content size, scrollport bounds, padding, and rem size, then clamps before deciding whether the scrollport consumed the event.
- A scrollbar authored as a child of a scrollable element moves with the scrolled content; drag geometry taken from its own layout bounds makes a small thumb drag jump to the end. Use `ScrollbarHandle::viewport_bounds()`.
- Mobile touch enters GPUI as scroll deltas from `gpui_mobile` (Android coalesces moves to one event per frame; both platforms attach `release_velocity` on touch end), and `ScrollActivity` turns release velocity into momentum on the same owner.
