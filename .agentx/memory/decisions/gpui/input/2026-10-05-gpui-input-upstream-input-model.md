---
agentx:
  kind: decision
  scope: decisions/gpui/input
  status: current
  created: 2026-10-05T23:30:00+02:00
  updated: 2026-10-05T23:30:00+02:00
  topics:
    - gpux
    - input-model
    - touch
    - drag
    - upstream-convergence
  supersedes:
    - 2026-07-30-gpui-input-pointer-capture-reconciliation.md
    - ../positioning-scroll/2026-07-29-gpui-positioning-scroll-shared-scroll-activity.md
  superseded_by: []
---

# Upstream Input Model

## Decision: Use Zed GPUI's input model unchanged and delete the gpux pointer layer

### Context: Zed's GPUI added raw `Touch` input, a portable `TouchGestureRecognizer`, and a single active drag with platform drag-out handoff. gpux carried a parallel model (`PointerEvent` with per-`PointerId` ownership and capture, `GestureMap` rules with a gesture arena, touch momentum in `ScrollActivity`, rotate events, multi-drag extras) that made every closure import a large reconciliation.

### Alternatives considered: Keep the gpux pointer layer and re-port Zed's touch work onto it; adopt Zed's model but keep `GestureMap` and pointer ownership as gpux additions; adopt Zed's model unchanged.

### Reasoning: Upstream now covers taps, pans with fling, long press, touch drag, and pinch through ordinary mouse, scroll, and gesture events, so the gpux layer duplicated it. Taking Zed's model unchanged removes the largest import conflict surface. gpux keeps only additions that sit beside the upstream model: `HitboxBehavior::Ignore`, `with_pointer_semantics_suppressed`, `Window::pressed_mouse_button`, `capture_scroll_wheel`, layered keymaps, action dispatch outcomes, and `dispatch_action_at`. RayX re-hosts rebindable pointer bindings, gesture detection, and typed drag-and-drop on these events.

### Trade-offs accepted: Rotate events, per-pointer simultaneous drags, programmatic drag start, source-side drag-end callbacks, mouse long press, and device-kind facts on pointer events are gone; consumers implement what they need on upstream events. On touch, drags start only through `TouchDragEvent` claims, so `on_drag` drag-and-drop is mouse-only until upstream adds touch drag-and-drop.

### Supersedes: [Per-pointer capture reconciliation](2026-07-30-gpui-input-pointer-capture-reconciliation.md) (2026-07-30, superseded) and [AD-041 shared ScrollActivity](../positioning-scroll/2026-07-29-gpui-positioning-scroll-shared-scroll-activity.md) (2026-07-29, superseded): touch pans and fling momentum now reach scroll owners as scroll events from Zed's recognizer, so `ScrollActivity` tracks only direct scrollbar drags and programmatic scrolling.
