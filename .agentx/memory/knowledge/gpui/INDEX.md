---
agentx:
  kind: index
  scope: knowledge/gpui
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - gpui
    - gpux-crate
    - framework-core
  supersedes: []
  superseded_by: []
---
# gpui Domain Index

The `gpux` package in `crates/gpui` (Rust name `gpux`): elements, window frame lifecycle, layout, input, text, and effects. Platform crates and the renderer have their own domains.

- [state](state.md) · [knowledge](knowledge.md) · [rules](rules.md) · [hypotheses](hypotheses.md)

## Subdomains

- [layout-style](layout-style/INDEX.md): Taffy integration, `Style` conversion, and request/prepaint/paint style handoff.
- [frame-pipeline](frame-pipeline/INDEX.md): `Window` draw lifecycle, scene, bounds tree, arena, element ids/state, deferred draws, content masks, DevKit inspection, frame diagnostics.
- [positioning-scroll](positioning-scroll/INDEX.md): fixed/sticky positioning, sticky stacking, top layer, scroll handles, `ScrollActivity`, wheel routing.
- [input](input/INDEX.md): action dispatch, pointer ownership/capture, drag, cursor order, text-input dispatch, input-map snapshots.
- [text](text/INDEX.md): `TextSystem`, line layout/wrap caches, `TextLayout`, and the neutral `editable_text` engine.
- [frame-effects](frame-effects/INDEX.md): frame-family elements, element effects, and window screen effects.
