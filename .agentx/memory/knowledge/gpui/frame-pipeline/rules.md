---
agentx:
  kind: rules
  scope: knowledge/gpui/frame-pipeline
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-11T03:30:00+02:00
  topics:
    - scene
    - arena
    - element-id
    - deferred
    - content-mask
    - thermal-state
    - devtools
  supersedes: []
  superseded_by: []
---
# Frame Pipeline Rules

- Preserve equal-key scene order for sprites. Use cached-key stable sorting only for the hot monochrome family; keep direct stable sorting for cold or empty families unless separate measurements justify the extra allocation path.
- Register arena drop records only when `std::mem::needs_drop::<T>()` is true.
- Keep `GlobalElementId` on direct `Arc<[ElementId]>` ownership with a cached `collections::FxHasher` hash and full-path equality. Keep reuse diagnostics hash-only behind `diagnostics`; do not add production interning without a measured design that improves throughput and tail latency.
- Keep the full accessed-state history for replay ranges and rollback; restrict finish-time removal to separately tracked replay keys.
- Keep deferred prepaint entries at stable arena indices; process them in place rather than moving completed entries behind cached ranges.
- Keep `gpux::deferred(...)` content-mask-preserving by default; use `Deferred::unclipped()` only for intentional top-level overlays.
- Preserve zero-width and zero-height content masks before device-pixel cover rounding; do not floor and ceil an empty mask independently.
- Elide inherited opacity transformation only when the factor is exactly `1.0`.
- Frame callbacks must not re-enter `AnyWindowHandle::update` to read app-owned platform snapshots; cache them through an owned subscription before scheduling frames.
- Remap inspection occurrence ids, parents, dispatch ids, focus, and property setters on cached prepaint replay, and truncate them on rollback.
- With view retention, copy a reused view's deferred draws with the records open at the copy, and stamp a reused record at the write generation of its reuse decision, never at its prepaint.
- Count an entity updated without a notify as changed for every reader that retained it; frames must equal the frame drawn from scratch.
- Splice only what can be copied exactly: refuse a deferred draw outside the gaps, check the copied content's reads and hovers before splicing, and keep a gap at its bounds; otherwise build the view and hand it the gaps as prebuilt.
- Everything a view's prepaint records per frame for composition (surface switches, native-surface placements) must be copied by every reuse path: `reuse_prepaint`/`reuse_paint`, retained `shifted` indices, and splice `StretchCopy`, and truncated by `transact`.
- A composition surface above native content takes pointer input only where it drew, never across the whole window.
- Whatever a scroll container's content adds to a frame while it is prepainted or painted must either be carried by `fast::layers::reuse` on composited frames or make the layer ineligible (`policy::eligible`, `paint::uncompositable`, `invalidate::note_uncarried`). A new primitive kind also needs `layers::scene` translation, `layers::tiles` hashing and `verify` expansion; a new per-frame record needs both.
- Gate scroll-offset snapping, glyph-origin quantization and path snapping on `WindowFastMode::snapping()`, never on `scroll_layers()`: a reference window snaps without layers (`Window::force_layer_snapping`), and with the switch off output must be today's.
- A layer composites only what one rectangular clip on device pixels can stand in for: gpux clips quads to the pixels covering a content mask and glyphs, underlines and paths to the mask itself.
- State an element inherits only while painting (opacity, transform, the surface or band it paints onto) is not seen by prepaint: record it with what was painted and, on a mismatch at reuse, show the old frame and rebuild on the next, as `retained::reuse_paint` and `layers::paint::repaint_if_painted_otherwise` do.
- Compare scroll layers against a window without them through `Window::painted_primitives` (tiles expanded into their content), frame by frame; a scenario or test window must put the platform pointer where its events are (`fast::scenarios::place_pointer`), because `draw_roots` reads it back from the platform window.
- A frame drawn while input other than a scroll is being dispatched must leave no scroll layer showing content away from where it was painted (`WindowLayers::before_input`): the window itself draws inside dispatch, before a key when it is dirty, and a composited pending scroll there hands the event stale listeners.
- Keep scroll-layer hooks inert without the switch: window-side hooks check `fast_mode.scroll_layers()`, hooks without a window check `fast::layers::on_thread()`, and a window turns layers on only where its renderer rasterizes tiles (`PlatformWindow::rasterizes_layer_tiles`, `PlatformHeadlessRenderer::rasterizes_layer_tiles`).
- Describe a frame that composites scroll layers through `layers::verify::expanded_scene` (what painting the content into the frame would have left), so `describe_rendered_frame` parity holds against a window drawn from scratch.
- Leave `Scene::insert_primitive`'s cull and ordering as upstream has them (bounds within the content mask). Keeping a shadow for its blur needs an order for it, and every order tried from the bounds a scene keeps (the reach, the nearest edge strip) put it over a neighbour its box never overlaps; see the hypotheses. Anything that tells where a primitive draws (tile membership, tile hashes) uses `layers::scene::drawn_bounds`.
- A frame drawn from scratch (`window.refreshing`) drops every scroll layer's record (`policy::forget_content`): the renderer may have lost the sprite atlas the kept content names. Content painted after it takes a new generation.
- Drop every scroll layer's record when the window removes an entry from its sprite atlas (`layers::atlas_entry_removed`, from `drop_image` and `drop_svg`): the atlas hands the slot out again, and tile hashes name a sprite by its atlas tile.
- A scroll layer's content is everything the container scrolls over (`LayerRecord::content_bounds`: from the container's origin to its scroll extent, padding and child margins included). Cut the painted region and test coverage against the same bounds.
- Keep a view's render reads whole for whatever it built and cut at the last `list(...)` it built only for that list's rows (`dependencies::RenderReads::shaping`): cutting them for every scroll container the view holds leaves a scrolling `div` built after a list stale when what the view read in between changes.
- A lease that is part of drawing a view (`render_next`, `rendering_rows_of`) stamps neither an update nor a write; any new framework lease of a view while a frame is drawn needs the same mark, or every retained reader of the view is built again each frame.
