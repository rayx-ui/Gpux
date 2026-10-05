---
agentx:
  kind: hypotheses
  scope: knowledge/renderer/wgpu
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - presentation-pass
    - telemetry
  supersedes: []
  superseded_by: []
---
# WGPU Renderer Hypotheses

- Removing the intermediate retained-scene texture and pass-through pass could save a full-screen pass per frame, but snapshot transitions read that texture; it needs a new snapshot contract, not a local render-pass deletion.
