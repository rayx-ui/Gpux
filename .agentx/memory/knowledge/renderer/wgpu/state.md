---
agentx:
  kind: state
  scope: knowledge/renderer/wgpu
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-14T12:00:00+02:00
  topics:
    - wgpu-renderer
    - wgsl
    - surfaces
    - resize
    - shader-effects
    - snapshot-transitions
    - webgl-fallback
    - srgb-compositing
    - border-antialiasing
  supersedes: []
  superseded_by: []
---
# WGPU Renderer State

## Purpose And Usage

Read before changing `gpui_wgpu` pipelines, shaders, surface configuration, resize handling, effects, or the browser backend transport.

## Current Technical Shape

Surfaces are configured with an empty `view_formats` list on a non-sRGB format (`Bgra8Unorm`, then `Rgba8Unorm`, then any non-sRGB format); an sRGB-only surface logs a warning. Paint is premultiplied encoded sRGB, so fixed-function blending and in-shader compositing (border over fill, coverage, gradients) happen in encoded sRGB. Linear-light math happens only inside effects that average light: blur taps decode with `premultiplied_srgb_to_linear`, average, and re-encode with `premultiplied_linear_to_srgb`. Images, HTML islands, and video already hold encoded sRGB and composite as sampled.

Quad border edges pass inner and outer coverage through `border_edge_coverage`, the grayscale-text gamma/contrast correction keyed on the border color, so 1 px rounded borders do not bead; WGPU reads `gamma_params` (gamma 1.8, contrast 1.0, `ZED_FONTS_*` overrides). Transformed borderless quads expand their raster bounds by one analytic-antialias pixel before evaluating signed-distance coverage; axis-aligned quads keep the zero-margin fast path.

Live resize coalesces drawable-size changes into scheduled frames; a frame with a pending drawable or canvas backing-store change must present and force a fresh GPUI draw, so surface size and scene geometry come from the same sample. Native surface reconfiguration waits for prior submitted work before `Surface::configure`; WASM polls without blocking. `WgpuRenderer::gpu_specs()` uses target-specific helpers so WASM never reads native-only backend policy fields. Dual-source blending is reported only when negotiated and not on the WebGL instance-data path.

Effects: element effects render through offscreen effect targets; backdrop blur samples a full-resolution source with a normalized multi-tap kernel (glass-blur/fast-Gaussian look); `glass(...)` is Liquid Glass refraction from an OverShifted-style squircle SDF (`sdSquircle(..., 3.0, 1.0)`, sample scale `pow(liquid_glass_refract(dist), 1.779)`, then noise/glow/tint); glow composites in a separate additive color pass that preserves destination alpha, so a glow over white saturates away and shows only on darker backdrops. Screen ripples and snapshot transitions (`crossfade`, `blur_crossfade`, `radial_reveal`, `ripple_reveal`) share one packed storage struct with a `kind` tag. The renderer retains a previous-frame color texture and copies it into per-snapshot textures when `Scene.active_screen_effect_snapshots` requests new ids; if an encoder overflows and is discarded, newly copied ids are marked uncaptured before retry.

Native GL (Android emulators, Linux GL) uses the WebGL2 limits, the texture instance path when the adapter lacks `VERTEX_STORAGE` or the device limits allow fewer than two storage buffers per stage (always under the WebGL2 limits), and `Rgba8Unorm` atlases; gamma parameters live in their own uniform buffer. Sampling fragment shaders fold clip and mask tests into their result instead of returning early. `render_to_image` reads the surface back when it supports `COPY_SRC` and otherwise renders the scene into a surface-sized texture and reads that back, reporting validation errors (native builds wait on an error scope).

Browser backends: startup prefers WebGPU and retries with WebGL2. WebGPU moves packed scene instances through storage buffers; WebGL2 carries the same complete record ABI (effects, HTML islands, paths, sprites, auxiliary families) through exact word-addressable `Rgba32Uint` textures. Adapter selection is surface-compatible, a failed WebGPU canvas is removed before the WebGL surface is created, and `?backend=webgl` / `?backend=webgpu` force one backend for diagnostics. HTML islands composite through a dedicated `html_islands` pipeline (upload path under [platform/web](../../platform/web/state.md)).

Ordinary presentation still renders through an intermediate retained-scene texture plus a pass-through pass, which snapshot transitions depend on.

Headless rendering (with `test-support` or `bench-support`, native only) is gpux's port of Zed #64718/#64842: `WgpuResources.surface` is optional, and `WgpuHeadlessRenderer` wraps a surface-free `WgpuRenderer` that shares the windowed pipelines, effects and `encode_scene_to_view`, rendering into per-size `COPY_SRC` targets in `Bgra8Unorm` or `Rgba8Unorm`. Captures composite over opaque black with 4x path sampling on every adapter. `WgpuContext::new_headless` caches one device per `HeadlessAdapterSelection` (`GPUX_WGPU_ADAPTER=software|hardware|<name>`, honoured only by headless contexts; `native_instance_flags()` also reads `WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER` (any value but `0`) into the instance flags of `create_headless` and `instance_with_backend_policy`, the one `WGPU_*` variable gpux reads, which is how Mesa's non-conformant `dzn` shows up under WSL: [decision](../../../decisions/renderer/wgpu/2026-10-14-renderer-wgpu-noncompliant-adapter-variable.md)) in a never-dropped static with one `DeviceErrorState`; each headless renderer observes errors from the generation it joined at, while windowed renderers keep their own uncaptured-error callback. `gpui_platform::current_headless_renderer()` returns it on Linux (Vulkan|GL, lavapipe without a GPU) and Windows (DX12, WARP without a GPU); macOS keeps Metal. The software selection prefers Vulkan, so on Linux it lands on lavapipe only when the Vulkan loader enumerates it (Mesa 25.2 ships `/usr/share/vulkan/icd.d/lvp_icd.json`; CI and WSL point `VK_DRIVER_FILES` at it) and otherwise falls back to llvmpipe over GL. `PlatformHeadlessRenderer::effect_capabilities()`/`gpu_specs()` reach test windows, and `TestAppContext::build_with_renderer` plus `VisualTestContext::capture_screenshot()` give tests real pixels.

