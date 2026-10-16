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
    - testkit
    - tooling
  supersedes: []
  superseded_by: []
---

## Decision: `gpux-testkit` is the single entry point for every lane

### Context: The lanes orchestrate cargo, wasm-bindgen, pnpm and Playwright, cargo-ndk, aapt2, apksigner and adb, XcodeGen, xcodebuild and simctl. Shell scripts per OS would drift between developers and CI.

### Alternatives considered: Per-OS shell scripts; an `xtask` crate; Node tooling as the driver.

### Reasoning: A publishable workspace crate, `tools/testkit` (package and binary `gpux-testkit`), runs `lane <ids...>` (`--list`, `--scene`, `--grep`), `web build|serve|e2e`, `android build|run` and `ios build|run`. It checks host prerequisites with actionable errors, resolves Windows `.cmd` shims, parses the conformance hosts' result lines, ends every command with `gpux-testkit: <p> passed, <f> failed, <s> skipped`, exits nonzero on a failure or when nothing was selected, and writes raw output under `artifacts-temp/testkit/<lane>/`. CI calls the same commands, so a local run reproduces a CI job; CI passes `GPUX_WGPU_ADAPTER=software` for lane C on runners without a GPU.

### Trade-offs accepted: The crate keeps the publishing shape: nothing from outside its directory, no RayX paths, no network beyond the package managers it invokes. The Node project (`tools/browser/`) and the Android and iOS shells (`tools/android`, `tools/ios`) live outside it, and the Rust crates build and test without Node.

### Supersedes: None. Builds on [crates.io packaging](../packaging/2026-10-05-repository-packaging-crates-io-packaging.md).
