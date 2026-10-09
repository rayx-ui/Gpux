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
event, or an overlay appearing under a still pointer, moved the hover elsewhere. When an overlay
is found to cover a replayed view only once the frame's hitboxes are known, the window draws that
frame again before showing it.

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
| Source | `d2e1461` with the fast-mode harness (sprint 1) on top, no mechanism enabled |
| Machine | Apple Silicon Mac (`Darwin 25.6.0 arm64`), Metal headless renderer |
| Run | `artifacts-temp/testkit/perf/baseline-s01/` (`run.txt` records the commit and machine) |

The final comparison runs on the same machine:

```bash
cargo run -p gpux-testkit -- perf fast-mode --name final --compare artifacts-temp/testkit/perf/baseline-s01
```
