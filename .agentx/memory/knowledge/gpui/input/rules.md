---
agentx:
  kind: rules
  scope: knowledge/gpui/input
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:30:00+02:00
  topics:
    - actions
    - touch-gestures
    - pointer-capture
    - drag
    - cursor
    - keymap-snapshot
  supersedes: []
  superseded_by: []
---
# Input Rules

- Take Zed's input model unchanged; add gpux input features beside it, never as a parallel pointer layer ([upstream input model](../../../decisions/gpui/input/2026-10-05-gpui-input-upstream-input-model.md), 2026-10-05, current).
- Deliver platform touches as raw `PlatformInput::Touch` and let `TouchGestureRecognizer` own taps, pans, fling momentum, long presses, and touch drags.
- Resolve overlapping cursor requests by the point-local hit-test order used for interaction.
- Preserve cancellation between `ActionDispatchEvent::Began` observers and handler dispatch; check the observer veto before resetting propagation.
- For drags that continue across nested controls, poll `Window::mouse_position()` and `Window::pressed_mouse_button()` during paint.
- Schedule browser animation-frame continuation from paint/render context; event callbacks update state and call `Window::refresh()`.
- Retain `KeymapSnapshot` with `Rc` and keep it UI-thread state; do not switch it to `Arc` unless every contained action becomes `Send + Sync`.
