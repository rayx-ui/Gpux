---
agentx:
  kind: rules
  scope: knowledge/gpui
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-07T16:45:00+02:00
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
