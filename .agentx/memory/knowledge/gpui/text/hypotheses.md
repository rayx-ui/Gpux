---
agentx:
  kind: hypotheses
  scope: knowledge/gpui/text
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - text-layout
    - glyph-paint
    - wasm-text-quality
  supersedes: []
  superseded_by: []
---
# Text Hypotheses

- Reconstructing one ephemeral `TextLayout` per measured leaf each frame is the dominant remaining warm text cost; a sound cross-frame owner with explicit invalidation could remove the 229 first-use layouts.
- Warm glyph paint still performs separate `TextSystem` raster-bounds and atlas cache lookups; a fused lookup may cut paint cost.
- Browser glyph quality at native device scale is unresolved: the 2026-08-04 WASM text quality loop rejected sampling-phase, native-alpha, hinting-alignment, local-supersampling, and browser-LCD candidates, and the [high-quality WASM text epic](../../../../tasks/high-quality-wasm-text-rendering/requirements.json) is blocked.
