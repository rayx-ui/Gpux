---
agentx:
  kind: knowledge
  scope: knowledge/renderer/wgpu
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-06T21:22:44+02:00
  topics:
    - wgsl
    - abi
    - dawn
    - premultiplied-alpha
    - shader-effects
    - resize
    - telemetry
  supersedes: []
  superseded_by: []
---
# WGPU Renderer Knowledge

- Match Rust `Pod` structs to WGSL storage layout exactly, including alignment. A WGSL `vec3<f32>` padding member once made the HTML-island storage struct 64 bytes against Rust's 48-byte `PackedHtmlIsland`, causing Dawn validation errors and blank frames. `PaintColor` (16 bytes, 4-byte aligned) must be four scalar WGSL `f32` fields; a `vec4<f32>` has 16-byte alignment and changes enclosing sprite strides.
- Dawn rejects WGSL compound assignment into a swizzle (`glass_color.rgb += delta`); rebuild the vector instead.
- An sRGB render view blends in linear light and makes translucent white over dark surfaces far brighter than the design: white at 4.5% over `#0a0a0a` is `#151515` in Figma but `#3e3e3e` in linear blending.
- A nearest-sampled half-size blur source turns high-contrast backdrops into mosaic blocks.
- Expanded glow quads can overlap neighboring effects; returning sampled source color from them in the normal replacement pass overwrites glass/blur content with stale pixels.
- Resize artifacts came from presenting a surface size from one live-resize message with scene geometry from another.
- Border antialiasing proof: Test Suite `rayx-border-antialiasing.spec.ts` measured 1x beading drop from 0.23 to 0.11 (white), 0.20 to 0.14 (accent), and 0.16 to 0.10 (white 15%).
- From the premultiplied-paint migration until 2026-10-06, `fs_quad` faded dashed borders through alpha alone, so every gap added the border color over the fill or backdrop and dashed borders rendered nearly solid on all WGPU backends. Test Suite `rayx-dashed-borders.spec.ts` proves gaps show the backdrop or fill on WebGPU and forced WebGL2 at DPR 1 and 2.
- Frame telemetry for the warmed wide DataTable: see [frame-pipeline knowledge](../../gpui/frame-pipeline/knowledge.md).
- A WebGPU family-wide instance prepack reduced host operations to five writes and six bind groups but repeatedly regressed DPR 2 acknowledged throughput, so it was reverted.
