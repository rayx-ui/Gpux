---
agentx:
  kind: decision
  scope: decisions/repository/validation
  status: current
  created: 2026-10-09T17:00:00+02:00
  updated: 2026-10-09T17:00:00+02:00
  topics:
    - gpux
    - validation
    - test-lanes
    - conformance
  supersedes: []
  superseded_by: []
---

## Decision: Seven test lanes named by where they can run

### Context: gpux had no runtime proof of its own; behaviour, pixels, real windows, the browser and mobile were exercised only through RayX apps. The `gpux-cross-platform-test-lanes` epic (planned 2026-10-07) gave every kind of proof a lane gpux runs itself.

### Alternatives considered: One test suite with feature flags per platform; lanes named by test type (unit, integration, end-to-end); hosted GPU runners for every lane.

### Reasoning: A lane is defined by the environment its tests need, so its prerequisites and CI runner follow from the name: A behaviour on `TestPlatform`, B the threaded WASM build in Chromium, C offscreen pixels through the headless renderer with per-backend goldens, D real desktop windows and OS input, E Android, F iOS, G the developer's real GPU. Every test belongs to exactly one lane through its location and selector (`crates/*/tests` and `gpux_tests_conformance/tests/lane_a.rs` for A, `lane_c.rs` for C, the desktop host for D, `tools/browser/specs` for B, the mobile hosts for E and F, `gpux-testkit lane g` for G). One scene catalog in `gpux-tests-conformance` feeds every lane, each scene naming the lanes it runs in. A, C and D run on every pull request on Linux, Windows and macOS; B runs a pull-request subset and a nightly set; E and F run nightly and on path-filtered pull requests; G is local. [docs/testing.md](../../../../../docs/testing.md) is the contract.

### Trade-offs accepted: A lane never silently degrades: a missing prerequisite or capability is a reported skip with its reason, and a failure where the lane must cover it. Timing thresholds exist only in lane G, so CI does not catch performance regressions. Real-clock hosts (B, D, E, F) skip steps that need the executor clock; only lane A asserts them. Hosted CI runs on pull requests, nightly schedules and manual dispatches, never on pushes to `main`: a merged pull request's tests already ran on the pull request, and running them again paid for them twice (owner, 2026-10-09).

### Supersedes: None. Builds on the [crate-ownership memory taxonomy](../agentx-memory/2026-10-05-repository-agentx-memory-taxonomy-transplant.md).
