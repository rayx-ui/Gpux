---
agentx:
  kind: rules
  scope: knowledge/repository/validation
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-07T16:45:00+02:00
  topics:
    - validation
    - lanes
    - testkit
    - runtime-proof
  supersedes: []
  superseded_by: []
---
# Validation Rules

- Test only what changed: the touched package's focused tests plus the lane scenes and specs that drive the touched behavior. Run a whole suite only when the change affects the whole codebase, and say why first.
- Prove runtime behavior in gpux's own lanes through `gpux-testkit` ([docs/testing.md](../../../../../docs/testing.md)). Never route gpux proof through RayX: RayX checks prove the consumer after an API change, not gpux.
- Give new behavior coverage in gpux: a conformance scene that names its lanes, a Playwright spec in `tools-node/specs/`, or a focused test beside the crate.
- Prove WGSL, packed-record, and backend-selection changes in lane B on WebGPU and, for transport changes, forced WebGL2, plus lane C; compile checks do not exercise Dawn validation.
- A lane never degrades silently: a missing prerequisite or capability is reported skipped with its reason, and runs that must cover a backend assert it (`GPUX_EXPECT_BACKEND`).
- Keep timing thresholds in lane G only; software profiles and hosted runners skip timing assertions with the reason.
- Never commit goldens from a hardware GPU; regenerate a software backend's goldens only for an intended rendering change and review every diff.
- Do not close iOS or Metal runtime work from Windows checks; it needs a Mac or the macOS CI lanes.
- Keep raw logs, screenshots, traces, and profiles in ignored `artifacts-temp/`, never in `.agentx/` or commits.
