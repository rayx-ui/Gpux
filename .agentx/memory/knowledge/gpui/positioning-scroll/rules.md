---
agentx:
  kind: rules
  scope: knowledge/gpui/positioning-scroll
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - fixed-positioning
    - sticky-positioning
    - sticky-stacking
    - scroll-activity
    - scrollbar
    - wheel-routing
  supersedes: []
  superseded_by: []
---
# Positioning And Scroll Rules

- Implement fixed/sticky behavior in `Style`/`Window`/element lifecycle when it affects layout, visual bounds, masks, and hitboxes; never simulate it with app-level overlay wrappers or hidden proxy elements.
- Resolve `div().fixed()` insets against `Window::content_mask().bounds` and keep that mask for fixed hitboxes, paint, and children. Use top-layer APIs or `Deferred::unclipped()` for intentional global overlays.
- Keep the shared positioned scene layer and per-primitive draw order inside positioned scopes; keep positioned hitbox ids with point-local hit-test ordering and never sort the stored hitbox vector globally.
- Keep scrollbar paint and hitboxes in the top layer, anchor drag geometry to the scroll handle's viewport bounds, and give sibling scrollbar layers caller-unique keyed element ids.
- Use `when_stuck(...)` for sticky visual effects; use `on_sticky_state_changed(...)` only when state or diagnostics need phase transitions.
- Use `StickyStacking::StackId` only for contiguous keyed runs and `StickyStacking::Level` for explicit hierarchy. Discover all start-edge candidates in the nearest scroll context before committing bounds; derive unkeyed lanes from resolved start insets.
- When a replacement reaches the outgoing suffix's combined primary-axis extent, displace only that suffix by the same overlap; root and keyed-run replacements displace the whole chain. Never cover or abruptly drop outgoing items.
- Traverse active sticky hierarchies deepest-first in prepaint and paint, then append incoming replacements.
- Keep relation storage linear and active lookup bounded to candidate search plus active depth; do not copy a stack per candidate or rescan prior candidates per frame.
- Stop wheel propagation only when the current scrollport's clamped offset changes; never swallow wheel input at a scroll edge.
- Put capture-phase wheel policy on the interactive hit-test surface that owns the event path; an ancestor behind an opaque `BlockMouse` child cannot arbitrate its wheel events.
- Keep programmatic scrolling policy-neutral: GPUI owns `ScrollBehavior`, geometry, and the single `ScrollActivity` driver; consumers resolve motion policy before submission. Retarget through the existing generation; never add a second animation or scroll state machine.
- A direct offset mutation or wheel, touch, thumb, selection, caret, or IME correction cancels active programmatic scrolling immediately.
- Use `ScrollHandle::ptr_eq` to tell idempotent reattachment from a real owner change; never infer identity from offsets, bounds, or copied metrics.
- Bracket real thumb gestures with `begin_scrollbar_drag`/`end_scrollbar_drag`; intermediate offsets must not settle the owner before release.
- Publish concrete thumb offsets in the pointer-input turn and invalidate the owner and unique dependents once; do not reintroduce a queued preview followed by a deferred acknowledgment frame.
