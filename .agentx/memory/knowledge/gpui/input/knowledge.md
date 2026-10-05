---
agentx:
  kind: knowledge
  scope: knowledge/gpui/input
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - pointer-ownership
    - text-input
    - devkit
    - actions
  supersedes: []
  superseded_by: []
---
# Input Knowledge

- `PointerInteractionOwner::action_invocation_id()` identifies one window dispatch for window-local diagnostics only; consumers' operation transactions and history receipts own their identities independently.
- Dropping cached hitbox-to-dispatch-node associations on prepaint reuse makes visible cached controls lose their action scope; the owning regressions live in `window/action_target.rs`.
- Use `dispatch_text_input` for literal text when a live input has or can obtain the platform input handler; key down/up dispatch through `Keystroke::with_simulated_ime()` would turn named keys such as Enter into newline text.
- `text_input_value` reads through `PlatformInputHandler::read_text_for_range(...)`; the context-reentering `text_for_range(...)` returns `None` inside an existing update of the same window.
- WASM animation-frame requests for paint-time continuations must come from paint/render context; requesting from an event callback with no rendered view on the stack panics in `Window::current_view()`.
