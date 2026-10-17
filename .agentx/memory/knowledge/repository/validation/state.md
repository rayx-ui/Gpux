---
agentx:
  kind: state
  scope: knowledge/repository/validation
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-17T10:00:00+02:00
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

Runtime proof is gpux's own: `gpux-testkit` ([`tools/testkit`](../../../../../tools/testkit)) runs lanes named by where they can run, ends every command with `gpux-testkit: <passed> passed, <failed> failed, <skipped> skipped`, fails when nothing is selected, and keeps raw output in `artifacts-temp/testkit/<lane>/`. The lanes share the scene catalog of [`gpux-tests-conformance`](../../../../../crates/gpux_tests_conformance), whose scenes name the lanes they run in:

- **A** runs scenes on `TestPlatform` with bundled fonts and the executor clock, no GPU.
- **B** builds the conformance `web_host` as threaded WASM (`web build --verify` checks shared memory, the thread entry point, and a single explicit constructor call) and drives it in Chromium through Playwright specs in `tools/browser/` with trusted CDP input. Profiles: `local-gpu` (new headless on the real GPU), `ci-linux-swiftshader`, `windows-software`; software profiles skip real-clock timing.
- **C** renders scenes offscreen through the headless renderer on the software adapter (WARP, lavapipe, Metal on macOS) and compares a golden subset per backend; `GPUX_EXPECT_BACKEND` asserts the backend and `GPUX_PROBES_ONLY` skips goldens.
  Lane C is fast on purpose: `cargo test` builds at `opt-level = 1` (`[profile.test]`, debug assertions and overflow checks on; `lane_c_profile` fails if either is off), the three scroll-layer suites compare frame bytes first and run *sampled* by default (the layered window draws every frame; the reference is drawn and compared at a subset of long one-pixel stretches; `GPUX_LANE_C_FULL=1` / `lane c --full` and lane G run every frame, `lane_c_sampling` lists the uncovered steps), `lane c --jobs <n>` runs the test groups in concurrent processes (weights in `LANE_C_WEIGHTS`), and every libtest lane writes `artifacts-temp/testkit/<lane>/timings.md`. Numbers and the unmeasured WARP full-mode gap: [lane-c-timings.md](../../../../../docs/measurements/lane-c-timings.md).
- **D** runs platform checks and scenes in real windows (`desktop_host`): presenting, resize, scale change, clipboard, OS input (`SendInput`, XTest), close.
- **E** builds a Gradle-free NativeActivity APK (`android_host`) and runs it over adb; the host asks the testkit for taps, swipes, background and resume, and screenshots, which pixel steps read. Emulators render through SwiftShader on the GL fallback: rendered-texture readback returns zeros there, and the surface is 16-bit, so color probes allow RGB565 rounding on frames that show it.
- **F** builds the `ios_host` static library and the XcodeGen shell of `tools/ios` unsigned for the Simulator, boots a pinned Simulator (`gpux-conformance`, an iPhone 16 on the newest iOS runtime), and follows the host's console; it needs macOS with Xcode and XcodeGen. Devices are run by hand with a signing team and `devicectl`.
- **G** runs only on a developer machine with a GPU and refuses under `CI`: lane B's every spec (including `@timing` frame-budget specs and DPR 2), lanes C and D on the hardware adapter with probes only, and the `#[gpux::bench]` suites, summarized in `artifacts-temp/testkit/g/timing.md`.

CI (`ci.yml`) runs check, test, package, and lanes A, C and D on `ubuntu-24.04`, `windows-2025` and `macos-15` for every pull request, except lane C on WARP, which a pull request runs only with `ci-os:windows` or `ci:all` (the nightly 03:17 UTC schedule runs everything, lane C in full mode; `plan.sh` holds the per-platform `--jobs`: WARP 3, others 1); `lane-b.yml` runs `@canary`/`@pr` specs on SwiftShader per pull request and every spec but `@timing` nightly; `lane-e.yml` runs an API 34 x86_64 emulator under KVM, and `lane-f.yml` a `macos-15` Simulator, nightly and on path-filtered pull requests. CI's lane A, C and D jobs run through `gpux-testkit`. A manual `update-goldens` dispatch regenerates each backend's goldens.

Performance work measures on a real GPU through lane G: the `gpux-platform` benchmarks (`randomized_element_tree`, `inspector_render`, `fast_mode`, run on an 8 MiB stack thread on every platform; on macOS `bench-support` enables `font-kit` and the Metal headless renderer drains an autorelease pool per frame) and lane B's `@timing` specs. `gpux-testkit perf fast-mode [--name <run>] [--compare <run-dir>]` runs `fast_mode` alone into `artifacts-temp/testkit/perf/<run>/` (`run.txt` records commit, tree cleanliness and machine; `fast-mode.md` reports Criterion medians per scenario and mode with mechanism rankings); lane G writes the same report to `artifacts-temp/testkit/g/fast-mode.md`. The fast-mode pre-optimization baseline is `artifacts-temp/testkit/perf/baseline-s01/` (commit `24d413c`, see `docs/fast-mode.md`). Fast-mode parity runs every lane-A and lane-C scene with fast mode on and off (`lane_a_fast_parity`, `lane_c_fast_parity`); the scene lists live in `crates/gpux_tests_conformance/tests/catalog/`. Earlier measurements on RayX's warmed wide-DataTable workload remain evidence in the decisions that cite them. A change is kept only when output and every established counter stay exact; transient evidence goes to ignored `artifacts-temp/`.

