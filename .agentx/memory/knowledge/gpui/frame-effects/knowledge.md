---
agentx:
  kind: knowledge
  scope: knowledge/gpui/frame-effects
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - frame-family
    - screen-effects
    - element-effects
  supersedes: []
  superseded_by: []
---
# Frame Effects Knowledge

- Element effects were packed in logical pixels until 2026-09-28, which rendered blur/glass radii and glow at half width on DPR 2; scaling at `paint_visual_effects` fixed it.
- A frame-local rounded-clip special case is insufficient because a child can render through any GPUI primitive path; the clip must be inherited content-mask state.
- RayX applies `layer_blur` through `rayx_theme::Blurred::blur_style` for design-system layer-blur styles.
- Consumer helpers schedule snapshot release after the transition duration; manual gpux callers must release retained snapshots explicitly.
