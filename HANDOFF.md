# Handoff: gpux cross-platform test lanes

Where the `gpux-cross-platform-test-lanes` epic stands and exactly what is left. Read this first,
then [docs/testing.md](docs/testing.md) (the lane contract) and the epic's AgentX specs in
[.agentx/tasks/gpux-cross-platform-test-lanes/](.agentx/tasks/gpux-cross-platform-test-lanes).

Last updated: 2026-10-08, branch `refactor/gpux-cross-platform-test-lanes` (pushed to `origin`).

**If you are the agent on the Mac, follow [Part 1](#part-1-the-mac-session-sprint-8) step by step.**

## Status

The epic adds seven test lanes, run by `cargo run -p gpux-testkit -- lane <a..g>`:

| Lane | What it proves | Implemented | Proven |
|---|---|---|---|
| A | behaviour on the test platform | yes | Windows (72/72), Linux (WSL), earlier macOS CI |
| B | the threaded WASM build in Chromium (Playwright, `tools-node/`) | yes | local GPU (Windows); SwiftShader CI profile in WSL |
| C | offscreen pixels, per-backend goldens | yes | WARP (Windows, 79/79), lavapipe (WSL, 79/79), earlier Metal CI |
| D | real desktop windows, OS input | yes | Windows (40/40), X11 under Xvfb (WSL); **macOS unproven since its fix** |
| E | Android host over adb | yes | local x86_64 emulator |
| F | iOS host on a Simulator | yes, **never built on a Mac** | **nothing yet**: unit tests and `aarch64-apple-ios` clippy pass on Windows |
| G | the developer's real GPU (timing, benchmarks) | yes | RTX 4080 SUPER (see the preflight below) |

AgentX state (`python <agentx>/agentx_task.py status --task-dir .agentx/tasks/gpux-cross-platform-test-lanes`):

| Sprint | State |
|---|---|
| s1–s3 | sealed |
| s4 testkit and windowed lane | sealed, checkpoint `b39e504` |
| s5 browser lane | sealed, checkpoint `21e02f8` |
| s6 Android lane | sealed, checkpoint `9c14137` |
| s7 lane G, docs, memory | sealed, checkpoint `0376af4` |
| s8 Apple lanes | epic `blocked`: `t27-macos-desktop-lanes` blocked (needs a Mac), `t28-ios-host-build` and `t29-ios-run-and-ci` pending; lane F source written |

43 of 48 criteria are implemented and verified. Left: **M-1, M-3** (t27), **F-1** (t28),
**F-2, F-3** (t29). All five are `manual` criteria proven by runs on macOS.

<!-- preflight -->
<!-- /preflight -->

## Owner decisions (2026-10-07)

- **CI is blocked** by GitHub Actions billing (`rayx-ui` org: "recent account payments have
  failed or your spending limit needs to be increased"). No job has started since run
  37599450308 on `1357ce7`; runs 37643950115, 37647356794 and 37662580569 never started.
- **Continue locally; test macOS and iOS on a Mac later.** Recorded as a specification amendment
  (revision 91):
  - D-2, B-5 and E-3 accept local runs of the workflows' own commands while hosted CI is
    unavailable.
  - Every macOS run moved to criterion **M-3** (t27, s8), verified on a Mac.
- Playwright lives in `tools-node/`; the Android APK is packaged without Gradle.
- Toolchain pins (2026-10-07, `fe6a2e2`): the threaded WASM build uses `nightly-2026-06-23`
  (`GPUX_WEB_TOOLCHAIN` overrides it), lane E uses JDK 21. Move them together with `lane-b.yml`,
  RayX and RayX.CLI.

## Part 1: the Mac session (sprint 8)

### 1.0 What you need

- macOS on Apple silicon, Xcode 16 or later (Xcode 26 is fine), and its license accepted:
  `sudo xcodebuild -license accept`; `xcode-select -p` must point into `Xcode.app`
  (`sudo xcode-select -s /Applications/Xcode.app` otherwise).
- An iOS Simulator runtime: `xcodebuild -downloadPlatform iOS` (check with
  `xcrun simctl list runtimes`).
- The Metal compiler: `xcrun -f metal` must print a path. Xcode 26 ships it as a separate
  component: `xcodebuild -downloadComponent MetalToolchain`.
- `brew install xcodegen`.
- Rust: `rustup` installs 1.95.0 from `rust-toolchain.toml` on the first cargo command; add
  `rustup target add aarch64-apple-ios-sim aarch64-apple-ios`.
- Python 3 and the AgentX skill (`~/.claude/skills/agentx`, the same skill this Windows session
  used; copy it if the Mac does not have it).
- Optional, for lane B or G on the Mac: Node.js 22 with `pnpm`, `nightly-2026-06-23` with
  `rust-src`, and `wasm-bindgen-cli` at the `Cargo.lock` version. Sprint 8 does not need them.

Some of these need `sudo` or a GUI download; do them before starting the agent so it does not
stall.

### 1.1 Get the branch

```bash
git fetch origin
git switch refactor/gpux-cross-platform-test-lanes
git pull --ff-only
```

### 1.2 Resume the epic in AgentX

```bash
T=$PWD/.agentx/tasks/gpux-cross-platform-test-lanes
A=~/.claude/skills/agentx/scripts/agentx_task.py
python3 $A status --task-dir $T          # expect status "blocked", t27 blocked
python3 $A next --task-dir $T --worker <this-session-id> --adopt
```

`--adopt` is needed because the Windows session (worker `082cbae1-…`) owned the epic and has
stopped. `next` resumes t27 and returns the sprint-entry packet: read its `memory` pointers once
(the `platform/desktop` and `platform/mobile` rules) and t27's blocker note. Use one coordinator
session for the whole sprint.

### 1.3 t27: macOS lanes and CI commands (M-1, M-3)

Run each command and keep the final summary line of each; they are the evidence. Raw logs go to
`artifacts-temp/` (git-ignored).

```bash
# M-3 `check`: compiles every crate, including gpux-apple's real `xcrun metal` shader build
cargo check --workspace --all-targets

# M-3 `test`: the CI test job (fork excluded; gpux-conformance belongs to lanes A and C)
cargo test --workspace --no-fail-fast \
  --exclude gpux-wgpu --exclude gpux-wgpu-core --exclude gpux-wgpu-hal \
  --exclude gpux-wgpu-core-deps-apple --exclude gpux-wgpu-core-deps-emscripten \
  --exclude gpux-wgpu-core-deps-wasm --exclude gpux-wgpu-core-deps-windows-linux-android \
  --exclude gpux-conformance -- --test-threads=1
# MACCI-1's process reaping, shown on its own:
cargo test -p gpux-zed-util command::darwin -- --test-threads=1

# M-1 and M-3 lanes
cargo run -p gpux-testkit -- lane a
GPUX_EXPECT_BACKEND=metal cargo run -p gpux-testkit -- lane c
cargo run -p gpux-testkit -- lane d
```

Expected results:

| Command | Expected | Notes |
|---|---|---|
| `cargo check` | exit 0 | Last proven by run 37599450308's green `check (macos-15)`; later macOS changes (`dcb7bc5` and others) have never compiled on macOS. |
| `cargo test` | exit 0 | `test (macos-15)` was green in run 37599450308. |
| lane A | `72 passed, 0 failed` | Identical on every OS: 60 scene and catalog tests plus the 12 tests of the renderer-free groups. |
| lane C | `79 passed, 0 failed` | 77 scenes (at 1x and 2x) against `crates/gpux_conformance/tests/goldens/metal/`, plus 2 `lane_g` tests. The `metal` goldens came from the `macos-15` CI run. |
| lane D | about `38 passed, 0 failed, 2 skipped` | macOS skips `platform-os-input` (no accessibility permission for posting events) and `platform-scale-change` (the display sets the scale); input is injected in-app. |

If something fails:

| Symptom | Where to look |
|---|---|
| `cargo check` fails in `gpux-apple`'s build script | `xcrun -f metal`; on Xcode 26 install the Metal toolchain (1.0). `crates/gpui_apple/build.rs` compiles `src/shaders.metal`. |
| `cargo check` fails elsewhere | Fix at the source, macOS-only code (`crates/gpui_macos`, `crates/gpui_apple`, `crates/util/src/command/darwin.rs`). No git dependencies or `[patch]`; nothing in `vendor/wgpu`. |
| Lane D: text probes find blank text (the 10 failures of run 37599450308) | The fix `dcb7bc5` adds `gpui_platform`'s `font-kit` feature to `gpux-conformance`'s dev-dependencies so macOS windows get CoreText instead of `NoopTextSystem`. If text is still blank, check the text system `crates/gpui_macos/src/platform.rs` picks. |
| Lane C: golden diffs | Compare the diff images in `artifacts-temp/testkit/c/`. Regenerate (`GPUX_UPDATE_SNAPSHOTS=1 GPUX_EXPECT_BACKEND=metal cargo run -p gpux-testkit -- lane c`) only when the difference is an intended rendering change or the Metal goldens were taken on a different GPU family and the probes still pass; review every changed PNG and say why in the commit. |
| Lane C: a backend other than Metal | `GPUX_EXPECT_BACKEND=metal` fails the scene; check `crates/gpux_conformance/src/test_host.rs` and `gpui_platform::current_headless_renderer()`. |
| A test needs a timing delay held back | Real-clock hosts skip it by design (`Capability::ExecutorClock`); only lane A asserts it. |

Optionally run `cargo run -p gpux-testkit -- lane g` on the Mac as well (G-1 allows it; it needs
the lane B toolchain from 1.0). It is not required for sprint 8.

Then record t27 and start t28. The handoff carries the evidence:

```bash
python3 $A advance --task-dir $T --task t27-macos-desktop-lanes --worker <id> --handoff \
  'macOS <version>, Xcode <version>, <chip>. cargo check: exit 0. cargo test: <summary>. lane a: <line>. lane c (metal): <line>. lane d: <line>. Fixes: <commits or none>.'
```

Use single quotes for handoffs: backticks in double quotes are command substitutions.

### 1.4 t28: build the iOS host (F-1)

```bash
cargo run -p gpux-testkit -- ios build --simulator
```

It builds `ios_host` (`crates/gpux_conformance/examples/ios_host/`) as a staticlib for
`aarch64-apple-ios-sim`, runs `xcodegen` on `tooling/ios/project.yml`, then `xcodebuild` unsigned
into `target/gpux-ios/products/Debug-iphonesimulator/GpuxConformance.app`.

**This code has never been built on a Mac.** Expect fixes; this is what was guessed:

| Piece | File | What may need fixing |
|---|---|---|
| host | `crates/gpux_conformance/examples/ios_host/{lib,checks}.rs` | Registers through `gpui_mobile::ios::ffi::set_app_callback`; prints result lines to stdout; exits after `gpux-conformance-host: done`. |
| shell | `tooling/ios/{project.yml,Info.plist,Sources/main.m,Sources/gpux_ios_host.h}` | XcodeGen generates `tooling/ios/GpuxConformance.xcodeproj` (git-ignored). Linking is `-force_load $(GPUX_IOS_HOST_LIB) -lc++` plus upstream gpui-mobile's framework list; a missing system library (for example `-liconv` or `-lresolv`) shows up as undefined symbols. `UILaunchScreen` keeps the app at native size. |
| build | `tooling/testkit/src/ios.rs` (`build`) | `xcodebuild -project … -target GpuxConformance -sdk iphonesimulator SYMROOT=target/gpux-ios/products OBJROOT=… CODE_SIGNING_ALLOWED=NO GPUX_IOS_HOST_LIB=…`. |
| gpui_mobile | `crates/gpui_mobile/src/ios/window.rs`, `ffi.rs` | New: `render_to_image` through the wgpu renderer, and a `Drop` that unregisters the window from the FFI list, clears UIKit back-pointers, drops the renderer, hides the UIWindow and autoreleases its UIKit objects. A crash when a scene window closes points here. |

Tests that guard these and run anywhere: `cargo test -p gpux-testkit --test ios` (7) and
`cargo test -p gpux-mobile --test ios_window_teardown` (3). Keep them passing; when you fix a
behaviour, extend them (or add a group) so the fix is covered. `gpui_mobile`'s iOS code has 36
pre-existing rustc warnings (missing commas in `msg_send!`, unsafe operations in unsafe fns);
they predate the epic and are not sprint 8's to fix.

Then `advance --task t28-ios-host-build` with a handoff naming the build result and any fixes.

### 1.5 t29: run lane F (F-2, F-3)

```bash
cargo run -p gpux-testkit -- lane f        # = ios run --simulator
```

It creates the Simulator `gpux-conformance` (iPhone 16 on the newest installed iOS runtime) on
first use, boots it, installs and launches the app with `simctl launch --console-pty`, and parses
the result lines. It runs five platform checks and every lane-F scene:

| Check | Expected | Values the check uses |
|---|---|---|
| `ios-startup` | PASS | a window opens and draws |
| `ios-metal` | PASS on Apple silicon; SKIP with the reason where the Simulator has no Metal device | wgpu renders through Metal and reads the frame back |
| `ios-text-metrics` | PASS | IBM Plex Sans at 16 px: ascent 16.4, descent 4.4, `m` 13.968, `0` 9.6 (±0.05, parsed from the font file); "Conformance" 96.432 px ± 2 |
| `ios-touch-tap` | PASS | `PlatformInput::Touch` injected in-app taps the element under it |
| `ios-lifecycle` | PASS | the testkit opens Settings (`com.apple.Preferences`), waits 2 s, reopens `gpux-conformance://foreground`; the window deactivates, reactivates and draws |

The console log and failing frames (copied from the app container's `Documents/frames/`) land in
`artifacts-temp/testkit/f/`. If the run hangs, check that the host prints
`gpux-conformance-host: done` and that `tooling/testkit/src/ios.rs` (`run`) stops reading the
console after it. If `ios-text-metrics` is off by a constant factor, check the scale factor the
window reports before changing the expected values, which come from the font file.

**F-3** asks for "a green run" of `lane-f.yml`. The workflow and the device flow
([docs/testing.md](docs/testing.md#on-a-device)) are written. If CI is still blocked, the owner's
2026-10-07 rule for D-2, B-5 and E-3 (a local run of the workflow's own command) does **not** yet
name F-3: ask the owner whether the local `lane f` run counts, and record the answer with
`amend --specification` the way revision 91 did; otherwise leave F-3 to the first CI run.

Device flow (optional, documentation only): build with
`--target aarch64-apple-ios`, sign with your team, install with `xcrun devicectl`; the exact
steps are in `docs/testing.md`.

Then `advance --task t29-ios-run-and-ci` with the run's summary line, the Simulator model and the
iOS runtime version.

### 1.6 Memory sync for sprint 8

`platform/mobile` already has lane F's design and the iOS window teardown (`ff718fb`).
After the runs, update only what the runs proved or changed, in
`.agentx/memory/knowledge/platform/mobile/` (and `platform/desktop/` for macOS): for example
whether the Simulator offers Metal, link flags the shell needed, or macOS lane D's text-system
fix. Bump `updated:` stamps and run
`python3 ~/.claude/skills/agentx/scripts/audit_agentx_memory.py`.

### 1.7 Close sprint 8

```bash
git add -A && git commit            # Conventional Commits, logically grouped; no co-author lines
python3 $A check --task-dir $T --sprint s8-apple-lanes     # s8 has no automated checks
python3 $A context --task-dir $T --sprint s8-apple-lanes   # the review packet (snapshotId)
```

Start a **fresh, independent reviewer** (a separate agent or session, not the coordinator) with
`~/.claude/skills/agentx/references/auditor.md`, the packet, and the evidence lines from the
handoffs. It writes `artifacts-temp/agentx/gpux-cross-platform-test-lanes/review-s8.json`
(`reviewerId`, `snapshotId`, all five `criterionIds`, `verdict`, `anchors`, `findings`) and
submits it:

```bash
python3 $A review --task-dir $T --sprint s8-apple-lanes --file artifacts-temp/agentx/gpux-cross-platform-test-lanes/review-s8.json
```

On a failed verdict, fix every finding as one batch (`next --task <id>` reopens a task), rerun
what the fixes touched, and send the delta to the same reviewer. Once it passes, commit the
AgentX state and checkpoint:

```bash
git add .agentx && git commit -m "docs(agentx): record the sprint 8 tasks and review"
python3 $A checkpoint --task-dir $T --sprint s8-apple-lanes --commit gpux=$(git rev-parse HEAD)
git add .agentx && git commit -m "docs(agentx): record the sprint 8 checkpoint"
```

The checkpoint refuses a commit whose product files differ from the reviewed snapshot: commit
everything (including this file) before `context`, and do not edit product files between the
review and the checkpoint.

## Part 2: finishing the epic (after sprint 8)

Any machine works, but the Mac is where everything is fresh.

```bash
python3 $A check --task-dir $T --epic        # accumulated sprint checks, then the 5 final checks
python3 $A context --task-dir $T --epic      # packet for a fresh, independent final reviewer
python3 $A review --task-dir $T --epic --file <final-review.json>
python3 $A status --task-dir $T              # must say "done"
```

The final checks are `final-fmt`, `final-clippy` (eight packages, `-D warnings`),
`final-web-check` (`gpux-web` on wasm32), `final-tests` (seven packages, `--test-threads=1`, at
least 700 tests) and `final-package` (`cargo package --workspace`, which needs a clean worktree).
The Windows preflight above ran them at the current HEAD.

Then:

1. Promote the epic's decisions into `.agentx/memory/decisions/` from the `memory.promotion`
   section of `context --epic`: d-lanes, d-no-rayx, d-lane-g, d-ios-lane, d-web-lane,
   d-android-lane, d-testkit. Run `audit_agentx_memory.py`.
2. Delete this file.
3. Push, and ask the owner whether to open the pull request to `main`.

## Part 3: when GitHub Actions works again

- Dispatch on this branch and record the run ids in the epic handoff:
  `gh workflow run ci.yml --ref refactor/gpux-cross-platform-test-lanes`, then `lane-b.yml` (with
  `all-specs: true` for the nightly selection), `lane-e.yml` and `lane-f.yml`.
- They are follow-ups, not gates, for D-2, B-5 and E-3; a green `macos-15` run can stand in for the
  Mac runs of M-3, and a green `lane-f.yml` run is F-3's evidence.
- `ci.yml`'s lane A and lane C jobs run through `gpux-testkit` for the first time.
- If `lane C (lavapipe)` reports golden diffs on the hosted runner, dispatch `ci.yml` with
  `update-goldens`, commit the uploaded artifact after reviewing it, and rerun.

## Where things live

| What | Where |
|---|---|
| lane contract, commands, prerequisites, env vars, goldens | [docs/testing.md](docs/testing.md) |
| testkit (lanes, `web`, `android`, `ios`) | `tooling/testkit/src/` (`lanes.rs`, `web.rs`, `android.rs`, `ios.rs`, `local_gpu.rs`) |
| conformance scenes, hosts | `crates/gpux_conformance/src/scenes/`, `examples/{desktop_host,web_host,android_host,ios_host}` |
| goldens | `crates/gpux_conformance/tests/goldens/{warp,lavapipe,metal}/` |
| Playwright specs and profiles | `tools-node/specs/`, `tools-node/playwright.config.ts` |
| Android and iOS shells | `tooling/android/AndroidManifest.xml`, `tooling/ios/` |
| CI | `.github/workflows/{ci,lane-b,lane-e,lane-f}.yml` |
| memory | `.agentx/memory/knowledge/repository/validation/`, `platform/{web,mobile,desktop}`, `renderer/wgpu` |
| raw logs and frames | `artifacts-temp/testkit/<lane>/` (ignored) |

## Rules for the next agent

- [CLAUDE.md](CLAUDE.md) applies: Conventional Commits with meaningful scopes and **no
  co-author lines**; change nothing in `vendor/wgpu` or in RayX; crates.io dependencies only.
- Use the AgentX runtime for every state change; never hand-edit `state.json`.
- Test only what you changed: the lanes and focused groups above, not whole suites (the final
  checks are the exception).
- A check must be able to fail: assert a known pixel or state, and prove a regression test
  against the old code.
- Keep raw output in `artifacts-temp/`; never commit logs, screenshots or frames.

## Known follow-ups outside the epic

- Headless rendering on llvmpipe over GL (Linux with no Vulkan adapter) fails validation:
  `screen_effects_layout` is invalid and 76 of 77 lane C tests fail. Lane C selects lavapipe with
  `VK_DRIVER_FILES` (`lvp_icd.json` since Mesa 25.2), so CI is unaffected.
- Lane E pins NDK 28.0.12674087, which is r28-beta2; `find_sdk` accepts it by its base revision.
  Moving to a stable r28 means changing `NDK_VERSION`, `lane-e.yml` and the docs together.
- Real-clock hosts (desktop, browser, Android, iOS) skip steps that need the executor clock to hold
  a delay back (`Capability::ExecutorClock`); only lane A asserts them.
- Wayland lane D is local-only (weston's headless backend has no seat).
- `gpui_mobile`'s iOS code has 36 pre-existing warnings (missing commas in `msg_send!`, unsafe
  operations in unsafe fns).
