---
agentx:
  kind: state
  scope: knowledge/gpui/layout-style
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-10T00:30:00+02:00
  topics:
    - taffy
    - layout
    - style
    - style-handoff
    - measured-layout
    - node-pool
  supersedes: []
  superseded_by: []
---
# Layout And Style State

## Purpose And Usage

Read before changing `TaffyLayoutEngine`, `Style::to_taffy`, `Interactivity` style computation, or measured layout.

## Current Technical Shape

gpux uses registry Taffy `=0.14.0` with default features off and only `std`, `taffy_tree`, `flexbox`, `grid`, and `block_layout`; there is no local Taffy source or patch. Leaf layout goes through Taffy's public `compute_leaf_layout`, so upstream sizing, padding, border, aspect-ratio, and early-return rules wrap gpux measurement; diagnostic `FlowRoot` containers count as block layout. Taffy's `content_size` feature stays off because GPUI derives scroll content extents from child layout bounds during prepaint.

`TaffyLayoutEngine` keeps a window-owned pool of layout nodes bounded by the largest tree that window has built. Frame clear takes each tracked measurement closure, marks that node dirty, and keeps the empty context slot; measured reuse refills the slot, unmeasured reuse sees an empty slot and keeps default leaf measurement. Reuse compares the retained ordered child ids with the current slice and calls `set_children` only on change; a capacity-retaining frame-local `FxHashSet` of current children lets a computed root detach a stale prior parent after the tree shrinks. `set_style` runs only when the freshly converted Taffy style differs from the retained one.

Style is computed per phase. `Interactivity::prepaint` keeps its pre-resolution style and recomputes only when an applicable `when_stuck(...)` refinement or a sticky-state listener can change it after sticky publication. `Window` owns a capacity-retaining draw-local `Style` store: structurally static `Interactivity` instances carry an index into it from prepaint to paint. Focus, in-focus, focus-visible, hover, group-hover, active, and group-active refinements block retention, and an active drag blocks reuse for hitboxed elements so drag-over builders, drop predicates, and cursor state still run at paint. The store clears owned values at every draw boundary.

The gpux-owned `Div`, `Img`, `Svg`, and `UniformList` request paths borrow their request `Style` through internal Window/Taffy adapters, retain the owned value in the same draw-local slot, consume it in prepaint, and replace it with the final prepaint style for paint. The public consuming-callback contract of `Interactivity::request_layout` is unchanged for external custom elements, which keep recomputing.

Measured leaves are re-executed every frame: a stable prior result is never reused from Taffy's numeric inputs alone.

Under fast mode's layout retention (`FastMode::layout_retention`, `crates/gpui/src/fast/layout.rs`, `layout_key.rs`), nodes are kept by a 64-bit key instead of the ordinal pool, which remains the off path: the hash of the element's path (`ElementId` or position among unidentified siblings), list items keyed by index under their list (`layout_as_list_item`), and layout requested during prepaint keyed under the prepainting element. A frame that switches retention starts from a fresh engine. Unclaimed nodes are released at clear, list-row nodes linger 240 frames, a duplicate key gets a transient node. A kept node's style is converted only when its `layout_fingerprint` differs or its `LayoutFields` (every field `to_taffy` reads, plus rem size and scale) differ exactly, written only when the conversion differs; children only when ordered ids differ; the stretched root keeps its requested style aside and is not rewritten. Clear still takes every closure; it dirties every measured leaf except text leaves, which their next text carries (same text, runs, style, scale factor and font generation), replays (new closure gives every logged size), or dirties; any other element claiming a former text leaf dirties it. After each `compute_layout`, text leaves reachable from the root that were last measured at another width are measured at their laid-out content size.

## Backlinks

- [Taffy integration](../../../../../crates/gpui/src/taffy.rs)
- [Style conversion](../../../../../crates/gpui/src/style.rs)
- [Div and Interactivity](../../../../../crates/gpui/src/elements/div.rs)
- [gpux manifest Taffy features](../../../../../crates/gpui/Cargo.toml)
- 2026-08-30 current [Registry Taffy 0.14.0](../../../decisions/gpui/layout-style/2026-08-30-gpui-layout-style-registry-taffy-0-14.md)
- 2026-08-31 current [Scroll extents without Taffy content-size](../../../decisions/gpui/layout-style/2026-08-31-gpui-layout-style-disable-taffy-content-size.md)
- 2026-08-31 current [Explicit invalidation before measured-layout reuse](../../../decisions/gpui/layout-style/2026-08-31-gpui-layout-style-measured-layout-invalidation.md)
- 2026-08-31 current [Text-owned measured-layout scope](../../../decisions/gpui/layout-style/2026-08-31-gpui-layout-style-text-measurement-scope.md)
- 2026-08-31 current [Conditional prepaint style recompute](../../../decisions/gpui/layout-style/2026-08-31-gpui-layout-style-conditional-prepaint-style-recompute.md)
- 2026-08-31 current [Frame-bounded prepaint style handoff](../../../decisions/gpui/layout-style/2026-08-31-gpui-layout-style-frame-bounded-prepaint-style-handoff.md)
- 2026-08-31 current [Gpux-owned request style handoff](../../../decisions/gpui/layout-style/2026-08-31-gpui-layout-style-request-style-handoff.md)
