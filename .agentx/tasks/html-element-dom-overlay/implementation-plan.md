---
agentx:
  kind: knowledge
  scope: repo:tasks/html-element-dom-overlay
  status: current
  created: 2026-07-03T18:30:00+02:00
  updated: 2026-07-03T18:30:00+02:00
  topics:
    - html-element
    - dom-overlay
    - implementation
  supersedes: []
  superseded_by: []
---

# Implementation Plan: HTML Element DOM Overlay

Plan note: Do not spawn sub-agents unless the user explicitly asks for sub-agents, delegation, or parallel agent work.

## Sprint 1: GPUI API And Frame Metadata

### Group 1: Source Baseline And Test Harness
Dependency: none
Parallelization: sequential
- [x] Re-read current source anchors before editing: `crates/gpux/crates/gpui/src/elements/html_island.rs`, `crates/gpux/crates/gpui/src/scene.rs`, `crates/gpux/crates/gpui/src/window.rs`, `crates/gpux/crates/gpui_web/src/window.rs`, `crates/gpux/crates/gpui_web/src/html_island.rs`, `crates/gpux/crates/gpui_web/src/events.rs`, and parent-scroll virtualization sources. Owned scope: source inspection notes only. Validation: implementation notes identify the exact insertion points and no stale plan assumption contradicts source.
- [x] Add targeted GPUI tests for an `html_element` primitive before implementation, covering stable id, input mode, lifetime, content revision, logical bounds, logical content mask, opacity, and fallback behavior. Owned scope: `crates/gpux/crates/gpui/src/elements/html_element.rs` tests and/or `scene.rs` tests. Validation: tests fail before implementation.

### Group 2: Public Element API
Dependency: depends on Group 1
Parallelization: sequential
- [x] Add `crates/gpux/crates/gpui/src/elements/html_element.rs` with `html_element`, `HtmlElement`, `HtmlElementInputMode`, trusted HTML, input mode, keep-alive/lifetime, fallback, and style support. Owned scope: GPUI element module. Validation: targeted GPUI element tests compile and pass.
- [x] Export the public API from `crates/gpux/crates/gpui/src/elements/mod.rs` and the crate surface used by apps. Owned scope: GPUI exports. Validation: a small Rust usage sample in tests compiles with `html_element(...)`.
- [x] Add rustdoc that clearly distinguishes `html_element` from `html_island`, states trusted-markup-only behavior, and documents stable id / iframe reload rules. Owned scope: `html_element.rs` docs. Validation: docs are present and no docs imply iframe canvas capture.

### Group 3: Scene And Paint Metadata
Dependency: depends on Group 2
Parallelization: sequential
- [x] Add `HtmlElementPrimitive`, `HtmlElementInputMode`, and `HtmlElementLifetime` to `crates/gpux/crates/gpui/src/scene.rs`; store a `Scene::html_elements` vector and clear/finish it consistently. Owned scope: `scene.rs`. Validation: scene tests assert metadata storage and clearing.
- [x] Add `Window::paint_html_element(...)` near `paint_html_island(...)`, capturing scaled bounds, logical CSS-pixel bounds, logical CSS-pixel content mask, current opacity, input mode, lifetime, revision, and trusted HTML. Owned scope: `window.rs`. Validation: element paint test verifies logical bounds/masks under a clipped parent.
- [x] Ensure native/non-web renderers ignore `html_element` metadata safely and render fallback when provided. Owned scope: GPUI element/fallback path. Validation: native targeted cargo test/check for GPUI.

## Sprint 2: Web Overlay Root And Registry Lifecycle

### Group 1: Web Module And Overlay Root
Dependency: depends on Sprint 1
Parallelization: sequential
- [x] Add `crates/gpux/crates/gpui_web/src/html_element.rs` with `WebHtmlElementRegistry`, root structs, diagnostics structs, and helper CSS setters. Owned scope: new web module and `gpui_web.rs` module registration. Validation: wasm-target check compiles with required `web-sys` features.
- [x] Create one `data-rayx-html-element-overlay` root in `WebWindow::new` as a sibling above the canvas using `web_sys::Document::create_element` and `HtmlElement` styles. Owned scope: `gpui_web/src/window.rs`. Validation: Playwright/debug probe can find exactly one overlay root after app startup.
- [x] Add `html_element_registry` ownership to `WebWindowInner` and initialize diagnostics with zero roots. Owned scope: `gpui_web/src/window.rs` and `html_element.rs`. Validation: global diagnostics object reports `source: "gpui_web"` or equivalent and zero counts before any story roots appear.

