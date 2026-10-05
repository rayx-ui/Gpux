---
agentx:
  kind: knowledge
  scope: repo:tasks/html-element-dom-overlay/source-baseline
  status: current
  created: 2026-07-03T18:45:00+02:00
  updated: 2026-07-03T18:45:00+02:00
  topics:
    - html-element
    - source-baseline
    - gpux
    - wasm
  supersedes: []
  superseded_by: []
---

# Source Baseline: HTML Element DOM Overlay

## GPUI Element API

- `crates/gpux/crates/gpui/src/elements/html_island.rs` is the closest public element pattern: stable `ElementId`, trusted HTML stored as `SharedString`, revision hash via `DefaultHasher`, optional fallback, `StyleRefinement`, and `Element` implementation that paints fallback on native/unsupported paths.
- `crates/gpux/crates/gpui/src/elements/mod.rs` declares `mod html_island;` and `pub use html_island::*;`; add the new `html_element` module/export beside it and extend the existing public API test.

## Scene Metadata

- `crates/gpux/crates/gpui/src/scene.rs` currently stores `Scene::html_islands`, clears it in `Scene::clear`, sorts it in `Scene::finish`, and exposes it to renderer batches through `Primitive::HtmlIsland` and `PrimitiveBatch::HtmlIslands`.
- `Scene::insert_primitive` computes `primitive.bounds().intersect(&primitive.content_mask().bounds)` and returns early for empty clipped bounds. Fully offscreen or fully clipped `html_element` primitives can therefore be absent from the frame; the web registry should interpret absence as default prune or keep-alive parking.
- Unlike `html_island`, `html_element` should remain scene side metadata for `gpui_web` and should not be added to `PrimitiveBatch` unless a future renderer genuinely draws it.
- `HtmlIslandPrimitive` keeps only scaled `content_mask`; `html_element` needs an additional logical CSS-pixel mask for direct DOM positioning.

## Window Paint Helper

- `Window::paint_html_island` in `crates/gpux/crates/gpui/src/window.rs` is the direct insertion point. It captures scale, current transform, snapped/transformed bounds, logical bounds, snapped/paint content mask, element opacity, content revision, and trusted HTML.
- `paint_html_element` should live next to it and push `HtmlElementPrimitive` metadata with `logical_content_mask` derived from the same snapped or transformed content mask.

## Web Runtime

- `crates/gpux/crates/gpui_web/src/window.rs` appends the real `HtmlCanvasElement` to `document.body`, appends the hidden input, creates `WebHtmlIslandRegistry`, then writes HTML-in-canvas diagnostics.
- `WebWindow::draw(scene)` applies pending canvas backing-size changes before syncing `html_island`; `html_element` sync should happen after those size updates and use `canvas.getBoundingClientRect()` for CSS viewport positioning before the WGPU renderer draw.
- `crates/gpux/crates/gpui_web/src/gpui_web.rs` must register a new `html_element` module.

## HTML Island Registry Pattern

- `crates/gpux/crates/gpui_web/src/html_island.rs` provides useful lifecycle and diagnostics patterns: stable id map, revision-gated `set_inner_html`, diagnostics counters, stale-root pruning, event-target lookup, and test-only snapshots.
- `html_element` must not reuse island topology: islands are direct canvas children for texture upload; DOM overlays need one body sibling overlay root with clipping wrapper plus content child roots.

## Web Events

- `crates/gpux/crates/gpui_web/src/events.rs` skips GPUI pointer/wheel/context-menu/drag/drop paths when `is_dom_owned_html_island_event_target(...)` returns true.
- Add a parallel DOM-owned `html_element` target guard so overlay iframe/input events do not focus the hidden GPUI input or receive GPUI `prevent_default`.

## Parent-Scroll Virtualization

- `VirtualList::track_external_scroll(..., VirtualListExternalScrollBehavior::ParentViewport)` keeps the virtualized element in normal flow, derives visible range from `window.content_mask().bounds`, and paints visible items under a GPUI content mask.
- This matches the DOM-overlay design: no duplicate DOM scroll container is needed; each rendered `html_element` should mirror GPUI bounds and the current intersected content mask.
