---
agentx:
  kind: rules
  scope: knowledge/renderer/wgpu
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-10T23:30:00+02:00
  topics:
    - wgsl
    - abi
    - srgb-compositing
    - shader-effects
    - resize
    - webgl-fallback
  supersedes: []
  superseded_by: []
---
# WGPU Renderer Rules

- Keep ordinary paint premultiplied encoded sRGB (`PaintColor`) through scene and shader stages and render to a non-sRGB surface format. Do linear-light math only inside effects that average light. Do not reintroduce per-fragment HSL conversion.
- Update Rust packed structs and WGSL together, assert sizes and offsets in Rust tests, and run lane B's WebGPU specs (`dawn.spec.ts` fails on any validation error); compile checks do not exercise Dawn layout validation.
- Match WGSL storage alignment to the Rust ABI, not merely byte size.
- Do not use WGSL compound assignment on swizzles.
- Keep backdrop blur full-resolution and smoothly filtered.
- Keep `backdrop_blur(...)` (glass-blur/fast Gaussian) and `glass(...)` (Liquid Glass refraction) as separate responsibilities, and keep Liquid Glass on the squircle-SDF coordinate-refraction model rather than edge-only bevel displacement.
- Composite glow through the additive color pass that preserves destination alpha; the source-replacement pass ignores glow.
- Keep hard-edged demo backdrop geometry outside the interior of blur and glass samples so examples do not show fake rectangular holes.
- Keep shader-effect support capability-driven: non-WGPU backends keep effect flags false until their native path renders the effects.
- Coalesce live-resize drawable changes into the scheduled frame path; a pending size change must present and force a fresh draw. Wait for prior GPU work before native surface reconfiguration; keep WASM resize nonblocking.
- Keep browser backend selection WebGPU-first with WebGL2 fallback and the full record ABI on WebGL2 through `Rgba32Uint` textures. Run lane B's forced-WebGL2 and automatic-WebGPU specs whenever transport bindings, record layouts, or backend selection change.
- Never return early from a fragment shader after `textureSample`: some GL ES drivers (Android emulators') drop every fragment of such a shader. Fold clip, mask, and kind tests into the result through `visible_if`; `tests/shader_sampling.rs` guards it.
- Choose GL behavior by capability: GL devices request WebGL2 limits on every platform, devices without `VERTEX_STORAGE` or whose device limits allow fewer than the two storage buffers per stage `instances_layout` binds read instance records from textures (decide from the device limits, not only the adapter's flags: a GL device's WebGL2 limits allow none even where ANGLE reports vertex storage), and GL prefers `Rgba8Unorm` color atlases because BGRA comes from an ES extension some drivers report but cannot sample.
- Bind each uniform block to its own buffer; GL drivers that bind two ranges of one buffer to two blocks give the second the wrong range.
- Shade an untransformed primitive from the fragment's own position (`local_fragment_position`, `local_tile_position`), not from a varying interpolated from the vertices: on Apple GPUs a varying differs in its last bits with what the rasterizer clips the primitive to, which a scroll-layer tile whose edge the primitive crosses shows as a one-level difference. Keep position-dependent noise (the gradient dither) a function of the whole pixel within the primitive. The Metal shader follows the same two rules.
- Scroll-layer tiles are drawn by the renderer's own batch functions: a batch function binds group 0 through `fast::layers::globals_bind_group`, never `globals_bind_group` directly, and forwards a reserved polychrome texture id to `fast::layers::composite`.
- Release the tile cache (`fast::layers::release`) wherever the target's size or alpha mode changes, the sprite atlas is cleared, or a frame that planned tiles is not submitted: `raster::plan` counts the tiles it planned as held.
- Raise GPU errors on a cached headless device only in `tests/headless_renderer_gpu_errors.rs`, one test per adapter selection: every renderer on that device observes them, so they fail concurrent tests that share it.
