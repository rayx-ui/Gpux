# Fast mode

Fast mode is gpux's adaptation of gpui-fast: layout retention, view retention, splice, scroll
layers and opt-in window composition. [docs/fast-mode.md](../../../../docs/fast-mode.md) is the
reference (switches, mechanisms, measurements); [docs/upstream/gpui-fast.md](../../../../docs/upstream/gpui-fast.md)
maps every gpux file to the gpui-fast file it adapts and says how to compare a later gpui-fast fix.

## Where it lives

- Mechanisms are `fast/` modules of the crate they touch (`crates/gpui/src/fast/`,
  `crates/gpui_wgpu/src/fast/`, `crates/gpui_apple/src/fast/`, the platform crates' `fast/composition`).
  Files outside `fast/` only call in. Keep Zed's crate boundaries; add new code under `fast/`.
- `fast/mode.rs` holds `FastMode`, the compiled defaults (all on), the `GPUX_*` variables and
  gpui-fast's `GPUI_*` aliases, normalization (view retention needs layout retention; splice and
  scroll layers need view retention; scroll layers need a renderer that rasterizes tiles).
- Behaviour tied to scroll layers (snapping of offsets, glyph origins, paths) is gated by the
  window's `fast_mode.snapping()`, so `GPUX_FAST_MODE=0` restores stock output.

## Working on it

- A change to a mechanism extends that mechanism's own test group in `crates/gpui/src/fast/tests/`
  (`cargo test -p gpux --features test-support --lib fast::tests::<group>::`), plus the oracle group
  that compares incremental frames with frames drawn from scratch (`oracle*.rs`).
- The defaults are on, so a test of stock behaviour (upstream's cached views, a frame that paints
  everything) opens its window with `window.set_fast_mode(FastMode::OFF)`; tests of one mechanism
  open theirs in the mode they are about.
- Parity: `lane_a_fast_parity` (every catalog scene on and off), `lane_c_fast_parity` (byte-equal
  pixels on and off), the scroll-layer lane C/B/E/F groups in [docs/testing.md](../../../../docs/testing.md).
- Counters: `cargo test -p gpux --features test-support,bench-support --test fast_mode_attribution`.
- Timing is lane G only: `cargo run -p gpux-testkit -- perf fast-mode --name <run> [--compare <earlier>]`.
- A frame that differs from one drawn from scratch is a bug whatever mode: fix the mechanism, never
  the oracle. Retained scenes replay what was painted, so anything that changes how a primitive is
  produced without invalidating a view (text rendering mode, background appearance) has to make the
  window draw from scratch (`Window::refresh_for_text_policy` is the hook for text).
- The three rules an application follows with view retention on are in [the application contract](../../../../docs/fast-mode.md#the-application-contract): state outside entities needs `cx.notify()`, notify and write globals only on change, scroll through `ScrollHandle`/`ListState`.