List layers are proved by `fast::tests::list_layers` (gpui-fast's uniform, list and row groups against a reference window; `gpux`, which compares quads, accessibility trees, debug bounds, tab order, listeners and whole frame descriptions, with what a pointer hits down two columns of the rows, over scripted and random histories (wheel turns by whole and fractional pixels, moves, clicks, presses and releases without a move, a sheet from another view, notifications, focus changes; lists plain, padded, told of their rows' focus, with rows handling text input), in a scrolling `div` too; `oracle`, the layers oracle over a uniform list and a list), by the list tests of `lane_c_scroll_layers` (pixels) and by the list scenes of `lane_a_fast_parity`, which is what first showed overscan rows in the element tree. What a long scroll keeps (lingering row nodes, rows a list layer holds, state after repeated passes) is bounded by `fast::tests::layer_memory`; footprint and heap against fast mode off are measured by `crates/gpux_tests_benchmark/` (`python3 crates/gpux_tests_benchmark/run.py --target macos --scenarios scroll --configs off,on`, and `gpux-bench --frames 3000|9000` for plateaus).

Lane G on macOS needs the display awake: with it asleep every lane D window reports `no frame within 10s` (hold it with `caffeinate -d -u`). `perf fast-mode` (the `fast_mode` bench) runs only on macOS: in the Linux VM its scenarios stop at their own check because the harness's windows draw no glyphs, and it needs a GPU adapter to open its windows. Lane G ran on Windows DX12 (RTX 4080 SUPER) for the fast-mode epic (2026-10-14; browser p95, lanes B, C and D in full, `fast_mode` failing at the same glyph check), and under WSL only lanes C and D ran on the GPU, through a locally built Mesa `dzn` (Vulkan on D3D12, not timing proof; lane B and browser timings cannot run there; setup in `LINUX-WSL.md`): [WSL decision](../../../decisions/repository/validation/2026-10-14-repository-validation-wsl-gpu-lanes-c-d.md), [PERF-4 decision](../../../decisions/repository/validation/2026-10-14-repository-validation-wgpu-desktop-perf-4.md), [measurements](../../../../../docs/measurements/wgpu-desktop-timings.md). No Linux GPU timing and no `fast_mode` table off the Mac exist. The final comparison is in `docs/fast-mode.md`.

## Backlinks

- [Testing guide](../../../../../docs/testing.md)
- [Repository instructions](../../../../../AGENTS.md)
- [gpux skill](../../../../../.claude/skills/gpux/SKILL.md)
- [gpux-testkit](../../../../../tools/testkit)
- [Conformance lanes](../../../../../crates/gpux_tests_conformance/tests)
- [Playwright specs](../../../../../tools/browser/specs)
- [CI workflow](../../../../../.github/workflows/ci.yml), [lane B workflow](../../../../../.github/workflows/lane-b.yml), [lane E workflow](../../../../../.github/workflows/lane-e.yml), [lane F workflow](../../../../../.github/workflows/lane-f.yml)
- 2026-10-05 current [RayX consumer validation memory](../../../../../../RayX/.agentx/memory/knowledge/tooling/validation/state.md)
- 2026-10-16 current [Test tooling layout](../../../decisions/repository/validation/2026-10-16-repository-validation-test-tooling-layout.md)
- 2026-10-07 current [Seven test lanes](../../../decisions/repository/validation/2026-10-07-repository-validation-test-lanes.md)
- 2026-10-07 current [gpux owns its runtime proof](../../../decisions/repository/validation/2026-10-07-repository-validation-gpux-owns-runtime-proof.md)
- 2026-10-07 current [gpux-testkit entry point](../../../decisions/repository/validation/2026-10-07-repository-validation-gpux-testkit.md)
- 2026-10-14 current [WSL lanes C and D on the GPU](../../../decisions/repository/validation/2026-10-14-repository-validation-wsl-gpu-lanes-c-d.md)
- 2026-10-14 current [PERF-4 wgpu-desktop half](../../../decisions/repository/validation/2026-10-14-repository-validation-wgpu-desktop-perf-4.md)
- 2026-10-07 current [Local lane G](../../../decisions/repository/validation/2026-10-07-repository-validation-local-gpu-lane-g.md)
- 2026-10-07 current [Browser lane B](../../../decisions/platform/web/2026-10-07-platform-web-browser-lane-b.md)
- 2026-10-07 current [Android lane E](../../../decisions/platform/mobile/2026-10-07-platform-mobile-android-lane-e.md)
- 2026-10-07 current [iOS lane F](../../../decisions/platform/mobile/2026-10-07-platform-mobile-ios-lane-f.md)