Composition overlays on wgpu platforms use `WgpuRenderer::new_sharing` / `replace_surface_sharing_context` (`gpui_wgpu/src/fast/composition.rs`): a premultiplied-alpha surface renderer sharing the window's GPU context, atlas and pipelines (pipelines do not depend on the surface alpha mode).

`new_sharing_target` makes a sharing renderer on a non-window `SurfaceTargetUnsafe` (DirectComposition visuals) and `new_sharing_with_context` on a surface the caller made on a given context (the browser, whose window renderer keeps no context). A transparent sharing renderer reconfigures itself with its overlay alpha mode (`use_overlay_alpha_mode`): `PreMultiplied` on browser WebGPU, whose surface capabilities report only `Opaque`. On Metal (iOS overlays) it is `PostMultiplied` when the surface offers that and not `PreMultiplied`: either makes the `CAMetalLayer` non-opaque, which Core Animation composites as premultiplied. Other transparent surfaces, a transparent browser window's own canvas included, keep the mode `new_internal` picks.

Scroll-layer tiles (`gpui_wgpu/src/fast/layers/`, adapted from gpui-fast): `WgpuRenderer::fast_layers` holds a `TileCache` of render-target-format textures keyed by layer and tile and tagged with the content generation they hold. `raster::plan` runs once per frame in `encode_scene_to_view` (the cache's `begin_frame` applies the generation contract and LRU eviction within `gpux::LAYER_TILE_BUDGET_BYTES`), and `raster::rasterize` encodes the planned tiles inside the overflow retry loop before `main_pass`, through `draw_quads`/`draw_shadows`/`draw_underlines`/`draw_*_sprites` with per-tile-size globals. `draw_polychrome_sprites` forwards reserved texture ids to `composite::draw`, a run per tile. The polychrome fragment shader takes a texture as premultiplied when `PolychromeSprite.pad` is `LAYER_TILE_PREMULTIPLIED`. Each renderer has its own cache, so a composed window's overlay renderers do too. `WgpuHeadlessRenderer` and the Linux, Windows, browser, Android and iOS windows answer `rasterizes_layer_tiles()`. `WgpuRenderer::release_layer_tiles` clears the cache (`PlatformWindow::release_layer_tiles` calls it on mobile and the web): the next frame's `begin_frame` finds every composited tile missing and plans it from the layer frames the scene carries. Under `diagnostics`, `layer_tile_counters` reports tiles held, rasterized and releases.

Transformed sprites (turned or scaled glyphs and images) are filtered by hand within their atlas tile (`sample_tile` in `shaders.wgsl`, mirrored in the Metal shader): four `textureLoad`s by index clamped to the tile, mixed in float, from the position within the tile. The atlases pack tiles with no gutter, so the sampler filtered a fragment at the edge in the neighbouring tile and rounded its weights by where the atlas placed the tile; the output then depended on what else the atlas held. Untransformed sprites and layer tile sprites keep the sampler at their fragment's own position (in WGSL the four loads sit in a branch that only transformed fragments take and assign, and the one return comes last, because some GL ES drivers drop fragments of a shader that returns early after sampling; Metal returns early, which is no GL ES path).

## Backlinks

- [Headless renderer tests](../../../../../crates/gpui_wgpu/tests/headless_renderer.rs)
- [Headless renderer GPU-error tests](../../../../../crates/gpui_wgpu/tests/headless_renderer_gpu_errors.rs)
- [WGPU renderer](../../../../../crates/gpui_wgpu/src/wgpu_renderer.rs)
- [WGPU composition overlays](../../../../../crates/gpui_wgpu/src/fast/composition.rs)
- [WGPU scroll-layer tiles](../../../../../crates/gpui_wgpu/src/fast/layers/mod.rs)
- [WGPU tile tests](../../../../../crates/gpui_wgpu/src/fast/layers/tests.rs)
- [WGPU context](../../../../../crates/gpui_wgpu/src/wgpu_context.rs)
- [Shaders](../../../../../crates/gpui_wgpu/src/shaders.wgsl)
- [Storage-buffer shaders](../../../../../crates/gpui_wgpu/src/shaders_storage.wgsl)
- [WebGL shaders](../../../../../crates/gpui_wgpu/src/shaders_webgl.wgsl)
- [HTML island texture](../../../../../crates/gpui_wgpu/src/html_island_texture.rs)
- 2026-09-30 current [Encoded sRGB compositing](../../../decisions/renderer/wgpu/2026-09-30-renderer-wgpu-srgb-compositing.md)
- 2026-08-07 current [WebGPU-first with full WebGL2 fallback](../../../decisions/renderer/wgpu/2026-08-07-renderer-wgpu-browser-backend-fallback.md)
- 2026-07-04 current [Shader effect backend capabilities](../../../decisions/renderer/wgpu/2026-07-04-renderer-wgpu-shader-effects-backend-capabilities.md)
- 2026-07-16 superseded [Downlevel surface formats](../../../decisions/renderer/wgpu/2026-07-16-renderer-wgpu-downlevel-surface-formats.md)
