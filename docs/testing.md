# Testing gpux

gpux proves itself in lanes: groups of tests named by where they can run. `gpux-testkit` is the
single entry point:

```text
cargo run -p gpux-testkit -- lane <lane>... [--list] [--scene <id>] [--grep <pattern>]
```

Every run ends with `gpux-testkit: <passed> passed, <failed> failed, <skipped> skipped` and
exits nonzero on any failure or when nothing was selected. Raw output goes to
`artifacts-temp/testkit/<lane>/`. The conformance scenes the lanes share live in
[`crates/gpux_conformance`](../crates/gpux_conformance).

| Lane | What it proves | Command |
|---|---|---|
| A | Behaviour on the test platform: no GPU or display | `gpux-testkit lane a` |
| B | A real browser: the threaded WASM build on WebGPU, driven by Playwright with trusted input | `gpux-testkit lane b` |
| C | Offscreen pixels through the headless renderer, with per-backend goldens | `gpux-testkit lane c` |
| D | Real desktop windows: presenting, resizing, scale changes, clipboard, OS input | `gpux-testkit lane d` |

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
real clock like lane D. Raw output, traces and failure screenshots go to
`artifacts-temp/testkit/b/`.

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
