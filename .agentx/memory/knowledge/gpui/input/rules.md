---
agentx:
  kind: rules
  scope: knowledge/gpui/input
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - actions
    - pointer-ownership
    - pointer-capture
    - drag
    - cursor
    - keymap-snapshot
  supersedes: []
  superseded_by: []
---
# Input Rules

- Only a handled typed pointer action may claim `PointerInteractionOwner`. Raw handlers may keep candidates and continue the matching owner but never initiate or commit durable work without it. Keep the dispatch invocation id window-local, separate from operation and history identities.
- Keep pointer ownership and capture keyed by `PointerId`. After owner/hitbox reconciliation, refresh the captured hitbox from the per-pointer capture map before terminal Up or Cancel; never deliver through a removed pre-reconciliation hitbox.
- Drag-style callbacks must not transfer active drags out of their pointer sequences; snapshot only callback inputs and use the hitboxes of the frame being painted for drag feedback.
- Resolve overlapping cursor requests by the point-local hit-test order used for interaction.
- Preserve cancellation between `ActionDispatchEvent::Began` observers and handler dispatch; check the observer veto before resetting propagation.
- For drags that continue across nested controls, poll `Window::mouse_position()` and `Window::pressed_mouse_button()` during paint.
- Schedule browser animation-frame continuation from paint/render context; event callbacks update state and call `Window::refresh()`.
- Retain `KeymapSnapshot` and `GestureMapSnapshot` with `Rc` and keep them UI-thread state; do not switch them to `Arc` unless every contained action becomes `Send + Sync`.
