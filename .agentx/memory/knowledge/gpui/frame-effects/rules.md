---
agentx:
  kind: rules
  scope: knowledge/gpui/frame-effects
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - frame-family
    - effect-frame
    - screen-effects
  supersedes: []
  superseded_by: []
---
# Frame Effects Rules

- Keep shader effects off broad `Styled`/`div()` APIs; use `effect_frame()` for element effects and screen-effect regions.
- Keep every frame-family element single-child with `.child(...)` or one progress-aware child source.
- Use stable semantic ids for `animated_frame(id)` and `transition_frame(id)`, never per-render, positional, or row-index values.
- Treat `transition_frame()` as image-space replacement, not tree diffing or shared-element morphing.
- Implement rounded clipping for frame-family elements at the scene/renderer clip level so child subtrees and effects inherit it.
- Do not release a retained screen-effect snapshot until every active transition referencing it has expired.
- Keep effect support capability-driven; a backend reports element, screen, or snapshot effect support only when its native path renders them.
