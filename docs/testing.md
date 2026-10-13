# Testing gpux

gpux proves itself in lanes: groups of tests named by where they can run. `gpux-testkit` is the
single entry point:

```text
cargo run -p gpux-testkit -- lane <lane>... [--list] [--scene <id>] [--grep <pattern>] [--headed]
```

Every run ends with `gpux-testkit: <passed> passed, <failed> failed, <skipped> skipped` and
exits nonzero on any failure or when nothing was selected. `--list` prints the exact selection
without running it. Raw output goes to `artifacts-temp/testkit/<lane>/`. The conformance scenes the
lanes share live in [`crates/gpux_conformance`](../crates/gpux_conformance); each scene names the
lanes it runs in.

gpux needs nothing outside this repository to prove itself. RayX consumes gpux, and a change to
an API RayX uses ships with the matching RayX change, but RayX's tests are not gpux's proof.

## Lanes

| Lane | What it proves | Command | Local host |
|---|---|---|---|
| A | Behaviour on the test platform: layout, input, focus, text, scrolling, semantics; no GPU or display | `gpux-testkit lane a` | any |
| B | A real browser: the threaded WASM build on WebGPU (and forced WebGL2), driven by Playwright with trusted input | `gpux-testkit lane b` | Chromium on the machine's GPU |
| C | Offscreen pixels through the headless renderer, with per-backend goldens | `gpux-testkit lane c` | any: renders on the software adapter |
| D | Real desktop windows: presenting, resizing, scale changes, clipboard, OS input | `gpux-testkit lane d` | a desktop session |
| E | Android on a device or emulator: startup, the lane-E scenes, OS touch, background and resume | `gpux-testkit lane e` | an attached device or emulator |
| F | iOS: startup, wgpu on Metal, CoreText metrics, touch, background and resume, and the lane-F scenes | `gpux-testkit lane f` | a Mac with Xcode: a Simulator, or a device by hand |
| G | The developer's real GPU: lane B's every spec, `@timing` and `@dpr2` included; lanes C and D on the hardware adapter with probes only; the benchmarks | `gpux-testkit lane g [--headed]` | a machine with a GPU; refuses to run under CI |

### CI and cadence

| Lane | Workflow job | Runner | Cadence |
|---|---|---|---|
| A | `ci.yml` lane A | `ubuntu-24.04`, `windows-2025`, `macos-15` | every pull request |
| B | `lane-b.yml` | `ubuntu-24.04`: headless Chromium, WebGPU on SwiftShader (profile `ci-linux-swiftshader`), 4 shards | pull requests: `@canary` and `@pr` at DPR 1; nightly at 03:17 UTC (or a dispatch with `all-specs`): every spec except `@timing`, DPR 2 included |
| C | `ci.yml` lane C | `ubuntu-24.04` (lavapipe), `windows-2025` (WARP), `macos-15` (Metal) | every pull request |
| D | `ci.yml` lane D | `ubuntu-24.04` (X11 under Xvfb, lavapipe), `windows-2025` (WARP), `macos-15` | every pull request |
| E | `lane-e.yml` | `ubuntu-24.04`: KVM-accelerated API 34 x86_64 emulator, SwiftShader GL | nightly at 03:47 UTC, and pull requests that touch `gpui_mobile`, `gpui_wgpu`, `gpux_conformance`, `tooling/android` or `tooling/testkit` |
| F | `lane-f.yml` | `macos-15`: an iPhone 16 Simulator on the newest installed iOS runtime | nightly at 04:07 UTC, and pull requests that touch `gpui_mobile`, `gpui_wgpu`, `gpux_conformance`, `tooling/ios` or `tooling/testkit` |
| G | none: local only | the developer's GPU | before releases, and for renderer, platform or performance changes |

`ci.yml` also runs `check` (format, `cargo check --workspace --all-targets`, Clippy, the web
checks), `test` (`cargo test --workspace` without the conformance crate, one test at a time,
because GPU-backed tests hang DX12 when they create devices concurrently) and `package`
(`cargo package --workspace`) on every pull request.

Timing thresholds live only in lane G: hosted runners have no GPU, and software rasterizers make
frame times meaningless. Scenes and specs that assert real-clock timing are skipped on the
software profiles with the reason.

## Prerequisites

