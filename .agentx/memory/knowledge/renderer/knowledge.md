---
agentx:
  kind: knowledge
  scope: knowledge/renderer
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - renderer
  supersedes: []
  superseded_by: []
---
# Renderer Knowledge

- The renderer record ABI is shared by Rust `Pod` structs, WGSL storage layouts, the WebGL2 texture transport, and (for common primitives) Metal; a layout change touches all of them. See [wgpu knowledge](wgpu/knowledge.md).
