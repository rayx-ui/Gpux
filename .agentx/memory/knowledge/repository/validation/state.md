---
agentx:
  kind: state
  scope: knowledge/repository/validation
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-09T17:00:00+02:00
  topics:
    - validation
    - focused-checks
    - lanes
    - testkit
    - runtime-proof
    - wasm
    - android
    - performance-method
  supersedes: []
  superseded_by: []
---
# Validation State

## Purpose And Usage

Read before choosing how to prove a gpux change. Lane commands, prerequisites, environment variables, and CI cadence live in [docs/testing.md](../../../../../docs/testing.md); the iteration commands in [AGENTS.md](../../../../../AGENTS.md#validation).

## Current Technical Shape

Iteration runs the touched package's focused checks from the Gpux root: `cargo fmt --check`, `cargo check -p <package> --all-targets`, `cargo test -p <package> <filter>`, and warning-denying `cargo clippy -p <package> --all-targets --no-deps`. Web crates also run `cargo check -p gpux-web --target wasm32-unknown-unknown --no-default-features` (the default multithreaded build needs nightly `-Z build-std`, which `gpux-testkit web build` provides); Android uses `cargo ndk -t arm64-v8a check -p gpux-mobile`. Manifest changes run `cargo package --workspace`.

Runtime proof is gpux's own: `gpux-testkit` ([`tooling/testkit`](../../../../../tooling/testkit)) runs lanes named by where they can run, ends every command with `gpux-testkit: <passed> passed, <failed> failed, <skipped> skipped`, fails when nothing is selected, and keeps raw output in `artifacts-temp/testkit/<lane>/`. The lanes share the scene catalog of [`gpux-conformance`](../../../../../crates/gpux_conformance), whose scenes name the lanes they run in:

- **A** runs scenes on `TestPlatform` with bundled fonts and the executor clock, no GPU.
- **B** builds the conformance `web_host` as threaded WASM (`web build --verify` checks shared memory, the thread entry point, and a single explicit constructor call) and drives it in Chromium through Playwright specs in `tools-node/` with trusted CDP input. Profiles: `local-gpu` (new headless on the real GPU), `ci-linux-swiftshader`, `windows-software`; software profiles skip real-clock timing.
- **C** renders scenes offscreen through the headless renderer on the software adapter (WARP, lavapipe, Metal on macOS) and compares a golden subset per backend; `GPUX_EXPECT_BACKEND` asserts the backend and `GPUX_PROBES_ONLY` skips goldens.
- **D** runs platform checks and scenes in real windows (`desktop_host`): presenting, resize, scale change, clipboard, OS input (`SendInput`, XTest), close.
- **E** builds a Gradle-free NativeActivity APK (`android_host`) and runs it over adb; the host asks the testkit for taps, swipes, background and resume, and screenshots, which pixel steps read. Emulators render through SwiftShader on the GL fallback: rendered-texture readback returns zeros there, and the surface is 16-bit, so color probes allow RGB565 rounding on frames that show it.
- **F** builds the `ios_host` static library and the XcodeGen shell of `tooling/ios` unsigned for the Simulator, boots a pinned Simulator (`gpux-conformance`, an iPhone 16 on the newest iOS runtime), and follows the host's console; it needs macOS with Xcode and XcodeGen. Devices are run by hand with a signing team and `devicectl`.
- **G** runs only on a developer machine with a GPU and refuses under `CI`: lane B's every spec (including `@timing` frame-budget specs and DPR 2), lanes C and D on the hardware adapter with probes only, and the `#[gpux::bench]` suites, summarized in `artifacts-temp/testkit/g/timing.md`.

CI (`ci.yml`) runs check, test, package, and lanes A, C and D on `ubuntu-24.04`, `windows-2025` and `macos-15` for every pull request; `lane-b.yml` runs `@canary`/`@pr` specs on SwiftShader per pull request and every spec but `@timing` nightly; `lane-e.yml` runs an API 34 x86_64 emulator under KVM, and `lane-f.yml` a `macos-15` Simulator, nightly and on path-filtered pull requests. CI's lane A, C and D jobs run through `gpux-testkit`. A manual `update-goldens` dispatch regenerates each backend's goldens.

Performance work measures on a real GPU through lane G: the `gpux-platform` benchmarks (`randomized_element_tree`, `inspector_render`, run on an 8 MiB stack thread on every platform) and lane B's `@timing` specs. Earlier measurements on RayX's warmed wide-DataTable workload remain evidence in the decisions that cite them. A change is kept only when output and every established counter stay exact; transient evidence goes to ignored `artifacts-temp/`.

## Backlinks

- [Testing guide](../../../../../docs/testing.md)
- [Repository instructions](../../../../../AGENTS.md)
- [gpux skill](../../../../../.claude/skills/gpux/SKILL.md)
- [gpux-testkit](../../../../../tooling/testkit)
- [Conformance lanes](../../../../../crates/gpux_conformance/tests)
- [Playwright specs](../../../../../tools-node/specs)
- [CI workflow](../../../../../.github/workflows/ci.yml), [lane B workflow](../../../../../.github/workflows/lane-b.yml), [lane E workflow](../../../../../.github/workflows/lane-e.yml), [lane F workflow](../../../../../.github/workflows/lane-f.yml)
- 2026-10-05 current [RayX consumer validation memory](../../../../../../RayX/.agentx/memory/knowledge/tooling/validation/state.md)
- 2026-10-07 current [Seven test lanes](../../../decisions/repository/validation/2026-10-07-repository-validation-test-lanes.md)
- 2026-10-07 current [gpux owns its runtime proof](../../../decisions/repository/validation/2026-10-07-repository-validation-gpux-owns-runtime-proof.md)
- 2026-10-07 current [gpux-testkit entry point](../../../decisions/repository/validation/2026-10-07-repository-validation-gpux-testkit.md)
- 2026-10-07 current [Local lane G](../../../decisions/repository/validation/2026-10-07-repository-validation-local-gpu-lane-g.md)
- 2026-10-07 current [Browser lane B](../../../decisions/platform/web/2026-10-07-platform-web-browser-lane-b.md)
- 2026-10-07 current [Android lane E](../../../decisions/platform/mobile/2026-10-07-platform-mobile-android-lane-e.md)
- 2026-10-07 current [iOS lane F](../../../decisions/platform/mobile/2026-10-07-platform-mobile-ios-lane-f.md)
