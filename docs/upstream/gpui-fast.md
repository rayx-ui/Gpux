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
| `dependencies.rs` | `fast/dependencies.rs`, `fast/retained.rs` (`note_hover_read`, `hovers_unchanged`) | Adapted: hover reads first, for gpux's existing cached views (the oracle found cached views replaying stale hover styles after a wheel event moved the pointer). A cached view that read hovers is built again while the pointer is over its hitboxes, instead of being drawn twice. Entity, global and ambient-input dependencies are pending (view retention). |
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
| `layout.rs`, `layout_key.rs`, `text.rs` | the same files (`text_style.rs`, `layout_bounds.rs` folded in where needed) | Pending (layout retention). |
| `retained.rs`, `dispatch.rs`, `global_id.rs`, `interactivity.rs` | the same files | Pending (view retention). |
| `splice.rs` | `fast/splice.rs` | Pending (splice). |
| `composition.rs` | `fast/composition.rs` | Pending (window composition). |
| `layers/` (the rest) | `fast/layers/` | Pending (scroll layers). |
| — | `fast/windows_manifest.rs` | Excluded: gpux keeps its own Windows manifest handling. |

### Other crates

| gpux file | gpui-fast origin | Status |
|---|---|---|
| `crates/gpui_platform/benches/fast_mode.rs` | `crates/gpui_perf/src/runner.rs` | Adapted: a `#[gpux::bench]` suite, `fast/<scenario>/<mode>`, printing `gpux-fast-stats:` lines. |
| `tooling/testkit/src/fast_mode.rs` | `crates/gpui_perf/src/main.rs` (its report) | Adapted: `gpux-testkit perf fast-mode` and lane G's `fast-mode.md`. |
| `crates/*/src/fast/layers`, `crates/*/src/fast/composition` | the renderer and platform `fast/` modules | Pending (scroll-layer renderers, composition per platform). |
| — | the Direct3D 11 renderer | Excluded: gpux renders Windows through wgpu on DX12. |
| — | `compat/` gpui-pre stand-ins, `script/check-upstream` | Excluded: gpux tracks Zed through its own import process. |

## Comparing a later gpui-fast fix

1. Clone gpui-fast outside the repository (`git clone https://github.com/longbridge/gpui-fast`)
   and list what changed since the represented commit:
   `git -C <clone> log --oneline b8f0c02..<new> -- crates/gpui/src/fast crates/*/src/fast`.
2. For each changed file, open the gpux file of the same name in the table above and read the two
   side by side; `git -C <clone> show 8848cdc:<path>` gives Zed's baseline of any hooked file.
3. Port the fix by hand into the gpux file, add or extend the mechanism's own test group, and run
   that group plus the mechanism's oracle group.
4. Update the represented commit here and name the gpui-fast commit and files in the commit body.
