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
    - implementation
  supersedes: []
  superseded_by: []
---

# Implementation Plan: HTML Island WASM Canvas Integration

Plan note for parallel groups: Do not spawn sub-agents unless the user explicitly asks for sub-agents, delegation, or parallel agent work.

## Sprint 1: Baseline Diagnostics And Canary Harness

### Group 1: Source Baseline And Runtime Evidence Contract
Dependency: none
Parallelization: sequential
- [x] Re-read current source for `gpui_web`, `gpui_wgpu`, GPUI `Scene`, GPUI elements, `xtask/src/wasm.rs`, and `tools-node/playwright.config.ts` before changing code. Owned scope: source inspection only. Validation: implementation notes name current files and no assumptions contradict current source.
- [x] Define the initial diagnostics contract for `__rayxHtmlIslandDiagnostics`, including support flags, canvas layout subtree state, island counts, dirty counts, upload counts, texture allocation counts, renderer path, and last error. Owned scope: design notes and eventual web diagnostics module. Validation: Playwright can read a structured object even in unsupported mode.
- [x] Add a short developer note in the implementation branch explaining that RayX diagnostics must be used for every runtime slice: temporary traces at layout/event/render boundaries, headed Playwright traces, JSON counters, screenshots, and removal of trace-only helpers before final commit. Owned scope: task implementation notes or code comments only where useful. Validation: final implementation audit lists diagnostics evidence per sprint.

### Group 2: Dedicated Chrome Canary Project Readiness
Dependency: none
Parallelization: sequential
- [x] Preserve the existing `chrome-canary-html-island` Playwright project in `tools-node/playwright.config.ts`, including `testMatch: ["rayx-html-island.spec.ts"]`, `channel: "chrome-canary"`, `headless: false`, and `CanvasDrawElement` in `htmlInCanvasArgs`. Owned scope: Playwright config. Validation: `pnpm exec tsc --noEmit --pretty false` from `tools-node/`.
- [x] Add `tools-node/tests/rayx-html-island.spec.ts` as a minimal diagnostics-first spec that attaches browser version, launch project, support object, console messages, page errors, failed requests, screenshot, and trace evidence. Owned scope: Playwright tests. Validation: `pnpm run test:wasm:html-island` can run against a served app and either reports unsupported mode cleanly or reaches required capability checks.
- [x] Keep normal `rayx-wasm.spec.ts` and normal Chromium projects unchanged except for shared helpers that are genuinely reused. Owned scope: `tools-node/tests`. Validation: normal `pnpm run test:wasm` still targets the existing Chromium project.

### Group 3: Upload Binding Spike
Dependency: depends on Group 1 diagnostics shape
Parallelization: parallelizable with implementation research because it is isolated to web API discovery and does not require the public element API yet.
- [x] Create a tiny web-only probe path that checks whether Chrome Canary exposes `layoutsubtree`, `paint`, `requestPaint`, `captureElementImage`, `drawElementImage`, and `GPUQueue.copyElementImageToTexture` under the expected names. Owned scope: temporary Lab/dev probe or new `gpui_web` capability module. Validation: Canary diagnostics record exact available API names.
- [x] Determine whether current `wgpu` v29/web bindings can call the direct WebGPU element upload path. Owned scope: web-only spike code. Validation: notes classify path as direct binding, JS reflection/raw-handle bridge, local binding extension, wgpu patch, fallback canvas, or blocked.
- [x] Remove any temporary spike code that is not part of the selected implementation path. Owned scope: cleanup. Validation: `git diff` has no dead spike artifacts.

## Sprint 2: Browser Capability And Canvas Host

### Group 1: HTML-In-Canvas Capability Module
Dependency: depends on Sprint 1 diagnostics contract
Parallelization: sequential
- [x] Add `crates/gpux/crates/gpui_web/src/html_in_canvas.rs` with safe runtime feature detection for layout subtree, paint event/requestPaint, element snapshot/capture, drawElementImage, and WebGPU upload support. Owned scope: `gpui_web` web-only source and `Cargo.toml` web-sys/js-sys features. Validation: unsupported browsers return structured false flags without panic.
- [x] Expose capability diagnostics to the browser global diagnostics object. Owned scope: `gpui_web` diagnostics wiring and host integration only if needed. Validation: Playwright sees the object before any `html_island` element exists.

