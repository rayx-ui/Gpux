---
agentx:
  kind: rules
  scope: knowledge/platform/web
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-11T14:00:00+02:00
  topics:
    - wasm
    - multithreading
    - browser-window
    - clipboard
    - wasm-assets
    - html-island
    - html-element
    - keyboard
  supersedes: []
  superseded_by: []
---
# Web Rules

- Route threaded browser apps through `gpui_platform::web_init` once before GPUI use and keep its explicit `__wasm_call_ctors` call. After toolchain changes, disassemble the final wasm-bindgen module: the constructor call belongs in `web_init`, not in wrappers on every export.
- Keep `gpui_web` default features enabled for browser apps and request `multithreaded` explicitly at app dependency sites; use `gpui_platform::application()`, never `single_threaded_web()`, unless a task proves single-threaded output is required.
- Serve threaded WASM with COOP/COEP/CORP headers and verify both the generated binary (`gpux-testkit web build --verify`) and the lane B specs that exercise the change before calling browser work complete.
- Keep default window bounds tied to the visible viewport and initialize the backing store as logical size × `devicePixelRatio` before the first frame.
- On browser-window teardown, cancel pending animation frames and disconnect observers before retained closures drop, and guard callbacks with closed state.
- Keep clipboard writes on the trusted action stack, pairing the async Clipboard API with the synchronous one-shot `copy` fallback; never focus a temporary input, whose focus events can re-enter GPUI.
- Handle browser paste on the trusted DOM `paste` event; do not rely on async clipboard reads or keydown-only paths. Keep the paste shortcut (Ctrl+V, or Cmd+V on macOS) out of GPUI keydown dispatch: Zed's keydown handler prevents the default of every keystroke GPUI handles, which suppresses the trusted `paste` event.
- Keep the IME edit import (`register_input`) from diffing into the stored element selection: the import always replaces the whole app selection, so a prefix or suffix shared with the selected text must stay inside the replacement. Upstream Zed (through d544470d1d) lacks this cap; remove it only when upstream fixes the import.
- Keep WASM asset futures local and foreground-spawned; do not add `Send` back to the WASM `Asset::load` signature. Use `Svg::external_path(...)` for browser-served SVG and `Svg::path(...)` for native asset paths.
- Keep HTML island roots as direct children of the real canvas when layout subtree is enabled; never create scratch canvases, cloned DOM, proxy roots, staging surfaces, or fallback upload paths.
- Gate island uploads on browser paint readiness and treat "No cached paint record for element" as a retry deferral, not a failure. Without layout subtree support, create no island roots or uploads.
- Keep island and element diagnostics behind `diagnostics`; diagnostics must never retain content-derived samples. Publish browser globals as `__gpux<Name>` through `gpui_wgpu::web_globals`, which still reads the legacy `__rayx<Name>` names and publishes them under the `rayx_diagnostics` alias. `web_globals::publish` exists only with `diagnostics`, so production builds cannot publish globals.
- Use `html_element(...)`, not `html_island(...)`, for provider-owned iframes that must stay interactive DOM. Give elements stable semantic ids and use `keep_alive(true)` only for provider sessions that must survive temporary absence.
- Size `html_element(...)` with ordinary `Styled` APIs and use `extra_styles(...)` only for trusted root decoration, never for runtime geometry, transform, visibility, opacity, or pointer behavior.
- Add `credentialless` to third-party iframe markup in threaded pages unless the provider already satisfies the page's cross-origin isolation policy.
- Decide keyboard conventions (the secondary modifier, keystroke names and symbols, the `os` key context) through `KeyboardOs::current()`, never `cfg!(target_os = ...)`: a wasm32 build says nothing about the visitor's keyboard. Lane B specs press the host's modifier, so the Playwright config must keep the browser's own user agent.
- Keep `gpux-testkit`'s `DEFAULT_TOOLCHAIN` (`nightly-2026-06-23`) equal to `lane-b.yml`'s `GPUX_WEB_TOOLCHAIN` and to RayX's `web-toolchain` pin; move all of them together using RayX's `.claude/skills/rayx/references/wasm-toolchain-update.md`. Lane E and the testkit's Android prerequisite use JDK 21.
- Run lane B with Node 22 or later first on PATH (`/opt/homebrew/opt/node@22/bin` on the macOS dev machine, where nvm puts Node 17 first); Playwright refuses older Node.
- Keep GPUI's own helper elements above the canvas (the IME mirror textarea) `pointer-events: none`: a hit-testable one takes the pointer where it sits, which GPUI sees as the pointer leaving the window.
- Treat a browser window's canvas and its composition input shields as one pointer target: enter and leave between them must not reach GPUI as window hover changes.
- Select the scroll-layers specs by title (`--grep "scroll-layers: "`): `scroll-layers` also matches the spec's file name and would run its `@timing` test, which only lane G runs. Scale a synthesized wheel delta by `devicePixelRatio`: Chromium divides it by the ratio.