### Group 2: Root Creation And Content Revision
Dependency: depends on Group 1
Parallelization: sequential
- [x] Implement stable-id root creation with a clipping wrapper and content child; set diagnostic data attributes and base CSS (`position`, `overflow`, `contain`, `transform-origin`). Owned scope: `gpui_web/src/html_element.rs`. Validation: registry tests or Playwright DOM inspection confirm shape.
- [x] Implement content sync gated only by `content_revision`, with `set_inner_html(...)` called only when revision changes. Owned scope: registry content sync. Validation: test/probe counts content updates across rerender with stable id.
- [x] Add diagnostics counters for created roots, content updates, active roots, and last error. Owned scope: diagnostics writer and registry. Validation: Playwright reads counters after a story frame.

### Group 3: Default Prune And Keep-Alive Parking
Dependency: depends on Group 2
Parallelization: sequential
- [x] Implement per-frame seen/unseen lifecycle tracking and default prune for absent roots. Owned scope: registry lifecycle. Validation: navigating away from a non-keep-alive example removes its DOM root and increments prune count.
- [x] Implement `HtmlElementLifetime::KeepAliveHidden` / `.keep_alive(true)` parking with hidden offscreen CSS, no `display: none`, no `innerHTML` clear, and preserved last nonzero size. Owned scope: registry lifecycle. Validation: iframe element identity survives absence and restoration.
- [x] Add diagnostics for parked roots, keep-alive roots, pruned roots, and restored roots. Owned scope: registry diagnostics. Validation: Playwright verifies counter transitions.

## Sprint 3: Geometry, Scroll, Resize, And Clipping Sync

### Group 1: Frame Sync Entry Point
Dependency: depends on Sprint 2
Parallelization: sequential
- [x] Call `WebHtmlElementRegistry::sync_roots(&scene.html_elements, canvas_rect)` from `WebWindow::draw(scene)` after pending canvas size updates and before the renderer draw for that frame. Owned scope: `gpui_web/src/window.rs`. Validation: diagnostics show sync attempts for frames containing `html_element`.
- [x] Ensure sync uses `canvas.getBoundingClientRect()` and primitive logical CSS-pixel geometry, not physical pixels or offset-based mouse coordinates. Owned scope: registry sync. Validation: DPR1/DPR2 Playwright geometry assertions match CSS rects.

### Group 2: Geometry Diffing And CSS Updates
Dependency: depends on Group 1
Parallelization: sequential
- [x] Implement `HtmlElementGeometry` diffing so unchanged geometry skips DOM writes while changed geometry updates wrapper left/top/width/height, content width/height, transform, opacity, visibility, and pointer mode. Owned scope: registry geometry sync. Validation: geometry update/skip counters behave across idle frames and resize frames.
- [x] Ensure parent scroll and nested content-mask changes move/clip overlay roots without content replacement. Owned scope: GPUI primitive masks and registry geometry. Validation: Playwright scrolls StoryContainer and asserts iframe node identity plus changed DOM rect.
- [x] Ensure GPUI-driven `html_element` width/height changes update DOM size without content replacement. Owned scope: registry geometry and Lab size example. Validation: Playwright clicks size controls and asserts iframe node identity plus rect size change.

### Group 3: Rectangular Clipping And Invisible Frames
Dependency: depends on Group 2
Parallelization: sequential
- [x] Apply rectangular clipping by intersecting `logical_bounds` with `logical_content_mask.bounds` and sizing the wrapper to the intersection. Owned scope: registry geometry. Validation: Playwright/screenshot proves iframe does not bleed outside scroll/container clip.
- [x] Hide or park roots when clip is empty according to lifetime policy without replacing HTML. Owned scope: registry visibility. Validation: offscreen scroll hides/parks, onscreen scroll restores, iframe identity survives for keep-alive.
- [x] Document V1 limitations for rounded clips, non-rect masks, rotation, and complex transforms in rustdoc or Lab story copy. Owned scope: docs/story text. Validation: visible documentation exists.

## Sprint 4: Input, Focus, Safety, And Provider Behavior

