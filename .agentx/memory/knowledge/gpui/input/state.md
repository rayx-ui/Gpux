---
agentx:
  kind: state
  scope: knowledge/gpui/input
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-06T02:00:00+02:00
  topics:
    - actions
    - touch-gestures
    - pointer-capture
    - drag
    - cursor
    - text-input
    - keymap-snapshot
  supersedes: []
  superseded_by: []
---
# Input State

## Purpose And Usage

Read before changing action dispatch, touch gestures, pointer capture, drag and drop, cursor resolution, or input maps.

## Current Technical Shape

`Window::dispatch_action_at` routes an explicit action from a window-local position through the frontmost hit-tested branch with a local action handler, preserving keyboard focus and the caller's invocation source. Nonblocking hitboxes without a local action path are skipped, a hit-tested node remains the fallback for global handlers, occluding overlays stop targets beneath them, and missing targets are unhandled. `dispatch_action_from` is the focused path for keyboard activation. Cached prepaint keeps hitbox identities and remaps their dispatch-node associations into the reused subtree. A stopped-propagation request from an `ActionDispatchEvent::Began` observer is honored before global or window handlers run. Called from a mouse or key listener, `dispatch_action_at` restores that event's propagation afterward, because action dispatch reuses the event propagation flag; otherwise a pointer binding recognized during a release would stop the release before the clicked control's own listeners. `FocusHandle::dispatch_action_with_source` dispatches from the node that rendered a focus handle with an explicit invocation source and the same propagation guarantee; a component that recognized a binding for its own key context, such as a nested selection area, uses it so position-based routing cannot reach an ancestor.

gpux uses Zed GPUI's input model unchanged. Platforms emit `MouseDown`/`MouseUp`/`MouseMove`, `ScrollWheel`, raw `Touch`, and `Pinch` input. The window's `TouchGestureRecognizer`, tuned by `Platform::gestures`, turns touches into synthesized mouse press/release taps (`ClickEvent::Touch` for click listeners), pan and fling steps delivered as `ScrollWheelEvent`s at the pan's start position, phased `LongPressEvent`s, and `TouchDragEvent`s. A touch that starts while no other touch is recognized offers a `TouchDragEvent` `Started`; an element that claims it with `prevent_default` turns the whole touch into a drag. `Window::capture_pointer` routes moves and the release to one hitbox until mouse up.

One active drag (`App::active_drag`) starts from `on_drag` after a mouse-move threshold, is drawn by `Window` at the pointer, and is consumed by the first hovered `on_drop` of its type, with `can_drop` gating acceptance; a release without a drop clears it, and platform drag-out handoff suspends it. `Window` exposes the pressed mouse button beside pointer position so paint-time continuations survive nested controls that consume bubble handlers. Cursor requests resolve against point hit-test order, not request insertion order.

`Window::dispatch_text_input(&str, &mut App)` routes literal text through the active platform input handler, and `Window::text_input_value(&mut App)` reads the active input text; DevKit uses them for `type`/`fill`, while `press` stays a key down/up path.

`KeymapSnapshot` holds boxed actions and is UI-thread state retained with `Rc`; one snapshot plus map revision is captured at input-sequence start and carried to completion.

## Backlinks

- [Window action targeting](../../../../../crates/gpui/src/window/action_target.rs)
- [Window](../../../../../crates/gpui/src/window.rs)
- [Gestures](../../../../../crates/gpui/src/gestures.rs)
- [Interactive events](../../../../../crates/gpui/src/interactive.rs)
- [Keymap](../../../../../crates/gpui/src/keymap.rs)
- 2026-10-05 current [Upstream input model](../../../decisions/gpui/input/2026-10-05-gpui-input-upstream-input-model.md)
- 2026-07-30 superseded [Per-pointer capture reconciliation](../../../decisions/gpui/input/2026-07-30-gpui-input-pointer-capture-reconciliation.md)
