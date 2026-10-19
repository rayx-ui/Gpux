---
agentx:
  kind: decision
  scope: decisions/renderer/wgpu
  status: current
  created: 2026-09-30T00:00:00+02:00
  updated: 2026-10-19T12:38:00+02:00
  topics:
    - gpux
    - wgpu
    - color
    - blending
    - surface-formats
  supersedes:
    - 2026-07-16-renderer-wgpu-downlevel-surface-formats.md
  superseded_by: []
---

## Decision: Composite WGPU paint in encoded sRGB, like Figma, CSS and the Metal renderer

### Context: After the DS 1.2 shadcn tuning, translucent tokens (neutral fills white 4.5%, borders white 15%, disabled veils white 9%) rendered far brighter in RayX than in Figma. `PaintColor` was premultiplied linear-light sRGB and the WGPU target an sRGB view, so the GPU blended in linear light: white 4.5% over #0a0a0a became #3e3e3e instead of Figma's #151515. The macOS Metal renderer already rendered to BGRA8Unorm and matched.

### Alternatives considered: Keep linear blending and retune every translucent token (impossible to match Figma over every backdrop); make the blend space a renderer option; or composite in encoded sRGB and keep linear light only where effects average light.

### Reasoning: Design tools, browsers and CSS blend modes all composite encoded sRGB values, so RayX must as well for designs to match. `PaintColor` is now premultiplied encoded sRGB (`PaintGamutPolicy::ClipSrgb`), every adapter renders to a non-sRGB surface format with no alternate views, images/HTML islands/video are composited as sampled, OKLab gradients re-encode, and blur taps decode to linear, average and re-encode. Future Photoshop-style blend modes follow the W3C formulas on encoded values; a linear variant can decode per mode.

### Trade-offs accepted: Anti-aliased edges and text coverage blend in sRGB (thinner, as in Zed and browsers); text and quad border edges (`border_edge_coverage` in WGPU `fs_quad` and Metal `quad_fragment`) apply the DirectWrite gamma/contrast correction so 1 px rounded borders do not bead (Test Suite `rayx-border-antialiasing.spec.ts`), while fill edges stay uncorrected; a surface that advertises only sRGB formats encodes paint twice and renders too light, which logs a warning.

### Supersedes: `2026-07-16-runtime-gpux-foundation-downlevel-surface-formats.md` (every adapter now renders directly to a non-sRGB surface format).