### Group 1: Pointer And Focus Modes
Dependency: depends on Sprint 3
Parallelization: sequential
- [x] Implement `DomOwned` pointer behavior: overlay root remains `pointer-events: none`, content child uses `pointer-events: auto`, and browser-owned interaction does not force GPUI hidden input focus. Owned scope: registry CSS and any needed event guard. Validation: Playwright clicks/focuses iframe or same-origin input inside overlay.
- [x] Implement `GpuiOwned` pointer behavior: content child uses `pointer-events: none` so canvas/GPUI receives input. Owned scope: registry CSS. Validation: Playwright clicks a GPUI control beneath/near a GPUI-owned overlay.
- [x] Audit `gpui_web/src/events.rs` for document-level event paths; if any global listeners can intercept overlay events, add a target-classification guard parallel to `html_island`. Owned scope: web event handling only if needed. Validation: DOM-owned overlay events do not generate unwanted GPUI pointer/default-prevention behavior.

### Group 2: Iframe Safety Examples
Dependency: depends on Group 1
Parallelization: sequential
- [x] Add example iframe markup helpers/constants in the Lab story using `title`, explicit sizing, `allow`, `sandbox` where appropriate, and `referrerpolicy` where useful. Owned scope: Lab story examples. Validation: markup appears in code examples and rendered DOM.
- [x] Add story documentation explaining trusted-only HTML, provider frame policies, cross-origin opacity of iframe internals, and dynamic-size limits. Owned scope: Lab story text. Validation: story includes caveats without claiming CORS/canvas capture workarounds.
- [x] Add a same-origin `srcdoc` or local fixture iframe for deterministic input/lifecycle tests. Owned scope: Lab story/test fixture. Validation: Playwright can interact without external network dependency.

### Group 3: Keep-Alive Product Cases
Dependency: depends on Group 1
Parallelization: sequential
- [x] Add a keep-alive example that proves iframe/root identity survives ordinary rerender, scroll absence, and tab/section toggles. Owned scope: Lab story state and registry diagnostics. Validation: Playwright stores a DOM identity marker and verifies it remains stable.
- [x] Add docs warning that `keep_alive(true)` should use stable provider/session ids, not row indices or random render-time strings. Owned scope: rustdoc and Lab text. Validation: docs include good/bad id examples.
- [x] Add diagnostics warning or count for parked keep-alive roots so apps can notice long-lived hidden providers. Owned scope: registry diagnostics. Validation: Playwright reads parked counts.

## Sprint 5: Lab Story And RayX Diagnostics

### Group 1: Dedicated Lab Story
Dependency: depends on Sprint 4
Parallelization: sequential
- [x] Add `HtmlElementStory` under `apps/lab/src/stories/` as a normal scrollable story page, separate from `HtmlIslandStory`. Owned scope: Lab story source. Validation: native build/check compiles.
- [x] Register the story route, metadata, localization, and navigation entry following current Lab conventions. Owned scope: Lab route/story registration/locales. Validation: story opens by direct route in WASM and native.
- [x] Use existing codeview/source snippet components to show each example's markup and explain what is demonstrated. Owned scope: Lab story content. Validation: story renders examples and code snippets without layout overlap.

### Group 2: Scroll And Virtualization Examples
Dependency: depends on Group 1
Parallelization: sequential
- [x] Add parent-scroll example inside StoryContainer scroll content that proves overlay position follows GPUI scroll. Owned scope: Lab story. Validation: Playwright scrolls and inspects DOM rect movement.
- [x] Add parent-scroll virtualization example using `VirtualListExternalScrollBehavior::ParentViewport` or a focused story section that proves virtualized rows can host `html_element` and follow parent viewport masks. Owned scope: Lab story and virtualized state usage only. Validation: Playwright scrolls virtualized content and confirms visible roots are created/parked/pruned correctly.
- [x] Add size-change controls for the overlay box and verify layout remains stable. Owned scope: Lab story. Validation: Playwright button interaction changes DOM rect sizes.

