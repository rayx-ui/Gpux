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
hitboxes: only there can hitboxes the frame adds later (an overlay) change its hovers.

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
