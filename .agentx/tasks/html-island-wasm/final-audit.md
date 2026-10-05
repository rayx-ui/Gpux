---
agentx:
  kind: knowledge
  scope: repo:tasks/html-island-wasm/final-audit
  status: current
  created: 2026-07-01T07:45:00+02:00
  updated: 2026-07-01T07:45:00+02:00
  topics:
    - html-island
    - wasm
    - audit
  supersedes: []
  superseded_by: []
---

# HTML Island WASM Final Audit

## Scope Audited

Source of truth:

- `requirements.md`
- `design.md`
- `implementation-plan.md`
- current source and tests
- validation commands run on 2026-07-01

## Requirement Evidence

| Requirement | Evidence | Status |
| --- | --- | --- |
| Public constructor is `html_island`; public type is `HtmlIsland`; no alternate constructor needed. | `crates/gpux/crates/gpui/src/elements/html_island.rs`; `crates/gpux/crates/gpui/src/elements/mod.rs`; `cargo test -p gpux --manifest-path crates/gpux/Cargo.toml html_island --lib`. | Proven |
| Stable id and trusted HTML content are carried from element to scene primitive. | `HtmlIsland::id`, `trusted_html_revision`, `Window::paint_html_island`, `HtmlIslandPrimitive`; scene tests for DOM sync metadata and scaled bounds. | Proven |
| Native and unsupported targets render explicit fallback or no-op unsupported behavior. | `HtmlIsland::fallback`; native fallback unit test; unsupported-mode Playwright test reads structured diagnostics without crash. | Proven |
| `DomOwned` and `GpuiOwned` input modes exist and are covered. | `HtmlIslandInputMode`; `gpui_web` target classification; Lab story uses both modes; Playwright story test covers DOM focus/input and GPUI-owned interaction. | Proven |
| Browser capability detection exposes diagnostics and does not panic when unsupported. | `gpui_web/src/html_in_canvas.rs`; generated host diagnostics; Playwright unsupported-mode diagnostics test. | Proven |
| Real canvas is layout-subtree enabled only for the experimental opt-in path. | `enable_layout_subtree_if_requested`; `?rayx-html-island=1`; Playwright asserts `layoutsubtree` in Canary required mode. | Proven |
| DOM roots are direct children of the real GPUI canvas and are created, synced, and pruned. | `WebHtmlIslandRegistry::sync_roots`, `create_root`, `prune_except`; Playwright direct canvas child and prune tests. | Proven |
| Dirty tracking and requestPaint flow update textures without continuous upload loops. | `mark_dirty_from_paint_event`, `dirty_upload_jobs`, `has_pending_upload_work`, `html_island_force_frame`; Playwright bounded-counter test. | Proven |
| Failed upload/snapshot paths skip only the failed island and keep the frame alive. | `record_upload_failure`, `discard_html_island_texture`; injected upload failure Playwright test. | Proven |
| Scene primitive batches preserve ordering relative to other primitives. | `Scene::batches`, `PrimitiveBatch::HtmlIslands`; scene ordering unit test; Lab clipping/scene-order Playwright test. | Proven |
| Renderer composites HTML island textures in WebGPU and skips invalid sizes/masks. | `gpui_wgpu::WgpuRenderer::upload_html_island_element_to_texture`, `draw_html_islands`, `html_island_has_drawable_area`; renderer drawable-area tests; Playwright pixel proof. | Proven |
| Texture cache reallocates only on descriptor changes and prunes removed islands. | `HtmlIslandTextureCache`; texture cache unit tests; resize and navigation prune Playwright tests. | Proven |
| Web pointer coordinates are canvas-relative even when DOM descendants are event targets. | `gpui_web/src/events.rs` uses `clientX/clientY - canvas.getBoundingClientRect()`; Playwright story exercises DOM child interaction and GPUI controls. | Proven |
| Security model is trusted-markup only and does not claim sanitization, iframe, remote-page, or cross-origin support. | `HtmlIsland` rustdoc and `docs/xtask/wasm.md`. | Proven |
| Diagnostics expose support, lifecycle, dirty, upload, texture, renderer, and error state. | `__rayxHtmlIslandDiagnostics` writer; Playwright diagnostics assertions and attachments. | Proven |
| Dedicated Chrome Canary project exists and uses installed Canary with CanvasDrawElement/WebGPU flags. | `tools-node/playwright.config.ts`; `tools-node/package.json`; `docs/xtask/wasm.md`; `pnpm run test:wasm:html-island`. | Proven |
| Normal WASM validation stays on baseline Chromium projects and does not require the experimental API. | `xtask/src/wasm.rs` explicitly selects `chromium` and `chromium-dpr2`; normal headed WebGPU validation passed with 2 tests. | Proven |
| Lab story contains visual, interactive, dirty update, clipping/order, fallback, and diagnostics semantics. | `apps/lab/src/stories/html_island_story.rs`; route/locales tests; Playwright story test. | Proven |
| Documentation covers API, input modes, fallback, trusted HTML, Chrome Canary, flags, normal validation, and Canary validation. | `crates/gpux/crates/gpui/src/elements/html_island.rs`; `crates/gpux/crates/gpui/src/scene.rs`; `docs/xtask/wasm.md`. | Proven |
| AgentX memory reflects current implementation and open direct-upload uncertainty without promoting it prematurely. | `.agentx/memory/knowledge/runtime/gpux-foundation/state.md`; `.agentx/memory/knowledge/runtime/gpux-foundation/knowledge.md`; `.agentx/memory/knowledge/runtime/gpux-foundation/hypotheses.md`; `.agentx/memory/knowledge/tooling/validation/state.md`. | Proven |

## Validation Evidence

Passed:

- `cargo fmt --all`
- `cargo check -p xtask`
- `pnpm exec tsc --noEmit --pretty false` from `tools-node/`
- `cargo test -p gpux --manifest-path crates/gpux/Cargo.toml html_island --lib`
- `cargo +nightly check -p gpui_web --manifest-path crates/gpux/Cargo.toml --target wasm32-unknown-unknown --tests -Z build-std=std,panic_abort`
- Served Lab WASM plus `pnpm run test:wasm:html-island` from `tools-node/`: 8 passed in Chrome Canary.
- `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`: 2 passed in baseline Chromium projects after normal/Canary split.

Known non-blocking validation note:

- `python agentx\scripts\audit_agentx_memory.py` reports 11 pre-existing missing-frontmatter errors in unrelated `.agentx/tasks/*` and older decision files. The touched html-island memory files have frontmatter and were reread after edit.

## Open Follow-Up

- Direct `GPUQueue.copyElementImageToTexture` remains a hypothesis until `wgpu` or a narrow binding exposes a durable raw browser queue/element-image path. The current proven renderer path is `intermediate-canvas`.