### Group 3: Diagnostic Integration
Dependency: depends on Groups 1 and 2
Parallelization: sequential
- [x] Write `globalThis.__rayxHtmlElementDiagnostics` from `gpui_web` with active/parked/pruned/content/geometry/input counters and last geometry sample. Owned scope: `gpui_web/src/html_element.rs` and diagnostics writer. Validation: Playwright attaches JSON diagnostics.
- [x] Add DevKit/rendered-frame semantics to Lab examples where useful so RayX diagnostics can identify story controls and GPUI bounds without fake interactivity. Owned scope: Lab story semantics. Validation: RayX diagnostics can identify relevant examples.
- [x] Use temporary runtime traces around layout, sync, and lifecycle while validating, then remove trace-only code before completing the sprint. Owned scope: diagnostics cleanup. Validation: final diff has structured diagnostics only.

## Sprint 6: Playwright And Runtime Validation

### Group 1: HTML Element Overlay Playwright Spec
Dependency: depends on Sprint 5
Parallelization: sequential
- [x] Add `tools-node/tests/rayx-html-element.spec.ts` covering startup diagnostics, overlay root topology, stable-id rerender reuse, content revision replacement, geometry sync, size sync, parent scroll, keep-alive park/restore, default prune, DomOwned input, and GpuiOwned passthrough. Owned scope: Playwright tests. Validation: `pnpm exec tsc --noEmit --pretty false`.
- [x] Add a Playwright project or test command if needed for targeted HTML element overlay validation using normal headed Chromium WebGPU flags, not Canary-only `CanvasDrawElement`. Owned scope: `tools-node/playwright.config.ts` and `package.json` if needed. Validation: targeted test command runs.
- [x] Attach diagnostics JSON, browser version, screenshots, console messages, page errors, and failed local runtime requests for failures. Owned scope: Playwright test helpers. Validation: failure artifacts are produced under ignored output directories.

### Group 2: Regression Coverage
Dependency: depends on Group 1
Parallelization: sequential
- [x] Extend normal WASM validation only where shared helpers need to recognize the new diagnostics; do not weaken existing WebGPU, SharedArrayBuffer, or page-error assertions. Owned scope: `tools-node/tests/rayx-wasm.spec.ts` only if needed. Validation: normal wasm spec still passes; no `rayx-wasm.spec.ts` changes were needed.
- [x] Run existing HTML island Canary validation if shared web window/event code changed. Owned scope: validation only. Validation: unmodified suite was inspected/attempted during this sprint, then left unchanged after scope was corrected back to `html_element`.
- [x] Add targeted Rust tests for registry lifecycle and scene metadata where feasible, including keep-alive and geometry diff behavior. Owned scope: Rust tests in gpui/gpui_web modules. Validation: targeted cargo test/check passes.

### Group 3: Full Runtime Proof
Dependency: depends on Groups 1 and 2
Parallelization: sequential
- [x] Run `cargo fmt --all` after confirming there are no unrelated user changes that would be formatted unexpectedly; if unrelated dirty files exist, use narrower formatting or document the blocker. Owned scope: formatting. Validation: `cargo fmt --all` passed.
- [x] Run targeted cargo checks/tests for `gpui`, `gpui_web`, `gpui_wgpu` if touched, and Lab. Owned scope: validation. Validation: targeted `gpux`/`gpui_web`/Lab tests passed, and `cargo clippy --workspace --all-features -- -D warnings` passed.
- [x] Run `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`. Owned scope: validation. Validation: headed WebGPU WASM runtime proof passed, 10/10 tests.
- [x] Run targeted HTML element overlay Playwright validation against the served Lab WASM app. Owned scope: validation. Validation: `pnpm run test:wasm:html-element` passed in headed DPR1/DPR2 projects.

## Sprint 7: Hardening, Documentation, And Final Audit

### Group 1: Resource And Lifecycle Hardening
Dependency: depends on Sprint 6 validation findings
Parallelization: sequential
- [x] Audit route/window teardown so overlay roots are removed when the `WebWindow` is destroyed and keep-alive roots do not survive outside their owning window. Owned scope: `gpui_web` window/registry lifecycle. Validation: `WebHtmlElementRegistry::drop` removes the overlay root and Playwright verifies route navigation prunes/removes active roots.
- [x] Add or document keep-alive resource guidance and diagnostics so apps can track parked provider frames. Owned scope: rustdoc, Lab story, diagnostics. Validation: rustdoc/Lab text and `__rayxHtmlElementDiagnostics` expose parked and keep-alive counts.
- [x] Verify hidden/parked roots are not accidentally reachable when inactive, or document accessibility trade-offs if provider behavior limits what can be hidden. Owned scope: registry parking CSS and docs. Validation: parked roots are moved offscreen, hidden, opacity 0, and pointer-disabled while preserving iframe identity.

