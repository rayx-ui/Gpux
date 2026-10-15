---
agentx:
  kind: rules
  scope: knowledge/gpui
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-16T09:00:00+02:00
  topics:
    - gpui
    - framework-boundary
    - async-app
  supersedes: []
  superseded_by: []
---
# gpui Rules

- Modify gpux only for framework-level behavior that consumers cannot implement from public APIs; use existing public APIs first.
- Keep framework contracts policy-neutral. Do not import RayX application configuration, platform-info services, theming, or component policy into gpux.
- Foreground tasks that can resume from a re-entrant GPUI callback use `AsyncApp::try_update_entity(...)` and let their feature owner retry, stop, or surface the busy result. Do not call the panicking `update_entity` path where an active application borrow is a valid transient state.
- Gate diagnostics-only collectors, retained diagnostic state, and injected failure hooks behind the `diagnostics` feature (`rayx_diagnostics` is a compatibility alias); production builds must not carry them.
- Measure performance changes on the reference workload with full-run aggregation where cache tiers matter, and keep a change only when behavior and every established counter stay exact (see [validation rules](../repository/validation/rules.md)).
- Per-element DevKit nodes, semantics and writable properties exist only with the `devtools` feature (default; `test-support` turns it on). Crates that read `devtools_frame_snapshot` or semantic locators (`gpux_conformance`) ask for it explicitly; code that builds semantics for every frame uses `Window::set_devtools_semantics_with` so nothing is built without the feature.
- Colors an application writes as `rgb(..)` or `hsla(..)` convert to `Color` through per-thread caches; do not add a `Color` construction to a per-element path that bypasses `From<Rgba>`/`From<Hsla>`.
- A list layer holds `paint::list_overscan_viewports()` of rows on each side of those shown (0.15, shipped). Scripted layer tests that spell out rows run at 0.5 under `cfg(test)`; tests that measure what a layer holds call `run_at_shipped_list_overscan()`.
