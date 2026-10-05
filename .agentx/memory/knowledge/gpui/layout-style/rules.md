---
agentx:
  kind: rules
  scope: knowledge/gpui/layout-style
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - taffy
    - layout
    - style
    - style-handoff
    - measured-layout
  supersedes: []
  superseded_by: []
---
# Layout And Style Rules

- Keep Taffy defaults disabled and declare only `std`, `taffy_tree`, `flexbox`, `grid`, and `block_layout`. Do not enable `content_size`, `flexbox_balance`, `float_layout`, `calc`, or `detailed_layout_info` without a concrete gpux consumer and fresh native plus headed Test Suite layout proof.
- Keep pooled Taffy context slots empty between frames: take every tracked measurement closure during clear, mark that node dirty, and refill an existing slot on measured reuse. Never retain frame-local closures or captured application state.
- Do not reuse a prior measured-leaf output from Taffy's numeric inputs alone; require an explicit caller-owned invalidation key or generation before skipping closure execution.
- Scope measured-layout optimization to the proven owner. The reference workload's measurement is all text; do not add generic list, uniform-list, or custom measurement caching without a workload that exercises those owners.
- Keep the layout-node pool owned by one window and bounded by its largest observed tree. Retain child topology across clear, compare exact ordered child ids on reuse, call `set_children` only on change, and detach a computed root only when its retained parent is not current.
- Skip `set_style` only on exact equality with the freshly converted style; never infer style stability from node position or element identity.
- Do not optimize warm node requests by suppressing measured-context dirty marks or deduplicating child inserts; frame clear already dirties, and the inserts are unique.
- In `Interactivity::prepaint`, reuse the pre-resolution style unless an applicable stuck refinement or sticky-state listener can change it after publication.
- Carry structurally static prepaint styles through the window-owned draw-local store with only an optional index on `Interactivity`. Block retention for focus, in-focus, focus-visible, hover, group-hover, active, and group-active refinements; block consumption for hitboxed elements during active drags; clear at every draw boundary while retaining capacity bounded by the largest eligible draw.
- Keep request-to-prepaint style reuse private to gpux-owned `Div`, `Img`, `Svg`, and `UniformList`. Leave external `Interactivity::request_layout` consumers on the public consuming-callback path, and never extend the handoff across draw boundaries or into a public or cross-frame cache.
