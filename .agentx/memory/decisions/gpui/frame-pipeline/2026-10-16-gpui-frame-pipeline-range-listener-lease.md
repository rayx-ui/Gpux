---
agentx:
  kind: decision
  scope: decisions/gpui/frame-pipeline
  status: current
  created: 2026-10-16T23:00:00+02:00
  updated: 2026-10-16T23:00:00+02:00
  promoted_from: gpux-virtual-list
  topics:
    - gpux
    - fast-mode
    - virtual-list
    - view-retention
  supersedes: []
  superseded_by: []
---

# Call VirtualList's range listener as part of drawing the view holding the list

## Decision: Call VirtualList's range listener as part of drawing the view holding the list

### Context: RayX's `ListViewState` and `TreeStateBase` record the range their `VirtualList` publishes (`on_rendered_range`) in themselves and emit `VisibleRangeChanged`. gpux counts an entity updated without a notification as changed, so the view holding the `ListView` or `TreeBase` wrapper, which reads the state as it renders, was built again on every frame that scrolled the list to another range (every frame for a list, every other frame for a tree, against none for a bare list).

### Alternatives considered: Keep the range in a `Cell` outside the state entity in RayX; skip the update when the range is unchanged; exempt the listener's lease in gpux.

### Reasoning: Emitting `VisibleRangeChanged` from the state requires updating it, which gpux counted as a change wherever the range is kept, so no RayX-only change helps. The listener is called by the framework on every frame the list is drawn, before its rows render; gpux already treats a lease that is part of drawing a view (`render_next`, `rendering_rows_of`) as neither an update nor a write. `VirtualList` now calls the listener inside `fast::layers::lists::render_items`.

### Trade-offs accepted: A view that shows the range must take it from an event the listener emits, which notifies it; a view that reads the range from the holder as it renders is not built again for it (documented in `docs/fast-mode.md`, held by `fast::tests::virtual_list_layers::range`).

### Supersedes: None.
