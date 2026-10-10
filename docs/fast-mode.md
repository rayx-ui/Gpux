# Fast mode

Fast mode is gpux's set of incremental-rendering mechanisms, adapted from
[gpui-fast](upstream/gpui-fast.md): layout retention, view retention, splice, scroll layers, and
window composition. Each can be switched per window at runtime. With every switch off a window
draws exactly as gpux draws without fast mode.

> Fast mode is landing mechanism by mechanism. Every switch defaults **off** until the last
> mechanism has its proof; this page grows with each one.

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

Splice has no variable, as in gpui-fast; set it through `Window::set_fast_mode`.

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
to GPUI, which leaves no shadow blending over native views). The Android side is gpux's Java
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

The final comparison runs on the same machine:

```bash
cargo run -p gpux-testkit -- perf fast-mode --name final --compare artifacts-temp/testkit/perf/baseline-s01
```

## Measurements

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
