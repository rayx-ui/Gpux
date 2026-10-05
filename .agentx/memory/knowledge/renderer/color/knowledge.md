---
agentx:
  kind: knowledge
  scope: knowledge/renderer/color
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - gpux-color
    - paint-color
  supersedes: []
  superseded_by: []
---
# Color Knowledge

- Clones share per-space and `PaintColor` caches; every `with_*` result owns fresh caches. Equality and hashing include authored semantics and normalize negative zero, so equal finite colors hash identically; cache population never affects equality.
- Headed WASM proof measured 10,000 warm `paint_color()` reads with one normalization and no further conversions or Rust allocations.
- Until 2026-09-30 `PaintColor` held linear-light sRGB and surfaces used an sRGB view; it now holds encoded sRGB (see [encoded sRGB compositing](../../../decisions/renderer/wgpu/2026-09-30-renderer-wgpu-srgb-compositing.md)).
