---
agentx:
  kind: hypotheses
  scope: knowledge/platform/desktop
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-19T12:38:00+02:00
  topics:
    - metal
  supersedes: []
  superseded_by: []
---
# Desktop Hypotheses

- Settled 2026-10-19: the Metal border edge coverage port matches WGPU. `paint-contract-border-beading` measured the same coefficients on Metal (lane C at 1x and 2x, lane D at 2x) and WebGPU (lane B): white 0.109, accent 0.147, white 15% 0.095 at 1x (limits 0.15, 0.17, 0.12) and 0.036, 0.040, 0.040 at 2x (limit 0.06), so the hard-coded `BORDER_GAMMA_RATIOS`/`BORDER_ENHANCED_CONTRAST` stand.
