---
agentx:
  kind: decision
  scope: decisions/platform/web
  status: current
  created: 2026-07-02T17:38:08+02:00
  updated: 2026-07-02T17:38:08+02:00
  topics:
    - runtime
    - gpux-foundation
    - html-island
  supersedes: []
  superseded_by: []
---

## Decision: Keep HTML island canonical roots under the real GPUI canvas

### Context: Chrome Canary validation showed the HTML island story rendering correctly while DevTools also showed several additional canvas elements. The HTML-in-canvas API requires rendered elements to be direct children of the canvas whose layout subtree and paint APIs are used, so the DOM topology needed a durable interpretation.

### Alternatives considered: Treat the extra canvases as acceptable host surfaces; keep canonical roots under the real GPUI canvas and treat extra canvases as temporary upload staging; remove the staging path immediately and require direct WebGPU `copyElementImageToTexture`.

### Reasoning: The HTML-in-canvas API model requires the rendered DOM source to be a direct child of the canvas whose layout subtree and paint APIs are used. Scratch canvases and cloned DOM are not an acceptable architectural host surface for RayX `html_island`, even as a transient upload workaround. Current Rust `wgpu` exposes standard external image copies but not the experimental `GPUQueue.copyElementImageToTexture(element, ...)` API for arbitrary DOM elements, so the correct near-term behavior is to keep live island roots directly under the real GPUI canvas and fail closed with explicit diagnostics until a direct `wgpu`/raw-handle bridge exists.

### Trade-offs accepted: HTML island DOM-owned input and accessibility can work through the live direct roots, but HTML island pixels are not composited through the WebGPU scene until direct element-to-texture upload is implemented. This avoids preserving a misleading clone/canvas fallback that contradicts the agreed API shape.

### Supersedes: None

### Superseded by: [HTML island wgpu direct upload](2026-07-02-platform-web-html-island-wgpu-direct-upload.md), for the fail-closed upload-bridge assumption only.
