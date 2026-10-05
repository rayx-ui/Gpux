---
agentx:
  kind: knowledge
  scope: repo:tasks/html-island-wasm
  status: current
  created: 2026-07-01T00:00:00+02:00
  updated: 2026-07-01T03:40:00+02:00
  topics:
    - html-island
    - wasm
  supersedes: []
  superseded_by: []
---

# Epic: HTML Island WASM Canvas Integration

## Scope

Create a consolidated implementation plan for the experimental RayX WASM element named `html_island`. The element hosts trusted browser DOM content as direct children of the real `gpui_web` canvas, then composites snapshots of that DOM into the WebGPU-rendered GPUI scene.

This plan is the current successor to `.agentx/tasks/html-island/`. Implement new work from this task folder unless the user explicitly reopens the older plan.

## Current Confirmed State

- `tools-node/playwright.config.ts` already defines a dedicated `chrome-canary-html-island` project.
- The dedicated project runs `rayx-html-island.spec.ts`, uses the installed `chrome-canary` channel, runs headed, and enables `CanvasDrawElement` together with the existing WebGPU, SharedArrayBuffer, Vulkan, developer WebGPU, unsafe WebGPU, and GPU blocklist flags.
- `tools-node/package.json` already exposes `pnpm run test:wasm:html-island` as `playwright test --project=chrome-canary-html-island`.
- Normal WASM validation remains on the existing Chromium projects and should keep using `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`.
- `gpui_web::WebWindow` creates and owns one real `HtmlCanvasElement`, appends it to `document.body`, creates the hidden input element, and passes the canvas to `gpui_wgpu::WgpuRenderer::new_from_canvas`.
- `gpui_web` currently computes pointer positions from `offsetX` and `offsetY`; this must change when DOM children can become event targets.
- GPUI `Scene` currently has no HTML island primitive. Web renderer support currently batches quads, paths, sprites, underlines, and macOS-only surfaces.
- The nested gpux workspace uses Zed's `wgpu` v29 branch. The current repo has no existing `ElementImage`, `copyElementImageToTexture`, or HTML-in-canvas upload binding.
- `rayx_components_webview` is not a valid solution for this feature because it is an overlay-style native webview for macOS/Windows and cannot prove canvas-scene compositing.

## Problem Statement

RayX WASM currently renders GPUI content into a WebGPU-backed canvas. That keeps the Rust UI path clean, but it cannot reuse browser-native HTML layout, accessibility, rich DOM rendering, and form controls inside the GPUI scene. The experimental HTML-in-canvas browser feature appears to make selected canvas child DOM subtrees available to canvas rendering and hit testing, but support is gated and unstable.

RayX should implement this as an explicit experimental renderer capability, not as an app-level DOM overlay, TypeScript runtime bridge, managed interop bridge, or WebView workaround.

## Outcomes

- RayX authors can write `html_island(...)` in Rust UI code.
- The element compiles on native targets with explicit fallback or unsupported behavior.
- On supported Chromium-family builds, each island DOM root is a direct child of the real `gpui_web` canvas.
- Island DOM roots stay synchronized with GPUI layout bounds, input mode, content revision, and lifecycle.
- Supported browser runs composite island content into the WebGPU canvas in GPUI scene order.
- Unsupported browser runs keep normal WASM validation green and expose clear diagnostics instead of crashing.
- Development and validation use RayX diagnostics and the dedicated Chrome Canary Playwright project.

## In Scope

- Add public GPUI element API:
  - `html_island`
  - `HtmlIsland`
  - `HtmlIslandInputMode`
  - `HtmlIslandSupport` or equivalent diagnostics-facing support model.
