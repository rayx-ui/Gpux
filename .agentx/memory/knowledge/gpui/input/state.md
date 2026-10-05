---
agentx:
  kind: state
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
    - text-input
    - keymap-snapshot
  supersedes: []
  superseded_by: []
---
# Input State

## Purpose And Usage

Read before changing action dispatch, pointer/gesture ownership, capture, drag feedback, cursor resolution, or input maps.

## Current Technical Shape

`Window::dispatch_action_at` routes an explicit action from a window-local position through the frontmost hit-tested branch with a local action handler, preserving keyboard focus and the caller's invocation source. Nonblocking hitboxes without a local action path are skipped, a hit-tested node remains the fallback for global handlers, occluding overlays stop targets beneath them, and missing targets are unhandled. `dispatch_action_from` is the focused path for keyboard activation. Cached prepaint keeps hitbox identities and remaps their dispatch-node associations into the reused subtree. A stopped-propagation request from an `ActionDispatchEvent::Began` observer is honored before global or window handlers run.

`Window` owns at most one active `PointerInteractionOwner` per `PointerId`. A handled mapped pointer action claims the sequence with its action name, target, hitbox, button, start/current positions, gesture revision, phase, and a window-local dispatch invocation id; raw move/release code may only continue that owner. Capture loss, input-map replacement, target removal, or window teardown cancels it once. Reconciliation may move capture to a newly painted descendant hitbox while keeping the owner, so terminal Up/Cancel dispatch refreshes its hitbox from the per-pointer capture map.

Drag styling snapshots callback inputs without taking or reinserting active drags. `Window::drag_hit_test` uses current-frame hitboxes during paint and rendered-frame hitboxes outside paint. `Window` exposes the pressed mouse button beside pointer position so paint-time continuations survive nested controls that consume bubble handlers. Cursor requests resolve against point hit-test order, not request insertion order.

`Window::dispatch_text_input(&str, &mut App)` routes literal text through the active platform input handler, and `Window::text_input_value(&mut App)` reads the active input text; DevKit uses them for `type`/`fill`, while `press` stays a key down/up path.

`KeymapSnapshot` and `GestureMapSnapshot` hold boxed actions and are UI-thread state retained with `Rc`; one snapshot plus map revision is captured at input-sequence start and carried to completion.

## Backlinks

- [Window action targeting](../../../../../crates/gpui/src/window/action_target.rs)
- [Window](../../../../../crates/gpui/src/window.rs)
- [Gestures](../../../../../crates/gpui/src/gestures.rs)
- [Gesture map](../../../../../crates/gpui/src/gesture_map.rs)
- [Keymap](../../../../../crates/gpui/src/keymap.rs)
- 2026-07-30 current [Per-pointer capture reconciliation](../../../decisions/gpui/input/2026-07-30-gpui-input-pointer-capture-reconciliation.md)