### Group 2: Documentation And Skill/Memory Updates
Dependency: depends on Group 1
Parallelization: sequential
- [x] Update gpux/RayX reusable guidance if implementation teaches durable rules about `html_element`, iframe overlays, keep-alive, or browser validation. Owned scope: `.codex/skills/gpux` or `.codex/skills/rayx` only if durable guidance changed. Validation: RayX overlay guidance now includes `html_element` DO/DON'T examples.
- [x] Update AgentX memory only for implemented current behavior and durable decisions; do not copy the task plan into memory. Owned scope: `.agentx/memory`. Validation: memory updated for gpux foundation, Lab, and validation behavior; `audit_agentx_memory.py` currently reports pre-existing `.agentx/tasks/rayx-rtl-ltr-system` frontmatter/backlink errors outside this task.
- [x] Keep `html_island` docs/tests separate from `html_element` docs/tests and update cross-links only where they prevent misuse. Owned scope: docs/story text. Validation: `HtmlElementStory` and `rayx-html-element.spec.ts` are separate; no HTML-island files are changed.

### Group 3: Final Audit
Dependency: depends on Groups 1 and 2
Parallelization: sequential
- [x] Audit requirements against implementation: API, registry, geometry sync, scroll, keep-alive, iframe examples, diagnostics, Playwright, and existing `html_island` non-regression. Owned scope: audit notes or final implementation report. Validation: every `html_element` acceptance criterion is covered by source/tests/runtime evidence.
- [x] Remove temporary traces, debug flags, unused helpers, and experimental dead paths. Owned scope: all touched source. Validation: scoped `rg` cleanup audit found no checked-in trace/TODO/debug hooks.
- [x] Produce final validation summary with exact commands run, failures/blockers if any, screenshots/traces location, and residual risks. Owned scope: final report only. Validation: final response summarizes the completed implementation and validation.

## Sprint 8: Responsive Fit, Static DOM Styles, Extra Styles, And Wheel Behavior

### Group 1: Public API And Scene Metadata
Dependency: depends on Sprint 7
Parallelization: sequential
- [x] Add `HtmlElementContentFit`, `HtmlElementWheelBehavior`, and `extra_styles(...)` to the public `html_element` API and carry those values through `HtmlElementPrimitive`. Owned scope: GPUI element, scene primitive, and paint helper. Validation: targeted gpux `html_element` tests pass.

### Group 2: Web Registry Behavior
Dependency: depends on Group 1
Parallelization: sequential
- [x] Move common HTML element overlay/root/content/capture CSS into one per-window stylesheet and keep inline styles for dynamic geometry/state plus caller extra declarations. Owned scope: `gpui_web` HTML element registry. Validation: headed Playwright asserts classes and absence of static inline bloat.
- [x] Implement `content_fit(Fill)` for top-level trusted DOM children and apply `extra_styles` to the root clipping element without changing content revision. Owned scope: registry property sync. Validation: headed Playwright asserts responsive YouTube iframe fit and extra border/radius styles without iframe overflow.
- [x] Implement `Browser`, `ForwardIfObservable`, and `CaptureForward` wheel behavior, reusing the normal GPUI `ScrollWheelEvent` dispatch path. Owned scope: web event helper and registry wheel listeners. Validation: headed Playwright asserts forwarded wheel counters and capture-layer behavior.

### Group 3: Lab, Documentation, And Validation
Dependency: depends on Group 2
Parallelization: sequential
- [x] Update the Lab HTML Element story with responsive YouTube sizing, extra root styles, and wheel behavior examples. Owned scope: Lab story only. Validation: Lab unit tests and headed HTML element Playwright pass.
- [x] Update reusable RayX/AgentX guidance for content fit, extra styles, static class styling, and wheel behavior limitations. Owned scope: RayX skill and `.agentx/memory`. Validation: current behavior is documented in narrow state/rules/knowledge plus a runtime/gpux-foundation decision.
- [x] Run targeted Rust, TypeScript, generated WASM, normal headed WASM/WebGPU, and targeted headed HTML element Playwright validation. Owned scope: validation only. Validation: all listed commands pass.
