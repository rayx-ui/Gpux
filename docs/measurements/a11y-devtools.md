# Accessibility and devtools: before and after

What the DevKit node tree and the accessibility bypass cost gpux before the gpux-a11y-devtools
epic (PERF-1), and, once the epic has replaced both with the AccessKit tree, what the result costs
(PERF-2, added at the end of the epic). Every figure comes from `benchmark/run.py` on the machine
below; the raw reports stay in `artifacts-temp/agentx/gpux-a11y-devtools/` (git-ignored). Timing
claims about this epic come from these runs only, never from CI.

## Setup

| | |
|---|---|
| Machine | Apple M5, 24 GB, macOS 26.6.2 (arm64) |
| Browser | Playwright's Chromium 147.0.7727.15, headed, WebGPU |
| Commit | `58257fe` (origin/main after the macOS improvement loop, PR #8), in three scratch worktrees |
| Run | 2026-10-16, between 16:30 and 17:06; 120 warm-up and 600 measured frames, 3 repetitions, fast mode on |
| Clocks | held: `--hold-clocks 3` runs three busy processes beside the desktop runs |

The three builds of `58257fe`:

- **(b) as is**: the benchmark as committed. It takes `gpux` with `default-features = false`, so
  it has no `devtools` feature: no DevKit nodes.
- **(a) + devtools**: the benchmark with gpux's `devtools` feature added to its `gpux` features,
  which is what every app on gpux's default features runs today, RayX included
  (`variant-a-devtools.patch`).
- **(c) accessibility forced**: (b) with the window's accessibility flag created on
  (`AtomicBool::new(true)` in `Window::new`, `variant-c-a11y-forced.patch`), as if a screen reader
  had activated every window: the AccessKit tree is built every frame and fast mode bypasses
  itself (cached view replay, retained paint, scroll layers and kept content layout). Nothing
  reaches the macOS adapter, which only sends updates once VoiceOver activates it.

The patches are under `artifacts-temp/agentx/gpux-a11y-devtools/` and were never committed; each
variant's raw reports and `report.md` are in
`artifacts-temp/agentx/gpux-a11y-devtools/wt-<variant>/benchmark/results/before-<variant>/`.
Commands, run in each worktree in turn (a, b, c):

```bash
python3 benchmark/run.py --target macos --target web-webgpu --configs on --scenarios retention,text,scroll --reps 3 --hold-clocks 3 --name before-<variant>
python3 artifacts-temp/agentx/gpux-a11y-devtools/compare.py "(b) as is=<b results>" "(a) + devtools=<a results>" "(c) accessibility forced=<c results>"
```

Each cell is the median over the repetitions; the ratio is against (b). One repetition of (a)'s
`table-stream` on macOS timed out after 300 s and was dropped by the harness (`log-a.txt`), so
that cell is the median of two. While it hung, `ps` showed the process with 0.42 s of CPU and
not advancing, which suggests the window stopped receiving frames; the cause was not established. "Frame time" is
gpux's own `Window::draw` histogram (layout, prepaint and paint, without present);
dirty-to-present latency is the time from a frame's first invalidation to its present, which on
macOS snaps to the display's frame steps (about 4 or 8 ms at 120 Hz), so a latency that halves or
grows by half between builds (`dashboard-all-panels`, `table-stream`, the list scrolls) moved one
step, not a measured change in work; (c)'s sub-millisecond latencies on `text-typing` and the
list scrolls are not explained by these runs and are not read as a result. CPU per
frame is the main thread's CPU time (macOS) or Chrome's main-thread task time (web). `gpu_ms` is
measured on macOS only.

## Findings before the epic

- **DevKit nodes cost every default-features app CPU and allocations on every frame**, even with
  nothing inspecting it. On macOS (a) takes 17–26% more main-thread CPU than (b) on the retention
  scenarios that reuse most of the frame (`dashboard-clock` 0.34 → 0.43 ms, `dashboard-one-panel`
  0.54 → 0.64 ms), 19–22% on the scrolling scenarios that composite layers (`scroll-div-cards`,
  `scroll-data-table`) and 31% on `Window::draw` p50 for `dashboard-clock`; it allocates up to ten
  times as often (`dashboard-clock` 191 → 1,925 allocations per frame) and holds up to 7.5 MB more
  live heap (`scroll-div-cards`). In the browser the CPU overhead is up to 13% and the allocation
  counts grow the same way.
  Scenarios that rebuild most of the frame anyway (`dashboard-all-panels`, `text-relabel`,
  `text-log-stream`) barely move.
- **Accessibility turns fast mode off.** With the tree forced on, (c) costs 1.9–6.2× (b)'s CPU on
  every scenario fast mode helps (`dashboard-clock` 0.34 → 2.13 ms, `scroll-div-cards` 1.30 →
  5.49 ms on macOS; up to 2.5× in the browser) and up to 33× the allocations, because cached view
  replay, retained paint and scroll layers are bypassed and every element is laid out,
  prepainted and painted again. A screen-reader user gets the slow path today. Three of the four
  scrolling scenarios allocate fewer bytes in (c) and end with a smaller footprint only because
  without scroll layers no tiles are kept; `scroll-div-cards`, which rebuilds its 400 cards every
  frame, allocates more (1.15× on macOS, 1.14× in the browser), and `scroll-variable-list` holds
  more live heap on macOS (17.8 → 19.6 MB).
- GPU time does not depend on either: the scene is the same. The `gpu_ms` drop of (c)'s list
  scrolls is the tiles it no longer composites.

## Before: origin/main `58257fe`

### macOS native (Metal)

Reports: (b) as is: 33 runs, (a) + devtools: 32 runs, (c) accessibility forced: 33 runs

#### CPU ms per frame (main thread, mean)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 0.34 | 0.43 (1.26×) | 2.13 (6.17×) |
| `dashboard-one-panel` | 0.54 | 0.64 (1.17×) | 2.23 (4.11×) |
| `dashboard-all-panels` | 4.82 | 4.82 (1.00×) | 4.69 (0.97×) |
| `table-stream` | 0.76 | 0.80 (1.05×) | 1.67 (2.20×) |
| `text-typing` | 0.45 | 0.47 (1.03×) | 1.02 (2.26×) |
| `text-log-stream` | 0.85 | 0.87 (1.01×) | 0.86 (1.01×) |
| `text-relabel` | 6.90 | 6.73 (0.98×) | 6.74 (0.98×) |
| `scroll-uniform-list` | 0.23 | 0.26 (1.12×) | 0.51 (2.20×) |
| `scroll-variable-list` | 0.19 | 0.20 (1.02×) | 0.37 (1.93×) |
| `scroll-div-cards` | 1.30 | 1.58 (1.22×) | 5.49 (4.24×) |
| `scroll-data-table` | 0.61 | 0.72 (1.19×) | 1.38 (2.28×) |

#### GPU ms per frame (Metal, mean)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 3.69 | 3.60 (0.98×) | 3.69 (1.00×) |
| `dashboard-one-panel` | 3.69 | 3.61 (0.98×) | 3.69 (1.00×) |
| `dashboard-all-panels` | 3.63 | 3.69 (1.02×) | 3.69 (1.02×) |
| `table-stream` | 3.10 | 2.74 (0.88×) | 3.09 (1.00×) |
| `text-typing` | 1.86 | 1.70 (0.91×) | 1.60 (0.86×) |
| `text-log-stream` | 3.69 | 3.57 (0.97×) | 3.64 (0.99×) |
| `text-relabel` | 2.66 | 2.85 (1.07×) | 2.85 (1.07×) |
| `scroll-uniform-list` | 3.42 | 3.31 (0.97×) | 1.73 (0.51×) |
| `scroll-variable-list` | 3.39 | 3.21 (0.95×) | 1.54 (0.46×) |
| `scroll-div-cards` | 3.43 | 3.23 (0.94×) | 3.16 (0.92×) |
| `scroll-data-table` | 3.53 | 3.45 (0.98×) | 2.68 (0.76×) |

#### Frame time: `Window::draw` p50 (ms)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 0.291 | 0.381 (1.31×) | 1.973 (6.78×) |
| `dashboard-one-panel` | 0.476 | 0.578 (1.22×) | 2.068 (4.35×) |
| `dashboard-all-panels` | 4.620 | 4.628 (1.00×) | 4.502 (0.97×) |
| `table-stream` | 0.706 | 0.742 (1.05×) | 1.564 (2.21×) |
| `text-typing` | 0.378 | 0.400 (1.06×) | 0.953 (2.52×) |
| `text-log-stream` | 0.790 | 0.796 (1.01×) | 0.792 (1.00×) |
| `text-relabel` | 6.693 | 6.554 (0.98×) | 6.533 (0.98×) |
| `scroll-uniform-list` | 0.136 | 0.163 (1.20×) | 0.433 (3.19×) |
| `scroll-variable-list` | 0.095 | 0.100 (1.05×) | 0.313 (3.28×) |
| `scroll-div-cards` | 0.861 | 1.144 (1.33×) | 5.165 (6.00×) |
| `scroll-data-table` | 0.426 | 0.529 (1.24×) | 1.286 (3.02×) |

#### Frame time: `Window::draw` p95 (ms)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 0.345 | 0.448 (1.30×) | 2.109 (6.12×) |
| `dashboard-one-panel` | 0.583 | 0.712 (1.22×) | 2.208 (3.79×) |
| `dashboard-all-panels` | 4.895 | 4.985 (1.02×) | 4.669 (0.95×) |
| `table-stream` | 0.806 | 0.849 (1.05×) | 1.666 (2.07×) |
| `text-typing` | 0.454 | 0.514 (1.13×) | 1.056 (2.32×) |
| `text-log-stream` | 0.860 | 0.902 (1.05×) | 0.865 (1.01×) |
| `text-relabel` | 7.000 | 6.836 (0.98×) | 6.902 (0.99×) |
| `scroll-uniform-list` | 0.210 | 0.224 (1.07×) | 0.517 (2.47×) |
| `scroll-variable-list` | 0.214 | 0.210 (0.98×) | 0.420 (1.97×) |
| `scroll-div-cards` | 1.008 | 1.234 (1.22×) | 5.362 (5.32×) |
| `scroll-data-table` | 0.519 | 0.699 (1.35×) | 1.397 (2.69×) |

#### Dirty-to-present latency p50 (ms)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 4.07 | 4.12 (1.01×) | 4.04 (0.99×) |
| `dashboard-one-panel` | 4.03 | 4.11 (1.02×) | 4.04 (1.00×) |
| `dashboard-all-panels` | 8.22 | 4.70 (0.57×) | 4.58 (0.56×) |
| `table-stream` | 4.03 | 6.22 (1.54×) | 4.03 (1.00×) |
| `text-typing` | 4.03 | 4.09 (1.02×) | 1.01 (0.25×) |
| `text-log-stream` | 4.04 | 4.10 (1.01×) | 4.03 (1.00×) |
| `text-relabel` | 6.84 | 6.84 (1.00×) | 6.60 (0.97×) |
| `scroll-uniform-list` | 8.32 | 4.17 (0.50×) | 0.65 (0.08×) |
| `scroll-variable-list` | 4.11 | 4.15 (1.01×) | 0.35 (0.08×) |
| `scroll-div-cards` | 8.10 | 8.09 (1.00×) | 5.28 (0.65×) |
| `scroll-data-table` | 8.33 | 4.20 (0.50×) | 4.03 (0.48×) |

#### Dirty-to-present latency p95 (ms)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 4.49 | 4.46 (1.00×) | 4.43 (0.99×) |
| `dashboard-one-panel` | 4.59 | 4.46 (0.97×) | 4.43 (0.97×) |
| `dashboard-all-panels` | 8.57 | 5.07 (0.59×) | 4.79 (0.56×) |
| `table-stream` | 4.41 | 6.57 (1.49×) | 4.43 (1.00×) |
| `text-typing` | 4.42 | 4.47 (1.01×) | 1.10 (0.25×) |
| `text-log-stream` | 4.45 | 4.43 (0.99×) | 4.40 (0.99×) |
| `text-relabel` | 7.18 | 7.07 (0.98×) | 6.99 (0.97×) |
| `scroll-uniform-list` | 8.68 | 4.55 (0.53×) | 4.37 (0.50×) |
| `scroll-variable-list` | 4.51 | 4.56 (1.01×) | 0.46 (0.10×) |
| `scroll-div-cards` | 8.46 | 8.45 (1.00×) | 5.49 (0.65×) |
| `scroll-data-table` | 8.70 | 4.60 (0.53×) | 4.45 (0.51×) |

#### Allocations per frame

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 191 | 1925 (10.08×) | 6228 (32.60×) |
| `dashboard-one-panel` | 628 | 2362 (3.76×) | 6491 (10.34×) |
| `dashboard-all-panels` | 17563 | 19297 (1.10×) | 16231 (0.92×) |
| `table-stream` | 1687 | 3273 (1.94×) | 6941 (4.11×) |
| `text-typing` | 470 | 770 (1.64×) | 7222 (15.36×) |
| `text-log-stream` | 3623 | 3943 (1.09×) | 3623 (1.00×) |
| `text-relabel` | 20556 | 23556 (1.15×) | 20556 (1.00×) |
| `scroll-uniform-list` | 697 | 1228 (1.76×) | 1112 (1.59×) |
| `scroll-variable-list` | 407 | 494 (1.22×) | 333 (0.82×) |
| `scroll-div-cards` | 5864 | 11464 (1.96×) | 12144 (2.07×) |
| `scroll-data-table` | 1600 | 3348 (2.09×) | 3579 (2.24×) |

#### Allocated KB per frame

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 376.0 | 384.4 (1.02×) | 2093.3 (5.57×) |
| `dashboard-one-panel` | 1304.0 | 1312.4 (1.01×) | 2270.0 (1.74×) |
| `dashboard-all-panels` | 7855.0 | 7863.5 (1.00×) | 7371.4 (0.94×) |
| `table-stream` | 529.5 | 537.9 (1.02×) | 1394.5 (2.63×) |
| `text-typing` | 710.9 | 781.3 (1.10×) | 1348.4 (1.90×) |
| `text-log-stream` | 2578.5 | 2586.4 (1.00×) | 2560.9 (0.99×) |
| `text-relabel` | 3776.0 | 3789.0 (1.00×) | 3654.3 (0.97×) |
| `scroll-uniform-list` | 677.3 | 680.5 (1.00×) | 347.2 (0.51×) |
| `scroll-variable-list` | 815.0 | 820.2 (1.01×) | 123.5 (0.15×) |
| `scroll-div-cards` | 3879.6 | 3968.5 (1.02×) | 4480.1 (1.15×) |
| `scroll-data-table` | 2324.7 | 2334.0 (1.00×) | 1076.3 (0.46×) |

#### Live Rust heap at the end (MB)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 24.8 | 28.5 (1.15×) | 24.7 (1.00×) |
| `dashboard-one-panel` | 24.8 | 28.5 (1.15×) | 24.7 (1.00×) |
| `dashboard-all-panels` | 25.0 | 28.6 (1.14×) | 24.9 (1.00×) |
| `table-stream` | 21.9 | 23.8 (1.08×) | 21.8 (1.00×) |
| `text-typing` | 16.9 | 17.5 (1.03×) | 16.9 (1.00×) |
| `text-log-stream` | 16.0 | 16.6 (1.03×) | 16.0 (1.00×) |
| `text-relabel` | 34.3 | 38.0 (1.11×) | 34.2 (1.00×) |
| `scroll-uniform-list` | 15.3 | 16.2 (1.06×) | 13.9 (0.91×) |
| `scroll-variable-list` | 17.8 | 18.1 (1.01×) | 19.6 (1.10×) |
| `scroll-div-cards` | 42.7 | 50.2 (1.17×) | 39.0 (0.91×) |
| `scroll-data-table` | 30.7 | 32.4 (1.06×) | 24.3 (0.79×) |

#### Memory footprint at the end (MB)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 121.8 | 124.8 (1.02×) | 121.8 (1.00×) |
| `dashboard-one-panel` | 122.9 | 125.0 (1.02×) | 122.4 (1.00×) |
| `dashboard-all-panels` | 122.5 | 126.6 (1.03×) | 122.3 (1.00×) |
| `table-stream` | 120.8 | 120.8 (1.00×) | 121.0 (1.00×) |
| `text-typing` | 115.6 | 115.6 (1.00×) | 115.5 (1.00×) |
| `text-log-stream` | 114.8 | 113.0 (0.98×) | 114.5 (1.00×) |
| `text-relabel` | 130.8 | 135.2 (1.03×) | 131.1 (1.00×) |
| `scroll-uniform-list` | 144.2 | 144.7 (1.00×) | 112.0 (0.78×) |
| `scroll-variable-list` | 150.7 | 151.1 (1.00×) | 120.0 (0.80×) |
| `scroll-div-cards` | 185.8 | 191.5 (1.03×) | 139.6 (0.75×) |
| `scroll-data-table` | 165.8 | 163.5 (0.99×) | 124.6 (0.75×) |

### WASM in Chromium, WebGPU

Reports: (b) as is: 33 runs, (a) + devtools: 33 runs, (c) accessibility forced: 33 runs

#### CPU ms per frame (main thread, mean)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 1.99 | 2.09 (1.05×) | 3.63 (1.83×) |
| `dashboard-one-panel` | 2.19 | 2.32 (1.06×) | 3.69 (1.69×) |
| `dashboard-all-panels` | 5.42 | 5.50 (1.02×) | 5.47 (1.01×) |
| `table-stream` | 2.24 | 2.32 (1.03×) | 3.40 (1.52×) |
| `text-typing` | 2.02 | 2.10 (1.04×) | 2.82 (1.40×) |
| `text-log-stream` | 2.38 | 2.39 (1.00×) | 2.39 (1.00×) |
| `text-relabel` | 7.10 | 7.25 (1.02×) | 7.11 (1.00×) |
| `scroll-uniform-list` | 1.78 | 1.78 (1.00×) | 2.04 (1.15×) |
| `scroll-variable-list` | 1.73 | 1.74 (1.01×) | 2.00 (1.15×) |
| `scroll-div-cards` | 2.74 | 3.09 (1.13×) | 6.86 (2.51×) |
| `scroll-data-table` | 2.19 | 2.33 (1.06×) | 3.08 (1.40×) |

#### Frame time: `Window::draw` p50 (ms)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 1.465 | 1.616 (1.10×) | 3.281 (2.24×) |
| `dashboard-one-panel` | 1.821 | 1.975 (1.08×) | 3.336 (1.83×) |
| `dashboard-all-panels` | 5.059 | 5.161 (1.02×) | 5.038 (1.00×) |
| `table-stream` | 1.830 | 1.925 (1.05×) | 2.966 (1.62×) |
| `text-typing` | 1.555 | 1.651 (1.06×) | 2.466 (1.59×) |
| `text-log-stream` | 2.055 | 2.065 (1.00×) | 2.055 (1.00×) |
| `text-relabel` | 6.738 | 6.869 (1.02×) | 6.742 (1.00×) |
| `scroll-uniform-list` | 1.080 | 1.120 (1.04×) | 1.561 (1.44×) |
| `scroll-variable-list` | 0.860 | 0.930 (1.08×) | 1.415 (1.65×) |
| `scroll-div-cards` | 1.840 | 2.220 (1.21×) | 6.373 (3.46×) |
| `scroll-data-table` | 1.505 | 1.656 (1.10×) | 2.666 (1.77×) |

#### Frame time: `Window::draw` p95 (ms)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 2.081 | 2.386 (1.15×) | 3.516 (1.69×) |
| `dashboard-one-panel` | 2.015 | 2.177 (1.08×) | 3.572 (1.77×) |
| `dashboard-all-panels` | 5.284 | 5.341 (1.01×) | 5.353 (1.01×) |
| `table-stream` | 2.195 | 2.265 (1.03×) | 3.250 (1.48×) |
| `text-typing` | 1.861 | 2.036 (1.09×) | 2.701 (1.45×) |
| `text-log-stream` | 2.241 | 2.310 (1.03×) | 2.271 (1.01×) |
| `text-relabel` | 7.016 | 7.135 (1.02×) | 6.967 (0.99×) |
| `scroll-uniform-list` | 1.561 | 1.616 (1.04×) | 1.986 (1.27×) |
| `scroll-variable-list` | 1.611 | 1.620 (1.01×) | 2.161 (1.34×) |
| `scroll-div-cards` | 2.136 | 2.517 (1.18×) | 6.656 (3.12×) |
| `scroll-data-table` | 1.731 | 1.901 (1.10×) | 2.982 (1.72×) |

#### Dirty-to-present latency p50 (ms)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 1.65 | 1.79 (1.09×) | 3.39 (2.06×) |
| `dashboard-one-panel` | 1.98 | 2.14 (1.08×) | 3.44 (1.74×) |
| `dashboard-all-panels` | 5.18 | 5.28 (1.02×) | 5.18 (1.00×) |
| `table-stream` | 2.00 | 2.08 (1.04×) | 3.12 (1.56×) |
| `text-typing` | 1.72 | 1.82 (1.06×) | 2.61 (1.52×) |
| `text-log-stream` | 2.19 | 2.20 (1.00×) | 2.20 (1.00×) |
| `text-relabel` | 6.86 | 7.00 (1.02×) | 6.87 (1.00×) |
| `scroll-uniform-list` | 1.42 | 1.44 (1.01×) | 1.74 (1.23×) |
| `scroll-variable-list` | 1.28 | 1.32 (1.03×) | 1.63 (1.27×) |
| `scroll-div-cards` | 2.05 | 2.41 (1.18×) | 6.48 (3.16×) |
| `scroll-data-table` | 1.92 | 2.04 (1.07×) | 2.82 (1.47×) |

#### Dirty-to-present latency p95 (ms)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 2.34 | 2.61 (1.11×) | 3.65 (1.56×) |
| `dashboard-one-panel` | 2.20 | 2.37 (1.07×) | 3.71 (1.69×) |
| `dashboard-all-panels` | 5.43 | 5.49 (1.01×) | 5.53 (1.02×) |
| `table-stream` | 2.42 | 2.46 (1.02×) | 3.47 (1.44×) |
| `text-typing` | 2.15 | 2.28 (1.06×) | 2.87 (1.33×) |
| `text-log-stream` | 2.42 | 2.48 (1.02×) | 2.46 (1.02×) |
| `text-relabel` | 7.16 | 7.30 (1.02×) | 7.16 (1.00×) |
| `scroll-uniform-list` | 2.07 | 2.08 (1.00×) | 2.22 (1.07×) |
| `scroll-variable-list` | 2.20 | 2.24 (1.02×) | 2.40 (1.09×) |
| `scroll-div-cards` | 2.69 | 3.03 (1.13×) | 6.77 (2.51×) |
| `scroll-data-table` | 2.28 | 2.47 (1.08×) | 3.18 (1.39×) |

#### Allocations per frame

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 231 | 1965 (8.51×) | 6268 (27.13×) |
| `dashboard-one-panel` | 859 | 2593 (3.02×) | 6722 (7.83×) |
| `dashboard-all-panels` | 26708 | 28442 (1.06×) | 25377 (0.95×) |
| `table-stream` | 3215 | 4801 (1.49×) | 8469 (2.63×) |
| `text-typing` | 723 | 1023 (1.42×) | 7475 (10.34×) |
| `text-log-stream` | 3713 | 4033 (1.09×) | 3713 (1.00×) |
| `text-relabel` | 46513 | 49513 (1.06×) | 46513 (1.00×) |
| `scroll-uniform-list` | 855 | 1384 (1.62×) | 1238 (1.45×) |
| `scroll-variable-list` | 509 | 597 (1.17×) | 412 (0.81×) |
| `scroll-div-cards` | 5892 | 11492 (1.95×) | 12153 (2.06×) |
| `scroll-data-table` | 2399 | 4145 (1.73×) | 4338 (1.81×) |

#### Allocated KB per frame

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 362.6 | 371.0 (1.02×) | 1716.9 (4.73×) |
| `dashboard-one-panel` | 1308.0 | 1316.4 (1.01×) | 1919.8 (1.47×) |
| `dashboard-all-panels` | 8740.9 | 8749.4 (1.00×) | 8315.5 (0.95×) |
| `table-stream` | 708.6 | 717.0 (1.01×) | 1359.5 (1.92×) |
| `text-typing` | 961.2 | 1031.6 (1.07×) | 1427.8 (1.49×) |
| `text-log-stream` | 2548.5 | 2556.4 (1.00×) | 2531.5 (0.99×) |
| `text-relabel` | 7072.7 | 7085.8 (1.00×) | 6951.7 (0.98×) |
| `scroll-uniform-list` | 590.6 | 593.7 (1.01×) | 307.6 (0.52×) |
| `scroll-variable-list` | 659.7 | 664.9 (1.01×) | 150.4 (0.23×) |
| `scroll-div-cards` | 3232.2 | 3321.1 (1.03×) | 3676.7 (1.14×) |
| `scroll-data-table` | 2150.2 | 2159.5 (1.00×) | 1020.1 (0.47×) |

#### Live Rust heap at the end (MB)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 41.5 | 43.5 (1.05×) | 41.5 (1.00×) |
| `dashboard-one-panel` | 41.5 | 43.5 (1.05×) | 41.5 (1.00×) |
| `dashboard-all-panels` | 41.8 | 43.8 (1.05×) | 43.9 (1.05×) |
| `table-stream` | 39.4 | 40.4 (1.03×) | 39.4 (1.00×) |
| `text-typing` | 36.2 | 36.5 (1.01×) | 36.2 (1.00×) |
| `text-log-stream` | 33.4 | 33.7 (1.01×) | 33.4 (1.00×) |
| `text-relabel` | 51.2 | 53.2 (1.04×) | 51.2 (1.00×) |
| `scroll-uniform-list` | 33.5 | 34.0 (1.01×) | 33.2 (0.99×) |
| `scroll-variable-list` | 36.0 | 36.2 (1.00×) | 36.2 (1.00×) |
| `scroll-div-cards` | 57.7 | 61.7 (1.07×) | 54.4 (0.94×) |
| `scroll-data-table` | 46.2 | 47.2 (1.02×) | 42.7 (0.93×) |

#### Memory footprint at the end (MB)

| Scenario | (b) as is | (a) + devtools | (c) accessibility forced |
|---|---:|---:|---:|
| `dashboard-clock` | 49.4 | 51.2 (1.04×) | 49.0 (0.99×) |
| `dashboard-one-panel` | 49.5 | 51.6 (1.04×) | 49.0 (0.99×) |
| `dashboard-all-panels` | 49.6 | 51.6 (1.04×) | 53.1 (1.07×) |
| `table-stream` | 46.6 | 47.7 (1.02×) | 46.6 (1.00×) |
| `text-typing` | 42.8 | 43.1 (1.01×) | 42.8 (1.00×) |
| `text-log-stream` | 39.7 | 40.1 (1.01×) | 39.7 (1.00×) |
| `text-relabel` | 62.1 | 63.8 (1.03×) | 62.1 (1.00×) |
| `scroll-uniform-list` | 41.6 | 42.9 (1.03×) | 40.4 (0.97×) |
| `scroll-variable-list` | 46.9 | 47.0 (1.00×) | 42.2 (0.90×) |
| `scroll-div-cards` | 75.9 | 79.8 (1.05×) | 64.1 (0.84×) |
| `scroll-data-table` | 63.2 | 61.3 (0.97×) | 51.1 (0.81×) |

