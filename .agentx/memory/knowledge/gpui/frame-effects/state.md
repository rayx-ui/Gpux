---
agentx:
  kind: state
  scope: knowledge/gpui/frame-effects
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-17T10:00:00+02:00
  topics:
    - frame-family
    - effect-frame
    - animated-frame
    - transition-frame
    - screen-effects
    - element-effects
    - layer-blur
  supersedes: []
  superseded_by: []
---
# Frame Effects State

## Purpose And Usage

Read before changing frame-family elements, `ElementEffect`, screen effects, or effect capability reporting. Usage examples live in the gpux skill's frame-family and shader-effect sections.

## Current Technical Shape

`frame()` is the lightweight single-child geometry/clip container; `effect_frame()` adds shader element effects and `screen_effect_region(...)`; `animated_frame(id)` interpolates frame-owned geometry and compositing properties for a stable id (with optional progress-aware child rendering); `transition_frame(id)` replaces a stable region's child through a retained screen-effect snapshot. All are single-child, not `ParentElement`. Broad `Styled`/`div()` exposes no `.effect(...)`, `.effects(...)`, `.clear_effects()`, or `.screen_effect_region(...)`. Rounded `.clip()` on a frame becomes inherited renderer clip state through `ContentMask` radii for child subtrees, effects, screen-effect regions, HTML islands/elements, sprites, text paths, and surfaces.

Element effects are emitted as `VisualEffectPrimitive`s from element bounds and corner radii and batched in `Scene`, so replay and retained painting keep order. `Window::paint_visual_effects` stores `ElementEffect::prepare().scale(scale_factor)`, so blur/glass radii and glow radius/spread reach the renderer in device pixels. `ElementEffect::LayerBlur` (`layer_blur(radius)`) blurs the frame's own content: it prepares to the backdrop blur pass and `EffectFrame` paints it after the child over bounds widened by `outset()` (1.5 × radius), while pre-content effects still paint before the child.

`Window` owns screen effects: `play_screen_effect(...)` queues `ScreenEffect`s in capped queues, `tick_screen_effects` prunes by `gpux::Instant`, retained snapshot ids have explicit release APIs, and `Window::effect_diagnostics()` reports active counts, registered and missing regions, retained and missing snapshots, and renderer capabilities. Effect support is capability-driven: WGPU implements it; Metal and other non-WGPU paths report false.

## Backlinks

- [Frame](../../../../../crates/gpui/src/elements/frame.rs)
- [Effect frame](../../../../../crates/gpui/src/elements/effect_frame.rs)
- [Animated frame](../../../../../crates/gpui/src/elements/animated_frame.rs)
- [Transition frame](../../../../../crates/gpui/src/elements/transition_frame.rs)
- [Shader effects](../../../../../crates/gpui/src/shader_effects/mod.rs)
- 2026-07-06 current [Frame and effects element split](../../../decisions/gpui/frame-effects/2026-07-06-gpui-frame-effects-frame-effects-elements.md)
- 2026-07-07 current [Frame-family naming](../../../decisions/gpui/frame-effects/2026-07-07-gpui-frame-effects-frame-family-naming.md)
- 2026-07-04 current [Shader effect backend capabilities](../../../decisions/renderer/wgpu/2026-07-04-renderer-wgpu-shader-effects-backend-capabilities.md)
