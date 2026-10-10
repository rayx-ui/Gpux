---
agentx:
  kind: rules
  scope: knowledge/gpui/positioning-scroll
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-10T19:30:00+02:00
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
- Give a fixed or sticky element's whole subtree positioned hitbox ids (`Window::with_positioned_hitboxes`), not only its own hitbox: later normal content otherwise sorts above a stuck subtree's controls and takes their clicks.
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
- Move a tracked scroll offset only through `ScrollHandle::publish_offset_inner`, and a handle-less div's through its wheel listener with `fast::layers::invalidate::note_scrolled`: scroll layers tell a scroll from a change of what the owner renders by the publishes they note, and a notification sent for a scroll on any other path paints the layer again.
- Give every `ScrollHandle` getter that reads the offset, metrics or item positions `note_offset_read`, and every getter of tracked bounds `fast::layers::input::moved`: a retained owner must be built again when what it read moves, and bounds read on a composited frame must be where the content shows.
- Read a scroll offset or its metrics in `render` only when the view's own elements depend on them; draw what merely follows the scroll (a scrollbar, an indicator) from prepaint or paint, or the owner's scroll layer is painted again on every scrolled frame.
- Publish a scroll without updating the view that owns the container: from a scrollbar entity of its own or a listener that leases nothing. gpux counts an entity updated without a notification as changed, so a scroll published from a `cx.listener` of the owner repaints its scroll layer on every step.
