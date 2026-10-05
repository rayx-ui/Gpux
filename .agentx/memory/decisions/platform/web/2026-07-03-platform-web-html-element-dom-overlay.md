---
agentx:
  kind: decision
  scope: decisions/platform/web
  status: current
  created: 2026-07-03T18:30:00+02:00
  updated: 2026-07-03T18:30:00+02:00
  topics:
    - runtime
    - gpux-foundation
    - html-element
    - wasm
    - iframe
  supersedes: []
  superseded_by: []
---

## Decision: Add `html_element` as a real DOM overlay system, separate from `html_island`

### Context: `html_island` correctly keeps live DOM roots as direct children of the real GPUI canvas and uploads them through Chrome's experimental `copyElementImageToTexture` path, but current browser security behavior does not paint cross-origin iframe contents into canvas. RayX product scenarios such as payment/banking frames, sportsbook streams, casino games, YouTube embeds, and other provider-owned iframes need the browser iframe itself to remain live DOM.

### Alternatives considered: Extend `html_island` with iframe fallbacks; clone/scratch-canvas iframe DOM for upload; create duplicate DOM scroll containers; make this a Lab-only app helper; implement runtime DOM through a Rust HTML parser crate.

### Reasoning: Iframes and provider-owned content should not be treated as canvas texture sources. A separate `html_element` primitive lets GPUI remain the layout, scroll, clipping, and lifecycle source of truth while `gpui_web` uses `web_sys` to create a sibling DOM overlay above the canvas. The web registry should key roots by stable id, replace HTML only on content revision changes, sync geometry every frame, and use explicit keep-alive parking for video/game/payment iframes that must survive rerenders or temporary absence.

### Trade-offs accepted: DOM overlays are always above the GPUI canvas, so GPUI cannot visually draw over iframe content in V1. Cross-origin iframe internals remain opaque, iframe wheel/focus behavior is browser/provider-owned, and app authors must manage stable ids, provider frame policy, and keep-alive resource use.

### Supersedes: None
