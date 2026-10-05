---
agentx:
  kind: state
  scope: knowledge/gpui
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
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

A frame runs request-layout, prepaint, and paint under `Window`; the [frame-pipeline](frame-pipeline/state.md) and [layout-style](layout-style/state.md) subdomains own that lifecycle. Diagnostics-only telemetry is compiled behind the `rayx_diagnostics` feature (forwarded by `gpui_wgpu` and `gpui_web`) and is absent from production builds.

`gpux::Application::run(...)` takes an async finish-launching callback with a `gpux::AsyncApp`. Startup code takes short mutable `App` access through `AsyncApp::update(...)` and never holds a borrowed `App` across an await. `AsyncApp::try_update_entity(...)` is the non-panicking entity update for foreground work that can resume while another GPUI callback owns the application `RefCell`; it returns an error when the app is unavailable or busy so the owning feature decides whether to retry.

## Backlinks

- [gpux crate manifest](../../../../crates/gpui/Cargo.toml)
- [Async app context](../../../../crates/gpui/src/app/async_context.rs)
- [gpux skill](../../../../.claude/skills/gpux/SKILL.md)
- 2026-10-05 current [RayX gpux integration memory](../../../../../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/state.md)
