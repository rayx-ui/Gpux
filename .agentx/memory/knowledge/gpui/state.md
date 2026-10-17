---
agentx:
  kind: state
  scope: knowledge/gpui
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-17T04:00:00+02:00
  topics:
    - gpui
    - framework-boundary
    - application-lifecycle
    - async-app
  supersedes: []
  superseded_by: []
---
# gpui State

## Purpose And Usage

Read this before changing `crates/gpui` or deciding whether a behavior belongs in gpux at all. Route to the narrowest subdomain for the touched area; this file holds only cross-subdomain facts.

## Current Technical Shape

gpux is Zed's GPUI closure plus RayX-local framework patches. The core crate publishes as `gpux` and is imported as `gpux::...`; platform, renderer, and support crates publish as `gpux-*` and keep their GPUI Rust names (see the [README crate table](../../../../README.md#crates)).

RayX is the main consumer. Application services, theming, components, virtualization, and DevKit live in RayX crates; gpux holds only framework-level behavior that RayX cannot build from public GPUI APIs. Framework contracts stay policy-neutral: gpux has no dependency on RayX application configuration or `PlatformInfoService`, and RayX resolves policy (motion, scrolling, theming) before calling GPUI.

A frame runs request-layout, prepaint, and paint under `Window`; the [frame-pipeline](frame-pipeline/state.md) and [layout-style](layout-style/state.md) subdomains own that lifecycle. Diagnostics-only telemetry is compiled behind the `diagnostics` feature (forwarded by `gpui_wgpu` and `gpui_web`; `rayx_diagnostics` remains an alias that enables it) and is absent from production builds.

Motion and timing state machines (gestures, programmatic scroll, `with_animation`, frame-family animations and snapshots, image loading timers, the window frame throttle) read time only from `BackgroundExecutor::now()`, which is real time in production and the `TestClock` under `TestDispatcher`. With `test-support`, tests shape real text with `#[gpux::test(text = "bundled")]` (only the `gpux_fonts` families), step frames with `simulate_frame`/`advance_frame` (inactive test windows throttle frames to about 30 per second), simulate IME, key-up, click counts and held-button drags, open windows with `TestWindowOptions`, read recorded platform and window requests instead of panics, take the AccessKit tree updates a test window sent (`take_accessibility_updates`), and find elements with `gpux::semantic_query` queries and `VisualTestContext::locator`. gpux's own conformance scenes live in [`gpux-tests-conformance`](../../../../crates/gpux_tests_conformance) (lanes under [validation](../repository/validation/state.md)).

`gpux::Application::run(...)` takes an async finish-launching callback with a `gpux::AsyncApp`. Startup code takes short mutable `App` access through `AsyncApp::update(...)` and never holds a borrowed `App` across an await. `AsyncApp::try_update_entity(...)` is the non-panicking entity update for foreground work that can resume while another GPUI callback owns the application `RefCell`; it returns an error when the app is unavailable or busy so the owning feature decides whether to retry.

Siblings with the same element id share a global element id and their element state (upstream GPUI tolerates them). The accessibility tree gives each one a node of its own (2026-10-17, gpux-a11y-devtools): the first keeps the id `GlobalElementId::accesskit_node_id` derives, later ones take the first free id hashed from it and a count (`A11yNodeBuilder::element_node_id`), stable while the duplicates keep their draw order; debug builds log each duplicate path once. `Window::a11y_node_id` gives the node the element being drawn holds, and focus, active descendant and action handlers register on it (`crates/gpui/tests/a11y_duplicate_ids.rs`). Fast mode builds the view holding such duplicates every frame, with or without the tree.

## Backlinks

- [gpux crate manifest](../../../../crates/gpui/Cargo.toml)
- [Async app context](../../../../crates/gpui/src/app/async_context.rs)
- [Test contexts](../../../../crates/gpui/src/app/test_context.rs)
- [Semantic queries](../../../../crates/gpui/src/semantic_query.rs)
- [gpux skill](../../../../.claude/skills/gpux/SKILL.md)
- 2026-10-05 current [RayX gpux integration memory](../../../../../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/state.md)
