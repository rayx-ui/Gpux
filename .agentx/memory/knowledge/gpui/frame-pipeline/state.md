---
agentx:
  kind: state
  scope: knowledge/gpui/frame-pipeline
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-10T01:00:00+02:00
  topics:
    - window
    - frame
    - scene
    - bounds-tree
    - arena
    - element-id
    - element-state
    - cached-view
    - hit-test
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

`Scene::BatchIterator` selects its first and second family heads with one linear scan per batch. `Scene::finish` sorts every family through `fast/scene.rs`: a stable counting sort by draw order (a packed-key sort when orders are sparse, nothing when already sorted), and sprites by packed `(order, texture, tile, index)` keys, so equal orders batch by texture.

The scene orders primitives with `fast/bounds_tree.rs` (gpui-fast's uniform grid) in all three draw-order bands: a frame inserting what the last one did replays its orderings, and orderings always equal comparing against every earlier bounds. The upstream R-tree (`crate::bounds_tree`) remains only as the tests' reference.

`Arena::alloc` records a type-erased destructor only when `std::mem::needs_drop::<T>()`; trivial allocations keep chunk lifetime and generation invalidation. `GlobalElementId` owns its path as `Arc<[ElementId]>` and caches a deterministic `collections::FxHasher` hash used for maps and AccessKit node ids; full path equality stays authoritative.

`WindowFrame` keeps the complete accessed-state history (needed for replay ranges and rollback) and separately owns only pending replay transfers. Direct `with_element_state` access moves state immediately; prepaint/paint replay copies keys into both lists; prepaint rollback truncates both; finish scans only pending replay keys. Deferred draws keep stable arena indices across cached prepaint reuse and are processed in place.

Cached views (`Entity::cached` / `AnyView::cached`) replay prepaint and paint ranges under a key of bounds, content mask, text style, and paint context (inherited element opacity and transform). Opacity and transforms are baked into primitives at paint, so `div` and the frame elements apply them around their children's prepaint as well as paint (`Window::with_element_paint_context`), and the key stores the context actually painted with: under an ancestor that applies either only while painting, the view renders instead of replaying, and a replay whose painted context changed after prepaint renders again on the next frame. Replayed hitboxes keep the previous frame's ids, so `WindowFrame::hit_test` orders candidates by id layer (normal, positioned, top layer) and then by reverse insertion, never by raw id. Cached views also record the hovers they read (`HitboxId::is_hovered`, through `fast/dependencies.rs`) while they render and paint, and replay only while each is unchanged against the current hit test, the pointer is outside the masked bounds of the hitboxes whose hover they read, and none holds the pointer captured; a view that read a hitbox it did not insert (an ancestor's `group_hover` group, reinserted under a new id whenever its owner draws) is never replayed. Prepaint's replay decision therefore holds at paint (asserted in debug), and a frame is drawn once. This applies in every fast mode. Only hovers something paints by are recorded: `Interactivity::hovered` is published through an unrecorded read, so cached views without hover styles replay under the pointer.

For view retention, `fast::dependencies` records what a subtree reads (entities via the entity map's access log, globals and global presence, `ScrollHandle`/`ListState` `StateVersion`s noted where the owning element uses them, ambient pointer/keys/button through the window getters) and stamps changes with gpui-fast's rules: a pure notify changes nothing a reader saw, an update without notify counts only inside a notified view, a write while drawing counts for foreign readers, rendering a view is not a write, `ElementInputHandler` queries go through `dependencies::query`. Ambient input is stamped after dispatch and when the window reloads it from the platform. Stamping starts at the first recording. Recordings nest (own versus subtree reads); replaying a record re-registers its entities as accessed.

Hot paths redraw through their owning view (`cx.notify(view)`): programmatic and animated scroll, hover/active/group-hover styles and abandoned presses, interactive and editable text, key dispatch. `Window::refresh()`, which turns every cache off for a frame, stays only at whole-window changes, each commented `// Whole-window:`; it does nothing while drawing.

`gpux::deferred(...)` inherits the current content mask; `Deferred::unclipped()` paints with no mask for top-level overlays that must escape a clipped parent. `ContentMask` carries bounds and corner radii (`rectangular`/`rounded`), and intersection and scaling preserve radii. `Window` keeps empty logical mask bounds empty before cover-rounding non-empty masks to device pixels, so a zero-width or zero-height mask cannot become a one-device-pixel strip at fractional scales.

Renderer paint paths prepare the original immutable `Color` or `Background` when inherited opacity is exactly `1.0`, reusing its lazy paint cache; other factors keep the authored-opacity transformation.

`Window` caches the platform thermal state in window-owned `Rc<Cell<ThermalState>>`, updated through an app thermal-state subscription, so idle frame callbacks never re-enter `AnyWindowHandle::update` (required for browser animation-frame scheduling during an active app borrow). A frame that starts with demand (dirty, awaiting presentation, or pending next-frame callbacks) requests one follow-up frame through a guarded trailing schedule, matching Zed's scheduling without its per-frame thermal App update.

DevKit inspection: `Window::devtools_frame_snapshot()` is a read-only surface populated during the element lifecycle from rendered occurrence ids and parents, dispatch nodes, bounds, hitbox and listener counts, global ids, source locations, component semantics, and writable-property metadata. `InspectableElementExt` attaches semantics without adding hitboxes, focus, or listeners; `Window::register_devtools_property_setter(...)` registers live property callbacks for the current occurrence. The rendered occurrence stack is separate from the dispatch stack. Cached prepaint replay remaps occurrence ids, parents, dispatch ids, focus, and setters; prepaint rollback truncates inspection ranges. Frame nodes use direct id-to-index lookup and materialize owned snapshot strings only on request. `InspectableElement` also forwards the wrapped element's AccessKit surface.

Under `diagnostics`, `gpui_wgpu` keeps a bounded latest-frame telemetry snapshot (primitive-family sizes, batches, buffer/atlas writes, bind groups, passes, restarts, submissions, sprite disorder, bounds-tree work, element-id depth, Taffy tree shape, style phases, text-cache tiers) that `gpui_web` publishes as numeric-only `__gpuxWgpuFrameDiagnostics` (and `__rayxWgpuFrameDiagnostics` under the `rayx_diagnostics` alias) for browser proof. Completed probes (deferred-intersection prune, element-state finish) were retired once their decision landed.

Fast mode (`crates/gpui/src/fast/`, adapted from gpui-fast) holds a per-window `FastMode` (`Window::fast_mode`/`set_fast_mode`, environment `GPUX_FAST_MODE`, `GPUX_LAYOUT_RETENTION`, `GPUX_VIEW_RETENTION`, `GPUX_SCROLL_LAYERS`), every compiled default off until its mechanism is proven. Measurement-only `FastStats` (work counters in every mode, mechanism counters zero when off) and the frame description `Window::describe_rendered_frame` exist only under `test-support`/`bench-support`/`diagnostics` and `test-support`; the randomized oracle in `fast::tests` compares an incremental window with a from-scratch one after every step, with draw-on-flush off (`TestAppContext::set_draw_on_flush`).

## Backlinks

- [Fast mode](../../../../../crates/gpui/src/fast/mod.rs)

- [Window](../../../../../crates/gpui/src/window.rs)
- [Cached views](../../../../../crates/gpui/src/view.rs)
- [Scene](../../../../../crates/gpui/src/scene.rs)
- [Bounds tree](../../../../../crates/gpui/src/bounds_tree.rs)
- [Arena](../../../../../crates/gpui/src/arena.rs)
- [Deferred](../../../../../crates/gpui/src/elements/deferred.rs)
- [Inspector](../../../../../crates/gpui/src/inspector.rs)
- [Frame telemetry](../../../../../crates/gpui_wgpu/src/frame_telemetry.rs)
- 2026-06-29 current [Unclipped deferred overlays](../../../decisions/gpui/frame-pipeline/2026-06-29-gpui-frame-pipeline-unclipped-deferred-overlays.md)
- 2026-07-24 current [Cached thermal-state frame throttling](../../../decisions/gpui/frame-pipeline/2026-07-24-gpui-frame-pipeline-cached-thermal-frame-throttling.md)
- 2026-10-05 current [Frame-demand follow-up](../../../decisions/gpui/frame-pipeline/2026-10-05-gpui-frame-pipeline-frame-demand-follow-up.md)