### Group 2: Canvas Layout Subtree Setup
Dependency: depends on Group 1
Parallelization: sequential
- [x] Update `WebWindow::new` to opt the real canvas into HTML-in-canvas layout subtree behavior only when the experimental path is enabled and capability detection says it is safe. Owned scope: `crates/gpux/crates/gpui_web/src/window.rs`. Validation: diagnostics show whether layout subtree was enabled.
- [x] Add a canvas paint event listener and `requestPaint()` bootstrap path that records changed elements and dirty events without uploading textures yet. Owned scope: `gpui_web/src/window.rs` and helper module. Validation: Canary required-mode probe increments paint counters when supported.
- [x] Ensure unsupported browsers keep starting and drawing normal GPUI frames. Owned scope: `gpui_web`. Validation: `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`.

### Group 3: RayX Diagnostic Proof
Dependency: depends on Groups 1 and 2
Parallelization: sequential
- [x] Add temporary traces around capability detection and canvas setup while validating behavior, then remove them after diagnostics prove the state transitions. Owned scope: temporary runtime logging only. Validation: final code has structured diagnostics, not stray trace logs.
- [x] Capture Playwright JSON diagnostics, screenshot, browser version, and console/page error output in both normal and Canary project runs. Owned scope: validation artifacts only. Validation: artifacts are written under ignored output directories and remain untracked.

## Sprint 3: GPUI Element And Scene Primitive

### Group 1: Public `html_island` Element
Dependency: depends on Sprint 2 terminology
Parallelization: sequential
- [x] Add `crates/gpux/crates/gpui/src/elements/html_island.rs` with `html_island`, `HtmlIsland`, `HtmlIslandInputMode`, trusted HTML builder method, stable id, and fallback support. Owned scope: GPUI element module and exports. Validation: targeted cargo check for gpux and native compile.
- [x] Implement native/unsupported behavior by painting fallback or explicit empty unsupported state. Owned scope: element module. Validation: native Lab build or targeted element test compiles without web-only APIs.
- [x] Avoid aliases, compatibility shims, or alternate constructor names. Owned scope: public API. Validation: `rg` confirms only `html_island` is exported as the constructor.

### Group 2: Scene Primitive Integration
Dependency: depends on Group 1
Parallelization: sequential
- [x] Add `HtmlIslandPrimitive` to `crates/gpux/crates/gpui/src/scene.rs` with bounds, content mask, stable id, input mode, content revision, and DOM sync metadata. Owned scope: `scene.rs`. Validation: scene unit tests compile.
- [x] Update `Scene::clear`, `Scene::insert_primitive`, `Scene::finish`, `Scene::batches`, `Primitive::bounds`, `Primitive::content_mask`, `PrimitiveKind`, `Primitive`, and `PrimitiveBatch`. Owned scope: `scene.rs`. Validation: ordering tests include quads before/after islands.
- [x] Add `Window::paint_html_island(...)` or equivalent helper that applies scale factor, current transform, opacity where relevant, and content mask consistently with existing paint methods. Owned scope: `window.rs`. Validation: unit tests for scaled bounds/masks.

### Group 3: Test Platform Defaults
Dependency: depends on Group 2
Parallelization: sequential
- [x] Add any required default/no-op platform support so GPUI test windows and native platforms compile. Owned scope: `crates/gpux/crates/gpui/src/platform.rs` and test platform files if needed. Validation: GPUI test-support build.
- [x] Add source-level documentation that `html_island` is experimental and trusted-markup only. Owned scope: rustdoc. Validation: no docs claim production browser support.

## Sprint 4: Web DOM Registry And Input Integration

### Group 1: DOM Registry
Dependency: depends on Sprint 3 scene primitive
Parallelization: sequential
- [x] Add `WebHtmlIslandRegistry` in `crates/gpux/crates/gpui_web/src/html_island.rs`, keyed by stable island id and owning direct canvas child roots. Owned scope: new web module. Validation: pure create/update/prune tests where feasible.
- [x] Sync scene island primitives before draw: create missing roots, update trusted HTML on revision changes, sync logical bounds, set input mode CSS, and prune stale roots. Owned scope: `gpui_web/src/window.rs` and registry. Validation: Canary Playwright sees direct canvas children and expected island count.
- [x] Keep DOM roots invisible as overlays and rely on canvas compositing proof. Owned scope: registry CSS. Validation: pixel test fails if only DOM overlay is visible.

### Group 2: Dirty Tracking And Paint Events
Dependency: depends on Group 1
Parallelization: sequential
- [x] Map browser paint changed elements back to stable island ids and mark only affected islands dirty. Owned scope: registry and paint listener. Validation: DOM mutation increments dirty counters once.
- [x] Use `requestPaint()` for initial snapshots and explicit redraws. Owned scope: registry. Validation: first island snapshot appears without manual interaction in Canary required mode.
- [x] Add diagnostics counters for dirty marks, paint events, pruned roots, created roots, and content revision updates. Owned scope: diagnostics object. Validation: Playwright attaches counters.

