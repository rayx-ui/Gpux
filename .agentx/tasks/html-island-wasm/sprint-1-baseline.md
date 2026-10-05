---
agentx:
  kind: knowledge
  scope: repo:tasks/html-island-wasm/sprint-1
  status: current
  created: 2026-07-01T00:00:00+02:00
  updated: 2026-07-01T03:40:00+02:00
  topics:
    - html-island
    - diagnostics
    - wasm
  supersedes: []
  superseded_by: []
---

# Sprint 1 Baseline Notes

## 2026-07-01 Task 1.1 Source Baseline

Inspected current source in this worktree before implementation changes:

- `crates/gpux/crates/gpui_web/src/window.rs`
- `crates/gpux/crates/gpui_web/src/events.rs`
- `crates/gpux/crates/gpui/src/scene.rs`
- `crates/gpux/crates/gpui/src/elements/mod.rs`
- `crates/gpux/crates/gpui_wgpu/src/wgpu_renderer.rs`
- `xtask/src/wasm.rs`
- `tools-node/playwright.config.ts`
- `tools-node/package.json`
- `tools-node/tests/rayx-wasm.spec.ts`
- `crates/gpux/Cargo.toml`
- `Cargo.lock`
- `crates/gpux/Cargo.lock`

Current-state findings:

- `WebWindow::new` creates one real `web_sys::HtmlCanvasElement`, initializes the DPR-aware backing store, applies full-size canvas CSS, appends the canvas to `document.body`, appends a hidden input beside it, and constructs `WgpuRenderer::new_from_canvas`.
- `WebWindow::draw` currently updates pending physical canvas size and then calls `self.inner.renderer.borrow_mut().draw(scene)` directly; there is no DOM registry or HTML island pre-draw sync point yet.
- `gpui_web` pointer/wheel/drop helpers still compute positions through `event.offset_x()` and `event.offset_y()`, which are target-relative and must be replaced before direct canvas child DOM roots can target events.
- GPUI `Scene` currently stores shadows, quads, paths, underlines, monochrome sprites, subpixel sprites, polychrome sprites, and surfaces. `Primitive`, `PrimitiveKind`, `PrimitiveBatch`, `Scene::clear`, `Scene::insert_primitive`, `Scene::finish`, and `Scene::batches` have no HTML island variant.
- `gpui_wgpu::WgpuRenderer` creates its WASM surface from `wgpu::SurfaceTarget::Canvas(canvas.clone())`, draws batches in `encode_scene_to_view`, and treats `PrimitiveBatch::Surfaces` as macOS-only/no-op for web.
- `xtask/src/wasm.rs` builds the selected web wrapper, runs `wasm-bindgen`, verifies threaded WASM, writes the host page with thread/WebGPU status diagnostics, serves with COOP/COEP/CORP headers, and invokes `pnpm playwright test` without selecting the dedicated HTML island project.
- `tools-node/playwright.config.ts` already defines `chrome-canary-html-island` with `testMatch: ["rayx-html-island.spec.ts"]`, `channel: "chrome-canary"`, `headless: false`, and `htmlInCanvasArgs` including `CanvasDrawElement`.
- `tools-node/package.json` already defines `test:wasm:html-island` as `playwright test --project=chrome-canary-html-island`.
- `tools-node/tests/rayx-wasm.spec.ts` currently captures startup diagnostics, WebGPU adapter info, console/page/request errors, screenshots, viewport/DPR/canvas counters, and threaded WASM evidence for the normal Chromium projects.
- Repository source currently has no implementation symbols for `html_island`, `HtmlIsland`, `layoutsubtree`, `copyElementImageToTexture`, `drawElementImage`, `captureElementImage`, `ElementImage`, or `copy_external_image_to_texture` outside this epic's planning docs and Playwright's `CanvasDrawElement` flag.
- `crates/gpux/Cargo.toml` pins `wgpu` to Zed's `v29` branch. The root lockfile resolves `wgpu` `29.0.3` at `357a0c56e0070480ad9daea5d2eaa83150b79e88`; the nested gpux lockfile resolves `wgpu` `29.0.0` at `a466bc382ea747f8e1ac810efdb6dcd49a514575`.

No source evidence contradicted the current `html-island-wasm` requirements or design. The first implementation surfaces remain `gpui_web` capability/diagnostics support, Playwright Canary diagnostics, and later GPUI scene/renderer additions.

## 2026-07-01 Task 1.2 Diagnostics Contract

The initial browser diagnostics object is `globalThis.__rayxHtmlIslandDiagnostics`. It is installed by the generated WASM host page before the app module initializes, so Playwright can read a structured unsupported-mode object before the future `gpui_web` capability module exists.

Initial contract:

