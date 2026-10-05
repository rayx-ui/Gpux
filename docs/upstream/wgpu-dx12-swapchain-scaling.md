# Upstream: DX12 swap chain scaling

The `gpux-wgpu` fork in `vendor/wgpu` exists only because wgpu creates DX12 window swap chains
with `DXGI_SCALING_STRETCH`. This file tracks the upstream change that makes the fork unnecessary.

| | |
|---|---|
| Upstream issue | [gfx-rs/wgpu#10196](https://github.com/gfx-rs/wgpu/issues/10196) (swap chain scaling control) |
| Upstream change | `[dx12] Add Dx12BackendOptions::swapchain_scaling`, branch `dx12-swapchain-scaling` on trunk `14685942a` |
| Pull request | Not opened yet |

When a wgpu release ships the option: delete `vendor/wgpu`, depend on the crates.io `wgpu`, and
set `swapchain_scaling: wgpu::Dx12SwapchainScaling::None` in the `Dx12BackendOptions` that
`WgpuContext::instance_with_backend_policy` (`crates/gpui_wgpu/src/wgpu_context.rs`) passes when it
creates the wgpu instance.

## Pull request text

Title: `[dx12] Add Dx12BackendOptions::swapchain_scaling`

wgpu asks contributors to write significant descriptions by hand and to disclose LLM use; treat
the draft below as notes to rewrite, and fill in the LLM section accurately before submitting.

### Connections

Part of #10196. That issue asks for swap chain scaling control so applications get consistent
behavior across backends; this PR adds the DX12 part. Vulkan could follow with
`VK_EXT_swapchain_maintenance1` (`VkSwapchainPresentScalingCreateInfoEXT`).

### Description

DX12 swap chains created from a window handle always use `DXGI_SCALING_STRETCH`. Between a window
resize and the next `Surface::configure`, DXGI stretches the previous frame to the new window size,
so applications that redraw at the new size (UI toolkits in particular) wobble and smear during
live resizes.

This adds `Dx12BackendOptions::swapchain_scaling` (`Dx12SwapchainScaling::{Stretch, None}`) and
the `WGPU_DX12_SWAPCHAIN_SCALING` environment variable. `None` creates window swap chains with
`DXGI_SCALING_NONE`: the swap chain is presented unscaled at the window's top-left corner, and any
area it does not cover yet shows the window background. The default stays `Stretch`, so nothing
changes unless an application opts in. Composition swap chains (`DxgiFromVisual`, visuals,
`SwapChainPanel`, surface handles) only support stretching and keep it.

### Testing

- `cargo clippy -p wgpu-types -p wgpu-hal --features wgpu-hal/dx12 -- -D warnings`
- `RUSTDOCFLAGS="-D warnings" cargo doc -p wgpu-types --no-deps`
- The default (`Stretch`) takes the unchanged code path.
- gpux, a downstream GPUI fork, has created its Windows window swap chains with
  `DXGI_SCALING_NONE` on DX12 (Windows 11) through a patched wgpu.

### Squash or Rebase?

Single commit.

### LLM Use?

_Fill in before submitting._
