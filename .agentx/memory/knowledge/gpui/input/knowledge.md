---
agentx:
  kind: knowledge
  scope: knowledge/gpui/input
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-06T03:20:00+02:00
  topics:
    - touch-gestures
    - drag
    - text-input
    - devtools
    - actions
  supersedes: []
  superseded_by: []
---
# Input Knowledge

- A `TouchDragEvent` claim at `Started` removes the touch from tap, pan, and long-press recognition, so claiming it for a pan swallows taps on child controls. Observe the unclaimed offer to learn that a touch began, and take touch pans from the phased scroll steps that inner scroll containers consume first.
- `on_drop` takes the active drag before `can_drop` runs: a matching but rejected drop still ends the drag, and no ancestor target sees it.
- `TouchGestureRecognizer` treats a release sampled more than 40 ms after the last move as a stopped finger. Upstream times samples when it processes them, so a frame that blocked the main thread before the lift suppressed the fling and samples queued behind a stall had no usable velocity; gpux times them by `TouchEvent::timestamp`. Synthesized touches without a timestamp (gpux-devtools, CDP) are still timed when processed, so automation must lift right after its last move.
- Pressing a non-focusable element focuses its nearest `track_focus` ancestor (in RayX, the window `Root`) unless a descendant calls `window.prevent_default()` on the mouse down. Menu- and palette-like controls that dispatch to the focused element must prevent default, or their action starts from the ancestor and misses the editor.
- A cancelled mouse pointer reaches GPUI only as the next move without the held button: the web platform clears `pressed_button` on `pointercancel` and dispatches nothing, and later releases still dispatch `MouseUp`. Components that own a press end it on such a move.
- Dropping cached hitbox-to-dispatch-node associations on prepaint reuse makes visible cached controls lose their action scope; the owning regressions live in `window/action_target.rs`.
- Use `dispatch_text_input` for literal text when a live input has or can obtain the platform input handler; key down/up dispatch through `Keystroke::with_simulated_ime()` would turn named keys such as Enter into newline text.
- `text_input_value` reads through `PlatformInputHandler::read_text_for_range(...)`; the context-reentering `text_for_range(...)` returns `None` inside an existing update of the same window.
- WASM animation-frame requests for paint-time continuations must come from paint/render context; requesting from an event callback with no rendered view on the stack panics in `Window::current_view()`.
