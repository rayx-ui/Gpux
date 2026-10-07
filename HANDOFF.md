# Handoff: gpux cross-platform test lanes

Where the `gpux-cross-platform-test-lanes` epic stands, and exactly what is left. Read this first,
then [docs/testing.md](docs/testing.md) (the lane contract) and the epic's AgentX specs in
[.agentx/tasks/gpux-cross-platform-test-lanes/](.agentx/tasks/gpux-cross-platform-test-lanes).

Last updated: 2026-10-08, branch `refactor/gpux-cross-platform-test-lanes`.

## Status

The epic adds seven test lanes, run by `cargo run -p gpux-testkit -- lane <a..g>`:

| Lane | What it proves | Implemented | Proven |
|---|---|---|---|
| A | behaviour on the test platform | yes | Windows (72/72), Linux (WSL), earlier macOS CI |
| B | the threaded WASM build in Chromium (Playwright, `tools-node/`) | yes | local GPU (Windows); SwiftShader CI profile in WSL |
| C | offscreen pixels, per-backend goldens | yes | WARP (Windows, 79/79), lavapipe (WSL, 79/79), earlier Metal CI |
| D | real desktop windows, OS input | yes | Windows, X11 under Xvfb (WSL); **macOS still red/unproven** |
| E | Android host over adb | yes | local x86_64 emulator: 29 passed, 1 skipped |
| F | iOS host on a Simulator | yes, **never built on a Mac** | **nothing yet**: type-checks and passes clippy for `aarch64-apple-ios` from Windows |
| G | the developer's real GPU (timing, benchmarks) | yes | RTX 4080 SUPER: 388/388 at `a7439d7` (lane C has since gained the 2 `lane_g` tests) |

AgentX state (sprints in `.agentx/tasks/gpux-cross-platform-test-lanes/implementation-plan.json`):

| Sprint | State |
|---|---|
| s1–s3 | sealed (checkpointed) |
| s4 testkit and windowed lane | sealed, checkpoint `b39e504` |
| s5 browser lane | sealed, checkpoint `21e02f8` |
| s6 Android lane | sealed, checkpoint `9c14137` |
| s7 lane G, docs, memory | sealed, checkpoint `0376af4` |
| s8 Apple lanes | implemented in source, **not started in AgentX and not verified: needs a Mac** |

### AgentX progress

Updated at the end of the 2026-10-08 session; check `status` for the live state:

```bash
python ~/.claude/skills/agentx/scripts/agentx_task.py status --task-dir .agentx/tasks/gpux-cross-platform-test-lanes
```

<!-- progress -->
- **s1–s7 are sealed** with local evidence under the owner decision below. Each passed its sprint
  checks and an independent review, and was checkpointed: s4 `b39e504`, s5 `21e02f8`,
  s6 `9c14137`, s7 `0376af4`.
- **s8 is next, on a Mac.** `t27`–`t29` are still `pending` in AgentX: start them with `next`
  (section 1 below). 43 of 48 criteria are implemented; M-1, M-3 and F-1 to F-3 remain.
- **Hosted CI is still blocked by billing.** Runs 37643950115, 37647356794 and 37662580569 never
  started.
- Local evidence on this Windows machine and in WSL Ubuntu 24.04 (Mesa 25.2.8):
  - lane A 72/72 and lane C 79/79 on WARP;
  - lane C 79/79 on lavapipe;
  - lane D: 40/40 on Windows; 39 passed + 1 documented skip on X11 under Xvfb;
  - lane B: `ci-linux-swiftshader` PR selection 120/2/0, nightly 155/3/0 with `CI=1`;
  - lane E on a local x86_64 emulator;
  - lane G 388/388 on the RTX 4080 SUPER.
- The reviews established the rules recorded in memory (`repository/validation`, `platform/*`):
  - a check must be able to fail;
  - a delay held back goes under `Step::When(Capability::ExecutorClock, …)`;
  - lane E builds only with the pinned NDK, which is r28-beta2;
  - every conformance test target belongs to lane A or lane C, enforced by `lane_coverage`.
<!-- /progress -->

## Owner decisions (2026-10-07)

