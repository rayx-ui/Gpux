---
agentx:
  kind: hypotheses
  scope: knowledge/gpui/frame-pipeline
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-10T19:30:00+02:00
  topics:
    - window-draw
    - element-id
  supersedes: []
  superseded_by: []
---
# Frame Pipeline Hypotheses

- Fresh global-element-id arc allocation and broad `Window::draw` orchestration remain top sampled costs after the 2026-08 loop; a design that avoids per-frame path allocation without interning state may pay off, but small inline storage is already rejected by the depth distribution.
- While a scroll layer's content is painted, `Window::content_mask()` is the painted region (viewport plus overscan), not the viewport. `list` takes its scrollport from the content mask and gpui-mobile's `platform_view_element` places its native surface by it: check both when lists get layers and when layers compile on mobile.
- A layer's content keeps atlas tiles across frames that do not paint them; the renderers' tile work needs layer records dropped when the sprite atlas clears.
