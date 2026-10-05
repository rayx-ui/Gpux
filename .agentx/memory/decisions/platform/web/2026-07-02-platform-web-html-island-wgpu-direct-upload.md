---
agentx:
  kind: decision
  scope: decisions/platform/web
  status: superseded
  created: 2026-07-02T17:38:08+02:00
  updated: 2026-10-05T18:00:00+02:00
  topics:
    - runtime
    - gpux-foundation
    - html-island
    - wgpu
  supersedes: []
  superseded_by:
    - ../../repository/packaging/2026-10-05-repository-packaging-gpux-wgpu-fork.md
---

## Decision: Vendor wgpu for direct HTML island element-to-texture upload

### Context: Chrome Canary exposes experimental `GPUQueue.copyElementImageToTexture`, but the pinned Rust `wgpu` WebGPU backend did not expose that API. RayX `html_island` needed live canvas-child DOM pixels composited through the GPUI WebGPU scene without scratch canvases, cloned DOM, proxy roots, or fallback upload surfaces.

### Alternatives considered: Keep the fail-closed upload path until upstream `wgpu` exposes the API; add a private raw-handle bridge outside `wgpu`; vendor the pinned Zed `wgpu` workspace and add a narrow public queue method for the experimental API.

### Reasoning: The cleanest end-state API for gpui_wgpu is `wgpu::Queue::copy_element_image_to_texture(...)`; it keeps raw browser handles inside the WebGPU backend instead of leaking backend internals into gpui_wgpu. Vendoring the exact pinned `wgpu` workspace lets RayX carry the small experimental patch while preserving the existing Zed dependency source and package topology. The generated WASM web wrapper is its own temporary workspace, so xtask must emit the same patch entry and remove stale generated lockfiles so the patched backend is actually used in browser builds.

### Trade-offs accepted: The vendored `wgpu` workspace increases repository size and must be maintained during future gpux/wgpu updates. The API is Chrome Canary experimental and its dictionary shape is not the same as standard `copyExternalImageToTexture`; future browser changes may require updating the vendored mapper and Canary tests.

### Supersedes: [HTML island canonical roots](2026-07-02-platform-web-html-island-canonical-roots.md), for the prior fail-closed upload-bridge assumption only.
