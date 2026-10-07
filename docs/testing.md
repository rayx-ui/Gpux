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
| A | `ci.yml` lane A | `ubuntu-24.04`, `windows-2025`, `macos-15` | every pull request and push to `main` |
| B | `lane-b.yml` | `ubuntu-24.04`: headless Chromium, WebGPU on SwiftShader (profile `ci-linux-swiftshader`), 4 shards | pull requests and pushes: `@canary` and `@pr` at DPR 1; nightly at 03:17 UTC (or a dispatch with `all-specs`): every spec except `@timing`, DPR 2 included |
| C | `ci.yml` lane C | `ubuntu-24.04` (lavapipe), `windows-2025` (WARP), `macos-15` (Metal) | every pull request and push to `main` |
| D | `ci.yml` lane D | `ubuntu-24.04` (X11 under Xvfb, lavapipe), `windows-2025` (WARP), `macos-15` | every pull request and push to `main` |
| E | `lane-e.yml` | `ubuntu-24.04`: KVM-accelerated API 34 x86_64 emulator, SwiftShader GL | nightly at 03:47 UTC, pushes to `main`, and pull requests that touch `gpui_mobile`, `gpui_wgpu`, `gpux_conformance`, `tooling/android` or `tooling/testkit` |
| F | `lane-f.yml` | `macos-15`: an iPhone 16 Simulator on the newest installed iOS runtime | nightly at 04:07 UTC, pushes to `main`, and pull requests that touch `gpui_mobile`, `gpui_wgpu`, `gpux_conformance`, `tooling/ios` or `tooling/testkit` |
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
| any | B, G | a nightly toolchain with `rust-src` (`GPUX_WEB_TOOLCHAIN` overrides `nightly`); `wasm-bindgen-cli` at the version `Cargo.lock` pins; Node.js 22 or later with `pnpm`; in `tools-node`: `pnpm install` and `pnpm exec playwright install chromium`. |
| any | E | `ANDROID_HOME` with the pinned SDK packages, a JDK 17 or later, `cargo-ndk`, the Android Rust targets, and a device or an emulator with hardware virtualization; see [Lane E](#lane-e-android). |
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
| `ZED_DEVICE_ID` | windowed renderers | Pins a GPU by its PCI device id on machines with several. |
| `GPUX_WEB_PROFILE` | lane B | The Playwright profile `lane b` runs (`local-gpu` by default). |
| `GPUX_WEB_TOOLCHAIN` | lanes B and G | The nightly toolchain of the threaded WASM build. |
| `ANDROID_HOME`, `ANDROID_SERIAL` | lane E | The SDK, and the device to run on. |
| `CI` | lane G | Lane G refuses to run when it is set. |
| `SEED`, `ITERATIONS`, `BENCH_MEASUREMENT` | `#[gpux::test]`, `#[gpux::bench]` | Seeds and iterations of randomized tests and benchmarks, and what benchmarks measure (`wall-time`, `instructions`, `allocations`, ...). |

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
fonts and the deterministic executor clock, plus the catalog, image-comparison and golden-policy
groups. No GPU or display is involved, so lane A runs identically everywhere. The crates' own
focused tests (`cargo test -p <package> <filter>`) are lane A too.

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

- a nightly toolchain with `rust-src` and the wasm32 target (`GPUX_WEB_TOOLCHAIN` overrides
  `nightly`), because the threaded build rebuilds `std` with atomics;
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
  fetched assets, cursors, page lifecycle and safe areas. Browser-only scenes for these live in
  `web_host`, because the shared catalog runs every scene in lane A.
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
need. Beyond that:

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
renderer: wgpu on the software adapter on Windows (WARP) and Linux (lavapipe, or llvmpipe on GL),
Metal on macOS. Scenes check relative probes (a color at a point, a uniform region, two regions
that differ, a region that changed since a capture), and the golden subset is compared with the
committed goldens of the backend. Scenes render one at a time: they share one cached device,
which keeps DX12 stable.

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
| macOS | none: hosted macOS grants no accessibility permission for posting events | skipped: the display sets the scale | Scenes inject input in-app. |

Local Windows desktops need nothing extra; the run briefly opens windows, moves the cursor, and
types into its own text field. Scenes that assert exact animation timing (programmatic scroll,
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
`adb shell input`, and the home key followed by `am start` for going to the background and coming
back. Results, screenshots, logcat and the frames failing probes saved land in
`artifacts-temp/testkit/e/`.

`gpux-testkit android build [--abi x86_64|arm64-v8a]...` builds the APK on its own. It needs no
Gradle: the host is a NativeActivity without Java code, so `aapt2` links
[`tooling/android/AndroidManifest.xml`](../tooling/android/AndroidManifest.xml), the libraries go in
uncompressed and 16 KB aligned, and `apksigner` signs with a local debug key. Prerequisites, each
reported with the command that installs it:

- `ANDROID_HOME` with the pinned packages: `sdkmanager "ndk;28.0.12674087" "build-tools;36.0.0"
  "platforms;android-36" "platform-tools"`;
- a JDK 17 or later (`java`, `keytool`) and `cargo install cargo-ndk`;
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

Scenes that need exact animation timing or an input method stay out of lane E: the device runs on
the real clock, and adb cannot drive the input method. The fling check is skipped on software
renderers, where `adb shell input` delivers a fast swipe only after slow frames, as a tap.

## Lane F: iOS

`gpux-testkit lane f` (or `ios run --simulator [--scene <id>] [--grep <pattern>]`) builds the
`ios_host` example of `gpux-conformance` as a static library for the Simulator, generates the
Xcode project of [`tooling/ios`](../tooling/ios) with XcodeGen, and builds the app with
`xcodebuild`, unsigned; `ios build --simulator` stops there. It then boots its own Simulator,
`gpux-conformance`, created on first use as an iPhone 16 on the newest installed iOS runtime,
installs the app, and launches it with `simctl launch --console-pty`, reading the result lines from
its console. The host runs five platform checks and every lane-F scene:

- `ios-startup`: a window opens and draws;
- `ios-metal`: wgpu renders through Metal and reads its frames back;
- `ios-text-metrics`: CoreText measures the bundled IBM Plex Sans as its font file declares
  (ascent, descent and advances at 16 px), and shapes a line to its advances within kerning;
- `ios-touch-tap`: a touch injected into the window taps the element under it;
- `ios-lifecycle`: the testkit brings Settings to the front and reopens the host through its
  `gpux-conformance://` URL; the window deactivates, reactivates and draws again.

Pixel steps read frames back through the window's wgpu renderer. A Simulator without a Metal
device leaves the window without a renderer: `ios-metal` and the pixel steps then report themselves
skipped with that reason, and everything else still runs. The console log and the frames failing
pixel steps saved (in the app's `Documents/frames/`) land in `artifacts-temp/testkit/f/`.

### On a device

Devices need a signing team, so the testkit leaves them to you:

1. `cargo build -p gpux-conformance --no-default-features --features app-host --example ios_host
   --target aarch64-apple-ios` (after `rustup target add aarch64-apple-ios`).
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
   `#[gpux::bench]` Criterion suites `randomized_element_tree` and `inspector_render` on the GPU.
   Benchmarks run on a thread with an 8 MiB stack on every platform, so deep trees fit on
   Windows too. They run only for the whole selection, not with `--scene` or `--grep`.

Lane G writes `artifacts-temp/testkit/g/timing.md`: each part's results and duration, the GPU
the windows rendered on, the `@timing` specs' measurements (the `gpux-timing:` lines they print),
and every benchmark's estimate with its confidence interval. Raw logs stay in
`artifacts-temp/testkit/g/` (lanes C and D, the benchmarks) and `artifacts-temp/testkit/b/` (the
browser specs). Lane G is the proof for renderer, platform and performance changes before a
release; compare `timing.md` with a run of the base commit to judge a performance change.
