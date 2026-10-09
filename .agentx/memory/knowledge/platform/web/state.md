---
agentx:
  kind: state
  scope: knowledge/platform/web
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-09T17:00:00+02:00
  topics:
    - gpui-web
    - wasm
    - multithreading
    - wasm-constructors
    - browser-window
    - dpr
    - clipboard
    - wasm-assets
    - html-island
    - html-element
  supersedes: []
  superseded_by: []
---
# Web State

## Purpose And Usage

Read before changing browser startup, `WebWindow`, browser input/clipboard, WASM asset loading, HTML islands, or HTML elements. Threaded app setup is documented in the gpux skill's [multithreaded WASM apps](../../../../../.claude/skills/gpux/references/multithreaded-wasm-apps.md) reference.

## Current Technical Shape

`gpui_web` defaults to `multithreaded` (pulling `wasm_thread`), and `gpui_platform::application()` creates `WebPlatform::new(true)`; `single_threaded_web()` is an explicit opt-out. `gpui_platform::web_init` invokes the linker-synthesized `__wasm_call_ctors` once before GPUI use, so wasm-ld does not wrap every browser export with constructor calls. `gpui_platform` builds `FetchHttpClient` from the owning `WebPlatform` dispatcher.

Browser displays default to the full visible viewport, and `WebWindow` sizes its logical bounds and canvas/WebGPU backing store from viewport × `devicePixelRatio` before the first frame. Pointer coordinates come from `clientX/clientY` minus the canvas bounding rect, so events inside canvas-child DOM do not corrupt GPUI coordinates. Closing a `WebWindow` marks it closed, cancels the pending animation frame, disconnects its resize and safe-area observers, drops its media-query handle, and removes its canvas and IME mirror before retained JavaScript closures drop; frame and resize callbacks stop after closure.

The first top-level window takes the canvas the graphics context was created against. On WebGPU each later window gets its own canvas and a surface from the shared instance (`PreparedWebWindow::additional`), appended to the page body; a WebGL context is bound to its canvas, so WebGL supports one window. Each window listens on its own canvas and IME mirror, and windows are active while the document has focus.

Clipboard writes start `navigator.clipboard.writeText` during the trusted action gesture and also run a synchronous one-shot `copy` event fallback that saves the document selection, selects a focus-neutral offscreen range, supplies the exact text through `ClipboardEvent.clipboardData`, and restores the selection. Paste is bridged from the trusted DOM `paste` event: the plain-text payload becomes the cached `ClipboardItem` and replaces the active input-handler selection; platform clipboard reads return that cache.

Text input uses Zed's hidden IME mirror textarea (`ime_mirror.rs`): desktop keystrokes dispatch as key events, while IME and software-keyboard edits are imported from the mirror's `input` events by diffing its value against the stored text; gpux caps that diff at the stored selection so an edit always replaces the whole selection. Keyboard conventions follow the visitor's operating system: `WebPlatform` reports it through `KeyboardOs::set_web` (from `navigator.platform`, then the user agent; Apple systems are `MacOs`, Android `Unknown`) before any binding is parsed, so `Modifiers::secondary`, `secondary-` bindings, keystroke `unparse`/display and the `os` key context use Command in Apple browsers and Control elsewhere; the window's `is_mac` (paste shortcut, text insertion) comes from the same detection.

Wheel input keeps line deltas as GPUI line deltas and converts page deltas to pixels at the canvas's client size, so page-mode wheels scroll by the visible page.

On WASM, `Asset::load` futures are not `Send`, `App::fetch_asset` spawns them on the foreground executor, and `Svg::external_path(...)` resolves relative URLs against the page and fetches bytes with `reqwest` before `paint_svg`.

