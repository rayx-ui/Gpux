# Upstream: DX12 swap chain scaling

The `gpux-wgpu` fork in `vendor/wgpu` exists only because wgpu creates DX12 window swap chains
with `DXGI_SCALING_STRETCH`. This file tracks the upstream change that makes the fork unnecessary.

| | |
|---|---|
| Upstream issue | [gfx-rs/wgpu#10196](https://github.com/gfx-rs/wgpu/issues/10196) (swap chain scaling control) |
| Upstream change | `Add SurfaceConfiguration::scaling_mode`, commit `6ee057f81` on branch `surface-scaling-mode`, based on trunk `14685942a` |
| Pull request | Not opened yet |

The change follows the design a wgpu maintainer proposed on the issue: a scaling mode on
`SurfaceConfiguration`, the supported modes in `SurfaceCapabilities`, validation in wgpu-core, and
an `Auto` default that chooses stretching when the surface supports it and otherwise keeps the
platform's behavior.

When a wgpu release ships `SurfaceConfiguration::scaling_mode`: delete `vendor/wgpu`, depend on the
crates.io `wgpu`, and in the surface configuration of `WgpuRenderer`
(`crates/gpui_wgpu/src/wgpu_renderer.rs`) request `wgpu::SurfaceScalingMode::OneToOne` when the
surface's `SurfaceCapabilities::scaling_modes` contains it, and `Auto` otherwise. Only DX12 window
surfaces report `OneToOne`, which matches what the fork does today.

## Before opening the pull request

- Push the branch to a fork of gfx-rs/wgpu and open the pull request against `trunk`.
- Replace both `#XXXXX` placeholders in `CHANGELOG.md` with the pull request number, and check the
  `@dXnSix` handle in the same entries.
- Rewrite the description below in your own words and fill in the LLM section accurately; wgpu asks
  for hand-written descriptions and LLM disclosure.

## Pull request text

Title: `Add SurfaceConfiguration::scaling_mode`

### Connections

Part of #10196, following the design proposed there: a scaling mode on `SurfaceConfiguration`,
the supported modes in `SurfaceCapabilities`, validation in wgpu-core, and an `Auto` default.
This PR implements the modes for DX12; other backends report only `Auto` for now. Vulkan could
follow with `VK_EXT_swapchain_maintenance1` (`VkSwapchainPresentScalingCreateInfoEXT`), and Metal
with the layer's `contentsGravity`.

### Description

DX12 swap chains always use `DXGI_SCALING_STRETCH`. Between a window resize and the next
`Surface::configure`, DXGI stretches the previous frame to the new window size, so applications
that redraw at the new size (UI toolkits in particular) wobble and smear during live resizes.

This adds `SurfaceScalingMode::{Auto, Stretch, OneToOne}`, `SurfaceConfiguration::scaling_mode`,
and `SurfaceCapabilities::scaling_modes`:

- `Auto` (the default) resolves to `Stretch` when the surface lists it, and otherwise keeps the
  platform's behavior. Existing applications see no change.
- wgpu-core rejects a mode the surface does not list with
  `ConfigureSurfaceError::UnsupportedScalingMode`.
- DX12 lists `Stretch` and, for swap chains created from a window handle, `OneToOne`, which maps to
  `DXGI_SCALING_NONE`: the frame is presented unscaled at the window's top-left corner and the rest
  of the window shows its background color. Composition swap chains only support stretching.
- `ResizeBuffers` cannot change a swap chain's scaling, so reconfiguring DX12 with a different mode
  releases the swap chain and creates a new one.
- Vulkan, Metal, GLES and WebGPU list only `Auto`.

### Testing

- New wgpu-core unit tests: `cargo test -p wgpu-core --lib surface_configuration_tests`
  (`Auto` resolution with and without `Stretch`, explicit `OneToOne`, the unsupported-mode error).
- `cargo clippy --workspace --all-targets` (excluding `deno_webgpu` and `cts_runner`, whose V8
  build needs symlink privileges on this Windows machine).
- `RUSTDOCFLAGS="-D warnings" cargo doc -p wgpu-types -p wgpu-hal -p wgpu-core -p wgpu --no-deps`.
- On Windows 11 (DX12), a temporary build of `02_hello_window` alternated `OneToOne` and `Auto`
  across resizes and read the swap chain back with `IDXGISwapChain1::GetDesc1`: `OneToOne`
  produced `DXGI_SCALING_NONE`, `Auto` produced `DXGI_SCALING_STRETCH`, mode changes recreated the
  swap chain without errors, and the window closed cleanly.
- gpux, a downstream GPUI fork, has presented its Windows windows through `DXGI_SCALING_NONE`
  window swap chains on DX12 with a patched wgpu.

### Squash or Rebase?

Single commit.

### LLM Use?

_Fill in before submitting._
