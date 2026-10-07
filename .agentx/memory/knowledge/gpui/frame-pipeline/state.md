---
agentx:
  kind: state
  scope: knowledge/gpui/frame-pipeline
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-07T16:45:00+02:00
  topics:
    - window
    - frame
    - scene
    - bounds-tree
    - arena
    - element-id
    - element-state
    - deferred
    - content-mask
    - devtools
    - thermal-state
    - diagnostics
  supersedes: []
  superseded_by: []
---
# Frame Pipeline State

## Purpose And Usage

Read before changing `Window::draw`, prepaint/paint bookkeeping, `Scene`, `BoundsTree`, `Arena`, element ids/state, deferred drawing, content masks, or DevKit inspection hooks.

## Current Technical Shape

`Scene::BatchIterator` selects its first and second family heads with one linear scan per batch. Sprites keep stable equal-key order; the hot 128-byte monochrome family stable-sorts through cached `(DrawOrder, TileId)` keys, while subpixel and polychrome sprites sort directly.

`BoundsTree::find_max_ordering` rejects non-intersecting roots before stacking and filters non-intersecting children at expansion; non-full terminal insertion relies on the existing propagation loop instead of updating the same node twice.

`Arena::alloc` records a type-erased destructor only when `std::mem::needs_drop::<T>()`; trivial allocations keep chunk lifetime and generation invalidation. `GlobalElementId` owns its path as `Arc<[ElementId]>` and caches a deterministic `collections::FxHasher` hash used for maps and AccessKit node ids; full path equality stays authoritative.

`WindowFrame` keeps the complete accessed-state history (needed for replay ranges and rollback) and separately owns only pending replay transfers. Direct `with_element_state` access moves state immediately; prepaint/paint replay copies keys into both lists; prepaint rollback truncates both; finish scans only pending replay keys. Deferred draws keep stable arena indices across cached prepaint reuse and are processed in place.

`gpux::deferred(...)` inherits the current content mask; `Deferred::unclipped()` paints with no mask for top-level overlays that must escape a clipped parent. `ContentMask` carries bounds and corner radii (`rectangular`/`rounded`), and intersection and scaling preserve radii. `Window` keeps empty logical mask bounds empty before cover-rounding non-empty masks to device pixels, so a zero-width or zero-height mask cannot become a one-device-pixel strip at fractional scales.

Renderer paint paths prepare the original immutable `Color` or `Background` when inherited opacity is exactly `1.0`, reusing its lazy paint cache; other factors keep the authored-opacity transformation.

`Window` caches the platform thermal state in window-owned `Rc<Cell<ThermalState>>`, updated through an app thermal-state subscription, so idle frame callbacks never re-enter `AnyWindowHandle::update` (required for browser animation-frame scheduling during an active app borrow). A frame that starts with demand (dirty, awaiting presentation, or pending next-frame callbacks) requests one follow-up frame through a guarded trailing schedule, matching Zed's scheduling without its per-frame thermal App update.

DevKit inspection: `Window::devtools_frame_snapshot()` is a read-only surface populated during the element lifecycle from rendered occurrence ids and parents, dispatch nodes, bounds, hitbox and listener counts, global ids, source locations, component semantics, and writable-property metadata. `InspectableElementExt` attaches semantics without adding hitboxes, focus, or listeners; `Window::register_devtools_property_setter(...)` registers live property callbacks for the current occurrence. The rendered occurrence stack is separate from the dispatch stack. Cached prepaint replay remaps occurrence ids, parents, dispatch ids, focus, and setters; prepaint rollback truncates inspection ranges. Frame nodes use direct id-to-index lookup and materialize owned snapshot strings only on request. `InspectableElement` also forwards the wrapped element's AccessKit surface.

Under `diagnostics`, `gpui_wgpu` keeps a bounded latest-frame telemetry snapshot (primitive-family sizes, batches, buffer/atlas writes, bind groups, passes, restarts, submissions, sprite disorder, bounds-tree work, element-id depth, Taffy tree shape, style phases, text-cache tiers) that `gpui_web` publishes as numeric-only `__gpuxWgpuFrameDiagnostics` (and `__rayxWgpuFrameDiagnostics` under the `rayx_diagnostics` alias) for browser proof. Completed probes (deferred-intersection prune, element-state finish) were retired once their decision landed.

## Backlinks

- [Window](../../../../../crates/gpui/src/window.rs)
- [Scene](../../../../../crates/gpui/src/scene.rs)
- [Bounds tree](../../../../../crates/gpui/src/bounds_tree.rs)
- [Arena](../../../../../crates/gpui/src/arena.rs)
- [Deferred](../../../../../crates/gpui/src/elements/deferred.rs)
- [Inspector](../../../../../crates/gpui/src/inspector.rs)
- [Frame telemetry](../../../../../crates/gpui_wgpu/src/frame_telemetry.rs)
- 2026-06-29 current [Unclipped deferred overlays](../../../decisions/gpui/frame-pipeline/2026-06-29-gpui-frame-pipeline-unclipped-deferred-overlays.md)
- 2026-07-24 current [Cached thermal-state frame throttling](../../../decisions/gpui/frame-pipeline/2026-07-24-gpui-frame-pipeline-cached-thermal-frame-throttling.md)
- 2026-10-05 current [Frame-demand follow-up](../../../decisions/gpui/frame-pipeline/2026-10-05-gpui-frame-pipeline-frame-demand-follow-up.md)