### Group 3: Event And Focus Modes
Dependency: depends on Group 1
Parallelization: sequential
- [x] Replace pointer coordinate extraction in `crates/gpux/crates/gpui_web/src/events.rs` with `clientX/clientY - canvas.getBoundingClientRect()`. Owned scope: web event helpers. Validation: Playwright pointer checks around DOM child targets.
- [x] Implement DOM-owned target classification so pointer down on a `DomOwned` island does not force hidden GPUI input focus or unconditional preventDefault behavior. Owned scope: events and registry target lookup. Validation: Playwright clicks/focuses a DOM input inside an island.
- [x] Implement `GpuiOwned` behavior by setting `pointer-events: none` and preserving current GPUI event path. Owned scope: registry CSS/events. Validation: Playwright verifies GPUI control interaction near/behind a visual island.
- [x] Use temporary event traces during debugging and remove them after target classification is proven. Owned scope: diagnostics workflow. Validation: final diff has structured diagnostics only.

## Sprint 5: WebGPU Texture Upload And Renderer Compositing

### Group 1: Select Upload Path
Dependency: depends on Sprint 4 registry
Parallelization: sequential
- [x] Implement the selected upload path from Sprint 1: direct `copyElementImageToTexture` if possible, otherwise the smallest durable binding extension or measured `ElementImage`/intermediate-canvas fallback. Add the minimal Lab `HtmlIslandStory` route needed to produce one real scene island for upload proof; defer rich, interactive, fallback, clipping, and scene-order story cases to Sprint 6. Owned scope: `gpui_wgpu` web-only upload code, any narrow binding module, and minimal Lab upload-proof route. Validation: one island snapshot uploads to a texture in Canary required mode.
- [x] Record renderer path in diagnostics as `copyElementImageToTexture`, `binding-extension`, `intermediate-canvas`, or equivalent. Owned scope: diagnostics. Validation: Playwright asserts path is known and lastError is null.
- [x] Close or release any browser snapshot objects that require cleanup. Owned scope: upload helper. Validation: diagnostics do not show leaked snapshot handles after navigation/removal.

### Group 2: Dynamic Texture Cache
Dependency: depends on Group 1
Parallelization: sequential
- [x] Add `HtmlIslandTextureCache` in `crates/gpux/crates/gpui_wgpu/src/html_island_texture.rs`, keyed by stable id and physical size. Owned scope: renderer texture module. Validation: unit-testable resize/prune/cache-hit behavior.
- [x] Reallocate textures only when physical dimensions, format, or renderer support path changes. Owned scope: texture cache. Validation: resize Playwright counters show bounded reallocations.
- [x] Prune textures for islands absent from the current frame. Owned scope: texture cache and draw integration. Validation: story navigation/removal returns texture count to zero.

### Group 3: Draw In Scene Order
Dependency: depends on Group 2
Parallelization: sequential
- [x] Add renderer support for `PrimitiveBatch::HtmlIslands`, drawing dynamic textures with scene order preserved. Owned scope: `gpui_wgpu/src/wgpu_renderer.rs`, bind layouts, shaders if needed. Validation: quad/island/quad ordering test and visual proof.
- [x] Respect rectangular content masks and skip zero-sized islands. Owned scope: draw path. Validation: clipped island does not bleed outside bounds.
- [x] Handle canvas resize, DPR changes, and surface reconfiguration. Owned scope: renderer and web resize integration. Validation: Playwright resizes viewport and checks diagnostics plus pixels.
- [x] Use WebGPU error diagnostics and temporary renderer traces while debugging upload/draw failures; remove temporary traces after proof. Owned scope: diagnostics workflow. Validation: no console/page/WebGPU validation errors in final run.

## Sprint 6: Lab Story And End-To-End Canary Proof

### Group 1: RayX Lab Story
Dependency: depends on Sprint 5 rendering
Parallelization: sequential
- [x] Expand `HtmlIslandStory` under `apps/lab/src/stories/` with one rich visual `GpuiOwned` island, one interactive `DomOwned` island, a dirty-update control, a clipping/scene-order case, and explicit native/unsupported fallback. Owned scope: Lab story source. Validation: native and WASM builds compile.
- [x] Register the story in Lab navigation/routes/locales following existing conventions. Owned scope: Lab story registration and localization files. Validation: story can be opened in Lab.
- [x] Attach DevKit/rendered-frame semantics where existing RayX diagnostics can prove bounds and identity without adding fake interactivity. Owned scope: Lab story/semantics only if useful. Validation: RayX diagnostics can identify story controls and island bounds.

