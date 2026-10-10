---
agentx:
  kind: hypotheses
  scope: knowledge/gpui/frame-pipeline
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-10T23:30:00+02:00
  topics:
    - window-draw
    - element-id
  supersedes: []
  superseded_by: []
---
# Frame Pipeline Hypotheses

- Fresh global-element-id arc allocation and broad `Window::draw` orchestration remain top sampled costs after the 2026-08 loop; a design that avoids per-frame path allocation without interning state may pay off, but small inline storage is already rejected by the depth distribution.
- While a scroll layer's content is painted, `Window::content_mask()` is the painted region (viewport plus overscan), not the viewport. `list` takes its scrollport from the content mask and gpui-mobile's `platform_view_element` places its native surface by it: check both when lists get layers and when layers compile on mobile.
- A scene drops a shadow when its box leaves its content mask though its blur reaches three radii further, so a blur pops off at a viewport's edge; a scroll layer's tiles keep it (the one pixel difference between a layered frame and one drawn without layers, bounded in `lane_c_scroll_layers`). Removing it needs the kept shadow ordered as it is while its box shows: over the backgrounds under it, under neighbours its box does not overlap. A scene keeps only bounds clipped to masks, which cannot tell the two apart; ordering by reach is exact but chains draw orders (a batch per shadowed element) and puts a later sibling's blur over an earlier one, which changes how tight layouts look.
- Removing an image or SVG from the sprite atlas drops every scroll layer's content in the window, so an image cache that evicts while a list scrolls keeps that list's layer from compositing. Dropping only the layers whose content names the removed tile, or hashing something that identifies a tile's content, would keep the others.