- **CI is blocked** by GitHub Actions billing (`rayx-ui` org: "recent account payments have
  failed or your spending limit needs to be increased"). No job has started since run
  37599450308 on `1357ce7`.
- **Continue locally; test macOS and iOS on a Mac later.** Recorded as a specification amendment
  (revision 91):
  - D-2, B-5 and E-3 accept local runs of the workflows' own commands while hosted CI is
    unavailable (Windows here; `ubuntu-24.04` jobs in WSL; lane E on a local x86_64 emulator).
  - Every macOS run moved to a new criterion **M-3**, owned by `t27-macos-desktop-lanes` in
    `s8-apple-lanes`, verified on a Mac. Sprint 4 keeps only configuration and code obligations
    (M-2, MACCI-1, MACCI-2).
- Playwright lives in `tools-node/` (owner choice); the Android APK is packaged without Gradle.

## What is left

### 1. On a Mac: sprint 8 (`s8-apple-lanes`)

Criteria: **M-1, M-3** (task `t27-macos-desktop-lanes`), **F-1** (`t28-ios-host-build`), **F-2,
F-3** (`t29-ios-run-and-ci`). Read them with `context --sprint s8-apple-lanes`.

Prerequisites: Xcode with an iOS Simulator runtime (`xcodebuild -downloadPlatform iOS`),
`brew install xcodegen`, Rust 1.95.0 (`rust-toolchain.toml`) and
`rustup target add aarch64-apple-ios-sim aarch64-apple-ios`.

**macOS (M-1, M-3).** Run each and keep the summary lines:

```bash
cargo check --workspace --all-targets            # real `xcrun metal` compile in gpux-apple
cargo test --workspace --no-fail-fast \
  --exclude gpux-wgpu --exclude gpux-wgpu-core --exclude gpux-wgpu-hal \
  --exclude gpux-wgpu-core-deps-apple --exclude gpux-wgpu-core-deps-emscripten \
  --exclude gpux-wgpu-core-deps-wasm --exclude gpux-wgpu-core-deps-windows-linux-android \
  --exclude gpux-conformance -- --test-threads=1   # includes command::darwin::{process,tests}
cargo run -p gpux-testkit -- lane a c d
```

- Lane D on macOS was the last red job: 10 blank-text failures in run 37599450308. The fix
  (`dcb7bc5`: `gpui_platform`'s `font-kit` feature in the conformance dev-dependencies, so macOS
  windows get the CoreText text system instead of `NoopTextSystem`) has never run on macOS.
  If text is still blank, start in `crates/gpui_macos/src/platform.rs` (text system selection).
- Lane C on Metal compares the committed `crates/gpux_conformance/tests/goldens/metal/` goldens.
  If they differ only because of an intended rendering change, regenerate with
  `GPUX_UPDATE_SNAPSHOTS=1 cargo run -p gpux-testkit -- lane c` and review every PNG.
- macOS cannot be type-checked from Windows: `gpux-apple`'s build script runs only on a macOS
  host.

**iOS (F-1, F-2).** The lane-F code was written without a Mac. Expect to fix things on the first
build:

```bash
cargo run -p gpux-testkit -- ios build --simulator   # cargo staticlib, xcodegen, xcodebuild (unsigned)
cargo run -p gpux-testkit -- lane f                   # = ios run --simulator
```

| Piece | File | What was guessed and may need fixing |
|---|---|---|
| host | `crates/gpux_conformance/examples/ios_host/{lib,checks}.rs` | Registers through `gpui_mobile::ios::ffi::set_app_callback`; prints result lines to stdout; exits after `gpux-conformance-host: done`. |
| shell | `tooling/ios/{project.yml,Info.plist,Sources/main.m,Sources/gpux_ios_host.h}` | XcodeGen generates `tooling/ios/GpuxConformance.xcodeproj` (git-ignored). Linking: `-force_load $(GPUX_IOS_HOST_LIB) -lc++` plus upstream gpui-mobile's framework list; a missing system library (for example `-liconv` or `-lresolv`) would show as undefined symbols. `UILaunchScreen` keeps the app at native size. |
| build | `tooling/testkit/src/ios.rs` (`build`) | `xcodebuild -project … -target GpuxConformance -sdk iphonesimulator SYMROOT=target/gpux-ios/products OBJROOT=… CODE_SIGNING_ALLOWED=NO GPUX_IOS_HOST_LIB=…`. The app is expected at `target/gpux-ios/products/Debug-iphonesimulator/GpuxConformance.app`. |
| run | `tooling/testkit/src/ios.rs` (`run`) | Creates the Simulator `gpux-conformance` (iPhone 16, newest iOS runtime), `simctl bootstatus -b`, `install`, `launch --console-pty --terminate-running-process`; on `background` it launches `com.apple.Preferences`, waits 2 s and opens `gpux-conformance://foreground`; copies `Documents/frames/*.png` from `get_app_container`. |
| checks | `ios_host/checks.rs` | `ios-text-metrics` expects IBM Plex Sans at 16 px: ascent 16.4, descent 4.4, `m` 13.968, `0` 9.6 (±0.05; values parsed from the font file), "Conformance" 96.432 px ± 2. `ios-touch-tap` injects `PlatformInput::Touch` in-app. `ios-metal` is skipped (with the reason) when the Simulator offers no Metal device. |
| gpui_mobile | `crates/gpui_mobile/src/ios/window.rs` | New: `render_to_image` through the wgpu renderer, and a `Drop` that unregisters the window from the FFI list, clears UIKit back-pointers, drops the renderer, hides the UIWindow and autoreleases its UIKit objects. A crash on closing a scene window points here. |

Unit tests that run anywhere: `cargo test -p gpux-testkit --test ios`,
`cargo test -p gpux-mobile --test ios_window_teardown`. Type-check from any host:
`cargo clippy -p gpux-conformance --no-default-features --features app-host --example ios_host --target aarch64-apple-ios --no-deps -- -D warnings`.

**Device flow (F-3, documentation only):** see [docs/testing.md](docs/testing.md#on-a-device).

**Recording sprint 8 in AgentX** (sprints 1–7 are checkpointed):

```bash
T=.agentx/tasks/gpux-cross-platform-test-lanes
A=~/.claude/skills/agentx/scripts/agentx_task.py
python $A next --task-dir $T --worker <session-id>            # starts t27
python $A advance --task-dir $T --task t27-macos-desktop-lanes --worker <session-id>   # then t28, t29
python $A check --task-dir $T --sprint s8-apple-lanes
# independent review (references/auditor.md), then:
python $A review --task-dir $T --sprint s8-apple-lanes --file <review.json>
python $A checkpoint --task-dir $T --sprint s8-apple-lanes --commit gpux=<sha>
```

Record the Mac runs' summary lines (and Xcode/iOS runtime versions) in the review submission and
in the epic handoff (`advance --handoff`).

### 2. When GitHub Actions works again

- Dispatch `ci.yml`, `lane-b.yml` (with `all-specs` for the nightly selection), `lane-e.yml` and
  `lane-f.yml` on this branch (`gh workflow run <file> --ref refactor/gpux-cross-platform-test-lanes`)
  and record the run ids in the epic handoff. Since the amendment they are follow-ups, not gates,
  for D-2, B-5 and E-3; for M-3 a green `macos-15` run can replace the Mac run.
- `ci.yml`'s lane A and lane C jobs now run through `gpux-testkit` (first time in CI).

### 3. Finishing the epic

After every sprint is checkpointed:

```bash
python $A check --task-dir $T --epic        # final checks: fmt, clippy, web check, tests, package
python $A context --task-dir $T --epic      # for a fresh, independent final reviewer
python $A review --task-dir $T --epic --file <final-review.json>
python $A status --task-dir $T              # must say done
```

Then promote the epic's decisions into `.agentx/memory/decisions/` (the `memory.promotion`
section of `context --epic`), run `audit_agentx_memory.py`, and delete this file.

## Where things live

| What | Where |
|---|---|
| lane contract, commands, prerequisites, env vars, goldens | [docs/testing.md](docs/testing.md) |
| testkit (all lanes, `web`, `android`, `ios` commands) | `tooling/testkit/src/` (`lanes.rs`, `web.rs`, `android.rs`, `ios.rs`, `local_gpu.rs`) |
| conformance scenes, hosts | `crates/gpux_conformance/src/scenes/`, `examples/{desktop_host,web_host,android_host,ios_host}` |
| Playwright specs and profiles | `tools-node/specs/`, `tools-node/playwright.config.ts` |
| Android and iOS shells | `tooling/android/AndroidManifest.xml`, `tooling/ios/` |
| CI | `.github/workflows/{ci,lane-b,lane-e,lane-f}.yml` |
| memory | `.agentx/memory/knowledge/repository/validation/`, `platform/{web,mobile,desktop}`, `renderer/wgpu` |
| raw logs and frames | `artifacts-temp/testkit/<lane>/` (ignored) |

## Rules for the next agent

- [CLAUDE.md](CLAUDE.md) applies: Conventional Commits with meaningful scopes and **no
  co-author lines**; change nothing in `vendor/wgpu` or in RayX; crates.io dependencies only.
- Use the AgentX runtime for every state change; never hand-edit `state.json`.
- Test only what you changed (the epic's sprint checks are the selection).
- Keep raw output in `artifacts-temp/`; never commit logs, screenshots or frames.

## Known follow-ups outside the epic

- `crates/gpui_wgpu/tests/headless_renderer.rs`: the GPU-error test shared its device with
  others and failed in threaded runs (CI runs one at a time). A separate session is isolating it
  (`tests/headless_renderer_gpu_errors.rs`).
- Wayland lane D is local-only (weston's headless backend has no seat).
- Headless rendering on llvmpipe over GL (Linux with no Vulkan adapter) fails validation:
  `screen_effects_layout` is invalid, and 76 of 77 lane C tests fail. Lane C selects lavapipe
  with `VK_DRIVER_FILES` (the file is `lvp_icd.json` since Mesa 25.2), so CI is unaffected.
- Lane E pins NDK 28.0.12674087, which is r28-beta2. `find_sdk` accepts it by its base revision.
  Moving to a stable r28 release means changing `NDK_VERSION`, `lane-e.yml` and the docs
  together.
- Real-clock hosts (desktop, browser, Android, iOS) skip steps that need the executor clock to
  hold back a delay (`Capability::ExecutorClock`); only lane A asserts them.
- `gpui_mobile`'s iOS code has pre-existing warnings (missing commas in `msg_send!`, unsafe ops
  in unsafe fns) that predate the epic.