- `version`: numeric contract version, initially `1`.
- `available`: aggregate boolean; initially `false` until capability detection proves the required runtime path.
- `support.layoutSubtree`: whether the real canvas can opt into child layout subtree behavior.
- `support.paintEvent`: whether browser paint invalidation is observable.
- `support.requestPaint`: whether the bootstrap invalidation call exists.
- `support.captureElementImage`: whether browser element snapshot capture is available.
- `support.drawElementImage`: whether an intermediate canvas draw path is available.
- `support.copyElementImageToTexture`: whether the preferred WebGPU upload path is available.
- `support.fallbackExternalImageCopy`: whether a measured fallback upload path is available.
- `canvas.layoutSubtreeEnabled`: whether the real RayX canvas has layout subtree behavior enabled.
- `canvas.childCount`: count of direct canvas children owned by the HTML island registry.
- `islands.count`: live island root count.
- `islands.dirtyCount`: currently dirty island count.
- `islands.uploadCount`: successful snapshot/texture upload count.
- `islands.textureReallocationCount`: texture allocation/reallocation count.
- `renderer.path`: selected renderer path, initially `'unsupported'`.
- `renderer.lastError`: last diagnostics-facing capability, registry, upload, or draw error; initially `null`.

Sprint 2 should replace the host-only defaults with `gpui_web` capability detection and live counters without changing the top-level object name.

## 2026-07-01 Task 1.3 Diagnostics Workflow Note

Every runtime-changing slice of this epic must use RayX diagnostics as implementation evidence. For each slice, add temporary traces only where they clarify layout, event target classification, DOM registry state, paint invalidation, upload, or renderer draw boundaries; run headed Playwright or the target-specific RayX diagnostic route; attach JSON counters, screenshots, traces, console/page errors, and runtime state; then remove trace-only helpers before marking the slice complete.

Sprint audits should list the diagnostics evidence used for that sprint. Screenshot-only proof is not enough for `html_island`; browser-readable counters and canvas/runtime state must agree with any visual artifact.

## 2026-07-01 Task 1.7 Canary API Probe

The generated WASM host now installs a small browser reflection probe into `globalThis.__rayxHtmlIslandDiagnostics.probes`. The probe records the exact API names seen by Chrome Canary while keeping the renderer path unsupported until the real `gpui_web` and `gpui_wgpu` implementation exists.

Validated with `pnpm run test:wasm:html-island` against the served Lab WASM app on port 7879. The 2026-07-01 run recorded:

- `canvasLayoutSubtree`: `layoutSubtree`
- `canvasPaintEvent`: `paint`
- `canvasRequestPaint`: `requestPaint`
- `globalCaptureElementImage`: `null`
- `elementCaptureElementImage`: `null`
- `canvasDrawElementImage`: `CanvasRenderingContext2D.drawElementImage`
- `gpuQueueCopyElementImageToTexture`: `GPUQueue.copyElementImageToTexture`
- `gpuQueueCopyExternalImageToTexture`: `GPUQueue.copyExternalImageToTexture`

This means the current Canary path exposes direct WebGPU upload naming but does not expose a separate `captureElementImage` function under the probed global or element-prototype names.

## 2026-07-01 Task 1.8 Upload Binding Classification

Current source classification: **fallback canvas is available through existing `wgpu`; direct element upload is not typed in current bindings.**

Evidence:

- The pinned `wgpu` v29 checkout exposes `wgpu::Queue::copy_external_image_to_texture` for `cfg(web)`.
- `wgpu::ExternalImageSource` currently supports `ImageBitmap`, `HTMLImageElement`, `HTMLVideoElement`, `ImageData`, `HTMLCanvasElement`, `OffscreenCanvas`, and unstable `VideoFrame`.
- The current `ExternalImageSource` union does not include `ElementImage`, DOM `Element`, or another HTML-in-canvas element snapshot type.
- The web backend maps `copy_external_image_to_texture` to `GPUQueue.copyExternalImageToTexture`, not the experimental `GPUQueue.copyElementImageToTexture` name reported by Canary.
- The current Canary probe reports `CanvasRenderingContext2D.drawElementImage`, `GPUQueue.copyElementImageToTexture`, and `GPUQueue.copyExternalImageToTexture`, but no `captureElementImage` under the probed names.

Implementation implication: Sprint 5 can use the measured fallback path `drawElementImage` into an intermediate canvas followed by `wgpu::Queue::copy_external_image_to_texture` from `ExternalImageSource::HTMLCanvasElement` without patching `wgpu`. The preferred direct element upload path still needs one of: a local web binding extension, JS reflection/raw-handle bridge, or upstream/pinned `wgpu` patch that models the experimental source type.
