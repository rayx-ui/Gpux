---
agentx:
  kind: decision
  scope: decisions/gpui/frame-pipeline
  status: current
  created: 2026-10-13T10:00:00+02:00
  updated: 2026-10-13T10:00:00+02:00
  topics:
    - gpux
    - fast-mode
    - scroll-layers
    - tiles
  supersedes: []
  superseded_by: []
---

# Scroll layers composite tiles the renderer rasterizes once and never change the pixels

## Decision: Scroll layers composite tiles the renderer rasterizes once and never change the pixels

### Context: Scrolling a content of hundreds of primitives redraws all of them every frame; gpui-fast keeps the content in GPU tiles and composites them at the scroll offset.

### Alternatives considered: Retain the whole scene; cache per element texture; tiles in the core with a renderer-neutral contract.

### Reasoning: A scroll container's content is painted once into a layer, rasterized by the renderer into tiles (`scene::LayerFrame`, polychrome sprites at `LAYER_TILE_TEXTURE_BASE`), and composited on frames where only the offset changed. The policy promotes, demotes and bypasses per container (counted by reason: inactive, not promoted, demoted, ineligible, nested, moved); sticky content, backdrop effects, HTML islands and elements keep today's path. Tiles equal the pixels painted in place: offsets snap to device pixels while layers are on, a told clip's edge keeps what a shadow reaches (`edge_shadows`), a border tells one clip (`border_clips`), a transformed element's clip is cut by the viewport. Renderers keep tiles under a byte budget (64 MB, 32 MB in the browser and on mobile) and release them on resize, GPU errors, device loss and platform trim/background; the release hook never waits. The browser, Android and iOS use the wgpu tiles as they are.

### Trade-offs accepted: Telling the scene its clips costs scenes painted whole 1.2× (8000 bordered rows) to 1.46× (a 2000×20 cell table) against before; lists show no CPU gain in the browser (p95 +15–24%) though a `div` does (−4 to −16%); a layer gives up where the pointer's hover changes every row. Limits recorded in `docs/fast-mode.md`: fractional scale beyond about 50 000 device pixels, id-less hover rows, `layer_tile_id` assertion at about 1.05 M px.

### Supersedes: None.
