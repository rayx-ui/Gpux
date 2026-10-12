# Fast mode

Fast mode is gpux's set of incremental-rendering mechanisms, adapted from
[gpui-fast](upstream/gpui-fast.md): layout retention, view retention, splice, scroll layers, and
window composition. Each can be switched per window at runtime. With every switch off a window
draws exactly as gpux draws without fast mode.

**Layout retention, view retention, splice and scroll layers are on by default**, wherever the
target compiles them (scroll layers need a renderer that rasterizes their tiles: every gpux
renderer does, a test window without a renderer reports them off). Window composition stays opt-in
per window. `GPUX_FAST_MODE=0` turns everything off for a process; [Switches](#switches) lists
the finer variables.

## Switches

`gpux::FastMode` holds one switch per mechanism: `layout_retention`, `view_retention`, `splice`
and `scroll_layers`. `Window::fast_mode()` returns a window's effective mode and
`Window::set_fast_mode(mode)` changes it; the next frame is drawn from scratch.

A mode is normalized whenever a window takes it: view retention needs layout retention, and
splice and scroll layers need view retention. Scroll layers are also off on a target whose
renderer does not implement their tiles.

A window's initial mode comes from the compiled defaults and the environment, read when the
window is created:

| Variable | Effect |
|---|---|
| `GPUX_FAST_MODE=0` | Every mechanism off; nothing else is read. |
| `GPUX_FAST_MODE=1` | Every mechanism on, before the variables below apply. |
| `GPUX_LAYOUT_RETENTION=0\|1` | Layout retention. |
| `GPUX_VIEW_RETENTION=0\|1` | View retention. gpui-fast's `GPUI_VIEW_RETENTION` is read when this is unset. |
| `GPUX_SCROLL_LAYERS=0\|1` | Scroll layers. gpui-fast's `GPUI_SCROLL_LAYERS` is read when this is unset. |

Splice has no variable, as in gpui-fast; it is on with view retention by default, and
`Window::set_fast_mode` turns it off. A mode a variable cannot express (splice off and everything
else on) is set the same way, right after the window opens.

Because the defaults are on, a window an application opens, and every window of a test that does
not say otherwise, draws with the mechanisms. A test that is of stock behaviour (upstream's cached
views, or what a frame that paints everything does) opens its window with
`window.set_fast_mode(FastMode::OFF)`; `fast::tests::defaults` pins what a new window gets and what
each variable turns off. `Window::fast_mode()` says what a window runs.

What a retained scene replays is what was painted: a change of the application's text rendering
mode, of a window's background appearance, or of what its renderer supports (subpixel text) makes
the next frame draw from scratch, where frames drawn without fast mode follow it by painting
everything again.

### Hover after a scroll

An element with a hover style repaints when a move of the pointer changes whether it is
hovered, and what the move is compared with was written only when a move changed it. A hover
changes without a move too: the wheel brings another row under the pointer, and the frame that
follows paints that row hovered. When the pointer then moved back onto the row it had been
over, neither row differed from what had been written, nothing was repainted, and the window
went on showing the row the wheel had brought hovered. What is compared is now what was
painted (`fast/hover.rs`, two hooks in `elements/div.rs`), with fast mode on or off. Fast
mode's retained rows were already painted again by the hovers they read, so a list drawn from
its layer showed the right row where the same list without layers did not:
`lane_c_list_hover` and `fast::tests::list_hover` hold the two to each other after every move.

The state lives in the element's state, which an element without an id does not have: a hover
style on an element with no id is compared with nothing, as upstream's is, and a move that
changes which such row is hovered asks for no frame. Give a row that looks different under the
pointer an id. (The first review of s12d measured 43 of 190 frames wrong in a window without
layers for a `uniform_list` of such rows, the same at the sprint's base; rows with an id, which
the seventh review of s12c found 6 of 60 moves wrong for, are right.)

### Cached views and hovers

One piece of fast mode's groundwork is gpux behaviour in every mode: cached views
(`AnyView::cached`) record the hovers they read and are replayed only while each is unchanged.
Fast mode's oracle found that a cached view painted with a hover style kept it after a wheel
event, or an overlay appearing under a still pointer, moved the hover elsewhere. A cached view
that read hovers is also built again, rather than replayed, while the pointer is over its
hitboxes: only there can hitboxes the frame adds later (an overlay) change its hovers. A cached
view that read the hover of a hitbox it does not insert, such as the group around it
(`group_hover`), is built again every frame: that hitbox belongs to whoever inserts it, under a
new id each frame its owner draws.

## View retention

With `view_retention` on (it needs `layout_retention`), every entity-backed view is a retained
subtree. Each frame leaves a record per view drawn: where its hitboxes, dispatch nodes,
listeners, element states, line layouts and primitives went, the views nested in it, where it was
drawn (bounds, content mask, text style and the opacity and transform it was painted with), the
layout nodes it holds, the hovers it was drawn by, and what it read: entities, globals and their
presence, scroll and list state, and the window's pointer, modifiers and pressed button.

On the next frame a view is

- **reused** when nothing it read changed, it is drawn where and as it was, and its hovers
  stand: its stretch of last frame is copied, the records nested in it with it;
- **built at its retained layout** when only where it is drawn changed (it moved, or its content
  mask or painted context changed): it is rendered, but lays out at the nodes it kept;
- **built** otherwise, a changed text style included, which can measure text otherwise.

What builds a view again:

- an entity it read, updated and notified, or updated without a notify (a frame drawn from
  scratch would show the update); a notify alone changes nothing a view read;
- an entity it read, written by another view while the window draws;
- a global it read written, inserted or removed;
- a scroll handle or list it laid out scrolled, or a list whose layout changed what it lays out
  next;
- the pointer, modifiers or pressed button, when it read them;
- a hover it read, while the pointer is over the hitbox it read (a hover of a hitbox it did not
  insert, such as an ancestor's group, builds it every frame);
- an interaction inside it, which notifies it: it and the views around it are built, the views
  beside it reused.

Nothing is reused while the window refreshes, during a drag, while the inspector picks, while
AccessKit is active, or for a view whose id is drawn twice in a frame; each reason is counted in
`FastStats` (`views_bypassed_*`). A stateless `RenderOnce` component is part of the view it is in.
A view whose content has a sticky element pinned by a scroll container outside it is built rather
than laid out from its record, so the container pins it from what this frame registered.

### Splice

With `splice` on (it needs view retention), a view that is dirty only because views nested in
it changed is drawn from last frame around them. It must not have been notified, nothing it read
itself may have changed, and what is drawn around the changed views must read and be hovered as
it was. Each changed nested view is built again in its gap, where it was drawn, inheriting what
it had there: element ids, text style, the content masks it was prepainted and painted under,
element offset, rem size, opacity, transform, scroll and sticky contexts and hitbox behavior. The
stretches between gaps are copied, their dispatch nodes and DevKit nodes mapped across the gaps
so a node allocated before a gap and recorded after it still finds its parent.

A gap laid out at another node changes the view around it, which is then built, taking over the
nested views already built rather than building them twice (`views_prebuilt`). A gap laid out
differently at the node it had keeps the splice only if the view's own nodes, and the gap's
bounds, come out of layout as they were. A view that defers something outside its gaps is built,
since what is deferred is drawn after it. Each view whose own reads changed is marked dirty at
frame start, as if notified, so the views around it are spliced.

### Verify mode

Under the `diagnostics` feature, `GPUX_RETENTION_VERIFY=1` or `Window::set_retention_verify(true)`
checks each frame that may reuse views against the frame drawn from scratch: a shadow frame is
drawn first, recording every view and reusing none, and is never presented. The innermost view
whose part of the two frames differs is reported by its element id path and type through the log
and `Window::on_retention_mismatch`. Use it to find a view that reads state its dependencies
cannot see, such as an `Rc<RefCell<_>>` changed without a notify. Every view renders twice a
frame while it is on, so a render with side effects has them twice.

## Scroll layers

With `scroll_layers` on (it needs view retention), a scrolling `div` whose offset moves on two
frames in a row paints its content once into a layer: a scene in content space, cut into tiles
of 512 device pixels, covering the viewport and two viewports on either side. Frames that only
scroll it are then drawn from those tiles, moved by the offset, without prepainting or painting
the content. `uniform_list` and `list` get layers of their own, with a record per row (see
[Virtual lists](#virtual-lists)).

A window runs scroll layers only where its renderer rasterizes their tiles
(`PlatformWindow::rasterizes_layer_tiles`); in any other window the switch normalizes to off.

The wgpu renderer rasterizes tiles on Linux, Windows, in the browser, on Android and on iOS, and
the Metal renderer on macOS (see [Tiles in the renderers](#tiles-in-the-renderers) and
[In the browser and on mobile](#in-the-browser-and-on-mobile)).

While no window of a thread has the switch on, a scroll container does none of the layers'
bookkeeping.

A frame only scrolls a container when:

- nothing its content read changed, no view inside it was notified, and none rendered: a view
  inside the content is laid out from what the layer kept while nothing its render read
  changed, and renders again when something did, a model it does not observe included;
- the view holding it was not updated since it last rendered, and was notified no more often
  than scroll state published. The notifications a published offset or its metrics send the
  owner and the handle's dependent views (`ScrollHandle::register_dependent_view`) are the
  scroll's. A wheel and a scrollbar (`publish_scrollbar_offset`) publish at once and composite.
  gpux counts an entity updated without a notification as changed, so a scroll published from a
  listener of the owner itself (`cx.listener`) paints the layer again, as does an offset an
  application requests (`request_offset`, `scroll_to`) and notifies the owner for: it is applied
  when the container is next prepainted, and that notification cannot be told from a change;
- the owner's render did not read the offset or the metrics;
- no container inside the content scrolled;
- no hover the content read can have changed. Prepaint sees last frame's hit test, which the
  scroll leaves behind, so content that read hovers composites only while the pointer is over
  none of the hitboxes it read, moved to where they show now. With the pointer over a row that
  has a hover style, each scrolled frame paints the layer again and the policy falls back to
  drawing the container as without a layer.

On a composited frame the content's hitboxes, mouse and key listeners, focus, cursor styles,
element state, tab stops and window-control areas are carried over, moved by the scroll and
clipped to the viewport; tooltips are dropped once the layer moved. `ScrollHandle::bounds` and
`bounds_for_item` answer for where the content shows.

Carried listeners still hold the bounds they were painted with. So input other than a scroll
that reaches content shown away from where it was painted first paints the layer where it
shows, in a frame drawn before the event is dispatched: a press or a release anywhere, a key
with the focus inside the content, a tap, a long press, a touch down, and a move over the
content where nothing covers it (not the moves of a drag begun elsewhere, such as a scrollbar's,
nor moves while a hitbox outside the content holds the pointer). A touch pan and the momentum
of a fling scroll through the wheel path and composite.

Two things differ in time, not in what a frame shows:

- The frame drawn before such an input also draws whatever else was pending, a scroll a wheel
  just published for instance, where without layers the input would have been dispatched
  against the frame before it. No layer leaves that frame showing content away from where it
  was painted.
- Opacity, a transform and the surface a container paints onto are applied while painting, and
  an ancestor can change them after the container's prepaint decided to composite. That frame
  shows the layer as it was painted and the next paints it again, as a view drawn from last
  frame is built on the next.

A container goes without a layer, counted by reason in `FastStats::layer_bypasses`, when:

- `inactive`: the frame is drawn from scratch, during a drag, with accessibility active, or
  while the inspector picks. A frame drawn from scratch (`Window::refresh`, as after the
  renderer lost its sprite atlas, whose tiles the kept content would still name) also leaves no
  layer content painted before it: the container is painted into its layer again the next time
  it scrolls. So does removing an image or an SVG from the sprite atlas (`Window::drop_image`,
  `drop_svg`): the atlas hands the slot out again, and the tiles' hashes would not tell the next
  image from the one removed;
- `not-promoted`: it has not scrolled on two frames in a row;
- `demoted`: its content changed on 8 of the last 16 frames, painting it again cost more than
  the frames it composited saved, it draws over a path (paths are never rasterized into tiles),
  or its tiles would not fit the window's budget (64 MB; 32 MB in the browser and on mobile); it
  is tried again after 60 stable frames, doubling up to 1,920;
- `ineligible`: its content holds what no tile can: a deferred or anchored element, a
  children-prepainted listener, a focused text input, an animation-frame request, a visual
  effect, a screen-effect region, an HTML island or element, a native surface, content painted
  onto another composition surface, anything painted with a transformation of its own, or
  fixed, sticky or top-layer content; or the container is
  under a rounded clip, a transform or a clip that is not on device pixels, or over a background
  that is not one opaque solid quad. It is tried again after 8 frames, doubling up to 2,048
  while the content stays so;
- `nested`: it is painted inside another container's layer;
- `moved`: sticky or fixed positioning displaced it.

Every scroll-container frame is counted once: `layers_composited`, `layers_repainted`, or a
bypass reason.

The switch also snaps scroll offsets to device pixels, quantizes glyph origins so that they
shift by whole pixels, and snaps path vertices to 1/256 of a pixel, which tiles need to match a
direct draw. With it off, output is today's.

Frames drawn through layers are checked against frames drawn from scratch by a randomized
oracle (`fast::tests::oracle_layers`), which expands each layer's tiles into its content and
compares the two scenes exactly but for content masks, given to 1/1024 of a device pixel: a
mask is scaled from logical pixels, and one a layer moved into content space and back can
differ in its last bits. Pixel equality is the renderers' tests' to show.

### Virtual lists

A `uniform_list` or a `list` renders only the rows it shows. Its layer holds those and the rows
around them, each painted once at its place and kept by row with everything it handed the
frame: its part of the layer's scene, the hashes of the tiles it reaches, its hitboxes, the
hovers it was painted by, its prepaint and paint records, its layout nodes and what it read. A
frame that only scrolls the list renders the rows the layer lacks and no other:

- the rows the scroll brings within reach. A `uniform_list` holds `ceil(2 · viewport / item)`
  rows on either side of those it shows. A `list` holds two viewports of rows on either side
  when it is first painted, only the rows it shows after a change of its content painted them
  again, and half a viewport more on each frame that keeps them;
- the row under the pointer, and a row that was drawn hovered. A frame hit tests the pointer
  against its own hitboxes after its rows are prepainted, and another view can lay a sheet over
  the list, or take one away, in that very frame: a row that read hovers is kept only where no
  hitbox of the frame can change them, which is where every hover it read was false and of a
  hitbox of its own, the pointer is over none of its hitboxes and none holds the pointer
  captured. The hovers of the rows kept and shown count among those the view holding the list is
  drawn by, so a pointer that comes down on a kept row without having been moved there draws
  the view again;
- the rows the list shows whose reads changed, or which hold a view that was notified. A row
  the list does not show is rendered when it comes to: in gpux every row reads the view that
  holds its list, and a change of what they all read would otherwise rebuild several viewports
  of rows on each frame of it;
- a row whose element states were dropped when it left the viewport, as a list without a layer
  drops them, when it shows again.

A list's rows are rendered by a closure the view holding the list hands it, which commonly
leases that view (`Context::processor`). With scroll layers on, that lease is part of drawing
the view, as the lease that renders it is: neither an update nor a write. A row a layer holds
is therefore not changed by the rows rendered after it, in its own list or in another the view
holds. With scroll layers off the lease counts as it did.

The renderer a `list` is handed is built when `list(...)` is called, as the view renders. What
the view reads before that can shape the rows, and a change of it renders them again. What it
reads after (whether its list is at its end, which of its rows are in view, the state of an
input below it) cannot shape the rows and leaves the list on its layer; it still shapes
everything else the view builds, so a scrolling `div` beside the list is painted again for it.

A change of a `list`'s state from outside the wheel (a splice, a remeasure, a reset, a scroll
to an item or an offset) holds other rows than the layer does. When the view holding the list
was notified with it, the rows the list shows are rendered again and the layer kept, the rows
around them rendered as they come to show; a changed item count, or a change no notification
of that view came with, paints the rows afresh. A view that asks whether its list
`is_scrolled_to_end` reads a boolean, and is rendered again only when it flips; scrolling a
list to its end where it already is (`scroll_to_end`, as a view following its transcript calls
on every render) moves nothing. Each step of an animated scroll (`ScrollActivity`) changes the
state and notifies the view: the rows shown are rendered on each, as without a layer, and the
work budget gives the layer up while the animation lasts.

A frame shown from a list's layer tells what the frame drawn without it tells. The rows the
layer holds off screen give it their pixels and nothing else, as a list without a layer does
not draw them: their hitboxes are clipped away, their mouse listeners hear nothing (one that
listens for a press outside its element would hear every press), they are no stops for the tab
key, hold no focus, are not among the frame's DevKit elements
(`Window::devtools_frame_snapshot`), are not counted among the hitboxes of the elements around
them, have no bounds for tests to find them by, and handle no text input. The rows the list
shows are told row after row, in the tab order and among the DevKit elements, whichever of them
the frame carried and whichever it rendered.

The rows a list shows are those its own layout renders. A `list` shows the rows from its first
on that start above the height of its bounds, by its own offset, wherever the offset moved to
whole device pixels puts them; a list with padding ends its rows at the padding. A row that lies
across the list's bounds without being shown (under the padding) is neither held nor rendered. A
`list` told of its rows' focus (`ListState::splice_focusable`) draws the row holding it when it
is off screen, after the rows it shows, for the keyboard to reach: that row is no row of the
layer's, and is drawn into the frame in the list's own clip once the layer is composited.

Two things keep all that true where rows reach outside the room their list gives them:

- a row that draws above or below its room (a shadow under a card, glyphs taller than their
  line) is held only while the list shows it, rendered when it comes into view and dropped when
  it leaves, so that nothing of a row off screen shows over its neighbours;
- a row that can be hit outside its room keeps its list on today's path. The frame holds the
  hitboxes of the rows it carried before those of the rows it rendered, and a point under two
  hitboxes hits the later one: rows hit only inside their own rooms are hit alike in any order.
  The frame that finds such a row draws the rows the list shows and no other.

A list goes without a layer, counted `ineligible`, when it has rows that stick to its top
(`ListState::with_sticky_items`, `UniformList::sticky_items`), is a uniform list drawn bottom up
(`y_flipped`), has rows holding what no tile can (see the reasons above), or lies over no
opaque background.

`FastStats::list_rows_rendered` and `list_rows_kept` count the rows rendered into list layers
and the rows kept without rendering. `Window::describe_rendered_frame` describes a frame shown
from a list's layer by the rows its list shows, row after row, as it describes a scene
compositing layers by their content. A record of a row off screen is left out of it only while
it answers nothing: a hitbox that can be hit, a tab stop in the order, a node the dispatch tree
finds by its focus, a listener that listens or an input handler is told, and the frame differs
from the one drawn without the layer. At a fractional scale factor the hitboxes carried with a
row lie a few units in the last place from where laying the row out afresh puts them.

The `fast::tests::list_layers` group checks all of it against a window drawing from scratch:
what is rendered on each kind of frame, the frames' quads at scale factors 1 and 1.25, and
whole frame descriptions over random histories of wheel turns, pointer moves and clicks. The
layers oracle runs its histories over a uniform list and a list (`list_layers::oracle`), and
lane A's parity group runs the catalog's list scenes with every mechanism on and off.

### Tiles in the renderers

A frame that composites a layer carries the layer's content (`Scene::layers`) and one
polychrome sprite per visible tile, whose texture id lies in a range no atlas allocates. The
wgpu renderer (`gpui_wgpu`'s `fast::layers`) and the Metal renderer (`gpui_apple`'s) each keep
a tile cache per renderer, so each surface of a composed window has its own:

- A tile is a texture of the frame's own format, keyed by layer and tile and tagged with the
  generation of the content it holds. A frame rasterizes the tiles it composites and the cache
  lacks: new ones, the ones the content's new generation lists as dirty, and all of a layer
  whose generation the cache skipped. A dirty tile no frame shows waits until one does.
- Tiles are rasterized before the frame's first pass, in the same command encoder (wgpu) or
  command buffer (Metal), by the renderer's own batch functions, with a viewport the size of the
  tile and the layer's baked background as the clear colour. A tile never holds a path: the
  core draws a layer's paths into the frame, over its tiles.
- The sprite that composites a tile sets `PolychromeSprite::pad` to
  `LAYER_TILE_PREMULTIPLIED`: the polychrome shaders multiply an atlas texture's colour by its
  alpha, and a tile's is premultiplied already.
- A renderer keeps at most `LAYER_TILE_BUDGET_BYTES` of tiles (64 MB; 32 MB in the browser and
  on mobile), evicting the least recently composited, never one the frame being drawn uses. A
  layer no frame composited for 120 frames is released.
- Every tile is released when the drawable is resized, the alpha mode or transparency changes,
  a GPU error is reported for a frame (a Metal command buffer that failed, any error the wgpu
  device raised), a frame that was to rasterize tiles is not drawn, or the device is lost (the
  recovered renderer starts with an empty cache). A change of scale drops the window's layers,
  so their tiles age out.

### In the browser and on mobile

The browser, Android and iOS run the wgpu tiles as they are: no platform has a tile format or a
compositing path of its own. A platform window says it rasterizes tiles
(`PlatformWindow::rasterizes_layer_tiles`) and adapts what lifecycle gives the renderer.

- **Browser.** WebGPU and the forced WebGL2 transport draw tiles through the renderer's own
  pipelines (the transport carries full records, tile sprites included). The browser budget
  (32 MB) applies. A container holding HTML islands or elements keeps today's path. The web host
  (`gpux-conformance`'s `web_host`) takes `?layers=1` or `?layers=0` and sets the shown scene's
  `FastMode` to every mechanism or none; under `diagnostics` the window publishes the fast-mode
  counters of the last frame and of the run as `globalThis.__gpuxFastStats`
  (`{ frame, run, tiles }`, the last what the renderer holds, has rasterized and has released).
- **Release.** `PlatformWindow::release_layer_tiles` drops every tile of the window's renderer. A
  frame carries each layer's content next to the tiles it composites, so the next frame that
  composites a layer rasterizes the tiles it lacks again before it draws them: a released tile
  is never composited stale, and the core needs no word of it.
- **Android.** Windows with a surface rasterize tiles, on Vulkan and on the GL fallback. The tiles
  go with `onTrimMemory`, when the app pauses or the activity deactivates, and when the surface is
  destroyed (`term_window`). `android-activity` forwards only `onLowMemory`
  (`MainEvent::LowMemory`), so a NativeActivity host that wants every trim level names gpux's
  `dev.gpux.mobile.GpuxNativeActivity` as its activity (lane E's manifest does), whose
  `onTrimMemory` calls the hook; the embedded host calls `embedded::trim_memory`, which its
  application's own glue calls from `onTrimMemory`. The hook never waits: a frame holds the
  renderer outside the window's lock while it draws, so a release that finds it taken is done by
  the next frame before it draws. gpux skips an emulator's software Vulkan (a Vulkan surface keeps
  the window connected until it is destroyed, so one that cannot present would keep it from the
  GL fallback); `debug.gpux.software_vulkan=1` (lane E sets it for `GPUX_ANDROID_SOFTWARE_VULKAN=1`)
  lets the host pick it, which is how lane E proves Vulkan without a hardware driver.
- **iOS.** A window with a Metal renderer rasterizes tiles. The tiles go with
  `gpui_ios_did_receive_memory_warning` (the shell forwards `applicationDidReceiveMemoryWarning:`)
  and with `gpui_ios_did_enter_background`.
- **Touch.** A touch pan and the fling after it scroll like a wheel does, so mobile scrolling
  composites.

Lane B's `scroll-layers.spec.ts`, lane E's `scroll-layers-android-*` and lane F's
`scroll-layers-ios-*` check what the browser and the devices draw (see
[testing](testing.md)): each runs a page of cards, a scrolling `div` and a `list`, with every
mechanism and with none, scrolled by whole device pixels, and compares the pixels the two show
byte for byte; the counters of the first say how many frames came from tiles. A window that
refreshes draws from scratch, with no layer, so these checks wait for the frames a scroll asks
for and read the frame the window last drew rather than drawing another for the capture.

A tile has to hold the bytes the frame would. Three things in the shaders make it so. They apply
in every mode, so a frame's last bits can differ from those of gpux before them (the committed
goldens did not change):

- An untransformed quad, shadow, underline or sprite is shaded from the fragment's own
  position, which is exact. The position interpolated from the vertices differs in its last
  bits with what the rasterizer clips the primitive to, so a primitive crossing a tile's edge
  shaded differently there than in the window (seen on Apple GPUs). Transformed primitives
  keep the interpolated position.
- The gradient dither is seeded by the pixel within the quad, not by its position in the
  window, so a gradient dithers alike wherever it is drawn.
- A transformed sprite (a turned or scaled glyph or image) is filtered by hand, within its own
  atlas tile (`sample_tile`): the four texels around its position, clamped to the tile, are
  read by index and mixed in floating point, from the position within the tile alone. The
  sampler would filter at the tile's edge in a texel of the tile next to it (the atlas packs
  tiles without a gutter) and round its weights by where the atlas placed the tile and how
  large the atlas is, so the faint ink at a turned glyph's edge depended on which glyphs the
  atlas held beside it: a window that paints more glyphs than another (a layer's region does,
  in the frames where a container holding transformed content is tried again for a layer)
  drew a few edge pixels of turned text differently, and went on doing so. An untransformed
  sprite is sampled where its fragment lies, by the sampler, as before.

A shadow draws three blur radii past its box, and a scene used to drop a primitive whose box
had left its clip: a blur vanished a few pixels early as its element scrolled out of a
viewport, and a layer's tiles, which hold the content as it was painted, kept showing it. The
scene now keeps a shadow while what it draws reaches into its clip, with or without layers
(`fast/edge_shadows.rs`).

Where a blur is stacked must not depend on how far its content is scrolled either. A blur falls
on neighbours its shadow's box never overlaps, over those of a lower draw order and under the
others, and a draw order worked out from what a clip leaves in view changes as rows enter and
leave it: the shadow of a card in the next row lies over its own row's background, and so over
the row above; with its row out of view it would be level with that row instead, and hidden
under it. So at a clip's edge a scene orders primitives as it would were that clip not there,
every other clip staying where it is:

- a primitive the clip drops keeps an entry of its bounds, at the order it would have had;
- a primitive the clip cuts is ordered over the dropped ones, and the other cut ones, that its
  bounds lie over;
- a kept shadow is ordered over what lies under its box, dropped or drawn, and what is painted
  after it over its box is ordered past it: the element casting it, whose edge can show while
  the shadow's box, offset away from it, does not.

The window tells the scene each clip as it pushes and pops it, so the scene knows the clips a
primitive is painted in one by one, not only their intersection. With a clip not there a
primitive covers its bounds within the clips pushed inside that one: a child of a card that
clips its content, cut by the viewport the card scrolls in, reaches as far as the card, and a
row's border, drawn in a mask of its own, as far as the border. A border is painted as four
quads, one in each side's strip, inside one clip told to the scene (`ClipOp::Border`, with the
four strips): the strips are masks inside that clip, not clips of their own, and a quad's
bounds are taken within its strip when a clip around it is lifted, as they were with a clip for
each strip. That is two paint operations and no set of entries of a border's own, where there
were eight and four, and the scene it leaves is the same (`fast/border_clips.rs`). A transformed
element, whose masks are what its transformation makes of them and not what the clips told leave,
and an element thinner than its border, whose strips reach past it, or less than twice as thick,
whose strips overlap, keep a clip for each strip. What was painted before a clip
was pushed lies behind everything in it: a dropped primitive with nothing of the clip's under
it is ordered over what was painted, before the clip, where it would come into view. A clip
pushed where one was, after something else was painted, is another clip, though it clips
alike: two containers laid one over the other each keep what they hold apart. Only a clip
pushed again at once, with nothing painted since it was popped, goes on as the clip it was,
which is how a scene replayed range after range for a window's surfaces stays one scene.

Content in a clip has, primitive for primitive, the draw orders it has with the clip not there,
wherever the clip lies over it. A primitive wholly inside its mask meets nothing outside it and
is ordered as before, and a scene none of whose primitives a clip cuts or drops is the scene it
was. This applies with fast mode off too: a blur at the edge of a scroll container, a clipped
panel or the window no longer pops as its box crosses the edge.

A layer's tiles draw the content's primitives at the orders the content gave them as a whole,
not at orders worked out among the primitives over each tile, which changed at a tile's edge
when what lay under a shadow's box did not reach into the tile, and left a step in the blur
there. A list's rows are scenes of their own, ordered as painted one after another, and a
tile is drawn again when those orders change. Only the rows whose bounds meet are painted
together to find those orders out, near the origin wherever they stand in the list; a row that
meets no other is ordered as it is alone, and every row keeps its orders while the rows it
was ordered after stay. So a row scrolled into a list costs its own ordering: ordering the
whole content for every row made a frame that scrolled a row in cost 13.7 ms far down a list
of rows of forty quads, where the list painted without a layer takes 1.3 ms and such a frame
now takes 0.5 ms. A `list` paints the rows it shows in every frame, one after another in one
scene, which orders them as the content wants them: rows painted together that met nothing
else painted there keep the orders they were painted at, and are not painted again to find
them out (a frame scrolling a `list` of plain rows a row takes 1.35 ms, as before the kept
shadows, where painting them again made it 1.46 ms). Rows that each reach over the one before
are one run, ordered again together when a row joins or leaves it. Which rows meet is found
without comparing each with all: rows that follow one another down or across the content are
seen in one pass, and otherwise rows are taken in runs along an axis and compared within a
run (6.5 µs for a list holding 1339 rows, where comparing every row with every set had
doubled such a list's frame). A `uniform_list` of such
rows of forty quads takes 0.5 ms a frame, where it took 0.24 ms before and takes 1.35 ms
without a layer.

What is painted after a scrolling container lies over all of it, as it lies over the
container's layer when it has one: a bar laid over the container's edge, or its scrollbar, is
drawn over the blurs of the shadows inside, whatever their draw orders. The window tells the
scene where a scrolling container's content begins and ends, and the scene orders what follows
over what each shadow in it draws past its box, where that shows and, at the edges of the
clips around the container, where they cut it: a scrolling container inside scrolled content
is followed alike wherever the page's viewport lies over it. Inside the content, a primitive
beside a shadow that does not lie over its box is not ordered by it, kept or not: a blur and
such a neighbour are drawn by their draw orders, as they always were.

A layer's content is painted in its painted region, a clip of its own. The content is then
kept moved to where its tiles stand, every primitive at the order it was painted at, and
where it is drawn into a frame instead of composited, the replay tells the scene the viewport
it is clipped to: nothing orders the content again without knowing a clip it was painted in.

Only what leaves a paint operation changes what a scene holds of its clips, so a scene
replayed from another's operations ends where that one did. What lies behind a clip is found
from where it lies, in a grid of what had been painted by then, built when first asked,
which begins where those bounds do and has cells as large as it takes to span them: a
table of thousands of clipped cells costs what lies near each cell, not a pass over the
scene for each (in a release build 16 000 cells that each drop a line take 9.0 ms to insert
against 4.6 ms for the same cells dropping nothing, and 9.8 ms scattered at random; a table of
2000 rows of 20 clipped cells takes 19.3 ms against 20.5 ms with no clips told).

The rule has bounds. A primitive a clip drops from further off than 2048 device pixels leaves
no entry, at that clip or at the clips inside it, so a blur reaching further than that is
ordered without what lies under its box. A primitive is raised past what it lies over by at
most 256 orders at a time, which content whose rows each overlap the one before for hundreds
of rows exceeds. And a mask that is not what the clips the window pushed leave (the window's
own bounds, which everything outside a clip is painted in, is one) is one clip more,
innermost, taken as pushed when a primitive first comes with it and never popped. A list's
rows are painted in the regions of the frames that painted them, which their operations do
not tell of: where rows meet at a region's edge they are ordered by that rule.

The renderers' groups (`fast::layers::tests` in `gpui_wgpu`, `fast::layers` in `gpui_apple`)
compare content drawn from tiles with the same content drawn directly, byte for byte: tiles of
positive and negative coordinates, odd translations, a second frame from the cache, paths over
tiles, eviction and release. Lane C's `lane_c_scroll_layers` group scrolls real views (text
rows at scales 1, 1.5 and 2, cards with shadows and gradients, shadowed cards closer together
than their blurs reach, paths, both axes, content that changes while scrolled) in a window with
every mechanism on and in one with them off and the snapping forced on, and compares every
frame's pixels: equal everywhere, at every one-pixel step of a shadow's box crossing the
viewport's edge too. So are two shadowed containers laid one over the other with a veil
between them, and a page whose rows hold scrolling containers of their own under a shadowed
bar painted over each one's edge. The same group scrolls a
`uniform_list` and a `list` of text rows, at scales 1 and 1.5 and with rows that change while
scrolled, whose frames are equal everywhere.

At a scale factor that is not a whole number a logical pixel is not a whole number of device
pixels, and what is painted is worked out from the scroll offset in logical pixels: at 1.25 an
offset of 114 device pixels is 91.2 logical ones, which no `f32` holds. An edge that lies on a
device pixel comes out a few millionths of a pixel to one side of it or the other, by how far
the content is scrolled. A layer's content is painted at one scroll offset and shown at
others, so where that showed, tiles differed from painting. Two things made it show, and while
scroll layers snap (in a window with them, and in the reference they are held to) neither
does:

- a clip is grown to whole device pixels, and flooring an edge a hair short of a pixel takes
  in a row more: what a clipped card holds showed a row further in one frame than the other,
  and so did an underline. An edge within a sixty-fourth of a pixel past a pixel's edge now
  lies on it (`paint::cover_bounds`, `cover_mask`);
- a drop shadow's box is not snapped, since a blur hides where it begins, but a box a few
  millionths of a pixel off gives a blur a level off in a pixel or two. Its edges are put on a
  grid of sixty-fourths of a device pixel, which moves by whole pixels exactly
  (`paint::shadow_bounds`).

Where these hold, and where they do not:

- **Far down a container.** An edge is the container's origin plus the offset, in logical `f32`
  pixels, and the error grows with the sum: the 1/64 px absorbs it up to about 50 000 device
  pixels from a container's origin at fractional scale factors. A `div` of 5000 clipped cards or
  shadowed rows is equal to the reference at 20 000 and 50 000 px down at 1.1, 1.3333, 1.5, 1.75,
  2.5 and 3 (and at 1.25). Past it the layered frame differs from the reference by a level along
  the edge row of every card in view, in a share of the frames that depends on the scale: of 25
  steps at 70 000 px, 2 to 3 at 1.1, 13 to 15 at 1.3333, none at 1.5, 19 to 20 at 1.75, 19 to 22
  at 2.5 and 15 to 17 at 3 (at 100 000: 0 to 1 at 1.1, 3 to 8 at 1.3333, none at 1.5; at 1.25,
  first measured at 100 000, 6 to 8 of 25 steps, up to about 1 900 pixels a frame, and 18 of 25
  at 200 000). A clipped card loses a whole row (77 levels) from 300 000, measured at 1.25. A
  `uniform_list` stays equal to 250 000 px (measured at 1.1, 1.3333, 1.75, 2.5 and 3; to 300 000
  at 1.25), a `list` to 250 000 (at 1.1, 1.75, 2.5 and 3), both positioning their rows from a
  small offset, and content scrolling sideways to 50 000 (at 1.25, 1.3333, 1.75 and 2.5).
  Applying the offset in device pixels, after scaling, would remove the limit. At about 1.05
  million device pixels (2048 tiles) a layer's tile ids run out: `layer_tile_id` asserts in a
  debug build.
- **Against a window that does not snap.** Snapping alone (scroll layers on, in the window and in
  the reference) can differ from a window with none: a clip edge within 1/64 px past a pixel's
  boundary lies on it, and takes in or leaves out a row that flooring would not (at 2.625 a
  clipped card differs by a row, 80 levels, in a still frame). This is part of the snapping
  exception, which has always moved what is drawn by device pixels; with snapping off nothing
  changes.
- **Empty clips.** The mask of a primitive wholly outside a clip is empty and is not grown to the
  grid; the clips the window told the scene are. At a scale factor whose edges fall between
  pixels the two differ by a few millionths of a pixel, and the scene takes an empty mask for the
  clips told (`edge_shadows::told`).

A transformed element's clip mask is the bounding box of what its transformation makes of the
clip's rectangle. While a layer's content is painted the clip is cut by the layer's painted
region, the viewport and its overscan, so the box of a turned strip came out looser than the
one the reference makes (cut by the viewport first), and a border painted in four strips, each
the whole ring under one strip's mask, let through of the ring what the reference holds back,
in the frames in which a container that holds transformed content is tried again for a layer.
A transformed element's clip is now cut by the viewport while a layer's content is painted
(`paint::transformed_clip`): culling, and every mask that is not transformed, keep the painted
region, which tiles need for their overscan. `fast::tests::transformed_masks` and
`lane_c_layer_scales` hold the frames equal.

Content painted with a transformation of its own (an element moved, turned or scaled by
`translate`, `rotate` or `scale`) is shaded from positions the rasterizer interpolates, which
no tile reproduces to the last bit: its container is painted, not drawn from tiles.
`lane_c_layer_scales` scrolls the shadowed, clipped and listed views at 1.25, 1.5 and 1.75,
and the transformed ones, and every frame equals the reference's.

## Window composition

`Window::enable_window_composition()` lets a window present its one scene on several ordered
surfaces, so a platform-native view sits above GPUI's base content and below its overlays. The
API mirrors Zed PR #62379: the controller has `base_surface` and `overlay_surface`,
`create_native_surface`, `create_external_gpu_surface` and `create_gpui_surface`, and transactional
`place_above`, `place_below`, `reparent` and `remove_surface`; `Window::with_composition_surface`
paints a scope onto another GPUI surface. A `native_surface(surface)` element places a native or
external surface where it is laid out, handing it device-pixel bounds only when they change.

- **Base and overlay.** The root view paints onto the base surface. Deferred draws, prompts, the
  active drag, tooltips and the inspector paint onto the overlay, and so does gpux's top-layer
  band wherever it was painted. A range replayed onto a surface reopens the clip, positioned and
  top layers it began inside.
- **Reuse.** Views drawn from last frame (cached, retained, spliced) carry their surface
  switches and their native-surface placements.
- **Input.** A surface above native content takes pointer input only within its input region:
  the bounds of what it drew last frame, clipped, without shadows
  (`ComposedScene::layer_input_region`). A menu or a top-layer scrollbar on the overlay leaves
  the rest of a native view clickable; a click on the native view reaches it, not GPUI.
- **Native surfaces.** Once a `native_surface` element has placed a surface, the surface is
  shown only in frames that place it and hidden in the others. GPUI's content masks do not clip
  native surfaces: one inside a scroll area shows past the area's edges unless its content clips
  itself, or its element places it at its visible part, as gpui-mobile's `platform_view_element`
  does.
- **Text.** Text on the overlay is grayscale, since it blends over native content. Backdrop effects
  on the overlay sample only the overlay's GPUI content, never native pixels.
- **Off by default.** Until composition is enabled, the scene, the draw call and allocations are
  unchanged. The first composed frame is drawn afresh.

Platforms: macOS (overlay `NSView`s on `CAMetalLayer`s sharing the window renderer's device,
instance buffers and atlas; the overlay takes mouse events only within its input region).
Wayland (synchronized overlay subsurfaces and desynchronized native subsurfaces handing out
`RawWindowHandle::Wayland`, sized through `wp_viewport`, the overlays committed before the base;
an overlay's input region is its content). X11 (native child windows; GPUI content above them is
cut out of their shape, or, under a compositing manager with an ARGB visual, drawn into a
transparent override-redirect overlay window whose input shape is its content). wgpu platforms
share the window's GPU context, atlas and pipelines through `WgpuRenderer::new_sharing`. Windows
(the window keeps its wgpu DX12 HWND swap chain with `DXGI_SCALING_NONE`; a gpux-owned topmost
DirectComposition target holds native surface visuals, clipped and handed out as `IUnknown` for
WebView2's `SetRootVisualTarget`, and overlay visuals whose premultiplied swap chains wgpu makes
from `SurfaceTargetUnsafe::CompositionVisual`; gpux commits after every composed frame, a lost
device rebuilds the tree and calls `compositor_recreated`, and key downs of other windows are left
to them). The browser on WebGPU (overlay canvases above the `html_element` DOM overlay, sharing the
window's device, with an input shield clipped to each overlay's input region that hands pointer
events to the window's canvas, the pointer staying inside the window as it crosses between them; native surfaces are `html_element`'s, and WebGL2 reports composition
unsupported and keeps the single canvas). iOS (native `UIView` containers and a `GPUIOverlayView` on a transparent `CAMetalLayer`, subviews
of the Metal view; the overlay's renderer shares the window's through `new_sharing_target`, and
its `hitTest:withEvent:` takes a touch only within its input region). Android (each native surface
is a container in a non-focusable panel window attached to the activity's window, so the
NativeActivity host and the embedded host work alike; under Vulkan the overlay is a translucent
sub-panel window holding a `SurfaceView` that GPUI renders into, touchable only while it has
content; wgpu presents GLES surfaces only opaque, so under GL the whole scene is drawn into GPUI's
window and each container clips the overlay content above it out of itself and hands touches there
to GPUI, which leaves no shadow blending over native views; a panel whose views take text, a
`WebView` or an `EditText`, takes the window focus when touched so the soft keyboard reaches it,
and gives it back on a touch outside or the back key, without GPUI going inactive; an embedding
host whose `SurfaceView` does not fill the window names it with `Composition.setHostView`, and
panels, cut-outs, the overlay and touches follow its position). The Android side is gpux's Java
helper `dev.gpux.mobile` in `crates/gpui_mobile/java`: `gpux-testkit android build` compiles it
with `javac` and `d8`, and apps add those sources to their Gradle build (and keep
`dev.gpux.mobile.**` from R8). On
Wayland the placements, geometry and input regions handed to the compositor are logged at debug
level under `gpux::composition`.

gpui-mobile's platform views sit on composition: `platform_view_element` makes a native surface
for its view the first time it is laid out in a window, attaches the view to it
(`PlatformView::attach`), and places the surface at the part of the element inside the bounds of its content mask, the
view keeping its full frame inside (`PlatformView::set_frame`), so a scroll area clips it. On Android
`dev.gpux.mobile.PlatformViews` makes the view for its type: `container` and `webview` are built
in, and an app registers its own (a video surface, a camera preview) with
`PlatformViews.register` before GPUI creates them.

## Measuring

Fast mode's benefit is measured scenario by scenario in every cumulative mode (`off`, `layout`,
`views`, `splice`, `layers`) by the `fast_mode` bench of `gpux-platform`:

```bash
cargo run -p gpux-testkit -- perf fast-mode --name <run> [--compare artifacts-temp/testkit/perf/<earlier-run>]
```

It writes `artifacts-temp/testkit/perf/<run>/fast-mode.md`: per scenario, the median and
interval per mode with deltas against `off` and the previous mode, instructions, allocations and
the work counters; per mechanism, the scenarios ranked by benefit with regressions flagged.
`gpux-testkit lane g` runs the same suite with the other benchmarks and writes
`artifacts-temp/testkit/g/fast-mode.md`. Timings are compared only on a developer machine (lane
G); CI asserts the deterministic work counters instead
(`cargo test -p gpux --features test-support,bench-support --test fast_mode_attribution`).

### Pre-optimization baseline

The baseline every mechanism is compared with was captured after the measurement harness landed
and before any mechanism:

| | |
|---|---|
| Command | `cargo run -p gpux-testkit -- perf fast-mode --name baseline-s01` |
| Source | `24d413c`, a clean tree: the sprint-1 harness, no mechanism yet |
| Machine | Apple Silicon Mac (`Darwin 25.6.0 arm64`), Metal headless renderer |
| Run | `artifacts-temp/testkit/perf/baseline-s01/` (`run.txt` records the commit and machine) |

The scroll scenarios changed after the baseline, when scroll layers landed. Their scroll areas
sit on an opaque panel, and the scrolling `div`s leave a 40 px gutter beside their rows:
`scroll-child-view`, `sticky-scroll` and `effect-scroll` wheel with the pointer resting there,
over no row, and `scroll-same-view` with it over the rows, whose hover styles follow it, which
is the case a `div`'s layer never composites. A window's pointer is also where the scenario puts
it on every platform, where before a test window on macOS drew with the pointer at its top-left
corner. Compare their `layers` timings with `off` of the same run, not with the baseline.

The final comparison runs on the same machine:

```bash
cargo run -p gpux-testkit -- perf fast-mode --name final --compare artifacts-temp/testkit/perf/baseline-s01
```

## Measurements

### The final comparison

Run on the baseline's machine, an Apple silicon Mac (`Darwin 25.6.0 arm64`, macOS 26.6), at `9e216c9`
with only the AgentX state file changed:

```bash
cargo run -p gpux-testkit -- perf fast-mode --name final --compare artifacts-temp/testkit/perf/baseline-s01
```

The run is `artifacts-temp/testkit/perf/final/` (`fast-mode.md` is the report, `run.txt` names the
commit and machine). The suite draws each frame of a scenario and presents it to the headless Metal
renderer, so the frame's encoding and submission are in the median and the GPU's own execution is
not, and every frame has a floor of about 8 ms and 90 M instructions (`animated-frame`, a control no
mechanism can speed up, costs 8.2 ms in every mode). Each cell is the change of the median against `off` of the same run; a regression is a
slowdown beyond both confidence intervals and over 3%.

| Scenario | Off | Layout retention | View retention | Splice | Scroll layers |
|---|---|---|---|---|---|
| still-frame | 23.8 ms | -3.9% | -32.7% | -32.5% | -32.9% |
| one-panel | 24.3 ms | -3.3% | -31.9% | -30.1% | -30.6% |
| six-panels | 24.6 ms | -3.0% | -29.1% | -26.6% | -27.1% |
| **all-panels** | 25.4 ms | **+10.7%** | **+11.7%** | **+13.2%** | **+14.6%** |
| table-stream | 10.5 ms | -3.1% | -5.9% | -5.4% | -5.9% |
| settings-toggle | 8.5 ms | -1.5% | -2.5% | -1.9% | -1.1% |
| tree-expand | 8.8 ms | -0.4% | -0.5% | +0.6% | -0.6% |
| form-typing | 8.3 ms | +0.6% | -0.8% | -0.3% | +0.7% |
| hover-sweep | 9.4 ms | -0.4% | -0.3% | -0.5% | -0.5% |
| scroll-child-view | 11.4 ms | -4.1% | -4.8% | -3.5% | **-15.9%** |
| scroll-same-view | 11.1 ms | -1.6% | -1.2% | -1.0% | -0.9% |
| scroll-uniform-list | 9.3 ms | +0.2% | -0.7% | +0.5% | +0.9% |
| scroll-list | 9.2 ms | +0.3% | +1.4% | +0.9% | +1.5% |
| chat-scroll | 9.5 ms | -1.6% | -1.5% | -1.0% | +4.0% |
| sticky-scroll | 11.6 ms | -4.3% | -2.7% | -2.8% | -3.2% |
| effect-scroll | 11.3 ms | -2.0% | -3.3% | -1.9% | -1.1% |
| animated-frame | 8.2 ms | -1.0% | -1.4% | -1.5% | +0.3% |

The columns are cumulative (each mode has the mechanisms before it on), so a mechanism's own
benefit is the step from the column before it:

| Mechanism | Pays off | Neutral | Costs, and why |
|---|---|---|---|
| Layout retention | Every scenario that lays out unchanged text: 3.0–3.9% on the panel scenes (-8 to -10% instructions, -29 to -34% allocations), 3.1% on `table-stream`, 4.1–4.3% on the scrolling `div`s. | Lists, hover, typing and the controls: within ±1.6%. | `all-panels` **+10.7%** (+9.6% instructions, +15% allocations): every one of 60 panels changes every frame, so no node, measurement or view is reused, and the frame still pays a keyed claim for each of its 7861 nodes and a record of each text measurement. The baseline had no mechanism to cost this; the standalone optimizations since made `off` 8.6% cheaper and retention could not share that. An application that rebuilds nearly every view every frame pays this: set `GPUX_FAST_MODE=0`, or `window.set_fast_mode` for the windows that do. |
| View retention | Whole views reused: 27–30% over layout retention on `still-frame`, `one-panel` and `six-panels` (60 panels in the page, 0 to 6 changed; against `off`, -46 to -41% instructions and -83 to -72% allocations), 2.9% on `table-stream`, 1.1% on `settings-toggle`. | Everything that changes most of what it draws. | `all-panels` +0.9% over layout retention: records are made for views that are never reused. |
| Splice | Builds one view instead of three in `one-panel` and two fewer in `six-panels`, and copies the board around them. | `still-frame`, `table-stream`, the lists. | **+2.6% on `one-panel`, +3.5% on `six-panels`** over view retention. The board is a flat row of 60 panels, so copying it around one gap costs more than building the board's own few elements again; splice pays where a view is dirty only because something deep in it is and its own layout is large. No scenario of this suite is that shape (`oracle_splice` and `splice` hold what splice does), so none shows a gain. |
| Scroll layers | `scroll-child-view` **-12.8%** over splice (-15.9% over `off`; -26% instructions, -88% allocations): a scrolling `div` of 120 cards is composited from tiles on every frame. | `scroll-same-view` (its rows' hover styles follow the pointer, so no frame only scrolls: the layer is given up and the frame is today's), `sticky-scroll` and `effect-scroll` (their content cannot be tiled: counted as ineligible), `form-typing`, `hover-sweep`, `animated-frame`. | `scroll-uniform-list` +0.4%, `scroll-list` +0.6%, `chat-scroll` +5.1% over splice (+0.9%, +1.5% and +4.0% over `off`; none beyond the intervals). A virtual list's frame paints 400 primitives of its own without layers and 5 with (`chat-scroll`), and the suite's 8 ms floor hides the rest; what is left is the layer's own bookkeeping (the rows it keeps, the ordering of rows that meet; not timed apart), which the frames of this suite have little to set against. What tiles save is draws and fragments, which a renderer shows: see the browser and mobile measurements below. |

Against the baseline (`baseline-s01`, `24d413c`) `off` of the panel scenes is 7–10% faster (-8 to
-10% instructions): the standalone optimizations of the next section, which every mode shares. The
scrolling scenes' times are 13–17% higher in every mode of this run, `off` included, with the
instructions unchanged (±1%): they changed after the baseline (see the baseline's note on the
scenes), so compare their modes with `off` of the same run, as above.

### The cost of telling clips

Telling the scene each clip, so that a shadow at a clip's edge is kept and ordered as it is without
the clip (LYR-14), costs scenes that are painted whole. Scene-level probes build the scene such a
page makes, 25 rows in view of 8000 bordered rows or of 2000 rows by 20 cells, and time its
construction and `finish` in an optimized build (`opt-level` 3, debug assertions on; best of 9 and 5
rounds). Each row is a background, a line and four border quads; each cell a background and two lines, one
dropped by the cell's mask. They ran on the machine of the comparison above, at three commits
(`artifacts-temp/cost-probe.log`):

| Scene | Before s12c (`8285288`: no clip told) | After s12c (`49690f7`: a clip told for each strip of a border, for each cell) | After t55 (`86d1be3`, at head: one clip a border) |
|---|---|---|---|
| 8000 bordered rows | 12.05 ms, 151 operations | 14.7 ms (1.22×), 64 770 operations | 14.5 ms (1.20×), 16 770 operations |
| table of 2000 rows by 20 cells | 30.15 ms, 1001 operations | 44.3 ms (1.47×), 125 123 operations | 44.0 ms (1.46×), 125 123 operations |
| the same scene with nothing told, at head | 13.8 ms (1.15×), 771 operations | | 34.3 ms (1.14×), 3561 operations |

What s12c cost the bordered rows t55 gave back only in operations (a quarter as many, and a border's
own set of entries); the time is 1.5% under what four told clips cost, because the entries of a
border's strips were already kept in one pass. LYR-17's bound (at most 1.28 times what the scene
cost before the kept shadows) holds at 1.20. The table is not a border's scene: each cell tells one
clip of its own, which t55 does not change, and that is where its 1.46× stays. A scene that is not
painted whole (a list's rows, a scrolled `div` in tiles) pays only for the rows in view. The scene
the eighth review of s12c measured on the real path (empty masks taken for untold clips) cost
12.3 to 19.1 ms; s12d closed that.

### In the browser

Lane G's `@timing` tests of `scroll-layers.spec.ts` scroll the page of `web-scroll-layers` (a 440 by
340 canvas) by 60 wheel steps with every mechanism on and off, and print the 95th percentile of
gpux's frame work in its animation-frame callback, on Chromium on the Mac's GPU (Metal), WebGPU and
forced WebGL2. Three runs (`artifacts-temp/lane-b-timing.log` and the two after it), the p95 in
milliseconds, layers on against off:

| Scene | WebGPU | WebGL2 |
|---|---|---|
| cards in a scrolling `div` | 2.80 / 3.18 · 2.66 / 3.18 · 2.73 / 3.15 (-12 to -16%) | 3.12 / 3.33 · 3.12 / 3.25 · 2.97 / 3.34 (-4 to -11%) |
| rows of a `list` | 3.53 / 2.87 · 3.56 / 3.07 · 3.55 / 3.10 (+15 to +23%) | 4.16 / 3.42 · 3.88 / 3.13 · 4.00 / 3.26 (+22 to +24%) |

The slowest frame is longer with layers on in both scenes (5.6–8.9 ms against 3.2–5.5): it is the
frame that rasterizes tiles. A list's rows are composited from tiles in 58 of 60 frames, and the
frame work, which is CPU time (encoding the frame and submitting it), is not lower: the page is
small, so each frame's fixed cost is most of it, and what tiles save is draw calls and fragment
work, which the animation-frame callback does not time. The list's layer keeps its rows one by one
(`list_rows_kept`), and the rows the pointer's hover changes are rendered every frame. On a page
with a lot more on screen the balance would change; this one was chosen small so a software renderer
could run it.

### On a phone

The Android and iOS hosts run the same pages with every mechanism and with none
(`scroll-layers-android-timing`, `scroll-layers-ios-timing`): each page scrolled down and up four
times through the shared pan steps (128 frames), reporting the median, 95th percentile and slowest
of the frame work (drawing and presenting a frame) and the frames that came from tiles. **No
physical device was available: both runs are an emulator and a Simulator,** and the numbers are
those of the Mac they run on.

| Host | Scene | Layers on (p50 / p95) | Layers off (p50 / p95) |
|---|---|---|---|
| Android emulator (arm64, API 34, 320 by 640), GL | `div` | 20.5 / 21.8 ms | 34.8 / 39.0 ms (-41%, -44%) |
| | `list` | 20.5 / 23.9 ms | 34.8 / 39.7 ms (-41%, -40%) |
| Android emulator, software Vulkan (SwiftShader) | `div` | 6.1 / 8.6 ms | 10.3 / 19.0 ms (-41%, -55%) |
| | `list` | 7.7 / 9.9 ms | 10.6 / 16.6 ms (-28%, -41%) |
| iPhone Simulator (iOS 26.5), Metal | `div` | 5.2 / 6.2 ms | 8.0 / 8.2 ms (-35%, -25%) |
| | `list` | 4.3 / 5.0 ms | 4.6 / 5.3 ms (-7%, -5%) |

Every layers-on frame came from tiles (128 of 128) and none with layers off.

### Lane G on this Mac

`gpux-testkit lane g` ran on the Mac: lane B in the `local-gpu` profile (184 specs passed) and lane C on
the Metal adapter (probes only; 77 and 77 passed, the layer-scale, scroll-layer and list-hover groups
included). Its lane D part (real desktop windows) did not complete: with the display asleep every
window reported "no frame within 10s", and the rerun with the display held awake (`caffeinate -d -u`)
was stopped before it reached lane D. Lane D is proved in CI (`ci.yml`, three platforms); no lane G
desktop-window or benchmark result is recorded here.

### On a wgpu desktop

PERF-4 asks for lane G on a wgpu desktop as well (Windows DirectX 12 or Linux Vulkan on a GPU). **This
was not run: no machine with such a GPU was at hand.** What exists instead:

- Windows (DX12 on WARP, a software adapter) and Linux (lavapipe) run lanes A to E in CI, which prove
  the behaviour and the pixels of every mechanism there, not their timing; lane G refuses to run
  under CI because software adapters make timing meaningless.
- The `fast_mode` suite did not run in the Linux VM (lavapipe): its scenarios stop at their own check
  (the harness's windows drew no glyphs there, 60 primitives where 3900 are expected), and a machine
  without a GPU adapter cannot open the bench's windows at all. The suite is macOS-only until its Linux
  harness draws text.
- The wgpu renderer's tiles did run on a GPU in this comparison, through Metal: the iPhone Simulator
  above is wgpu on Metal (`gpui_wgpu` through `GPUIMetalView`), and Chromium's WebGPU and WebGL2
  are the browser's.

To close this on a Windows or Linux machine with a GPU, run `cargo run -p gpux-testkit -- lane g`
there and compare its `artifacts-temp/testkit/g/timing.md` and `fast-mode.md` with the tables above
(the `@timing` specs are the part that needs the GPU; `gpux-testkit lane g --headed` shows the
browser).

### Standalone scene and text optimizations

Each optimization was committed on its own and measured on the baseline machine with
`perf fast-mode --compare artifacts-temp/testkit/perf/baseline-s01`, one run per commit. Each run
contains the commits before it. One is kept only if it makes at least one scenario faster without
making another slower beyond noise.

Each cell is instructions / median time against the run before, as the geometric mean over the
five modes (no mechanism is on yet, so the modes measure the same work). Run to run, the median
time on this machine drifted by up to ±6% across every scenario at once, including scenarios a
commit cannot touch (number shaping's +4–6% everywhere with instructions unchanged, then −3–6%
back). The instruction counts do not drift, so keep or revert was decided on instructions, with
the times checked against them.

| Scenario | Off at baseline | Grid bounds tree | Draw-order sort | Number shaping | Path cache | Glyph runs | All five, vs baseline |
|---|---|---|---|---|---|---|---|
| still-frame | 26.39 ms | −10.0% / −8.8% | −0.5% / −2.5% | +0.1% / +4.1% | −0.0% / +0.3% | −0.6% / −5.9% | −10.9% / −12.7% |
| one-panel | 26.51 ms | −8.6% / −7.1% | −0.4% / −2.8% | +0.0% / +4.0% | +0.0% / +0.6% | −0.6% / −5.7% | −9.5% / −11.0% |
| six-panels | 26.87 ms | −9.0% / −7.8% | −0.5% / −3.0% | +0.1% / +4.5% | +0.1% / +0.6% | −0.6% / −6.2% | −9.9% / −11.8% |
| all-panels | 27.98 ms | −8.9% / −7.7% | −0.5% / −2.9% | +0.1% / +4.4% | +0.0% / +0.8% | −0.7% / −6.6% | −10.0% / −11.9% |
| table-stream | 10.43 ms | −1.4% / −0.0% | −0.3% / −1.1% | +0.0% / +6.0% | +0.0% / −0.2% | −0.5% / −4.9% | −2.1% / −0.5% |
| settings-toggle | 8.38 ms | −0.1% / +0.3% | −0.1% / −0.7% | +0.0% / +5.4% | +0.0% / −1.4% | −0.2% / −1.5% | −0.4% / +2.0% |
| tree-expand | 8.72 ms | −0.0% / +0.9% | −0.1% / −1.1% | +0.1% / +4.7% | −0.0% / −0.4% | −0.2% / −3.1% | −0.3% / +0.9% |
| form-typing | 8.18 ms | −0.1% / +0.8% | −0.0% / −0.3% | +0.1% / +4.1% | −0.0% / +0.1% | −0.1% / −2.4% | −0.2% / +2.1% |
| hover-sweep | 9.31 ms | −0.3% / −0.3% | −0.0% / −1.0% | +0.1% / +5.9% | −0.0% / −1.8% | −0.1% / −2.4% | −0.4% / +0.2% |
| scroll-child-view | 9.80 ms | +0.0% / +0.4% | −0.1% / −1.4% | +0.1% / +6.6% | −0.1% / −5.8% | +0.1% / +1.3% | +0.0% / +0.7% |
| scroll-same-view | 9.81 ms | +0.0% / −0.9% | −0.1% / −0.7% | +0.1% / +4.4% | −0.1% / −4.4% | +0.0% / +0.7% | −0.0% / −1.2% |
| scroll-uniform-list | 8.24 ms | −0.0% / −0.3% | −0.0% / −0.8% | +0.1% / +4.7% | −0.1% / −3.8% | −0.0% / +0.6% | −0.1% / +0.2% |
| scroll-list | 8.15 ms | −0.0% / +0.1% | −0.0% / −0.8% | +0.1% / +4.8% | −0.1% / −3.0% | −0.0% / +0.3% | −0.1% / +1.1% |
| chat-scroll | 8.30 ms | −0.0% / +0.9% | −0.0% / +0.2% | +0.0% / +2.3% | −0.1% / −1.8% | −0.2% / −0.6% | −0.3% / +1.0% |
| sticky-scroll | 9.88 ms | −0.0% / +1.2% | +0.1% / +3.6% | +0.0% / +0.0% | −0.1% / −4.1% | +0.1% / +0.2% | +0.0% / +0.9% |
| effect-scroll | 9.61 ms | +0.0% / +0.0% | +0.0% / +4.3% | +0.0% / +0.6% | −0.1% / −4.0% | +0.0% / −0.0% | −0.0% / +0.7% |
| animated-frame | 8.13 ms | −0.2% / +0.4% | −0.0% / +3.1% | +0.1% / +1.0% | −0.1% / −3.4% | −0.2% / −0.3% | −0.4% / +0.7% |

| Optimization | Commit | Run | Decision |
|---|---|---|---|
| Grid bounds tree with ordering replay | `04052f5` | `sc1-grid` | **Kept.** About 9% fewer instructions on the panel scenarios, nothing slower. |
| Counting draw-order sort, packed sprite keys | `c10896a` | `sc2-sort` | **Kept.** About 0.5% fewer instructions on the panel scenarios. Its three time rises over 3% (sticky-scroll, effect-scroll, animated-frame) come with unchanged instructions and are gone in the next run, so they are drift. |
| Number shaping | `9bb306d` | `sc3-numbers` | **Reverted** (`be88272`). Nothing faster: the scenarios repeat their numbers, so the line layout cache already keeps them, and number shaping saves at most a shaped line a frame (animated-frame). |
| Path tessellation cache | `04db2b2` | `sc3-paths` | **Reverted** (`20bdd1a`). Nothing faster: no scenario paints paths. |
| Glyph work once a run | `c854990` | `sc3-glyphs` | **Kept.** 0.5–0.7% fewer instructions on the panel and table scenarios, nothing slower. |

The last column is run `sc3-glyphs` against the baseline. That run still contains the two
reverted commits, whose instructions are within ±0.1% everywhere. The runs are in
`artifacts-temp/testkit/perf/sc1-grid/` to `sc3-glyphs/`; each `run.txt` names its commit.
