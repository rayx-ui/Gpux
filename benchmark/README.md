# gpux rendering benchmarks

Real-window rendering benchmarks that compare gpux with fast mode **off** and **on**. Every
target runs the same scenario code: a native desktop app, an iOS app and a WASM app.

They run **locally and on demand only**. `benchmark/` is a standalone Cargo workspace, not a
member of the gpux workspace, so no CI job, `cargo` command at the repository root or
`cargo package --workspace` builds it.

The latest results and their analysis are in [RESULTS.md](RESULTS.md).

## Running

```bash
python3 benchmark/run.py --target macos
```

`--target` picks what to measure. Repeat it for several targets, or pass `all-local` for every
target this host can run:

| Target | What runs | Host and prerequisites |
|---|---|---|
| `macos` | native app, a real Metal window of 1280×800 | macOS |
| `linux` | native app, a real window (Wayland or X11) | Linux, with the gpux prerequisites (`docs/testing.md`, lane D) |
| `windows` | native app, a real DirectX 12 window | Windows, with the gpux prerequisites |
| `ios-sim` | the iOS app (`ios/`), full screen on an iOS Simulator | macOS with Xcode, XcodeGen and the `aarch64-apple-ios-sim` Rust target |
| `web-webgpu` | the WASM app in Playwright's Chromium, WebGPU | the web toolchain lane B uses: `nightly-2026-06-23` with `rust-src`, `wasm-bindgen` matching `Cargo.lock`, Node.js 18+ and `pnpm install && pnpm exec playwright install chromium` in `tools-node/` |
| `web-webgl2` | the same, forced to WebGL2 | as above |
| `android` | planned (needs an APK shell like lane E's) | |

Options:

| Option | Default | Meaning |
|---|---|---|
| `--configs` | `off,on` | which configurations to run |
| `--scenarios` | all | comma list of scenario ids or groups (`retention`, `text`, `scroll`, `input`, `animation`) |
| `--reps` | 3 | repetitions of every run |
| `--frames` / `--warmup` | 600 / 120 | measured and warm-up frames per run |
| `--device` | `iPhone 17 Pro` | the iOS Simulator to run on |
| `--name` | date and time | the results directory under `results/` |
| `--skip-build` | | reuse the last build |

Examples:

```bash
# Only the scrolling scenarios on WebGPU, five repetitions.
python3 benchmark/run.py --target web-webgpu --scenarios scroll --reps 5

# Only fast mode on, for a quick look.
python3 benchmark/run.py --target macos --configs on --reps 1
```

**While a run is going, leave the machine alone.** The windows must stay visible and focused:
gpux limits an unfocused window to about 30 frames per second, and a moved pointer becomes a
hover the scenario did not ask for. Desktop runs keep the display awake with `caffeinate`. Every
report counts the frames its window was inactive (`inactive_frames`).

Results go to `results/<name>/`. That directory holds:

- `raw/<target>/<scenario>.<config>.r<n>.json`: one report per run;
- `meta.json`: the commits, the machine and the settings;
- `report.md`: the tables, written by `report.py`. Rerun `python3 benchmark/report.py results/<name>` after editing it.

## How it works

`run.py` builds the benchmark once per target from this checkout. Fast mode is chosen per window
with `Window::set_fast_mode`, so `off` and `on` run the same build. It then runs every scenario in
both configurations. Within each repetition the configurations of a scenario run in a rotating
order, so slow drift such as thermal state spreads evenly.

One run is one process, or one page in the browser. It opens one window and builds the scenario.
`driver.rs` then drives it from `Window::on_next_frame`. Each frame, the scenario's `step` changes
what an application would change at that moment: new data, a wheel event dispatched as the
platform would, the pointer moving. The window then draws and presents the frame. A run has three
phases:

1. The window's first frame is drawn and presented (startup metrics).
2. `--warmup` frames let atlases, caches and fast mode's retained state settle.
3. `--frames` frames are measured.

### Scenarios

| Id | Group | What changes every frame |
|---|---|---|
| `dashboard-clock` | retention | 48 panel views (~2.6k elements); only the status bar's clock |
| `dashboard-one-panel` | retention | one of the visible panels gets new data |
| `dashboard-all-panels` | retention | every panel gets new data (worst case for retention) |
| `table-stream` | retention | 6 of 60 row views × 12 cells get new values |
| `text-typing` | text | one character typed into one of 150 wrapped paragraph views |
| `text-log-stream` | text | a monospace log of 80 lines gets a new line |
| `text-relabel` | text | 1,500 numeric labels all change |
| `scroll-uniform-list` | scroll | a 36 px wheel step over a `uniform_list` of 100k rows × 6 columns |
| `scroll-variable-list` | scroll | a 36 px wheel step over a `list` of 20k wrapped chat messages |
| `scroll-div-cards` | scroll | a 36 px wheel step over a plain scrolling `div` of 400 cards (~4k elements) |
| `scroll-data-table` | scroll | a 48 px wheel step over a `uniform_list` of 50k rows × 16 cells |
| `hover-grid` | input | the pointer moves to the next of 1,600 hoverable cells |
| `tree-toggle` | input | one folder of a 2.3k-node file tree (~800 rows shown) expands or collapses |
| `animated-dots` | animation | 1,000 absolutely positioned dots all move |
| `effects-cards` | animation | a progress bar animates over 240 cards with shadows, gradients, borders and opacity |

Scroll scenarios reverse direction every few thousand pixels. Rows have no hover styles, so their
frames only scroll.

### Metrics

| Metric | Source | Notes |
|---|---|---|
| CPU per frame | main thread's CPU time between frame callbacks (`CLOCK_THREAD_CPUTIME_ID`, `GetThreadTimes`); in the browser, Chrome's main-thread `TaskDuration` over the measured frames (DevTools protocol) | the cost of a frame: the scenario's step, draw, present and the platform's work around them. The primary metric. |
| `Window::draw` p50 / p95 | gpux's own per-window profiler histogram (`profiler` feature) | layout, prepaint and paint, without present |
| frame rate, missed frames | intervals between frame callbacks | paced by the display; a missed frame is an interval over 1.5× the display's |
| process CPU | `getrusage` over the measured frames, all threads, as % of one core | not available in the browser |
| memory footprint | `phys_footprint` on Apple targets (Activity Monitor's "Memory", GPU allocations included), private bytes on Windows, RSS on Linux; WASM linear memory in the browser | at the end, its peak, and its growth over the measured frames (retained state that keeps growing shows here) |
| Rust heap | a counting global allocator | allocations and bytes per frame, live and peak heap |
| startup | window opened to first frame presented; the first frame's draw | |

Caveats:

- The iOS Simulator runs on the Mac's GPU and CPU, so its numbers describe the Simulator, not a phone.
- In the browser, the CPU figure is Chrome's main-thread task time. That includes the browser's own
  work and excludes the WASM worker threads.
- GPU execution time is not measured on any target. A frame the GPU cannot finish in time shows up
  as a lower frame rate.

## Adding a scenario

1. Add a builder in `src/scenarios/` and list it in `src/scenarios/mod.rs`.
2. Add the same id to `SCENARIOS` in `report.py`.

Run one scenario by hand:

```bash
cd benchmark && cargo run --release -- --scenario hover-grid --fast on --frames 240
```