- Add a GPUI scene primitive and renderer batch for HTML islands.
- Add `gpui_web` HTML-in-canvas capability detection, canvas `layoutsubtree` opt-in, paint/requestPaint wiring, DOM registry, and event target classification.
- Add web renderer support for dynamic island textures.
- Prefer direct WebGPU `GPUQueue.copyElementImageToTexture`; use a measured `ElementImage` or intermediate canvas fallback only if current Rust bindings block the direct path.
- Add `DomOwned` and `GpuiOwned` input modes.
- Add Lab story coverage for visual, interactive, fallback, resize, clipping, dirty update, and scene-order cases.
- Add Playwright coverage through `tools-node/tests/rayx-html-island.spec.ts` and the dedicated `chrome-canary-html-island` project.
- Use RayX diagnostics during implementation: rendered-frame/DevKit diagnostics, targeted temporary runtime traces, console diagnostics, Playwright traces, screenshots, JSON counters, and WebGPU/runtime evidence.
- Remove temporary traces before final commits for implementation work.

## Out Of Scope

- Making HTML-in-canvas a production-default path.
- Supporting Firefox or Safari before compatible APIs exist.
- Native desktop/mobile HTML rendering.
- Replacing GPUI text, image, SVG, input, or WebView systems wholesale.
- Rendering remote pages, iframes, cross-origin content, or unsanitized untrusted HTML.
- Reintroducing managed bridge code, TypeScript runtime bridge code, compatibility aliases, proxy APIs, or hidden adapter layers.
- Depending on Playwright-managed Chrome Canary downloads; this repo should use the installed Chrome Canary channel through Playwright.

## Functional Requirements

### Public Element API

- The constructor must be named `html_island`.
- The public element type should be named `HtmlIsland`.
- App code must supply a stable island id or derive one from an explicit element id; generated per-frame ids are not acceptable.
- V1 content accepts trusted HTML markup supplied from Rust.
- The API must expose explicit fallback content or an explicit empty unsupported state.
- The API must expose input mode:
  - `HtmlIslandInputMode::DomOwned`: browser DOM receives pointer, focus, keyboard, composition, and accessibility behavior first.
  - `HtmlIslandInputMode::GpuiOwned`: DOM is rendered but uses `pointer-events: none`; GPUI owns interaction.
- Native and unsupported web targets must compile and render fallback behavior without pretending that native HTML-in-canvas exists.
- The final API must use `html_island` only; do not add old-name aliases.

### Browser Capability Detection

- `gpui_web` must detect the current browser's support for the required HTML-in-canvas pieces without panicking.
- Detection must cover:
  - canvas layout subtree opt-in,
  - canvas `paint` event or equivalent invalidation event,
  - `requestPaint()` or equivalent bootstrap invalidation,
  - element snapshot/capture support,
  - WebGPU texture upload support for element snapshots or a measured fallback path.
- Detection must be exposed in diagnostics so Playwright can assert supported and unsupported modes.
- Normal WASM validation must not require the experimental API.
- Required-mode tests must fail loudly when the expected experimental API is absent.

### DOM Hosting

- `WebWindow` must opt the real canvas into layout subtree behavior only in the experimental feature path and only when support is available.
- Each island DOM root must be a direct child of the real canvas.
- A `WebHtmlIslandRegistry` must create, update, and prune DOM roots.
- DOM roots must sync:
  - stable island id,
  - trusted HTML content revision,
  - logical bounds,
  - physical size/DPR metadata,
  - input mode,
  - lifecycle presence in the current scene.
- Stale DOM roots must be pruned after frames where their scene primitive no longer appears.
- CSS must not disturb the canvas layout or fake overlay visibility; visible proof must come from canvas pixels.

### Scene And Compositing

- GPUI `Scene` must include an HTML island primitive.
- The primitive must carry bounds, content mask, stable id, input mode, content revision, and enough DOM sync metadata for web.
- The renderer must draw island snapshots in scene order relative to quads, paths, sprites, underlines, and other primitives.
- V1 must support rectangular bounds and content masks. Arbitrary transforms and rounded clipping may be deferred unless the chosen renderer path makes them cheap and reliable.
- Textures must update only when the island is dirty because content, bounds, DPR, support state, or browser paint events changed.
- Zero-sized islands must not create invalid textures or draw calls.

### Invalidation And Timing

- Browser `paint` events or equivalent signals must mark affected islands dirty.
- Initial snapshots must be requested with `requestPaint()` where required by the browser API.
- A dirty island should request a GPUI frame without causing continuous texture uploads.
- A one-frame lag between DOM mutation and GPUI texture update is acceptable for V1 if documented and proven in diagnostics.

