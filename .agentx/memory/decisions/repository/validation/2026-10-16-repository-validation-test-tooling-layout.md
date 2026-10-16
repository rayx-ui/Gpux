---
agentx:
  kind: decision
  scope: decisions/repository/validation
  status: current
  created: 2026-10-16T19:30:00+02:00
  updated: 2026-10-16T19:30:00+02:00
  promoted_from: gpux-tools-layout
  topics:
    - gpux
    - validation
    - layout
    - testkit
    - benchmark
  supersedes: []
  superseded_by: []
---

## Decision: Test tooling lives in `crates/gpux_tests_*` and `tools/`

### Context: gpux's test and benchmark tooling was spread over six places: `crates/gpux_conformance`, a root `tools-node/`, `tooling/{testkit,perf,android,ios}` and a root `benchmark/` workspace. The owner asked for one predictable place per kind, with pass/fail UI tests and improvement-loop benchmarks distinguishable (2026-10-16).

### Alternatives considered: Folders only, keeping the `gpux-conformance` and `gpux-benchmark` package names; a `tools/tests/` home for the Rust test crates; splitting the benchmark into `native/` and `wasm/` folders.

### Reasoning: Two homes, split by whether a thing is a Rust crate. `crates/` holds the library crates plus the verification crates named `gpux_tests_*` so they sort together: [`crates/gpux_tests_conformance`](../../../../../crates/gpux_tests_conformance) (package `gpux-tests-conformance`: the scene catalog, lane targets, goldens and app hosts that `gpux-testkit lane ...` runs in CI) and [`crates/gpux_tests_benchmark`](../../../../../crates/gpux_tests_benchmark) (package `gpux-tests-benchmark`: the improvement-loop benchmarks that its `run.py` runs locally). [`tools/`](../../../../../tools/README.md) holds what drives or hosts the tests: `testkit` (`gpux-testkit`), `perf` (`gpux-perf`, Zed's `#[perf]` timer), `browser` (lane B's Playwright project), `android` and `ios` (the lane E and F shells). Folder, package and library were renamed together while nothing was on crates.io, so every crate folder keeps matching its library's Rust name. The benchmark keeps one shared set of scenarios: they have no target-specific variants. `tools/testkit/tests/repository_layout.rs` guards the layout and catches references to the old locations in tracked live files.

### Trade-offs accepted: Runtime protocol identifiers keep the old word (`gpux-conformance:` result lines, `gpux-conformance-host:`, `__gpuxConformance`, `dev.gpux.conformance`, the Simulator name and APK file name), so a package name and a protocol name now differ. `crates/gpux_tests_benchmark` is a standalone, unpublished workspace inside `crates/`, exempt from the packaging rule for workspace crates. Older checkouts keep git-ignored build output in the old folders until deleted. Historical records (task and experiment records, changelog, history, measurements, the benchmark's results) keep the paths they were written with.

### Supersedes: None. Builds on [the test lanes](2026-10-07-repository-validation-test-lanes.md), [`gpux-testkit`](2026-10-07-repository-validation-gpux-testkit.md), [gpux owns its runtime proof](2026-10-07-repository-validation-gpux-owns-runtime-proof.md), [fast-mode proof and timing](2026-10-13-repository-validation-fast-mode-proof-and-timing.md) and [crates.io packaging](../packaging/2026-10-05-repository-packaging-crates-io-packaging.md).
