---
agentx:
  kind: decision
  scope: decisions/platform/web
  status: current
  created: 2026-06-29T21:28:38+02:00
  updated: 2026-06-29T21:28:38+02:00
  topics:
    - runtime
    - gpux-foundation
    - wasm-svg
  supersedes: []
  superseded_by: []
---

## Decision: Load browser SVG assets through local WASM asset futures

### Context: Browser-rendered default icons and local SVG images needed GPUI to load bytes from served `assets/...` URLs. The existing GPUI `Asset` API required `Send` futures and spawned asset loads on the background executor, which does not fit browser-local `web_sys`/HTTP asset futures.

### Alternatives considered: Keep all SVG rendering on synchronous `AssetSource::load`; add RayX-specific icon fetch code outside GPUI; require WASM asset futures to remain `Send`; make the browser icon code use remote URLs only.

### Reasoning: GPUI already has `use_asset` and `Svg::external_path(...)` as the right abstraction for async render assets. Splitting the `Asset::load` future bound by target keeps native behavior unchanged while allowing WASM loaders to use browser-local futures. Spawning on the foreground executor on WASM preserves the browser execution model and lets the existing `use_asset` invalidation path redraw after fetch completion.

### Trade-offs accepted: GPUI now has target-specific `Asset::load` signatures and spawn paths. Browser SVG loading depends on the served app asset tree being correct.

### Supersedes: None