### Group 2: Canary Playwright Required-Mode Tests
Dependency: depends on Group 1
Parallelization: sequential
- [x] Expand `tools-node/tests/rayx-html-island.spec.ts` to assert support diagnostics, layout subtree state, direct canvas children, island bounds, dirty update after mutation, DOM-owned focus/input, GPUI-owned pointer behavior, and no console/page errors. Owned scope: Playwright test. Validation: `pnpm exec tsc --noEmit --pretty false`.
- [x] Add pixel checks proving content is composited into the canvas rather than merely present as visible DOM. Owned scope: Playwright pixel helpers and screenshots. Validation: test fails when upload/draw is disabled but DOM children exist.
- [x] Attach JSON diagnostics, browser version, adapter info, screenshots, and traces to Playwright artifacts. Owned scope: test attachments. Validation: artifacts are generated under ignored directories.

### Group 3: Full Runtime Validation
Dependency: depends on Groups 1 and 2
Parallelization: sequential
- [x] Run `cargo fmt --all`. Owned scope: formatting. Validation: command passes.
- [x] Run targeted cargo checks/tests for gpui, gpui_web, gpui_wgpu, rayx, and Lab web wrapper. Owned scope: validation. Validation: commands pass.
- [x] Run normal headed WASM validation: `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`. Owned scope: validation. Validation: command passes on existing Chromium projects.
- [x] Run HTML island Canary validation with `pnpm run test:wasm:html-island` from `tools-node/` against the served WASM app or equivalent xtask orchestration if added. Owned scope: validation. Validation: command passes on installed Chrome Canary with the dedicated flags.

## Sprint 7: Hardening, Documentation, Memory, And Final Audit

### Group 1: Reliability And Resource Hardening
Dependency: depends on Sprint 6
Parallelization: sequential
- [x] Audit DOM root, event listener, texture, bind group, snapshot, and diagnostics lifetimes across story navigation, island removal, browser resize, and window close. Owned scope: `gpui_web` registry and `gpui_wgpu` texture cache. Validation: diagnostics return counts to zero after removal.
- [x] Harden failed snapshot/upload paths so one failed island renders fallback or skips itself without losing the whole GPUI frame. Owned scope: registry and renderer. Validation: injected unsupported/upload failure test does not crash.
- [x] Bound texture memory and upload frequency with diagnostics counters. Owned scope: texture cache and diagnostics. Validation: mutation/resize test shows no per-frame runaway upload loop.

### Group 2: Documentation And Examples
Dependency: depends on Sprint 6 public API stability
Parallelization: parallelizable with hardening because docs can be updated after API shape stabilizes.
- [x] Document `html_island` API, input modes, unsupported/native behavior, trusted HTML security contract, browser flags, Chrome Canary requirement, and experimental status. Owned scope: nearest gpux/RayX docs or rustdoc. Validation: docs do not claim broad browser support.
- [x] Add concise examples for `DomOwned`, `GpuiOwned`, and fallback usage. Owned scope: docs/rustdoc. Validation: examples compile where practical or are clearly marked as illustrative browser-only snippets.
- [x] Document how to run normal WASM validation and the dedicated Canary project. Owned scope: docs/rustdoc/task notes. Validation: commands match current `tools-node/playwright.config.ts` and `package.json`.

### Group 3: AgentX, Skill Guidance, And Final Audit
Dependency: depends on Groups 1 and 2
Parallelization: sequential
- [x] Update AgentX memory with confirmed implementation behavior, unresolved hypotheses, and decisions only after the implementation is proven. Owned scope: `.agentx/memory/knowledge/runtime/gpux-foundation`, `.agentx/memory/knowledge/tooling/validation`, and decisions if needed. Validation: memory reflects current source and does not promote experimental assumptions prematurely.
- [x] Update the repo-local RayX or gpux skill reference only if the implementation creates durable guidance beyond the current diagnostics and gpux rules; otherwise state explicitly in the final implementation report that no durable skill guidance changed. Owned scope: `.codex/skills` only if needed. Validation: skill references stay concise.
- [x] Produce a final audit mapping every requirement in this plan to code, tests, diagnostics, and validation evidence. Owned scope: final report or task notes. Validation: `state.json` can be marked complete only after every acceptance criterion has evidence.
