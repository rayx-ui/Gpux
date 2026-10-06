---
agentx:
  kind: rules
  scope: knowledge/renderer/wgpu
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-06T21:22:44+02:00
  topics:
    - wgsl
    - abi
    - srgb-compositing
    - premultiplied-alpha
    - shader-effects
    - resize
    - webgl-fallback
  supersedes: []
  superseded_by: []
---
# WGPU Renderer Rules

- Keep ordinary paint premultiplied encoded sRGB (`PaintColor`) through scene and shader stages and render to a non-sRGB surface format. Do linear-light math only inside effects that average light. Do not reintroduce per-fragment HSL conversion.
- Fade or mask premultiplied paint by scaling all four channels. Scaling alpha alone leaves color that premultiplied source-over adds to whatever lies beneath.
- Update Rust packed structs and WGSL together, assert sizes and offsets in Rust tests, and run headed WebGPU validation; compile checks do not exercise Dawn layout validation.
- Match WGSL storage alignment to the Rust ABI, not merely byte size.
- Do not use WGSL compound assignment on swizzles.
- Keep backdrop blur full-resolution and smoothly filtered.
- Keep `backdrop_blur(...)` (glass-blur/fast Gaussian) and `glass(...)` (Liquid Glass refraction) as separate responsibilities, and keep Liquid Glass on the squircle-SDF coordinate-refraction model rather than edge-only bevel displacement.
- Composite glow through the additive color pass that preserves destination alpha; the source-replacement pass ignores glow.
- Keep hard-edged demo backdrop geometry outside the interior of blur and glass samples so examples do not show fake rectangular holes.
- Keep shader-effect support capability-driven: non-WGPU backends keep effect flags false until their native path renders the effects.
- Coalesce live-resize drawable changes into the scheduled frame path; a pending size change must present and force a fresh draw. Wait for prior GPU work before native surface reconfiguration; keep WASM resize nonblocking.
- Keep browser backend selection WebGPU-first with WebGL2 fallback and the full record ABI on WebGL2 through `Rgba32Uint` textures. Run the forced-WebGL and automatic-WebGPU Test Suite proof whenever transport bindings, record layouts, or backend selection change.
