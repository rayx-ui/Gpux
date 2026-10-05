---
agentx:
  kind: state
  scope: knowledge/renderer
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - renderer
    - gpui-wgpu
    - backends
  supersedes: []
  superseded_by: []
---
# Renderer State

## Purpose And Usage

Read before changing how scene primitives become pixels on any backend.

## Current Technical Shape

`gpui_wgpu::WgpuRenderer` renders every platform except macOS: Windows (DX12), Linux (Wayland/X11), the browser (WebGPU with WebGL2 fallback), Android (Vulkan preferred with GL fallback), and iOS (Metal backend through wgpu). macOS keeps GPUI's native Metal renderer in `gpui_macos`. wgpu itself comes from the temporary `gpux-wgpu` fork (see [packaging](../repository/packaging/state.md)).

Scene construction prepares each `gpux::Color` into its cached `PaintColor` (premultiplied, gamma-encoded sRGB), and the renderer composites in encoded sRGB like Figma, CSS, and the Metal renderer. Shader effects are a WGPU-only capability; other backends report it unsupported.

## Backlinks

- [WGPU renderer](../../../../crates/gpui_wgpu/src/wgpu_renderer.rs)
- [macOS platform crate](../../../../crates/gpui_macos)
- [gpux_color paint](../../../../crates/gpux_color/src/color/paint.rs)
- 2026-09-30 current [Encoded sRGB compositing](../../decisions/renderer/wgpu/2026-09-30-renderer-wgpu-srgb-compositing.md)