Every host needs the toolchain [`rust-toolchain.toml`](../rust-toolchain.toml) pins (Rust 1.95.0
with Clippy, rustfmt and the wasm32 target). Each lane checks its own prerequisites and names the
command that installs a missing one. Platform setup is in
[`installation-and-usage.md`](installation-and-usage.md).

| Host | Lanes | Needs |
|---|---|---|
| Windows | A, C, D | nothing extra: WARP, the DX12 software adapter, ships with Windows. Visual Studio Build Tools as for any gpux build. |
| Linux | A, C, D | `pkg-config libfreetype6-dev libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-dev libvulkan1 mesa-vulkan-drivers`; lavapipe through `VK_DRIVER_FILES` for C and D; a display (or `xvfb-run`) and `xdotool` for D. [LINUX-WSL.md](../LINUX-WSL.md) sets up WSL. |
| macOS | A, C, D | the Xcode command-line tools; Metal is the GPU. |
| any | B, G | a nightly toolchain with `rust-src` (`nightly-2026-06-23` unless `GPUX_WEB_TOOLCHAIN` names another); `wasm-bindgen-cli` at the version `Cargo.lock` pins; Node.js 22 or later with `pnpm`; in `tools-node`: `pnpm install` and `pnpm exec playwright install chromium`. |
| any | E | `ANDROID_HOME` with the pinned SDK packages, a JDK 21, `cargo-ndk`, the Android Rust targets, and a device or an emulator with hardware virtualization; see [Lane E](#lane-e-android). |
| macOS | F | Xcode with an iOS Simulator runtime (`xcodebuild -downloadPlatform iOS`), XcodeGen (`brew install xcodegen`), and `rustup target add aarch64-apple-ios-sim`. |
| any | G | a hardware GPU, plus everything lanes B, C and D need. |

## Environment variables

| Variable | Read by | Meaning |
|---|---|---|
| `GPUX_WGPU_ADAPTER` | lane C, the benchmarks, headless rendering | `software` (lane C's default), `hardware`, or a substring of an adapter's name; empty picks the best adapter. Windowed renderers keep their own choice. |
| `GPUX_EXPECT_BACKEND` | lanes C and D | The backend every scene must render on: `warp`, `lavapipe`, `metal`, ..., or `hardware` for any GPU. CI and lane G set it, so a missing driver cannot silently move a run to another adapter. |
| `GPUX_UPDATE_SNAPSHOTS` | lane C | `1` writes the active backend's goldens instead of comparing. |
| `GPUX_PROBES_ONLY` | lane C | `1` skips golden comparisons and keeps the relative probes (lane G). |
| `GPUX_ARTIFACTS_DIR` | lanes C and D | Where failure images go instead of `artifacts-temp/testkit/<lane>/`. |
| `VK_DRIVER_FILES` | Linux | Points Vulkan at one driver, for example lavapipe: `/usr/share/vulkan/icd.d/lvp_icd.json` (`lvp_icd.x86_64.json` before Mesa 25.2). A path that does not exist leaves no Vulkan adapter, and rendering falls back to llvmpipe over GL. |
| `GPUX_OS_INPUT` | lane D | `optional` reports the OS-input check skipped when the session delivers no synthetic input (hosted Windows runners). |
| `GPUX_FAST_MODE`, `GPUX_LAYOUT_RETENTION`, `GPUX_VIEW_RETENTION`, `GPUX_SCROLL_LAYERS` | every window | The fast-mode switches ([fast-mode.md](fast-mode.md#switches)), read when a window opens. Fast mode is on by default, so every lane runs with it; `GPUX_FAST_MODE=0` reproduces a failure without it. Tests that compare modes set them with `Window::set_fast_mode`. |
| `ZED_DEVICE_ID` | windowed renderers | Pins a GPU by its PCI device id on machines with several. |
| `GPUX_WEB_PROFILE` | lane B | The Playwright profile `lane b` runs (`local-gpu` by default). |
| `GPUX_WEB_TOOLCHAIN` | lanes B and G | The nightly toolchain of the threaded WASM build (default `nightly-2026-06-23`). |
| `ANDROID_HOME`, `ANDROID_SERIAL` | lane E | The SDK, and the device to run on. |
| `CI` | lane G | Lane G refuses to run when it is set. |
| `SEED`, `ITERATIONS`, `BENCH_MEASUREMENT` | `#[gpux::test]`, `#[gpux::bench]` | Seeds and iterations of randomized tests and benchmarks, and what benchmarks measure (`wall-time`, `instructions`, `allocations`, ...). |

Fast mode is on in every lane, because it is on by default. What proves it is the groups that
compare it with itself off: the oracle and counter groups in lane A (`fast::tests::*`,
`lane_a_fast_parity`, `fast_mode_attribution`), `lane_c_fast_parity` and the scroll-layer groups
in lane C, the `scroll-layers` specs in lane B and the scroll-layer scenes of lanes E and F; the
timings are lane G's (`gpux-testkit perf fast-mode`, [fast-mode.md](fast-mode.md#measuring)).
What a new window gets with the defaults on, and what each variable turns off, is
`cargo test -p gpux --features test-support --lib fast::tests::defaults::`.

## Goldens

Lane C compares a curated subset of scenes (those with `golden: true`) with committed PNGs per
software backend, in `crates/gpux_conformance/tests/goldens/<backend>/`: `lavapipe`, `warp` and
`metal`. A golden is at most 32 KiB, all of them together at most 2 MiB, and the published package
leaves them out (`cargo test -p gpux-conformance --test golden_policy` checks this). Hardware GPUs
never get goldens: lane C skips golden steps on them, and lane G switches goldens off everywhere.

To update goldens after an intended rendering change:

1. Run lane C on your platform's backend with `GPUX_UPDATE_SNAPSHOTS=1`
   (`GPUX_UPDATE_SNAPSHOTS=1 cargo run -p gpux-testkit -- lane c`): WARP on Windows, lavapipe on
   Linux with `VK_DRIVER_FILES` set (WSL works), Metal on macOS.
2. For the backends you cannot run, dispatch the `Gpux` workflow with `update-goldens` checked.
   Each lane C job uploads its backend's regenerated goldens as `lane-c-<backend>-goldens`;
   download them into `crates/gpux_conformance/tests/goldens/<backend>/`.
3. Review every changed PNG before committing. A failing comparison writes the actual, expected
   and diff images under `artifacts-temp/testkit/c/`.

## Lane A: behaviour

`gpux-testkit lane a` runs the lane-A conformance scenes on `TestPlatform` with the bundled
fonts and the deterministic executor clock, plus every `gpux-conformance` test group that needs
no renderer: the catalog, image comparison, golden policy, the executor clock, host capabilities,
RGB565 tolerance and screen probes (`LANE_A_GROUPS` in `tooling/testkit/src/lanes.rs`). No GPU or
display is involved, so lane A runs identically everywhere. The crates' own focused tests
(`cargo test -p <package> <filter>`) are lane A too.

## Lane B: a real browser

Lane B builds the `web_host` example of `gpux-conformance` as a threaded WASM app and drives it
in Chromium with Playwright, from [`tools-node`](../tools-node):

```text
cargo run -p gpux-testkit -- web build [--verify]   # target/gpux-web/dist
cargo run -p gpux-testkit -- web serve [--port 8000] # cross-origin isolated; open /?scene=<id>
cargo run -p gpux-testkit -- web e2e [--profile local-gpu] [--grep <pattern>] [--headed]
cargo run -p gpux-testkit -- lane b [--list] [--scene <id>] [--grep <pattern>]
```

`lane b` runs the `@canary` and `@pr` specs in the profile `GPUX_WEB_PROFILE` names
(`local-gpu` by default); `web e2e` runs any selection. Both build first and fail with an
actionable message when a prerequisite is missing:

- a nightly toolchain with `rust-src` and the wasm32 target (`nightly-2026-06-23` unless
  `GPUX_WEB_TOOLCHAIN` names another), because the threaded build rebuilds `std` with atomics;
- the `wasm-bindgen` CLI at the version `Cargo.lock` pins
  (`cargo install wasm-bindgen-cli --version <version>`);
- `pnpm`, and in `tools-node`: `pnpm install` and `pnpm exec playwright install chromium`.

`web build --verify` checks the binary: shared memory, the `wasm_thread_entry_point` export, and
a single explicit constructor call (`gpui_platform::web_init`), so wasm-ld added no
`.command_export` wrappers that rerun constructors on every worker.

The specs:

- `scenes.spec.ts` runs every lane-B scene's own script with trusted CDP input (mouse, wheel,
  keyboard, touch, IME). The app checks the script's expectations, and its pixel steps against a
  screenshot of the page (`@pr` at DPR 1, `@dpr2` at DPR 2).
- One spec per `gpui_web` area covers the rest: backend fallback, viewport and resize, window
  close, pointer buttons and hover, wheel delta modes, keyboard repeat and AltGr, touch, IME,
  clipboard and its fallback, HTML elements and islands, canvas text fallback, worker threads,
  fetched assets, cursors, page lifecycle, safe areas, and the shortcut modifier
  (`shortcut-modifier.spec.ts`: Command in Apple browsers and Control elsewhere, presented as a
  Mac and as a Windows browser, so it answers the same on every host). Browser-only scenes for these live in
  `web_host`, because the shared catalog runs every scene in lane A.
- `scroll-layers.spec.ts` scrolls a `div` and a `list` of cards (`web-scroll-layers`) with trusted
  wheel input on WebGPU and on forced WebGL2, with every fast-mode mechanism (`?layers=1`) and
  with none (`?layers=0`). It reads `__gpuxFastStats` (published under `diagnostics`, which the
  testkit builds the host with) for the frames composited from tiles and compares the pixels of
  the two pages byte for byte (`@pr` at DPR 1, `@dpr2` at DPR 2); a scroller holding a DOM
  element and one holding an HTML island are drawn without tiles; its `@timing` tests scroll the cards and the list on WebGPU and on WebGL2 with
  layers on and off, print each p95 of gpux's frame work (`gpux-timing:` lines, which lane G copies into its
  summary) and hold the one from tiles under the 60 Hz budget, in lane G only. Select its other tests with
  `--grep "scroll-layers: "`.
- `dawn.spec.ts` renders every scene and fails on any WebGPU validation error. Every spec also
  fails on console errors, uncaught exceptions and crashes, and checks that the page is
  cross-origin isolated and renders through WebGPU unless a spec forces WebGL2.

| Profile | Where | Chromium |
|---|---|---|
| `local-gpu` | A developer machine with a GPU | new headless (`channel: 'chromium'`) on the real GPU |
| `ci-linux-swiftshader` | GitHub's ubuntu runners: no GPU, no display | WebGPU and ANGLE on SwiftShader |
| `windows-software` | Windows without a usable GPU | WebGPU on WARP |

Scenes asserting exact animation timing or fling distances stay out of lane B, which runs on the
real clock like lane D. Specs tagged `@timing` assert frame budgets; only lane G runs them. Raw
output, traces and failure screenshots go to `artifacts-temp/testkit/b/`.

### Chromium flags and troubleshooting

Every profile passes `--enable-blink-features=CanvasDrawElement`, which the HTML-island specs
need, and keeps the browser's own user agent: Playwright's desktop Chrome device would report
Windows on every host, and gpux follows the browser's keyboard conventions (Command as the
secondary modifier on a Mac), so specs press the host's modifier. Beyond that:

- `local-gpu` uses new headless Chromium (`channel: 'chromium'`) with `--enable-unsafe-webgpu`.
  On Windows the old headless shell gets no WebGPU adapter (its ANGLE runs on SwiftShader), so
  apps fall back to WebGL2; the specs' backend check catches that.
- `ci-linux-swiftshader` needs `--enable-features=Vulkan --use-angle=swiftshader
  --use-vulkan=swiftshader`; without either SwiftShader flag the canvas stays blank ("device
  lost: Destroyed"). Chromium keeps only the last `--enable-features`, so the profile repeats
  Playwright's own `CDPScreenshotNewSurface` there.
- `windows-software` uses `--use-angle=d3d11-warp`.

When a spec fails before it runs anything:

- *the page must be cross-origin isolated*: the build was served without the COOP, COEP and
  CORP headers that `SharedArrayBuffer` needs; serve it with `web serve` or run `web e2e`.
- *the scene must render through webgpu*: Chromium found no WebGPU adapter and the app fell back
  to WebGL2. Check the profile's flags and the GPU blocklist; `?backend=webgpu` forces WebGPU
  (no fallback) and `?backend=webgl` forces WebGL2 when opening a scene by hand.
- *no catalog.json*: the specs enumerate scenes from the build; run `web build` first.
- `net::ERR_NO_BUFFER_SPACE` on Windows: the system ran out of socket buffers during many
  parallel page loads. Rerun the spec.
- A headed browser whose window is hidden pauses `requestAnimationFrame`, and with it gpux's frame
  loop; keep it visible or run headless.

## Lane C: offscreen pixels

`gpux-testkit lane c` renders every lane-C scene at scale 1 and 2 through the platform's headless
renderer: wgpu on the software adapter on Windows (WARP) and Linux (lavapipe), Metal on macOS.
Scenes check relative probes (a color at a point, a uniform region, two regions that differ, a
region that changed since a capture), and the golden subset is compared with the committed
goldens of the backend. Scenes render one at a time: they share one cached device, which keeps
DX12 stable. The full run also runs the `lane_g` group, which checks lane G's probes-only mode
on the same renderer, `lane_c_fast_parity`, which renders every scene with fast mode on and off
and compares the pixels, and three groups that compare every frame of a window drawing from
the renderer's tiles with a window drawn without scroll layers: `lane_c_scroll_layers` scrolls
views at whole scale factors, `lane_c_layer_scales` at 1.25, 1.5 and 1.75 and with content
painted transformed, and `lane_c_list_hover` moves the pointer over lists whose rows have a
hover style. Without a Vulkan adapter, Linux falls back to llvmpipe over GL, where the
renderer's effects pipeline does not validate yet, so lane C fails there: select lavapipe with
`VK_DRIVER_FILES`.

## Lane D: real desktop windows

`gpux-testkit lane d` runs the `desktop_host` example of `gpux-conformance` on
`gpui_platform::application()`. It runs the platform checks
(`gpux_conformance::platform_checks`) and every lane-D scene in real windows, with input
injected into the window, and pixels read back from the window's own surface with
`render_to_image`. The `platform-os-input` check drives the real OS input path instead:

| Platform | OS input | Scale change | Notes |
|---|---|---|---|
| Windows | `SendInput` | `WM_DPICHANGED` sent to the window | Hosted CI sessions run with `GPUX_OS_INPUT=optional`, which reports the OS-input check skipped if the session delivers no synthetic input. |
| Linux X11 | XTest through `xdotool` | skipped: the X server sets the scale | Run under `xvfb-run -a -s "-screen 0 1280x1024x24"` with `WAYLAND_DISPLAY` unset; needs `xdotool`. Select lavapipe with `VK_DRIVER_FILES`. |
| Linux Wayland | none: Wayland gives clients no synthetic input | skipped: the compositor sets the scale | Informational only, see below. |
| macOS | none: hosted macOS grants no accessibility permission for posting events | skipped: the display sets the scale | Scenes inject input in-app. The host comes to the front and moves the cursor to every injected pointer position (`CGWarpMouseCursorPosition` needs no permission): the active window re-reads the cursor every frame. |

Local Windows desktops need nothing extra; the run briefly opens windows, moves the cursor, and
types into its own text field. On a local Mac, keep the display awake for the run
(`caffeinate -d cargo run -p gpux-testkit -- lane d`) and leave the windows uncovered: macOS
stops a window's frames while it is hidden or the display sleeps, and the scenes then fail with
"no frame within 10s". Scenes that assert exact animation timing (programmatic scroll,
animated and transition frames) and IME composition stay in lanes A and C: lane D runs on the
real clock, and a host cannot drive the platform's input method.

### Wayland

Wayland is local-only. CI tried `weston --backend=headless` (run 37599450308): the desktop host
stopped at startup with "Wayland compositor does not provide wl_seat", because weston's headless
backend has no seat and gpux's Wayland client requires one. Locally, WSLg's compositor runs
lane D, and two checks cannot pass there for reasons outside gpux:

- **Clipboard.** A Wayland client may set the selection only with the serial of a recent input
  event. In-app injection produces none, so the compositor keeps the previous selection.
- **Hover.** The host cannot move the cursor away from scene windows, so a scene can start
  hovered wherever the real cursor rests (under WSLg, the Windows cursor).

Everything else, including surface capture, resizing and in-app scene input, passes on
WSLg's compositor.

## Lane E: Android

`gpux-testkit lane e` (or `android run [--serial <serial>] [--scene <id>] [--grep <pattern>]`)
builds the `android_host` example of `gpux-conformance` for the device's ABI, installs it on the
first attached device or emulator (`ANDROID_SERIAL` or `--serial` picks one), launches it and
follows logcat. The host runs the startup check, every lane-E scene, and the checks that need the
operating system, which it asks the testkit to perform: taps and swipes through
`adb shell input`, the home key for going to the background, and `am start` for coming back once
the host saw the system take its surface. Results, screenshots, logcat and the frames failing
probes saved land in `artifacts-temp/testkit/e/`.

`gpux-testkit android build [--abi x86_64|arm64-v8a]...` builds the APK on its own. It needs no
Gradle: the host is a NativeActivity whose only Java is gpux's own helpers
(`crates/gpui_mobile/java`, window composition and platform views), which `javac` and `d8` compile
against the pinned `android.jar` into `classes.dex`; `aapt2` links
[`tooling/android/AndroidManifest.xml`](../tooling/android/AndroidManifest.xml), the libraries go in
uncompressed and 16 KB aligned, and `apksigner` signs with a local debug key. Prerequisites, each
reported with the command that installs it:

- `ANDROID_HOME` with the pinned packages: `sdkmanager "ndk;28.0.12674087" "build-tools;36.0.0"
  "platforms;android-36" "platform-tools"`;
- a JDK 21 (`java`, `javac`, `keytool`) and `cargo install cargo-ndk`;
- `rustup target add x86_64-linux-android aarch64-linux-android`.

Without a device, create an emulator like CI's: API 34 with Google APIs and the small `Nexus 4`
screen, rendering through SwiftShader. The host is built for whatever ABI the emulator reports.

```bash
sdkmanager "emulator" "system-images;android-34;google_apis;x86_64"
avdmanager create avd --name gpux-api34 --device "Nexus 4" \
  --package "system-images;android-34;google_apis;x86_64"
emulator -avd gpux-api34 -gpu swiftshader_indirect -no-snapshot -no-boot-anim
```

The emulator needs hardware virtualization: KVM on Linux (`/dev/kvm`), the Windows Hypervisor
Platform or the Android Emulator hypervisor driver on Windows, and the Hypervisor framework on
macOS. On Apple silicon, use the `arm64-v8a` system image instead of `x86_64`. `-gpu host`
renders faster on a machine with a GPU, but CI proves the SwiftShader path.

Pixel steps read the screen: the host asks for `adb screencap`, which reaches it through
`/data/local/tmp` and `run-as`. The emulators' GL drivers read rendered textures back empty, so
the renderer's own `render_to_image` (which the Android window implements) cannot serve them
there; the startup check reports whether it works on the device. Emulators render through
SwiftShader and run on the GL fallback, whose surface has 16-bit color (RGB565): on frames that
show it, color probes allow its rounding of up to 4 levels per channel, while difference checks
keep their tolerance. Shrinking the emulator's screen (`adb shell wm size 720x1280`,
`adb shell wm density 320`) keeps software frames fast.

On Apple silicon the arm64 emulator exposes a software SwiftShader Vulkan beside its GL driver.
gpux skips software Vulkan on Android and gives each backend its own surface (Vulkan, then GL),
because a Vulkan surface keeps the window connected until it is destroyed; the scenes then run
on GL. `adb logcat` can drop lines from its stream while adb transfers a screenshot, so the host
repeats a screenshot request until the file arrives, and the testkit counts results from the
device's log buffer (`adb logcat -d`) when it kept more of them than the stream. Checks that read
the screen after the app starts or returns capture again until the system's window animation
(fade or scale in) has ended.

The `composition-android-*` and `platform-view-android*` checks open a composed window with a
native `View` and read the result as the others do: the screen for what shows where, taps and a
swipe for who takes the touch, and the process's own window list
(`WindowInspector.getGlobalWindowViews`) for which panel windows exist. The emulators render
through GL, where the overlay is cut out of native views, so `composition-android-shadow` skips
there; it runs where gpux renders through Vulkan. Lane E builds the NativeActivity host
(`android-native-activity`); the embedded host is `gpux-mobile`'s default feature set, which the
lane E workflow type-checks (`cargo check -p gpux-mobile --target x86_64-linux-android`) and no
lane runs.

The `scroll-layers-android-div`, `-list`, `-resume` and `-timing` checks scroll the pages of
`gpux_conformance::scroll_layers` with every fast-mode mechanism and with none: touch pans
injected into the window and a swipe through `adb shell input swipe` (a slow one on a software
renderer, as above) must come from tiles in most of their frames; wheel steps of whole device
pixels must show the same screen pixels, byte for byte, in the two windows; and the resume check
tells apart the releases a device lets it tell apart, each followed by the renderer rasterizing
the tiles again: `adb shell am send-trim-memory` (`onTrimMemory`, which the host's
`GpuxNativeActivity` forwards) and the pause the home key brings (the release is seen while the
surface is still there). The surface is taken after that, with a trim of the system's own, so
all the check shows of the surface loss is an empty cache once it is gone: `term_window`'s own
release is by inspection, as is the embedded host's `trim_memory`. The host reports the counters in the result
line, so the build carries `diagnostics`. The checks run on the GL fallback, which an emulator
offers by default, and again with `GPUX_ANDROID_SOFTWARE_VULKAN=1`, which lets the host pick the
emulator's SwiftShader Vulkan (gpux skips it otherwise) and makes each check fail unless its
window renders through Vulkan.

`scroll-layers-android-timing` scrolls each page down and up four times with layers on and off and
reports the median, 95th percentile and slowest frame work (drawing and presenting a frame, from the
window's `take_frame_work_micros`, which `diagnostics` compiles) and how many frames came from tiles,
as `gpux-timing:` lines in its result. It holds the counters (frames from tiles with layers on, none
with them off), not the times: an emulator's are of the machine it runs on.

```bash
GPUX_ANDROID_SOFTWARE_VULKAN=1 cargo run -p gpux-testkit -- lane e --grep scroll-layers
```

Scenes that need exact animation timing or an input method stay out of lane E: the device runs on
the real clock, and adb cannot drive the input method. `adb shell input swipe` injects each event
once the app handled the one before and sends only the moves its duration has time left for, and
the app handles input between frames: a swipe shorter than a frame arrives as a press and a
release, a tap. The pan check therefore swipes for at least eight of the window's frames (600 ms
on a GPU, several seconds on SwiftShader), and the fling check, which needs a fast swipe, is
skipped on software renderers.

## Lane F: iOS

`gpux-testkit lane f` (or `ios run --simulator [--scene <id>] [--grep <pattern>]`) builds the
`ios_host` example of `gpux-conformance` as a static library for the Simulator, generates the
Xcode project of [`tooling/ios`](../tooling/ios) with XcodeGen, and builds the app with
`xcodebuild`, unsigned; `ios build --simulator` stops there. It then boots its own Simulator,
`gpux-conformance`, created on first use as an iPhone 16 on the newest installed iOS runtime,
installs the app, and launches it with `simctl launch --console-pty`, reading the result lines from
its console. The host runs its platform checks and every lane-F scene:

- `ios-startup`: a window opens and draws;
- `ios-metal`: wgpu renders through Metal and reads its frames back;
- `ios-text-metrics`: CoreText measures the bundled IBM Plex Sans as its font file declares
  (ascent, descent and advances at 16 px), and shapes a line to its advances within kerning;
- `ios-touch-tap`: a touch injected into the window taps the element under it;
- `ios-lifecycle`: the testkit brings Settings to the front and, once the host saw its window
  deactivate, launches the host again, which brings the running one back; the window reactivates
  and draws again. (Reopening it through a URL of its own makes a fresh Simulator ask
  "Open in …?" and wait for a tap.)

The `scroll-layers-ios-div`, `-list`, `-resume` and `-timing` checks do the same on Metal (the
timing check as `scroll-layers-android-timing` does): touch pans
injected into the window come from tiles, wheel steps of whole device pixels show the same pixels
with every mechanism and with none, and a page whose app received a memory warning
(`gpui_ios_did_receive_memory_warning`, which the shell's `applicationDidReceiveMemoryWarning:`
calls) and then went to the background and came back gave its tiles back each time and
rasterized them again. They skip where the Simulator has no Metal device.

Pixel steps read frames back through the window's wgpu renderer. The Simulator on Apple silicon
offers Metal (lane F ran 30 of 30 on an M5 with iOS 26.5, Xcode 26.4). A Simulator without a Metal
device leaves the window without a renderer: `ios-metal` and the pixel steps then report themselves
skipped with that reason, and everything else still runs. The console log and the frames failing
pixel steps saved (in the app's `Documents/frames/`) land in `artifacts-temp/testkit/f/`. A host that
panics or that dyld cannot load ends the run at once with that line as the failure.

The shell force-loads the whole Rust library, including io-surface's OpenGL texture binding, which
gpux reaches through core-video on every Apple target; its CGL functions exist only on macOS, so
`tooling/ios/Sources/macos_gl_stubs.c` defines them (aborting, never called). Undefined-symbol
flags (`-Wl,-U`) are not enough: dyld binds them at launch and the app never starts.

### On a device

Devices need a signing team, so the testkit leaves them to you:

1. `cargo build -p gpux-conformance --no-default-features --features app-host,diagnostics
   --example ios_host --target aarch64-apple-ios` (after `rustup target add aarch64-apple-ios`).
2. `xcodegen generate --spec tooling/ios/project.yml`.
3. `xcodebuild -project tooling/ios/GpuxConformance.xcodeproj -target GpuxConformance
   -configuration Debug -sdk iphoneos -allowProvisioningUpdates DEVELOPMENT_TEAM=<team id>
   GPUX_IOS_HOST_LIB=$PWD/target/aarch64-apple-ios/debug/examples/libios_host.a
   SYMROOT=$PWD/target/gpux-ios/products build`.
4. `xcrun devicectl list devices`, then `xcrun devicectl device install app --device <device>
   target/gpux-ios/products/Debug-iphoneos/GpuxConformance.app`.
5. `xcrun devicectl device process launch --console --device <device> dev.gpux.conformance`
   prints the result lines; `--scene <id>` or `--grep <pattern>` after the bundle id narrows the
   run. When the host prints `gpux-conformance-host: background`, go to the home screen and open
   the app again.

## Lane G: the local real GPU

`gpux-testkit lane g` is the only lane that measures time and the only one that needs a GPU. It
runs on the developer's machine and refuses to run when `CI` is set:

1. **Lane B in the `local-gpu` profile**, every spec: `@canary`, `@pr`, `@dpr2` and the `@timing`
   specs no other profile runs. `--headed` shows the browser. `timing.spec.ts` scrolls a
   virtualized list and requires the 95th percentile of gpux's frame work (layout, paint and
   submission in its animation-frame callback) to stay inside the 60 Hz frame budget.
2. **Lane C on the hardware adapter** (`GPUX_WGPU_ADAPTER=hardware`, Metal on macOS) with
   `GPUX_PROBES_ONLY=1` and `GPUX_EXPECT_BACKEND=hardware`: every probe runs, no golden is
   compared, and a scene that lands on a software rasterizer fails.
3. **Lane D on the GPU**: windows already prefer the hardware adapter; the desktop host's
   `desktop-backend` result fails when they render on a software rasterizer instead (macOS
   windows name no adapter and report it skipped).
4. **The benchmarks**: `cargo bench -p gpux-platform --features bench-support` runs the
   `#[gpux::bench]` Criterion suites `randomized_element_tree`, `inspector_render` and `fast_mode`
   (every fast-mode scenario in every cumulative mode, macOS only; `gpux-testkit perf fast-mode`
   runs it alone and can `--compare` an earlier run) on the GPU.
   Benchmarks run on a thread with an 8 MiB stack on every platform, so deep trees fit on
   Windows too. They run only for the whole selection, not with `--scene` or `--grep`.

Lane G writes `artifacts-temp/testkit/g/fast-mode.md` (the `fast_mode` report) and `artifacts-temp/testkit/g/timing.md`: each part's results and duration, the GPU
the windows rendered on, the `@timing` specs' measurements (the `gpux-timing:` lines they print),
and every benchmark's estimate with its confidence interval. Raw logs stay in
`artifacts-temp/testkit/g/` (lanes C and D, the benchmarks) and `artifacts-temp/testkit/b/` (the
browser specs). Lane G is the proof for renderer, platform and performance changes before a
release; compare `timing.md` with a run of the base commit to judge a performance change.
