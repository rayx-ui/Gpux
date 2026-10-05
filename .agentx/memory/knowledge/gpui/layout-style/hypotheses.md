---
agentx:
  kind: hypotheses
  scope: knowledge/gpui/layout-style
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - taffy
    - style-generation
  supersedes: []
  superseded_by: []
---
# Layout And Style Hypotheses

- A sound caller-visible layout-style generation could let pooled Taffy requests skip `Style::to_taffy` conversion and retained-style equality when unchanged. All 1,123 warmed styles match, but pool position is not an identity contract; measure generation availability and maintenance cost before changing behavior.
