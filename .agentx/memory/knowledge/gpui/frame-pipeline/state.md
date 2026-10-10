---
agentx:
  kind: state
  scope: knowledge/gpui/frame-pipeline
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-10T19:30:00+02:00
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

For view retention, `fast::dependencies` records what a subtree reads (entities via the entity map's access log, globals and global presence, `ScrollHandle`/`ListState` `StateVersion`s noted where the owning element uses them, ambient pointer/keys/button through the window getters) and stamps changes: a pure notify changes nothing a reader saw, an update without notify is a change for every reader (gpui-fast counts it only inside a notified view; gpux keeps frames equal to the frame drawn from scratch), a write while drawing counts for foreign readers, rendering a view is not a write, `ElementInputHandler` queries go through `dependencies::query`. A `ListState` bumps its version when its own layout changes what it lays out next (measured heights, scroll target, bounds). Ambient input is stamped after dispatch and when the window reloads it from the platform. Stamping starts at the first recording. Recordings nest (own versus subtree reads); replaying a record re-registers its entities as accessed.

With `view_retention` on, `fast/retained.rs` keeps one record per entity view in `WindowFrame::fast_retained` (prepaint/paint ranges, nested count, context of bounds, content mask, text style and painted context, own and subtree dependencies, hovers with their area, layout keys and a retained layout). `ViewElement` routes through `fast::retained`: a view is reused (its ranges copied by `reuse_prepaint`/`reuse_paint`, nested records shifted, records stamped at the write generation of the reuse decision), built at its retained layout when only its bounds, content mask or painted context changed, or built (a changed text style builds). A view whose content registers a sticky candidate with an outer scroll context keeps no retained layout. A reused layout carries its element states, overwritten with `ReleasedElementState` tombstones if the view is built after all, and gives kept text leaves a measuring closure. Deferred draws carry their enclosing records (copied and nested ones included) and their hitboxes count as those records' own for hovers. Reuse is bypassed, and counted per view, while refreshing, dragging, inspector picking, AccessKit is active, or for a duplicated id. Under `diagnostics`, verify mode (`GPUX_RETENTION_VERIFY`, `Window::set_retention_verify`, `on_retention_mismatch`) draws a recording-only shadow frame first and reports the innermost differing view.

With `splice` on, `fast/splice.rs` draws a retained view that is dirty only through nesting (not notified, own reads and the copied content's reads and hovers unchanged, no deferred draw outside the gaps) from last frame around the outermost dirty nested views, each rebuilt in its gap at its own nodes under every inherited stack captured when it was last built (`Rebuild`: text style, offset, rem size, opacity, transform, scroll and sticky contexts, hitbox behavior, positioned depth; content masks from the record). Stretches between gaps copy prepaint with dispatch nodes and DevKit ids mapped across gaps, and a gap's hitbox change is added to every copied DevKit node up to the spliced root. A gap at another node, or a gap whose bounds or the view's own node layouts change, abandons the splice: the view is built (at its retained layout when abandoned at prepaint) and takes the built gaps over as prebuilt. View elements keep a function pointer to make their rebuild `AnyView`, taken only in frames that splice. At frame start, views whose own dependencies changed are marked dirty with their ancestors.

Window composition (`fast/composition.rs`, the API of Zed PR #62379 through gpui-fast): `Window::enable_window_composition` returns a controller over a surface tree (base and overlay GPUI surfaces, native, external-GPU and extra GPUI surfaces; transactional order changes, diffed before reaching `PlatformWindow::set_composition_order`). The root view paints onto the base surface; deferred draws, prompts, drag, tooltips and the inspector onto the overlay, and gpux's top-layer band moves to the overlay from wherever it was painted. `SurfaceStarts` record surface switches by scene index; `present` splits the scene into per-surface layers replayed balanced (clip, positioned and top layers reopened). Views drawn from last frame carry their switches (`reuse_starts`) and native-surface placements (`NativeSurfacePlacements`, indexed by `PrepaintStateIndex::fast_native_surfaces_index`). Text painted over native content is grayscale (`paints_over_native_content` in `should_use_subpixel_rendering`). A surface takes pointer input only within `ComposedScene::layer_input_region` (clipped primitive bounds, no shadows). A surface once placed by a `native_surface` element is hidden in frames that place it nowhere; content masks never clip native surfaces. Disabled composition leaves the scene and draw call unchanged; the first composed frame is drawn afresh. `TestComposition` on the test platform records orders, per-surface scenes, input regions and native-surface geometry.

Hot paths redraw through their owning view (`cx.notify(view)`): programmatic and animated scroll, hover/active/group-hover styles and abandoned presses, interactive and editable text, key dispatch. `Window::refresh()`, which turns every cache off for a frame, stays only at whole-window changes, each commented `// Whole-window:`; it does nothing while drawing.

`gpux::deferred(...)` inherits the current content mask; `Deferred::unclipped()` paints with no mask for top-level overlays that must escape a clipped parent. `ContentMask` carries bounds and corner radii (`rectangular`/`rounded`), and intersection and scaling preserve radii. `Window` keeps empty logical mask bounds empty before cover-rounding non-empty masks to device pixels, so a zero-width or zero-height mask cannot become a one-device-pixel strip at fractional scales.

Renderer paint paths prepare the original immutable `Color` or `Background` when inherited opacity is exactly `1.0`, reusing its lazy paint cache; other factors keep the authored-opacity transformation.

`Window` caches the platform thermal state in window-owned `Rc<Cell<ThermalState>>`, updated through an app thermal-state subscription, so idle frame callbacks never re-enter `AnyWindowHandle::update` (required for browser animation-frame scheduling during an active app borrow). A frame that starts with demand (dirty, awaiting presentation, or pending next-frame callbacks) requests one follow-up frame through a guarded trailing schedule, matching Zed's scheduling without its per-frame thermal App update.

DevKit inspection: `Window::devtools_frame_snapshot()` is a read-only surface populated during the element lifecycle from rendered occurrence ids and parents, dispatch nodes, bounds, hitbox and listener counts, global ids, source locations, component semantics, and writable-property metadata. `InspectableElementExt` attaches semantics without adding hitboxes, focus, or listeners; `Window::register_devtools_property_setter(...)` registers live property callbacks for the current occurrence. The rendered occurrence stack is separate from the dispatch stack. Cached prepaint replay remaps occurrence ids, parents, dispatch ids, focus, and setters; prepaint rollback truncates inspection ranges. Frame nodes use direct id-to-index lookup and materialize owned snapshot strings only on request. `InspectableElement` also forwards the wrapped element's AccessKit surface.

Under `diagnostics`, `gpui_wgpu` keeps a bounded latest-frame telemetry snapshot (primitive-family sizes, batches, buffer/atlas writes, bind groups, passes, restarts, submissions, sprite disorder, bounds-tree work, element-id depth, Taffy tree shape, style phases, text-cache tiers) that `gpui_web` publishes as numeric-only `__gpuxWgpuFrameDiagnostics` (and `__rayxWgpuFrameDiagnostics` under the `rayx_diagnostics` alias) for browser proof. Completed probes (deferred-intersection prune, element-state finish) were retired once their decision landed.

Fast mode (`crates/gpui/src/fast/`, adapted from gpui-fast) holds a per-window `FastMode` (`Window::fast_mode`/`set_fast_mode`, environment `GPUX_FAST_MODE`, `GPUX_LAYOUT_RETENTION`, `GPUX_VIEW_RETENTION`, `GPUX_SCROLL_LAYERS`), every compiled default off until its mechanism is proven. Measurement-only `FastStats` (work counters in every mode, mechanism counters zero when off) and the frame description `Window::describe_rendered_frame` exist only under `test-support`/`bench-support`/`diagnostics` and `test-support`; the randomized oracle in `fast::tests` compares an incremental window with a from-scratch one after every step, with draw-on-flush off (`TestAppContext::set_draw_on_flush`).

Scroll layers core (`fast/layers/`, see `gpui/positioning-scroll` for the scroll side): `Scene::layers` holds the `LayerFrame`s a frame composites (key, generation, baked background, content, dirty tiles); tile sprites are polychrome sprites with texture ids from `LAYER_TILE_TEXTURE_BASE`, inserted inside the viewport's layer, and `Scene::replay` copies the frames with them. A layer's content is painted into its own scene with the painted region as content mask and both mask stacks pushed, then translated into content space (every position and each primitive's transformation, `t + (I - R)·delta`); tile hashes take each primitive in tile space, transformation included. Paths nothing draws over are lifted and drawn over the tiles; content drawing over a path demotes the layer. Views inside a layer keep no retained record (the layer is their retention) and are laid out from kept layouts on composited frames (`ViewLayout::Kept`). Content painted onto another composition surface records its switches and is drawn into the frame with them; `paints_over_native_content` answers for the container's surface and band while a layer's scene is swapped in. The randomized layers oracle (`fast::tests::oracle_layers`) compares against a window with every mechanism off and layer snapping forced (`Window::force_layer_snapping`), runs each seed with the real policy and with layers kept (`WindowLayers::never_demote`), since the policy demotes under its constant content changes, draws the reference's pending frame before an input event exactly when the layered window drew one, and gives content masks to 1/1024 px (`layers::verify::settle_masks`). `describe_rendered_frame` describes a layered frame through `layers::paint::expanded_scene` and rounds nothing.

## Backlinks

- [Fast mode](../../../../../crates/gpui/src/fast/mod.rs)
- [Window composition](../../../../../crates/gpui/src/fast/composition.rs)

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
