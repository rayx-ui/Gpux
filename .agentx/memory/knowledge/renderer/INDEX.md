---
agentx:
  kind: index
  scope: knowledge/renderer
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - renderer
    - gpui-wgpu
    - gpux-color
  supersedes: []
  superseded_by: []
---
# Renderer Domain Index

Scene-to-pixels code: the shared `gpui_wgpu` renderer (package `gpux-wgpu-renderer`) and the `gpux_color` standards color engine that feeds it. The macOS Metal renderer is described under [platform/desktop](../platform/desktop/INDEX.md).

- [state](state.md) · [knowledge](knowledge.md) · [rules](rules.md) · [hypotheses](hypotheses.md)

## Subdomains

- [wgpu](wgpu/INDEX.md): `WgpuRenderer`, WGSL shaders and record ABI, surfaces, effects pipelines, browser WebGPU/WebGL2 transport.
- [color](color/INDEX.md): `gpux_color::Color`, `PaintColor`, and the encoded-sRGB paint contract.
