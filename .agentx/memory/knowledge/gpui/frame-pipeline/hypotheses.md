---
agentx:
  kind: hypotheses
  scope: knowledge/gpui/frame-pipeline
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - window-draw
    - element-id
  supersedes: []
  superseded_by: []
---
# Frame Pipeline Hypotheses

- Fresh global-element-id arc allocation and broad `Window::draw` orchestration remain top sampled costs after the 2026-08 loop; a design that avoids per-frame path allocation without interning state may pay off, but small inline storage is already rejected by the depth distribution.
