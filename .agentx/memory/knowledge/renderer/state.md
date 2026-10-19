---
agentx:
  kind: state
  scope: knowledge/renderer
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-19T12:38:00+02:00
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

`gpui_wgpu::WgpuRenderer` renders every platform except macOS: Windows (DX12), Linux (Wayland/X11), the browser (WebGPU with WebGL2 fallback), Android (Vulkan preferred with GL fallback), and iOS (Metal backend through wgpu). macOS keeps GPUI's native Metal renderer (`gpui_apple::MetalRenderer`, hosted by `gpui_macos`). wgpu itself comes from the temporary `gpux-wgpu` fork (see [packaging](../repository/packaging/state.md)).

Scene construction prepares each `gpux::Color` into its cached `PaintColor` (premultiplied, gamma-encoded sRGB), and the renderer composites in encoded sRGB like Figma, CSS, and the Metal renderer. Shader effects are a WGPU-only capability; other backends report it unsupported. The `paint-contract-*` conformance scenes probe inset shadows, gradient interpolation, pattern backgrounds, image alpha and 1 px border beading against values derived from the contract, not goldens, so the same numbers judge Metal (lanes C, D) and WGPU (lanes B, C, E, F); on 2026-10-19 Metal passed them in lanes C and D and WGPU on WebGPU in lane B, unchanged.

Both renderers rasterize scroll-layer tiles (`fast/layers/` in `gpui_wgpu` and `gpui_apple`): a tile cache per renderer, tiles drawn before the frame's first pass by the renderer's own batch functions, composited as polychrome sprites flagged premultiplied. The Metal renderer encodes the tiles first in the frame's command buffer and shares its instance buffer; a failed command buffer releases the cache on the next frame. Lane C's `lane_c_scroll_layers` compares views scrolled from tiles with a window without layers, pixel for pixel, on every backend.

## Backlinks

- [WGPU renderer](../../../../crates/gpui_wgpu/src/wgpu_renderer.rs)
- [macOS platform crate](../../../../crates/gpui_macos)
- [Metal renderer](../../../../crates/gpui_apple/src/metal_renderer.rs)
- [Metal scroll-layer tiles](../../../../crates/gpui_apple/src/fast/layers/mod.rs)
- [Lane C scroll layers](../../../../crates/gpux_tests_conformance/tests/lane_c_scroll_layers.rs)
- [Paint-contract scenes](../../../../crates/gpux_tests_conformance/src/scenes/paint_contract.rs)
- [gpux_color paint](../../../../crates/gpux_color/src/color/paint.rs)
- 2026-09-30 current [Encoded sRGB compositing](../../decisions/renderer/wgpu/2026-09-30-renderer-wgpu-srgb-compositing.md)
