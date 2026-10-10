# Upstream: gpui-fast

gpux's fast mode adapts the performance and composition work of **gpui-fast**, a
performance-focused GPUI fork by Longbridge.

| | |
|---|---|
| Repository | [longbridge/gpui-fast](https://github.com/longbridge/gpui-fast) |
| Version | v0.1.6, commit `b8f0c02` ("ci: allow release dependency tests time to compile on Windows (#53)") |
| Its Zed baseline | `8848cdc` (the Zed commit gpui-fast forked from) |
| Licence | Apache-2.0, copyright Zed Industries, Inc. and Longbridge (recorded in `LICENSE-APACHE`) |

gpui-fast is a **read-only reference**. Its code is adapted into gpux's own `fast/` modules,
never vendored, built or run: nothing in this repository depends on it. Commits that adapt a
gpui-fast file name the source commit and file in their body.

## How the code is laid out

Every fast-mode mechanism lives in a `fast/` module of the crate it touches, and each file is
named after the gpui-fast file it adapts, so a later gpui-fast fix can be read beside the gpux
file it concerns. Files outside `fast/` only call into it: a field typed by a `fast/` struct, a
call written by its full path (`crate::fast::...`), or a visibility change.

## Module map

Status: **adopted** (taken with renames only), **adapted** (what changed for gpux is noted),
**excluded** (with why), or **pending** (lands with a later sprint of the fast-mode epic; the
row is completed when it does).

### `crates/gpui/src/fast/`

| gpux file | gpui-fast origin | Status |
|---|---|---|
| `mod.rs` | `crates/gpui/src/fast/mod.rs` | Adapted: the same hook discipline; modules land with their mechanism, and `fast_stat!` compiles counters only under `test-support`, `bench-support` or `diagnostics`. |
| `mode.rs` | `fast/retained.rs` (`view_retention`, `set_view_retention`), `fast/layers/mod.rs` (`enabled`, `COMPILED`) | Adapted: one per-window `FastMode` for every mechanism, with implications normalized on set; `GPUX_FAST_MODE`, `GPUX_LAYOUT_RETENTION`, `GPUX_VIEW_RETENTION`, `GPUX_SCROLL_LAYERS`, and gpui-fast's `GPUI_VIEW_RETENTION` and `GPUI_SCROLL_LAYERS` as aliases; compiled defaults off until the epic's final sprint. |
| `stats.rs` | `fast/stats.rs` (`LayoutStats`, `FramePhaseTimes`) | Adapted: `FastStats` counts every mechanism per frame and per run, separates work counters (every mode) from mechanism counters (zero with fast mode off), and keeps layer bypasses by reason; timings are left to the bench. |
| `verify.rs` | `fast/layout_key.rs` (`describe_rendered_frame`), `fast/layers/verify.rs` (`drawing_sequence`, `describe`) | Adapted: describes gpux's every scene family, screen-effect requests, layers, hitboxes in hit-test order, cursors, tooltips, tab stops, listeners, input handlers, the dispatch tree and focus path, accessed element state and the DevKit tree; compares draw orders by rank. Layer expansion lands with scroll layers. |
| `dependencies.rs` | `fast/dependencies.rs`, `fast/retained.rs` (`note_hover_read`, `hovers_unchanged`) | Adapted: hover reads first, for gpux's existing cached views (the oracle found cached views replaying stale hover styles after a wheel event moved the pointer). A view that read hovers is built again while the pointer is over the hitboxes whose hovers it read, its own and those of what it deferred, instead of being drawn twice; one that read the hover of a hitbox it does not insert (an ancestor's group) is built again every frame. Entity, global, presence, scroll and list state, and ambient-input dependencies, with nested own/subtree sets, write stamps while drawing and queries that are not updates. gpux counts an entity updated without a notify as changed for every view that read it, where gpui-fast counts it only inside a view notified since: gpux holds every frame equal to the frame drawn from scratch, which shows the update. A list bumps its version when laying it out changes what it lays out next (measured heights, scroll target). |
| `layers/mod.rs` | `fast/layers/mod.rs` (`COMPILED`) | Adapted: the compiled gate; the rest is pending (scroll layers). |
| `scenarios/` | `crates/gpui_perf/src/scenarios/`, `fast/tests/retained_bench.rs` | Adapted: 17 compact fixtures shared by the `fast_mode` bench and the lane-A attribution tests, each checking its own work counters. gpui-fast's `gpui_perf` application is excluded. |
| `tests/oracle_support.rs`, `tests/oracle.rs` | `fast/tests/oracle.rs` | Adapted: two applications in lockstep (so ids match), gpux's features in the view, every OR-2 change kind, and mutation tests proving the description sees each field. |
| `tests/switches.rs`, `tests/stats.rs` | — | gpux's own. |
| `tests/bounds_grid.rs`, `tests/draw_order.rs`, `tests/glyph_runs.rs` | the tests in `fast/bounds_tree.rs`, `fast/scene.rs`, `fast/glyphs.rs` | Adapted: each ordering, sort and glyph run compared with the work it replaces (the R-tree, the comparison sort, per-glyph painting). |
| `tests/hover_dependencies.rs`, `tests/scoped_invalidation.rs` | — | gpux's own. |
| `bounds_tree.rs` | `fast/bounds_tree.rs` | Adopted: the uniform grid with ordering replay, for all three draw-order bands; it counts replayed orderings for `FastStats`. Upstream's R-tree stays only as the tests' reference. |
| `scene.rs` | `fast/scene.rs` | Adopted: the counting draw-order sort and the packed sprite keys (order, texture, tile), called from `Scene::finish` for every primitive family. |
| `glyphs.rs` | `fast/glyphs.rs` (the per-run part) | Adapted: `LineGlyphPainter` works out a run's rendering once and quantizes each glyph exactly as `Window::paint_glyph` does; glyph bounds are cached per frame. The layer-only quantization is pending (scroll layers). |
| — | `fast/number_shaping.rs`, `fast/path_cache.rs` | Measured and reverted: no scenario was faster with them (see [Measurements](../fast-mode.md#measurements)). |
| `layout_key.rs` | `fast/layout_key.rs` | Adapted: keys are derived only in frames that begin with layout retention on; list items go through `layout_as_list_item` in `list.rs` and `uniform_list.rs`. gpui-fast's frame phase times are left to the bench. |
| `layout.rs` | `fast/layout.rs` (claims, lifecycle, write elision, measure log, settling) | Adapted: keyed claims beside gpux's ordinal pool, which stays the off path; a frame that switches retention starts from a fresh engine; measurement closures are still taken at clear. The layout fingerprint is only an inequality pre-filter: equal fingerprints are confirmed by comparing every field the Taffy conversion reads (gpui-fast only asserts that in debug builds), so a request is converted only when it changed. Text leaves keep their inputs and measure log as data, never a closure: clear takes every closure and dirties every leaf but text, and probe measurements are settled after each `compute_layout` (gpui-fast `2e2c73a`). |
| `text.rs` | `fast/text.rs` (`TextMeasureInputs`, `adopt_measurement`, `update_decoration_runs`, `carry_measurement`, `carry_over_line_layouts`) | Adapted: carry and replay around gpux's own text measurement (letter spacing, truncation); a scale-factor change measures text afresh; the line layout cache keeps held lines only under layout retention. gpui-fast's `text_style.rs` shared style stack, recent-shape cache and shaping statistics are excluded. |
| `retained.rs` | `fast/retained.rs` (records, reuse paths, invalidation, deferred attribution) | Adapted: records live in `WindowFrame` and copy through gpux's `reuse_prepaint`/`reuse_paint`, which already carry every per-frame output (window-control hitboxes added); nested records are shifted, never copied again, and a rollback truncates them. A reused layout carries its element states, retracted if the view is built after all, and gives its kept text leaves back a measuring closure. A view whose content registers a sticky candidate with a scroll container outside it is always built, so the container pins it from candidates registered this frame, instead of republishing them. A changed text style builds the view (its kept nodes are claimed again). Deferred draws carry their enclosing records, and their hitboxes count as the records' own for hovers. Reasons nothing is reused are counted per view (`views_bypassed_*`). The prebuilt path lands with splice. |
| `verify.rs` (`retention`) | — | gpux's own: the `diagnostics` verify mode (`GPUX_RETENTION_VERIFY`, `Window::set_retention_verify`, `Window::on_retention_mismatch`) draws a recording-only shadow frame from scratch before each frame that may reuse views, and reports the innermost view whose part differs. |
| — | `fast/dispatch.rs`, `fast/global_id.rs`, `fast/interactivity.rs` | Not adopted with view retention: one-pass dispatch-node copying, a per-frame global-id cache (gpux already folds path hashes incrementally) and lazily allocated listener lists are allocation work on upstream structures, considered with the final comparison. |
| `tests/retained_records.rs`, `tests/reuse_paths.rs`, `tests/view_invalidation.rs`, `tests/verify_mode.rs`, `tests/oracle_views.rs`, `tests/view_support.rs` | `fast/tests/retained.rs` | Adapted: each copied output, path, dependency kind and bypass reason checked against a window drawing from scratch. |
| `splice.rs` | `fast/splice.rs` | Adapted: gaps inherit gpux's every stack (paint content mask, transform, scroll and sticky contexts, hitbox behavior, positioned depth besides gpui-fast's), and the stretches between gaps map DevKit ids and dispatch nodes across the gaps, since gpux records an element's DevKit node after its children; a gap's own DevKit node is patched with its new hitbox count, and a gap must keep its bounds. What is copied around the gaps is checked for changed reads and hovers before splicing rather than after. Prebuilt children are taken over by non-cached views. Adds `fast/tests/splice.rs`, `own_dirty.rs` and `oracle_splice.rs`. |
| `composition.rs` | `fast/composition.rs` | Adapted: the surface tree, controller and surface starts as gpui-fast has them; gpux moves its top-layer band to the overlay, replays positioned and top layers balanced as well as clips, diffs the platform order, draws the first composed frame afresh, makes overlay text grayscale at paint, and adds the `native_surface` element and a recording test platform. Platform backends land per sprint (macOS, Wayland and X11 so far). |
| `layers/` (the rest) | `fast/layers/` | Pending (scroll layers). |
| — | `fast/windows_manifest.rs` | Excluded: gpux keeps its own Windows manifest handling. |

### Other crates

| gpux file | gpui-fast origin | Status |
|---|---|---|
| `crates/gpui_platform/benches/fast_mode.rs` | `crates/gpui_perf/src/runner.rs` | Adapted: a `#[gpux::bench]` suite, `fast/<scenario>/<mode>`, printing `gpux-fast-stats:` lines. |
| `tooling/testkit/src/fast_mode.rs` | `crates/gpui_perf/src/main.rs` (its report) | Adapted: `gpux-testkit perf fast-mode` and lane G's `fast-mode.md`. |
| `crates/gpui_wgpu/src/fast/composition.rs` | `crates/gpui_wgpu/src/fast/composition.rs` | Adapted: `new_sharing` (gpui-fast's `new_sharing_atlas`) also shares compiled pipelines and bind group layouts when the surface takes the same format, and configures overlays transparent. |
| `crates/gpui_apple/src/fast/composition.rs` | `crates/gpui_apple/src/fast/composition.rs` | Adapted: `MetalRenderer::new_overlay`, sharing the device, instance buffer pool and sprite atlas, with `opaque = NO`. |
| `crates/gpui_macos/src/fast/composition.rs` | `crates/gpui_macos/src/fast/composition.rs` | Adopted: `GPUIOverlayView` with `hitTest:` gating, layer-backed native containers, diffed re-mounting (gpui-fast `8e1e198`), transactions during live resize; overlay renderers come from `new_overlay`. |
| `crates/gpux_conformance/examples/desktop_host/composition.rs` | — | gpux's own: lane D's `composition-macos-*` checks against AppKit's view tree, hit testing and window screenshots. The macOS overlay takes input only within its input region (gpui-fast takes the whole window while the overlay draws anything). |
| `crates/gpui_linux/src/fast/composition/wayland.rs` | `crates/gpui_linux/src/fast/composition/wayland.rs` | Adapted: synchronized overlay subsurfaces drawn by `new_sharing` renderers, desynchronized native subsurfaces, `wp_viewport` sizing, overlay-then-base commits. The overlay's input region is its content's (`layer_input_region`) rather than all-or-nothing, and placements, geometry and input regions are logged under `gpux::composition` for lane D. Only GPUI's subsurfaces get a `wp_viewport` (a producer may create its own on a native surface); a lost overlay renderer releases its swapchain before the new one is made; a pointer leave and enter between a window's surfaces in one `wl_pointer.frame` is a move, not an exit. |
| `crates/gpui_linux/src/fast/composition/x11.rs` | `crates/gpui_linux/src/fast/composition/x11.rs` | Adopted: native child windows with SHAPE cut-outs, compositor detection and the override-redirect overlay window. Occluders follow gpux's baked primitive transforms; opacity reads `PreparedBackground::is_opaque`. Under a compositor only opaque quads, without their rounded corners, are cut out; the overlay's root origin is asked for only when the window is configured; a crossing between the window and its overlay window is a move, not an exit. `x11rb`'s `shape` feature is declared rather than unified in. |
| `crates/gpux_conformance/examples/desktop_host/composition_linux.rs` | — | gpux's own: lane D's `composition-wayland-*` checks (the logged requests, accepted by the compositor) and `composition-x11-*` checks (the server's window tree, geometry, shapes and hit testing). |
| `crates/gpui_windows/src/fast/composition.rs` | `crates/gpui_windows/src/fast/composition.rs` | gpux's own, modelled on it: gpui-fast moves Zed's DirectX renderer onto a composition swap chain; gpux keeps the wgpu DX12 HWND swap chain (`DXGI_SCALING_NONE`) under a gpux-owned topmost DirectComposition target (spike result below), with overlay swap chains from `SurfaceTargetUnsafe::CompositionVisual` (`WgpuRenderer::new_sharing_target`), a commit after every composed frame, and the device-lost message rebuilding the tree as well. `is_gpui_window` and the portal shape are gpui-fast's. |
| `crates/gpui_web/src/fast/composition.rs` | — | gpux's own: overlay canvases above the `html_element` DOM overlay on the shared WebGPU device (`new_sharing_with_context`), input shields clipped to each overlay's input region forwarding pointer events to the window's canvas (a crossing between the canvas and a shield is not a window exit), overlay surfaces alone configured premultiplied (`use_overlay_alpha_mode`; a transparent window's own canvas keeps the mode wgpu picks); WebGL2 reports unsupported. |
| `crates/gpux_conformance/examples/desktop_host/composition_windows.rs`, `tools-node/specs/composition.spec.ts` | — | gpux's own: lane D's `composition-windows-*` (GDI screenshots of what DWM composed, the device-lost rebuild, accelerator pass-through, live resize) and lane B's composition spec. |
| `crates/*/src/fast/layers`, other `crates/*/src/fast/composition` | the renderer and platform `fast/` modules | Pending (scroll-layer renderers, composition on mobile). |
| — | the Direct3D 11 renderer | Excluded: gpux renders Windows through wgpu on DX12. |
| — | `compat/` gpui-pre stand-ins, `script/check-upstream` | Excluded: gpux tracks Zed through its own import process. |

## Windows base surface spike

gpui-fast hosts the window's own content in a DirectComposition visual, because Zed's DirectX
renderer presents through a composition swap chain. gpux renders Windows through wgpu on DX12,
whose HWND swap chain already carries `DXGI_SCALING_NONE` (the one change of the wgpu fork). The
spike compared the two places the base content can live once a window composes:

| Option | Base content | What it would change |
|---|---|---|
| (a) | The existing wgpu HWND swap chain, below a gpux-owned topmost DirectComposition target that holds only native surfaces and overlays | Nothing for windows that never compose. |
| (b) | A composition-visual swap chain (`SurfaceTargetUnsafe::CompositionVisual`) in the same tree as the native surfaces and overlays | Every composing window moves its base onto a second kind of swap chain, and resize behaviour then rests on DirectComposition's stretch rules instead of `DXGI_SCALING_NONE`. |

`composition-windows-resize` (lane D, `desktop_host/composition_windows.rs`) grows a window by
240 px in one step with no frame drawn in between and reads what DWM composed through GDI: the
rows of a line drawn at rows 30..33 before and after, and the newly exposed rows. It measures
(a) on the composed window and (b) on a window whose content is a visual-hosted swap chain. CI
run 38050809476 (`lane D (windows-2025)`, commit `79eafd5`) reported:

```
(a) base rows Some(30)..Some(33) -> Some(30)..Some(33) after growing 240 px, newly exposed rows read [0, 0, 0];
(b) visual-hosted line rows Some(30)..Some(33) -> Some(30)..Some(33), newly exposed rows read [0, 0, 0]
```

Neither option stretched the stale frame, and both left the newly exposed rows black until the
next frame, so the resize evidence does not tell them apart. gpux keeps (a): it leaves the
renderer path, the swap chain and the `DXGI_SCALING_NONE` behaviour of every window as they were,
adds DirectComposition only above them, and needs no second base presentation path to keep in
step with wgpu. The check keeps reporting both measurements, so a later wgpu or Windows change
that separates them shows in lane D.

## Comparing a later gpui-fast fix

1. Clone gpui-fast outside the repository (`git clone https://github.com/longbridge/gpui-fast`)
   and list what changed since the represented commit:
   `git -C <clone> log --oneline b8f0c02..<new> -- crates/gpui/src/fast crates/*/src/fast`.
2. For each changed file, open the gpux file of the same name in the table above and read the two
   side by side; `git -C <clone> show 8848cdc:<path>` gives Zed's baseline of any hooked file.
3. Port the fix by hand into the gpux file, add or extend the mechanism's own test group, and run
   that group plus the mechanism's oracle group.
4. Update the represented commit here and name the gpui-fast commit and files in the commit body.
