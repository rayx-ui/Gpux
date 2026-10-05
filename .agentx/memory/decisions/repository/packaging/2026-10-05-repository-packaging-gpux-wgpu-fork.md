---
agentx:
  kind: decision
  scope: decisions/repository/packaging
  status: current
  created: 2026-10-05T18:00:00+02:00
  updated: 2026-10-05T18:00:00+02:00
  topics:
    - gpux
    - wgpu
    - dx12
    - html-island
    - crates-io
  supersedes:
    - ../../platform/web/2026-07-02-platform-web-html-island-wgpu-direct-upload.md
  superseded_by: []
---

## Decision: wgpu 30 from crates.io through a minimal published `gpux-wgpu` fork, with the element-image upload owned by gpux

### Context: gpux built against a vendored copy of Zed's wgpu v29 branch patched through `[patch."https://github.com/zed-industries/wgpu.git"]` in the gpux workspace, the RayX root and every generated WASM manifest. The tree carried RayX's `Queue::copy_element_image_to_texture` API, a DX12 HWND `DXGI_SCALING_NONE` change and a suballocation shim. Published crates cannot use patches, and upstream wgpu 30.0.1 already contains Zed's changes and no longer needs the shim.

### Alternatives considered: Keep vendoring Zed's wgpu; depend on plain crates.io wgpu 30 and accept DX12 resize stretching until upstream changes; publish a fork of only wgpu-hal (dependents would still resolve upstream wgpu-hal); publish a fork of the whole wgpu-hal dependent chain.

### Reasoning: `vendor/wgpu` in Gpux holds seven crates copied from the crates.io 30.0.1 sources — wgpu, wgpu-core, wgpu-hal and the four wgpu-core-deps-* crates — renamed `gpux-*` at `30.0.1-gpux.1`, library targets only, whose one behavior change keeps HWND swap chains at `DXGI_SCALING_NONE` ([HWND scaling policy](../../platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dxgi-scaling.md)). Forking the chain is the only way a published gpux can reach the patched wgpu-hal; wgpu-types, naga and wgpu-naga-bridge stay upstream. The fork crates are Gpux workspace members, so no `[patch]` exists anywhere and generated WASM manifests need no wgpu entries. The HTML island upload no longer needs a wgpu API: gpui_wgpu calls Chrome's `GPUQueue.copyElementImageToTexture` through a wasm-bindgen structural binding on the handles from `Queue::as_webgpu()` / `Texture::as_webgpu()`, with the same dictionary shape and an explicit error on other backends. An upstream PR adds `SurfaceConfiguration::scaling_mode` with the supported modes in `SurfaceCapabilities::scaling_modes` (the design a wgpu maintainer proposed on gfx-rs/wgpu#10196; DX12 window surfaces list `OneToOne`, which maps to `DXGI_SCALING_NONE`); once a release ships it, gpux drops the fork and requests `OneToOne` where the surface lists it.

### Trade-offs accepted: Every wgpu upgrade recreates the fork (`.claude/skills/gpux/scripts/fork_wgpu.py` in Gpux) and republishes seven crates until upstream ships the option. The fork omits wgpu's own tests and examples. RayX and gpux code moved to the wgpu 30 API (`color_space`, `apply_limit_buckets`, `Queue::present`, fallible `get_mapped_range`).

### Supersedes: [HTML island wgpu direct upload](../../platform/web/2026-07-02-platform-web-html-island-wgpu-direct-upload.md): the direct element-to-texture upload stays, implemented in gpux instead of a vendored wgpu API.
