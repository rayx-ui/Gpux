---
agentx:
  kind: decision
  scope: decisions/renderer/wgpu
  status: current
  created: 2026-10-13T10:00:00+02:00
  updated: 2026-10-13T10:00:00+02:00
  topics:
    - gpux
    - wgpu
    - metal
    - shaders
    - fast-mode
  supersedes: []
  superseded_by: []
---

# Filter transformed sprites by hand within their tile, shade untransformed primitives from the fragment position

## Decision: Filter transformed sprites by hand within their tile, shade untransformed primitives from the fragment position

### Context: A tile must hold the bytes the frame would. The sampler filtered a transformed glyph or image at its tile's edge in the neighbouring tile of the atlas and rounded its weights by where the atlas placed it, so edge pixels depended on which other glyphs the atlas held; interpolated positions also differ in their last bits with what the rasterizer clips a primitive to.

### Alternatives considered: A gutter around atlas tiles; clamping texture coordinates; accepting a one-level difference.

### Reasoning: A transformed sprite is filtered by `sample_tile` (WGSL for the wgpu renderers, browser WebGPU and the WebGL2 transport; Metal for `gpui_apple`): four texels by index clamped to the tile, mixed in float from the position within the tile. An untransformed sprite keeps the sampler at its fragment's own position. Untransformed quads, shadows, underlines and sprites are shaded from the fragment position, and the gradient dither is seeded by the pixel within the quad.

### Trade-offs accepted: These apply in every mode and move edge pixels of transformed sprites and the last bits of a frame against gpux before them; committed goldens did not change.

### Supersedes: None.
