---
agentx:
  kind: rules
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
# Color Rules

- Keep `gpux_color` the single standards color owner, free of production dependencies and UI-neutral.
- Keep `ClipSrgb` the only paint normalization policy and normalize at most once per immutable `Color`; do not route the ordinary renderer path through the iterative CSS gamut mapper.
- Keep `PaintColor` exactly four `f32` in `#[repr(C)]`; any change is a renderer ABI change (see [WGPU rules](../wgpu/rules.md)).