HTML islands (experimental, Chrome Canary HTML-in-canvas): gpux owns the `HtmlIslandPrimitive` scene contract and window paint hook; the author-facing `html_island(...)` element lives in RayX `rayx_components_html`. `WebHtmlIslandRegistry` creates only direct real-canvas child roots keyed by stable island id when layout subtree, paint event, and `requestPaint()` are all supported, auto-enabling the canvas `layoutsubtree` attribute on the first frame with islands (`?gpux-html-island=1` or `globalThis.__gpuxHtmlIslandEnabled` remain early probes; the `rayx` spellings still work). It syncs trusted HTML on content revision, logical bounds via `transform-origin: 0 0` plus `translate(...)`, and physical size; tracks dirty roots from browser paint and input; and prunes stale or offscreen roots (only painted islands have roots). Upload jobs carry the live root to `gpui_wgpu` only after a paint event marks its cached paint record ready; `gpui_wgpu/src/html_island_upload.rs` calls `copyElementImageToTexture` through `Queue::as_webgpu()`/`Texture::as_webgpu()` with `{ source: element }` and `{ destination: { texture, mipLevel, origin, aspect }, colorSpace, premultipliedAlpha }`, falling back to the explainer's `(element, width, height, destination)` form when the browser reports no overload for the dictionary form, into textures with `COPY_DST | TEXTURE_BINDING | RENDER_ATTACHMENT`. `DomOwned` targets bypass GPUI pointer, wheel, context-menu, drag, and drop handling; `GpuiOwned` roots use `pointer-events: none`. Without layout subtree support no roots or uploads exist and the app's GPUI fallback is visible. Island diagnostics (`__gpuxHtmlIslandDiagnostics`, counters, injected upload failures) exist only under `diagnostics` and never sample content.

HTML elements: gpux owns `HtmlElementPrimitive`, the window paint hook, and the web registry; `html_element(...)` and its builder types live in RayX `rayx_components_html`. Roots live under one fixed body-sibling overlay above the canvas, keyed by stable ids, with revision-gated `innerHTML`, geometry diffing, rectangular content-mask clipping, input-mode pointer CSS, default pruning, keep-alive offscreen parking, static CSS classes plus inline geometry/state and caller `extra_styles`; `Drop` removes the overlay with its window. `HtmlElementContentFit::Fill` makes top-level trusted children fill the GPUI box. `HtmlElementWheelBehavior::ForwardIfObservable` forwards same-document wheel events; `CaptureForward` puts a transparent capture layer over cross-origin iframes. `gpui_web` publishes `__gpuxHtmlElementDiagnostics` under `diagnostics`.

Lane B proves this crate: one Playwright spec per area (backend fallback, viewport and resize, window close, pointer, wheel delta modes, keyboard, touch, IME, clipboard and its fallback, HTML elements and islands, canvas text fallback, worker threads, fetched assets, cursors, page lifecycle, safe areas, shortcut modifier) plus every lane-B conformance scene with trusted CDP input. The Playwright config keeps the browser's own user agent; Playwright's desktop Chrome device reports Windows on every host. Lane B passed 164/164 on an Apple M5 (2026-10-09).

## Backlinks

- [Lane B specs](../../../../../tools-node/specs)
- [gpui_web platform](../../../../../crates/gpui_web/src/platform.rs)
- [gpui_web window](../../../../../crates/gpui_web/src/window.rs)
- [gpui_web events](../../../../../crates/gpui_web/src/events.rs)
- [HTML island registry](../../../../../crates/gpui_web/src/html_island.rs)
- [HTML element registry](../../../../../crates/gpui_web/src/html_element.rs)
- [HTML island upload](../../../../../crates/gpui_wgpu/src/html_island_upload.rs)
- [Platform selection and web_init](../../../../../crates/gpui_platform/src/gpui_platform.rs)
- 2026-06-24 current [Multithreaded WASM](../../../decisions/platform/web/2026-06-24-platform-web-multithreaded-wasm.md)
- 2026-08-31 current [Explicit WASM constructors](../../../decisions/platform/web/2026-08-31-platform-web-explicit-wasm-constructors.md)
- 2026-06-29 current [WASM SVG asset loading](../../../decisions/platform/web/2026-06-29-platform-web-wasm-svg-asset-loading.md)
- 2026-07-01 current [HTML island URL opt-in](../../../decisions/platform/web/2026-07-01-platform-web-html-island-url-opt-in.md)
- 2026-07-02 current [HTML island canonical roots](../../../decisions/platform/web/2026-07-02-platform-web-html-island-canonical-roots.md)
- 2026-07-03 current [HTML element DOM overlay](../../../decisions/platform/web/2026-07-03-platform-web-html-element-dom-overlay.md)
- 2026-07-03 current [HTML element fit, styles, and wheel](../../../decisions/platform/web/2026-07-03-platform-web-html-element-fit-styles-wheel.md)
- 2026-10-05 current [gpux-wgpu fork and gpux-owned element upload](../../../decisions/repository/packaging/2026-10-05-repository-packaging-gpux-wgpu-fork.md)
- 2026-10-07 current [Browser lane B](../../../decisions/platform/web/2026-10-07-platform-web-browser-lane-b.md)