### Input, Focus, And Accessibility

- `DomOwned` islands must allow browser pointer, focus, keyboard, composition, and accessibility behavior to reach their DOM subtree.
- `GpuiOwned` islands must set `pointer-events: none` and preserve current GPUI event dispatch.
- Web pointer coordinates must use `clientX/clientY - canvas.getBoundingClientRect()` rather than `offsetX/offsetY`.
- Hidden GPUI input focus must not be forced when a DOM-owned island intentionally receives focus.
- Tests must include one DOM-owned interactive control and one GPUI-owned visual island.

### Security And Privacy

- The API treats supplied HTML as trusted application markup.
- Documentation must state that untrusted HTML must be sanitized before calling `html_island`.
- V1 must not support arbitrary remote pages, remote iframes, cross-origin image capture, or protected/private browser rendering states.
- Do not work around browser privacy limits.

### Diagnostics And Validation

- Every implementation sprint that changes runtime behavior must use RayX diagnostics, not screenshot-only guesswork.
- Temporary traces should be added around state transitions, layout, paint, DOM registry updates, input target classification, and WebGPU upload/draw boundaries while diagnosing.
- Temporary traces, auto-enabled debug flags, and trace-only helpers must be removed before implementation completion.
- Browser diagnostics must expose a stable object such as `globalThis.__rayxHtmlIslandDiagnostics` with support flags, island counts, dirty counts, upload counts, last error, layout subtree state, and renderer path.
- Playwright must attach JSON diagnostics, screenshots, and traces.
- Canary required-mode tests must use `chrome-canary-html-island` from `tools-node/playwright.config.ts`.
- Normal final runtime validation remains `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`.
- HTML island final required-mode validation should run `pnpm run test:wasm:html-island` from `tools-node/` against a served Lab WASM build or through equivalent xtask orchestration once wired.

## Acceptance Criteria

- `html_island` exists as the public element constructor.
- A Lab WASM story renders at least one island composited into the WebGPU canvas on Chrome Canary with the configured experimental flags.
- Pixel checks prove island content appears in canvas pixels, not merely as visible DOM overlay.
- The same story does not crash on native or unsupported browsers and shows explicit fallback behavior.
- Diagnostics expose capability, lifecycle, upload, dirty, and error state.
- Direct canvas child DOM roots are created and pruned correctly.
- Scene-order tests prove HTML island primitives order correctly relative to existing primitives.
- Pointer coordinate tests prove GPUI coordinates remain correct when event targets are DOM descendants.
- DOM-owned and GPUI-owned input modes both have Playwright coverage.
- `tools-node/playwright.config.ts` keeps the dedicated `chrome-canary-html-island` project and `rayx-html-island.spec.ts` binding.
- Normal WASM validation keeps passing without requiring HTML-in-canvas support.
- `cargo fmt --all`, targeted cargo checks/tests, `pnpm exec tsc --noEmit --pretty false`, normal headed WebGPU WASM validation, and Canary HTML island validation all pass before the implementation epic is marked complete.

## Assumptions

- The element name is final: `html_island`.
- The browser feature remains experimental and Chromium-family gated.
- Chrome Canary is installed on the development machine and Playwright can launch it with `channel: "chrome-canary"`.
- Current `wgpu` v29 bindings may not expose the exact direct HTML-in-canvas WebGPU API; an upload-path spike is required before committing to direct, binding-extension, or fallback upload.
- GPUI/gpux is the right layer for framework-level HTML island support because app code cannot cleanly own the real web canvas, scene primitive, or renderer pipeline.

## Risks

- Browser API names, flags, and semantics may change.
- The current Rust web bindings may require a small `wasm-bindgen`/JS reflection bridge, webgpu-sys extension, or pinned `wgpu` patch.
- DOM-owned input can conflict with current hidden input focus behavior if target classification is incomplete.
- Dirty tracking may create continuous repaint loops if browser paint events are treated too broadly.
- Canvas compositing proof can be fooled by overlay DOM unless tests explicitly hide or sample DOM separately and assert canvas pixels.
