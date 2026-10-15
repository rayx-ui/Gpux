---
agentx:
  kind: rules
  scope: knowledge/repository/validation
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-15T09:00:00+02:00
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
- A scene step that needs a delay held back (state before a timer fires) goes under `Step::When(Capability::ExecutorClock, …)`: only the test platform's executor clock can hold it; real-clock hosts report it skipped. Take the reference capture before the interaction that starts the delay.
- A check must be able to fail: assert a known pixel or state the change produces (`evaluate::screen_shows` on whole-screen screenshots), never "anything drew"; prove a regression test by running it against the old code.
- Never commit goldens from a hardware GPU; regenerate a software backend's goldens only for an intended rendering change and review every diff.
- Do not close iOS or Metal runtime work from Windows checks; it needs a Mac or the macOS CI lanes.
- Keep raw logs, screenshots, traces, and profiles in ignored `artifacts-temp/`, never in `.agentx/` or commits.
- Lane C's scroll-layer suites run sampled by default and say so; a sampling or concurrency level never applies silently, and a change to tile carry, snapping or scroll offsets runs `--full` (or the nightly) before it is called proved. Seed a difference for every reduced comparison (`lane_c_sampling`) so it can fail.
- Do not dispatch or repeat hosted CI runs to close an epic: the pull request's own run is the complete run, and `check --epic` is only a static/build gate.
