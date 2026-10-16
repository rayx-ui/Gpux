# Accessibility and devtools: before and after

What the DevKit node tree and the accessibility bypass cost gpux before the gpux-a11y-devtools
epic (PERF-1), and, once the epic has replaced both with the AccessKit tree, what the result costs
(PERF-2, [after the epic](#after-the-epic-perf-2)). Every figure comes from `benchmark/run.py` on the machine
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


## After: the epic (PERF-2)

Measured on 2026-10-17 between 00:30 and 01:10 on the same machine, browser and settings as the
before runs (120 warm-up and 600 measured frames, fast mode on, `--hold-clocks 3`), on the
epic's tree at `2c049b8` (the s09 checkpoint) with the benchmark's new `--accessibility` switch
(`GPUX_ACCESSIBILITY` on the desktop, `?accessibility=` in the browser) and the tree's node count
in each report (`a11y_nodes`). The benchmark still takes gpux with `default-features = false`;
since the epic there is no `devtools` feature to add, so (a) has no counterpart: every app now
runs what (b) ran.

- **(a′) epic**: no assistive technology and no inspection: nothing asks for the tree, none is
  built.
- **(c′) epic, always**: `GPUX_ACCESSIBILITY=always`, the tree built every frame with fast mode
  on (the before (c) had fast mode bypass itself).
- **(b) 58257fe, same session**: the before (b) binary run again, alternated with (a′) in the
  same session, because the machine ran the same (b) binary 1.05–1.42× slower on the CPU than
  the day before (table below), more than the differences being judged.

Commands, each round with `--reps 1` so the variants alternate (rounds b, a′ in
`run-control.sh`; a′, c′ in `run-after.sh`, three rounds each, one build reused with
`--skip-build`):

```bash
python3 benchmark/run.py --target macos --target web-webgpu --configs on --scenarios retention,text,scroll --reps 1 --hold-clocks 3 --skip-build --name after-a-r<n>
python3 benchmark/run.py --target macos --target web-webgpu --configs on --scenarios retention,text,scroll --reps 1 --hold-clocks 3 --skip-build --accessibility always --name after-c-r<n>
```

Raw reports and the merged repetitions are in `artifacts-temp/agentx/gpux-a11y-devtools/after/`.
macOS `dashboard-clock` timed out once in each of the (a′) and (c′) series (those cells are the
median of two): the window received no frames. A process sampled while hung
(`artifacts-temp/agentx/gpux-a11y-devtools/hang/sample-5.txt`, 1 of 12 back-to-back launches)
has its main thread idle in the AppKit run loop and no display-link thread, so nothing asked it
to draw. The before (a) build of `58257fe` hung the same way on `table-stream`, so this predates
the epic; it fits gpui_macos pausing the display link of a window reported occluded, which these
runs did not establish.

### Findings after the epic

- **(a′) costs what (b) costs.** Run in the same session, (a′) is within the run-to-run spread
  of (b) on every scenario: main-thread CPU 0.79–1.40× on macOS and 0.63–1.02× in the browser,
  with the per-repetition ranges overlapping on every scenario (`scroll-variable-list`, the
  widest, ran 0.24–0.36 ms for (b) and 0.20–0.40 ms for (a′)), and the allocation counts equal to
  one or two per frame (`dashboard-clock` 191 → 189). The DevKit node tree is gone, so the 17–26%
  CPU and up to ten-times allocation overhead that (a) carried for every default-features app,
  RayX included, is gone with it. No regression of (a′) against (b) is above the noise.
- **Building the tree no longer turns fast mode off.** With the tree built every frame, (c′)
  costs 0.96–1.34× (a′)'s CPU on macOS (`dashboard-clock` 0.55 → 0.74 ms, `scroll-div-cards`
  1.72 → 2.20 ms), where the before (c) cost 1.9–6.2× (b) (`dashboard-clock` 6.2×,
  `scroll-div-cards` 4.2×). Cached view replay, retained paint and scroll layers stay on and
  carry the tree.
- **What (c′) still costs is building the tree's nodes.** (c′) allocates 1.3–33× (a′)'s count per
  frame on macOS (`dashboard-clock` 189 → 6,284, `scroll-div-cards` 5,863 → 25,443) and 1.1–2.9×
  its bytes,
  because each frame builds an AccessKit node for every node of the tree (868 on the dashboards,
  2,801 on `scroll-div-cards`) even where fast mode carried the element itself. The retained
  tree is small: the live heap at the end grows by at most 2.3 MB.
- In the browser (c′) costs 0.61–1.48× (a′)'s CPU, against 1.0–2.5× for the before (c). The
  two list scrolls are cheaper with the tree on in these runs (0.61× and 0.63×), which these
  measurements do not explain and are not read as a result; `table-stream` (1.48×),
  `text-typing` (1.45×) and `scroll-div-cards` (1.39×) pay most for building their nodes.
- GPU time is the same for (a′) and (c′) on every scenario (the scene does not change).

### The tree with inspection on

The node count is the tree the last frame built under `GPUX_ACCESSIBILITY=always`; the retained
size is the end-of-run live Rust heap of (c′) less (a′)'s (medians), which includes the frame's
records of the tree.

| Scenario | macOS nodes | macOS live heap (c′ − a′, MB) | web nodes | web live heap (c′ − a′, MB) |
|---|---:|---:|---:|---:|
| `dashboard-clock` | 868 | +0.7 | 868 | +0.6 |
| `dashboard-one-panel` | 868 | +0.7 | 868 | +0.6 |
| `dashboard-all-panels` | 868 | +0.4 | 868 | +0.6 |
| `table-stream` | 794 | +0.6 | 794 | +0.6 |
| `text-typing` | 151 | +0.2 | 151 | +0.3 |
| `text-log-stream` | 161 | +0.1 | 161 | +0.2 |
| `text-relabel` | 1501 | +1.1 | 1501 | +1.1 |
| `scroll-uniform-list` | 181 | +0.3 | 175 | +0.3 |
| `scroll-variable-list` | 34 | +0.1 | 34 | +0.1 |
| `scroll-div-cards` | 2801 | +2.3 | 2801 | +2.3 |
| `scroll-data-table` | 593 | +0.6 | 593 | +0.6 |

### Session drift: (b) on two days

##### CPU ms per frame (main thread, mean), macOS

| Scenario | (b) 2026-10-16 | (b) 2026-10-17 |
|---|---:|---:|
| `dashboard-clock` | 0.34 | 0.42 (1.23×) |
| `dashboard-one-panel` | 0.54 | 0.74 (1.36×) |
| `dashboard-all-panels` | 4.82 | 5.36 (1.11×) |
| `table-stream` | 0.76 | 0.83 (1.10×) |
| `text-typing` | 0.45 | 0.62 (1.38×) |
| `text-log-stream` | 0.85 | 1.04 (1.22×) |
| `text-relabel` | 6.90 | 7.21 (1.05×) |
| `scroll-uniform-list` | 0.23 | 0.32 (1.42×) |
| `scroll-variable-list` | 0.19 | 0.25 (1.29×) |
| `scroll-div-cards` | 1.30 | 1.43 (1.10×) |
| `scroll-data-table` | 0.61 | 0.74 (1.21×) |

### Same session: (b) and (a′)

#### macOS native (Metal)

Reports: (b) 58257fe: 33 runs, (a') epic: 33 runs

##### CPU ms per frame (main thread, mean)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 0.42 | 0.43 (1.01×) |
| `dashboard-one-panel` | 0.74 | 0.59 (0.79×) |
| `dashboard-all-panels` | 5.36 | 4.96 (0.92×) |
| `table-stream` | 0.83 | 0.98 (1.18×) |
| `text-typing` | 0.62 | 0.54 (0.87×) |
| `text-log-stream` | 1.04 | 1.10 (1.06×) |
| `text-relabel` | 7.21 | 7.06 (0.98×) |
| `scroll-uniform-list` | 0.32 | 0.31 (0.94×) |
| `scroll-variable-list` | 0.25 | 0.35 (1.40×) |
| `scroll-div-cards` | 1.43 | 1.52 (1.06×) |
| `scroll-data-table` | 0.74 | 0.84 (1.14×) |

##### GPU ms per frame (Metal, mean)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 3.07 | 3.07 (1.00×) |
| `dashboard-one-panel` | 3.08 | 3.09 (1.00×) |
| `dashboard-all-panels` | 3.08 | 3.08 (1.00×) |
| `table-stream` | 2.59 | 2.59 (1.00×) |
| `text-typing` | 2.14 | 2.14 (1.00×) |
| `text-log-stream` | 3.06 | 3.06 (1.00×) |
| `text-relabel` | 2.62 | 2.62 (1.00×) |
| `scroll-uniform-list` | 2.87 | 2.87 (1.00×) |
| `scroll-variable-list` | 2.80 | 2.80 (1.00×) |
| `scroll-div-cards` | 2.82 | 2.82 (1.00×) |
| `scroll-data-table` | 2.93 | 2.93 (1.00×) |

##### Frame time: `Window::draw` p50 (ms)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 0.349 | 0.351 (1.01×) |
| `dashboard-one-panel` | 0.645 | 0.511 (0.79×) |
| `dashboard-all-panels` | 4.940 | 4.764 (0.96×) |
| `table-stream` | 0.745 | 0.826 (1.11×) |
| `text-typing` | 0.487 | 0.424 (0.87×) |
| `text-log-stream` | 0.936 | 0.933 (1.00×) |
| `text-relabel` | 7.008 | 6.885 (0.98×) |
| `scroll-uniform-list` | 0.200 | 0.190 (0.95×) |
| `scroll-variable-list` | 0.127 | 0.199 (1.57×) |
| `scroll-div-cards` | 0.930 | 1.035 (1.11×) |
| `scroll-data-table` | 0.492 | 0.597 (1.21×) |

##### Frame time: `Window::draw` p95 (ms)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 0.430 | 0.484 (1.13×) |
| `dashboard-one-panel` | 0.802 | 0.650 (0.81×) |
| `dashboard-all-panels` | 5.743 | 5.120 (0.89×) |
| `table-stream` | 1.077 | 1.533 (1.42×) |
| `text-typing` | 1.021 | 1.018 (1.00×) |
| `text-log-stream` | 1.105 | 1.867 (1.69×) |
| `text-relabel` | 7.455 | 7.188 (0.96×) |
| `scroll-uniform-list` | 0.329 | 0.345 (1.05×) |
| `scroll-variable-list` | 0.326 | 0.364 (1.12×) |
| `scroll-div-cards` | 1.157 | 1.113 (0.96×) |
| `scroll-data-table` | 1.111 | 0.810 (0.73×) |

##### Dirty-to-present latency p50 (ms)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 8.32 | 8.31 (1.00×) |
| `dashboard-one-panel` | 8.31 | 8.32 (1.00×) |
| `dashboard-all-panels` | 8.19 | 8.21 (1.00×) |
| `table-stream` | 8.29 | 4.11 (0.50×) |
| `text-typing` | 4.20 | 4.11 (0.98×) |
| `text-log-stream` | 8.29 | 4.11 (0.50×) |
| `text-relabel` | 8.22 | 8.23 (1.00×) |
| `scroll-uniform-list` | 8.31 | 8.30 (1.00×) |
| `scroll-variable-list` | 4.15 | 8.29 (2.00×) |
| `scroll-div-cards` | 8.03 | 8.05 (1.00×) |
| `scroll-data-table` | 4.22 | 8.30 (1.97×) |

##### Dirty-to-present latency p95 (ms)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 8.38 | 8.37 (1.00×) |
| `dashboard-one-panel` | 8.36 | 8.39 (1.00×) |
| `dashboard-all-panels` | 8.27 | 8.27 (1.00×) |
| `table-stream` | 8.35 | 4.21 (0.50×) |
| `text-typing` | 4.26 | 4.28 (1.00×) |
| `text-log-stream` | 8.34 | 4.18 (0.50×) |
| `text-relabel` | 8.27 | 8.29 (1.00×) |
| `scroll-uniform-list` | 8.44 | 8.44 (1.00×) |
| `scroll-variable-list` | 4.38 | 8.49 (1.94×) |
| `scroll-div-cards` | 8.31 | 8.34 (1.00×) |
| `scroll-data-table` | 4.32 | 8.48 (1.96×) |

##### Allocations per frame

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 191 | 189 (0.99×) |
| `dashboard-one-panel` | 628 | 626 (1.00×) |
| `dashboard-all-panels` | 17563 | 17557 (1.00×) |
| `table-stream` | 1687 | 1684 (1.00×) |
| `text-typing` | 470 | 468 (1.00×) |
| `text-log-stream` | 3623 | 3622 (1.00×) |
| `text-relabel` | 20556 | 20555 (1.00×) |
| `scroll-uniform-list` | 697 | 696 (1.00×) |
| `scroll-variable-list` | 408 | 407 (1.00×) |
| `scroll-div-cards` | 5864 | 5863 (1.00×) |
| `scroll-data-table` | 1600 | 1599 (1.00×) |

##### Allocated KB per frame

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 376.0 | 375.8 (1.00×) |
| `dashboard-one-panel` | 1304.0 | 1303.9 (1.00×) |
| `dashboard-all-panels` | 7855.0 | 7852.0 (1.00×) |
| `table-stream` | 529.5 | 529.1 (1.00×) |
| `text-typing` | 710.9 | 710.8 (1.00×) |
| `text-log-stream` | 2578.5 | 2578.4 (1.00×) |
| `text-relabel` | 3776.0 | 3775.9 (1.00×) |
| `scroll-uniform-list` | 677.3 | 675.3 (1.00×) |
| `scroll-variable-list` | 815.1 | 814.2 (1.00×) |
| `scroll-div-cards` | 3879.6 | 3879.6 (1.00×) |
| `scroll-data-table` | 2324.7 | 2322.3 (1.00×) |

##### Live Rust heap at the end (MB)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 24.8 | 24.8 (1.00×) |
| `dashboard-one-panel` | 24.9 | 24.8 (1.00×) |
| `dashboard-all-panels` | 25.1 | 24.9 (0.99×) |
| `table-stream` | 22.0 | 21.9 (1.00×) |
| `text-typing` | 17.0 | 17.0 (1.00×) |
| `text-log-stream` | 16.1 | 16.1 (1.00×) |
| `text-relabel` | 34.1 | 34.0 (1.00×) |
| `scroll-uniform-list` | 15.3 | 15.3 (1.00×) |
| `scroll-variable-list` | 17.7 | 17.8 (1.01×) |
| `scroll-div-cards` | 42.7 | 42.6 (1.00×) |
| `scroll-data-table` | 30.6 | 30.6 (1.00×) |

##### Memory footprint at the end (MB)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 121.8 | 124.1 (1.02×) |
| `dashboard-one-panel` | 124.0 | 124.0 (1.00×) |
| `dashboard-all-panels` | 124.1 | 124.1 (1.00×) |
| `table-stream` | 121.3 | 121.7 (1.00×) |
| `text-typing` | 116.1 | 116.3 (1.00×) |
| `text-log-stream` | 115.2 | 114.9 (1.00×) |
| `text-relabel` | 133.7 | 134.6 (1.01×) |
| `scroll-uniform-list` | 144.3 | 144.6 (1.00×) |
| `scroll-variable-list` | 153.8 | 152.6 (0.99×) |
| `scroll-div-cards` | 186.5 | 187.8 (1.01×) |
| `scroll-data-table` | 168.6 | 167.0 (0.99×) |

#### WASM in Chromium, WebGPU

Reports: (b) 58257fe: 33 runs, (a') epic: 33 runs

##### CPU ms per frame (main thread, mean)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 1.14 | 0.89 (0.78×) |
| `dashboard-one-panel` | 1.58 | 1.00 (0.63×) |
| `dashboard-all-panels` | 6.67 | 6.12 (0.92×) |
| `table-stream` | 1.60 | 1.13 (0.71×) |
| `text-typing` | 1.30 | 0.96 (0.74×) |
| `text-log-stream` | 1.61 | 1.28 (0.79×) |
| `text-relabel` | 8.00 | 7.64 (0.95×) |
| `scroll-uniform-list` | 0.77 | 0.51 (0.66×) |
| `scroll-variable-list` | 0.48 | 0.49 (1.02×) |
| `scroll-div-cards` | 1.67 | 1.68 (1.01×) |
| `scroll-data-table` | 1.53 | 1.09 (0.71×) |

##### Frame time: `Window::draw` p50 (ms)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 0.791 | 0.650 (0.82×) |
| `dashboard-one-panel` | 1.200 | 0.800 (0.67×) |
| `dashboard-all-panels` | 6.103 | 5.624 (0.92×) |
| `table-stream` | 1.186 | 0.925 (0.78×) |
| `text-typing` | 0.900 | 0.686 (0.76×) |
| `text-log-stream` | 1.330 | 1.111 (0.84×) |
| `text-relabel` | 7.594 | 7.303 (0.96×) |
| `scroll-uniform-list` | 0.430 | 0.245 (0.57×) |
| `scroll-variable-list` | 0.205 | 0.205 (1.00×) |
| `scroll-div-cards` | 1.080 | 1.106 (1.02×) |
| `scroll-data-table` | 0.975 | 0.675 (0.69×) |

##### Frame time: `Window::draw` p95 (ms)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 1.070 | 0.845 (0.79×) |
| `dashboard-one-panel` | 1.491 | 1.111 (0.75×) |
| `dashboard-all-panels` | 6.431 | 6.169 (0.96×) |
| `table-stream` | 2.230 | 1.170 (0.52×) |
| `text-typing` | 1.970 | 0.986 (0.50×) |
| `text-log-stream` | 1.436 | 1.236 (0.86×) |
| `text-relabel` | 7.979 | 7.520 (0.94×) |
| `scroll-uniform-list` | 0.600 | 0.430 (0.72×) |
| `scroll-variable-list` | 0.475 | 0.505 (1.06×) |
| `scroll-div-cards` | 1.316 | 1.300 (0.99×) |
| `scroll-data-table` | 1.146 | 0.986 (0.86×) |

##### Dirty-to-present latency p50 (ms)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 0.93 | 0.76 (0.82×) |
| `dashboard-one-panel` | 1.32 | 0.87 (0.66×) |
| `dashboard-all-panels` | 6.31 | 5.79 (0.92×) |
| `table-stream` | 1.32 | 0.99 (0.75×) |
| `text-typing` | 1.03 | 0.77 (0.75×) |
| `text-log-stream` | 1.44 | 1.17 (0.81×) |
| `text-relabel` | 7.74 | 7.43 (0.96×) |
| `scroll-uniform-list` | 0.63 | 0.39 (0.61×) |
| `scroll-variable-list` | 0.34 | 0.34 (1.00×) |
| `scroll-div-cards` | 1.20 | 1.21 (1.01×) |
| `scroll-data-table` | 1.30 | 0.93 (0.71×) |

##### Dirty-to-present latency p95 (ms)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 1.24 | 0.98 (0.79×) |
| `dashboard-one-panel` | 1.68 | 1.23 (0.73×) |
| `dashboard-all-panels` | 6.67 | 6.37 (0.96×) |
| `table-stream` | 2.44 | 1.28 (0.52×) |
| `text-typing` | 2.22 | 1.13 (0.51×) |
| `text-log-stream` | 1.57 | 1.35 (0.86×) |
| `text-relabel` | 8.17 | 7.67 (0.94×) |
| `scroll-uniform-list` | 0.85 | 0.65 (0.76×) |
| `scroll-variable-list` | 0.74 | 0.74 (1.01×) |
| `scroll-div-cards` | 1.63 | 1.62 (0.99×) |
| `scroll-data-table` | 1.55 | 1.34 (0.86×) |

##### Allocations per frame

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 231 | 229 (0.99×) |
| `dashboard-one-panel` | 859 | 857 (1.00×) |
| `dashboard-all-panels` | 26708 | 26702 (1.00×) |
| `table-stream` | 3215 | 3212 (1.00×) |
| `text-typing` | 723 | 721 (1.00×) |
| `text-log-stream` | 3713 | 3712 (1.00×) |
| `text-relabel` | 46513 | 46512 (1.00×) |
| `scroll-uniform-list` | 855 | 854 (1.00×) |
| `scroll-variable-list` | 509 | 508 (1.00×) |
| `scroll-div-cards` | 5892 | 5891 (1.00×) |
| `scroll-data-table` | 2399 | 2398 (1.00×) |

##### Allocated KB per frame

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 362.6 | 362.5 (1.00×) |
| `dashboard-one-panel` | 1308.0 | 1307.9 (1.00×) |
| `dashboard-all-panels` | 8740.9 | 8739.5 (1.00×) |
| `table-stream` | 708.6 | 708.4 (1.00×) |
| `text-typing` | 961.2 | 961.1 (1.00×) |
| `text-log-stream` | 2548.5 | 2548.5 (1.00×) |
| `text-relabel` | 7072.8 | 7072.7 (1.00×) |
| `scroll-uniform-list` | 590.6 | 589.6 (1.00×) |
| `scroll-variable-list` | 659.7 | 659.2 (1.00×) |
| `scroll-div-cards` | 3232.2 | 3232.1 (1.00×) |
| `scroll-data-table` | 2150.2 | 2149.0 (1.00×) |

##### Live Rust heap at the end (MB)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 41.5 | 41.5 (1.00×) |
| `dashboard-one-panel` | 41.5 | 41.5 (1.00×) |
| `dashboard-all-panels` | 41.8 | 41.8 (1.00×) |
| `table-stream` | 39.4 | 39.4 (1.00×) |
| `text-typing` | 36.2 | 36.2 (1.00×) |
| `text-log-stream` | 33.4 | 33.4 (1.00×) |
| `text-relabel` | 51.2 | 51.2 (1.00×) |
| `scroll-uniform-list` | 33.5 | 33.6 (1.00×) |
| `scroll-variable-list` | 36.0 | 36.1 (1.00×) |
| `scroll-div-cards` | 57.7 | 57.6 (1.00×) |
| `scroll-data-table` | 46.1 | 46.1 (1.00×) |

##### Memory footprint at the end (MB)

| Scenario | (b) 58257fe | (a') epic |
|---|---:|---:|
| `dashboard-clock` | 49.4 | 49.4 (1.00×) |
| `dashboard-one-panel` | 49.4 | 49.2 (1.00×) |
| `dashboard-all-panels` | 49.6 | 49.7 (1.00×) |
| `table-stream` | 47.1 | 47.2 (1.00×) |
| `text-typing` | 43.4 | 43.1 (0.99×) |
| `text-log-stream` | 39.7 | 40.2 (1.01×) |
| `text-relabel` | 60.8 | 60.8 (1.00×) |
| `scroll-uniform-list` | 42.0 | 42.3 (1.01×) |
| `scroll-variable-list` | 46.7 | 46.8 (1.00×) |
| `scroll-div-cards` | 76.8 | 76.9 (1.00×) |
| `scroll-data-table` | 59.8 | 62.2 (1.04×) |

### Alternated: (a′) and (c′)

#### macOS native (Metal)

Reports: (a') epic: 32 runs, (c') epic, always: 32 runs

##### CPU ms per frame (main thread, mean)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 0.55 | 0.74 (1.34×) |
| `dashboard-one-panel` | 0.87 | 1.01 (1.16×) |
| `dashboard-all-panels` | 5.49 | 5.56 (1.01×) |
| `table-stream` | 0.88 | 1.05 (1.19×) |
| `text-typing` | 0.61 | 0.78 (1.27×) |
| `text-log-stream` | 1.03 | 1.24 (1.20×) |
| `text-relabel` | 7.35 | 7.83 (1.06×) |
| `scroll-uniform-list` | 0.47 | 0.56 (1.19×) |
| `scroll-variable-list` | 0.38 | 0.37 (0.96×) |
| `scroll-div-cards` | 1.72 | 2.20 (1.28×) |
| `scroll-data-table` | 1.00 | 1.16 (1.15×) |

##### GPU ms per frame (Metal, mean)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 3.07 | 3.08 (1.00×) |
| `dashboard-one-panel` | 3.08 | 3.07 (1.00×) |
| `dashboard-all-panels` | 3.08 | 3.08 (1.00×) |
| `table-stream` | 2.59 | 2.59 (1.00×) |
| `text-typing` | 2.14 | 2.14 (1.00×) |
| `text-log-stream` | 3.06 | 3.06 (1.00×) |
| `text-relabel` | 2.62 | 2.61 (1.00×) |
| `scroll-uniform-list` | 2.87 | 2.87 (1.00×) |
| `scroll-variable-list` | 2.81 | 2.80 (1.00×) |
| `scroll-div-cards` | 2.83 | 2.82 (1.00×) |
| `scroll-data-table` | 2.92 | 2.92 (1.00×) |

##### Frame time: `Window::draw` p50 (ms)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 0.434 | 0.633 (1.46×) |
| `dashboard-one-panel` | 0.748 | 0.892 (1.19×) |
| `dashboard-all-panels` | 5.239 | 5.370 (1.03×) |
| `table-stream` | 0.795 | 0.966 (1.22×) |
| `text-typing` | 0.496 | 0.643 (1.30×) |
| `text-log-stream` | 0.907 | 1.097 (1.21×) |
| `text-relabel` | 7.090 | 7.606 (1.07×) |
| `scroll-uniform-list` | 0.305 | 0.378 (1.24×) |
| `scroll-variable-list` | 0.208 | 0.203 (0.97×) |
| `scroll-div-cards` | 1.138 | 1.680 (1.48×) |
| `scroll-data-table` | 0.706 | 0.858 (1.22×) |

##### Frame time: `Window::draw` p95 (ms)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 0.660 | 0.692 (1.05×) |
| `dashboard-one-panel` | 0.824 | 1.147 (1.39×) |
| `dashboard-all-panels` | 5.636 | 5.640 (1.00×) |
| `table-stream` | 0.946 | 1.100 (1.16×) |
| `text-typing` | 1.003 | 0.869 (0.87×) |
| `text-log-stream` | 1.401 | 1.175 (0.84×) |
| `text-relabel` | 7.487 | 7.975 (1.07×) |
| `scroll-uniform-list` | 0.535 | 0.446 (0.83×) |
| `scroll-variable-list` | 0.370 | 0.358 (0.97×) |
| `scroll-div-cards` | 1.198 | 1.780 (1.49×) |
| `scroll-data-table` | 0.810 | 1.663 (2.05×) |

##### Dirty-to-present latency p50 (ms)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 6.21 | 8.31 (1.34×) |
| `dashboard-one-panel` | 8.29 | 4.17 (0.50×) |
| `dashboard-all-panels` | 8.19 | 8.18 (1.00×) |
| `table-stream` | 8.31 | 8.31 (1.00×) |
| `text-typing` | 4.12 | 4.20 (1.02×) |
| `text-log-stream` | 4.19 | 8.27 (1.97×) |
| `text-relabel` | 8.22 | 8.22 (1.00×) |
| `scroll-uniform-list` | 4.27 | 8.29 (1.94×) |
| `scroll-variable-list` | 4.21 | 8.31 (1.97×) |
| `scroll-div-cards` | 8.01 | 8.04 (1.00×) |
| `scroll-data-table` | 8.30 | 4.27 (0.51×) |

##### Dirty-to-present latency p95 (ms)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 6.28 | 8.37 (1.33×) |
| `dashboard-one-panel` | 8.35 | 4.23 (0.51×) |
| `dashboard-all-panels` | 8.24 | 8.23 (1.00×) |
| `table-stream` | 8.36 | 8.37 (1.00×) |
| `text-typing` | 4.18 | 4.27 (1.02×) |
| `text-log-stream` | 4.24 | 8.34 (1.97×) |
| `text-relabel` | 8.27 | 8.29 (1.00×) |
| `scroll-uniform-list` | 4.37 | 8.44 (1.93×) |
| `scroll-variable-list` | 4.39 | 8.49 (1.93×) |
| `scroll-div-cards` | 8.31 | 8.34 (1.00×) |
| `scroll-data-table` | 8.47 | 4.37 (0.52×) |

##### Allocations per frame

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 189 | 6284 (33.24×) |
| `dashboard-one-panel` | 626 | 6703 (10.71×) |
| `dashboard-all-panels` | 17557 | 22788 (1.30×) |
| `table-stream` | 1684 | 7188 (4.27×) |
| `text-typing` | 468 | 1539 (3.29×) |
| `text-log-stream` | 3622 | 4606 (1.27×) |
| `text-relabel` | 20555 | 29585 (1.44×) |
| `scroll-uniform-list` | 696 | 2394 (3.44×) |
| `scroll-variable-list` | 406 | 704 (1.74×) |
| `scroll-div-cards` | 5863 | 25443 (4.34×) |
| `scroll-data-table` | 1599 | 7131 (4.46×) |

##### Allocated KB per frame

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 375.8 | 1085.6 (2.89×) |
| `dashboard-one-panel` | 1303.9 | 2013.6 (1.54×) |
| `dashboard-all-panels` | 7852.0 | 8557.1 (1.09×) |
| `table-stream` | 529.1 | 1206.0 (2.28×) |
| `text-typing` | 710.8 | 1130.3 (1.59×) |
| `text-log-stream` | 2578.5 | 2746.8 (1.07×) |
| `text-relabel` | 3775.9 | 5070.9 (1.34×) |
| `scroll-uniform-list` | 675.3 | 859.4 (1.27×) |
| `scroll-variable-list` | 814.2 | 862.5 (1.06×) |
| `scroll-div-cards` | 3879.6 | 6646.6 (1.71×) |
| `scroll-data-table` | 2322.3 | 2984.5 (1.29×) |

##### Live Rust heap at the end (MB)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 24.7 | 25.5 (1.03×) |
| `dashboard-one-panel` | 24.7 | 25.4 (1.03×) |
| `dashboard-all-panels` | 25.0 | 25.4 (1.02×) |
| `table-stream` | 21.9 | 22.5 (1.03×) |
| `text-typing` | 17.0 | 17.2 (1.01×) |
| `text-log-stream` | 16.1 | 16.2 (1.01×) |
| `text-relabel` | 34.1 | 35.2 (1.03×) |
| `scroll-uniform-list` | 15.3 | 15.5 (1.02×) |
| `scroll-variable-list` | 17.8 | 17.9 (1.00×) |
| `scroll-div-cards` | 42.6 | 44.9 (1.05×) |
| `scroll-data-table` | 30.6 | 31.2 (1.02×) |

##### Memory footprint at the end (MB)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 122.8 | 122.5 (1.00×) |
| `dashboard-one-panel` | 123.7 | 125.6 (1.02×) |
| `dashboard-all-panels` | 124.3 | 125.9 (1.01×) |
| `table-stream` | 121.1 | 121.6 (1.00×) |
| `text-typing` | 116.5 | 117.1 (1.01×) |
| `text-log-stream` | 115.3 | 114.9 (1.00×) |
| `text-relabel` | 133.7 | 136.4 (1.02×) |
| `scroll-uniform-list` | 144.5 | 144.9 (1.00×) |
| `scroll-variable-list` | 155.5 | 153.8 (0.99×) |
| `scroll-div-cards` | 189.7 | 190.8 (1.01×) |
| `scroll-data-table` | 167.3 | 167.9 (1.00×) |

#### WASM in Chromium, WebGPU

Reports: (a') epic: 33 runs, (c') epic, always: 33 runs

##### CPU ms per frame (main thread, mean)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 1.35 | 1.21 (0.90×) |
| `dashboard-one-panel` | 1.51 | 1.77 (1.18×) |
| `dashboard-all-panels` | 6.70 | 7.54 (1.12×) |
| `table-stream` | 1.62 | 2.40 (1.48×) |
| `text-typing` | 1.08 | 1.57 (1.45×) |
| `text-log-stream` | 1.44 | 1.81 (1.26×) |
| `text-relabel` | 8.21 | 8.20 (1.00×) |
| `scroll-uniform-list` | 1.11 | 0.68 (0.61×) |
| `scroll-variable-list` | 1.10 | 0.70 (0.63×) |
| `scroll-div-cards` | 2.44 | 3.38 (1.39×) |
| `scroll-data-table` | 1.98 | 1.94 (0.98×) |

##### Frame time: `Window::draw` p50 (ms)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 0.820 | 0.920 (1.12×) |
| `dashboard-one-panel` | 1.176 | 1.460 (1.24×) |
| `dashboard-all-panels` | 6.156 | 6.787 (1.10×) |
| `table-stream` | 1.240 | 1.646 (1.33×) |
| `text-typing` | 0.835 | 1.151 (1.38×) |
| `text-log-stream` | 1.195 | 1.455 (1.22×) |
| `text-relabel` | 7.778 | 7.827 (1.01×) |
| `scroll-uniform-list` | 0.635 | 0.370 (0.58×) |
| `scroll-variable-list` | 0.480 | 0.315 (0.66×) |
| `scroll-div-cards` | 1.455 | 2.507 (1.72×) |
| `scroll-data-table` | 1.146 | 1.380 (1.20×) |

##### Frame time: `Window::draw` p95 (ms)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 1.621 | 1.255 (0.77×) |
| `dashboard-one-panel` | 1.340 | 1.640 (1.22×) |
| `dashboard-all-panels` | 6.558 | 7.664 (1.17×) |
| `table-stream` | 1.431 | 3.527 (2.47×) |
| `text-typing` | 1.070 | 2.045 (1.91×) |
| `text-log-stream` | 1.411 | 1.696 (1.20×) |
| `text-relabel` | 8.286 | 8.094 (0.98×) |
| `scroll-uniform-list` | 0.930 | 0.605 (0.65×) |
| `scroll-variable-list` | 0.880 | 0.665 (0.76×) |
| `scroll-div-cards` | 2.951 | 2.757 (0.93×) |
| `scroll-data-table` | 2.051 | 1.675 (0.82×) |

##### Dirty-to-present latency p50 (ms)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 0.97 | 1.03 (1.06×) |
| `dashboard-one-panel` | 1.31 | 1.59 (1.22×) |
| `dashboard-all-panels` | 6.35 | 6.98 (1.10×) |
| `table-stream` | 1.39 | 1.81 (1.30×) |
| `text-typing` | 0.94 | 1.30 (1.38×) |
| `text-log-stream` | 1.28 | 1.59 (1.24×) |
| `text-relabel` | 7.93 | 7.97 (1.01×) |
| `scroll-uniform-list` | 0.88 | 0.53 (0.61×) |
| `scroll-variable-list` | 0.79 | 0.49 (0.62×) |
| `scroll-div-cards` | 1.64 | 2.68 (1.63×) |
| `scroll-data-table` | 1.51 | 1.70 (1.13×) |

##### Dirty-to-present latency p95 (ms)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 1.87 | 1.40 (0.75×) |
| `dashboard-one-panel` | 1.50 | 1.80 (1.20×) |
| `dashboard-all-panels` | 6.78 | 7.86 (1.16×) |
| `table-stream` | 1.60 | 3.81 (2.39×) |
| `text-typing` | 1.22 | 2.28 (1.86×) |
| `text-log-stream` | 1.55 | 1.86 (1.20×) |
| `text-relabel` | 8.44 | 8.25 (0.98×) |
| `scroll-uniform-list` | 1.20 | 0.81 (0.67×) |
| `scroll-variable-list` | 1.34 | 0.93 (0.69×) |
| `scroll-div-cards` | 3.23 | 3.13 (0.97×) |
| `scroll-data-table` | 2.88 | 2.08 (0.72×) |

##### Allocations per frame

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 229 | 6324 (27.61×) |
| `dashboard-one-panel` | 857 | 6934 (8.09×) |
| `dashboard-all-panels` | 26702 | 31933 (1.20×) |
| `table-stream` | 3212 | 8716 (2.71×) |
| `text-typing` | 721 | 1791 (2.49×) |
| `text-log-stream` | 3712 | 4696 (1.27×) |
| `text-relabel` | 46512 | 55542 (1.19×) |
| `scroll-uniform-list` | 854 | 2544 (2.98×) |
| `scroll-variable-list` | 508 | 807 (1.59×) |
| `scroll-div-cards` | 5891 | 25471 (4.32×) |
| `scroll-data-table` | 2398 | 7921 (3.30×) |

##### Allocated KB per frame

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 362.5 | 1049.5 (2.89×) |
| `dashboard-one-panel` | 1307.9 | 1994.9 (1.53×) |
| `dashboard-all-panels` | 8739.5 | 9421.8 (1.08×) |
| `table-stream` | 708.4 | 1363.2 (1.92×) |
| `text-typing` | 961.1 | 1375.6 (1.43×) |
| `text-log-stream` | 2548.5 | 2711.7 (1.06×) |
| `text-relabel` | 7072.7 | 8324.1 (1.18×) |
| `scroll-uniform-list` | 589.6 | 767.8 (1.30×) |
| `scroll-variable-list` | 659.3 | 706.6 (1.07×) |
| `scroll-div-cards` | 3232.2 | 5913.3 (1.83×) |
| `scroll-data-table` | 2149.0 | 2790.0 (1.30×) |

##### Live Rust heap at the end (MB)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 41.5 | 42.1 (1.02×) |
| `dashboard-one-panel` | 41.5 | 42.1 (1.01×) |
| `dashboard-all-panels` | 41.8 | 42.4 (1.01×) |
| `table-stream` | 39.4 | 40.0 (1.02×) |
| `text-typing` | 36.2 | 36.5 (1.01×) |
| `text-log-stream` | 33.4 | 33.6 (1.01×) |
| `text-relabel` | 51.2 | 52.3 (1.02×) |
| `scroll-uniform-list` | 33.6 | 33.8 (1.01×) |
| `scroll-variable-list` | 36.0 | 36.1 (1.00×) |
| `scroll-div-cards` | 57.6 | 59.9 (1.04×) |
| `scroll-data-table` | 46.1 | 46.7 (1.01×) |

##### Memory footprint at the end (MB)

| Scenario | (a') epic | (c') epic, always |
|---|---:|---:|
| `dashboard-clock` | 49.4 | 50.1 (1.02×) |
| `dashboard-one-panel` | 49.6 | 50.4 (1.02×) |
| `dashboard-all-panels` | 49.5 | 50.7 (1.02×) |
| `table-stream` | 46.9 | 48.0 (1.02×) |
| `text-typing` | 43.1 | 43.8 (1.01×) |
| `text-log-stream` | 40.0 | 39.9 (1.00×) |
| `text-relabel` | 60.8 | 63.7 (1.05×) |
| `scroll-uniform-list` | 41.9 | 42.2 (1.01×) |
| `scroll-variable-list` | 46.7 | 46.8 (1.00×) |
| `scroll-div-cards` | 76.4 | 80.5 (1.05×) |
| `scroll-data-table` | 62.2 | 64.3 (1.03×) |
