# gpux benchmark run `2026-10-16-macos-loop`

- Machine: Apple M5, 24 GB, macOS 26.6.2 (macOS-26.6.2-arm64-arm-64bit)
- gpux commit: `258bbf80b`
- Every run: 120 warm-up frames, then 600 measured frames; 3 repetitions, interleaved
- `off` and `on` are the same build with every fast-mode mechanism off or on (`Window::set_fast_mode`).
- A cell is the median over the repetitions. A change marked `~` is within the repetitions' spread or under 3% (noise); **bold** marks a change for the worse.

## Summary

Geometric means over the scenarios of the ratio of medians.

| Target | Metric | on / off | wins | losses | within noise |
|---|---|---:|---:|---:|---:|
| macOS native (Metal) | CPU ms per frame | 0.745 (-25.5%) | 11 | 2 | 2 |
| macOS native (Metal) | Window::draw p50 | 0.669 (-33.1%) | 11 | 1 | 3 |

## macOS native (Metal)

viewport 1280×801 @2x; display interval ≈8.3 ms; 90 of 90 runs reported, 9.5 min

**Frame cost: CPU** (ms of main-thread CPU per frame, mean)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.37 | 2.84 | -35% |
| `dashboard-one-panel` | 4.41 | 3.07 | -30% |
| `dashboard-all-panels` | 5.30 | 4.87 | -8.0% |
| `table-stream` | 3.42 | 2.80 | -18% |
| `text-typing` | 2.95 | 3.02 | ~+2.6% |
| `text-log-stream` | 2.84 | 2.88 | ~+1.4% |
| `text-relabel` | 6.82 | 6.19 | -9.1% |
| `scroll-uniform-list` | 3.60 | 1.91 | -47% |
| `scroll-variable-list` | 2.94 | 1.46 | -50% |
| `scroll-div-cards` | 12.5 | 3.84 | -69% |
| `scroll-data-table` | 2.94 | 3.55 | **+21%** |
| `hover-grid` | 6.01 | 4.96 | -18% |
| `tree-toggle` | 4.36 | 3.95 | -10% |
| `animated-dots` | 3.02 | 3.19 | **+5.4%** |
| `effects-cards` | 3.82 | 1.85 | -52% |

**Window::draw, median** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.23 | 2.59 | -39% |
| `dashboard-one-panel` | 4.27 | 2.84 | -33% |
| `dashboard-all-panels` | 5.13 | 4.71 | -8.2% |
| `table-stream` | 3.27 | 2.73 | -17% |
| `text-typing` | 2.91 | 2.84 | ~-2.3% |
| `text-log-stream` | 2.76 | 2.86 | ~+3.5% |
| `text-relabel` | 6.63 | 6.00 | -10% |
| `scroll-uniform-list` | 3.47 | 1.37 | -60% |
| `scroll-variable-list` | 2.68 | 0.81 | -70% |
| `scroll-div-cards` | 12.2 | 3.21 | -74% |
| `scroll-data-table` | 2.74 | 2.79 | ~+1.9% |
| `hover-grid` | 5.66 | 4.69 | -17% |
| `tree-toggle` | 4.21 | 2.88 | -31% |
| `animated-dots` | 2.44 | 2.60 | **+6.7%** |
| `effects-cards` | 3.65 | 1.58 | -57% |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.32 | 3.01 | -30% |
| `dashboard-one-panel` | 4.36 | 3.76 | -14% |
| `dashboard-all-panels` | 5.24 | 4.81 | -8.2% |
| `table-stream` | 3.51 | 3.31 | ~-5.7% |
| `text-typing` | 3.40 | 3.27 | -3.9% |
| `text-log-stream` | 3.07 | 3.27 | ~+6.5% |
| `text-relabel` | 6.77 | 6.14 | -9.3% |
| `scroll-uniform-list` | 4.06 | 1.81 | -55% |
| `scroll-variable-list` | 3.34 | 1.70 | -49% |
| `scroll-div-cards` | 12.4 | 3.69 | -70% |
| `scroll-data-table` | 3.07 | 3.95 | **+29%** |
| `hover-grid` | 5.81 | 4.76 | -18% |
| `tree-toggle` | 4.54 | 6.14 | **+35%** |
| `animated-dots` | 3.78 | 4.05 | **+7.0%** |
| `effects-cards` | 3.76 | 2.37 | -37% |

**Frame rate** (frames per second)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 120 | 120 | ~-0.0% |
| `dashboard-one-panel` | 120 | 120 | ~+0.0% |
| `dashboard-all-panels` | 120 | 120 | ~-0.0% |
| `table-stream` | 120 | 120 | ~+0.0% |
| `text-typing` | 120 | 120 | ~-0.0% |
| `text-log-stream` | 120 | 120 | ~-0.0% |
| `text-relabel` | 120 | 120 | ~+0.0% |
| `scroll-uniform-list` | 120 | 120 | ~+0.0% |
| `scroll-variable-list` | 120 | 120 | ~-0.0% |
| `scroll-div-cards` | 79.8 | 120 | +50% |
| `scroll-data-table` | 120 | 120 | ~-0.0% |
| `hover-grid` | 120 | 120 | ~+0.0% |
| `tree-toggle` | 120 | 120 | ~-0.0% |
| `animated-dots` | 120 | 120 | ~-0.0% |
| `effects-cards` | 120 | 120 | ~-0.0% |

**Missed frames** (intervals over 1.5× the display's, of the measured frames; changes in frames)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 0 | 0 | ~+0.00 |
| `dashboard-one-panel` | 0 | 0 | ~+0.00 |
| `dashboard-all-panels` | 0 | 0 | ~+0.00 |
| `table-stream` | 0 | 0 | ~+0.00 |
| `text-typing` | 0 | 0 | ~+0.00 |
| `text-log-stream` | 0 | 0 | ~+0.00 |
| `text-relabel` | 0 | 0 | ~+0.00 |
| `scroll-uniform-list` | 0 | 0 | ~+0.00 |
| `scroll-variable-list` | 0 | 0 | ~+0.00 |
| `scroll-div-cards` | 0 | 0 | ~+0.00 |
| `scroll-data-table` | 0 | 5 | **+5.00** |
| `hover-grid` | 0 | 0 | ~+0.00 |
| `tree-toggle` | 0 | 0 | ~+0.00 |
| `animated-dots` | 0 | 0 | ~+0.00 |
| `effects-cards` | 0 | 0 | ~+0.00 |

**Process CPU** (% of one core, all threads)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 54.7 | 40.4 | -26% |
| `dashboard-one-panel` | 55.3 | 41.9 | -24% |
| `dashboard-all-panels` | 66.4 | 61.4 | -7.5% |
| `table-stream` | 43.6 | 37.9 | -13% |
| `text-typing` | 39.0 | 41.7 | **+6.7%** |
| `text-log-stream` | 38.5 | 38.9 | ~+0.9% |
| `text-relabel` | 84.0 | 77.1 | -8.2% |
| `scroll-uniform-list` | 48.5 | 30.5 | -37% |
| `scroll-variable-list` | 41.0 | 25.7 | -37% |
| `scroll-div-cards` | 101 | 49.9 | -51% |
| `scroll-data-table` | 38.4 | 49.4 | **+29%** |
| `hover-grid` | 75.2 | 62.7 | -17% |
| `tree-toggle` | 54.6 | 50.4 | -7.7% |
| `animated-dots` | 39.9 | 42.0 | **+5.4%** |
| `effects-cards` | 48.0 | 27.1 | -43% |

**Memory footprint at the end** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 138 | 140 | ~+1.5% |
| `dashboard-one-panel` | 138 | 140 | ~+1.9% |
| `dashboard-all-panels` | 138 | 140 | ~+1.7% |
| `table-stream` | 134 | 136 | ~+1.7% |
| `text-typing` | 130 | 132 | ~+1.4% |
| `text-log-stream` | 131 | 131 | ~+0.1% |
| `text-relabel` | 147 | 152 | ~+2.9% |
| `scroll-uniform-list` | 126 | 164 | **+30%** |
| `scroll-variable-list` | 134 | 178 | **+33%** |
| `scroll-div-cards` | 142 | 207 | **+46%** |
| `scroll-data-table` | 136 | 197 | **+45%** |
| `hover-grid` | 158 | 161 | ~+1.7% |
| `tree-toggle` | 138 | 144 | **+4.3%** |
| `animated-dots` | 126 | 126 | ~+0.5% |
| `effects-cards` | 135 | 136 | ~+0.9% |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 0.19 | 0.22 | ~+0.03 |
| `dashboard-one-panel` | 0.34 | 0.42 | ~+0.08 |
| `dashboard-all-panels` | 0.23 | 0.25 | ~+0.02 |
| `table-stream` | 0.09 | 0.44 | **+0.34** |
| `text-typing` | 0.14 | 0.47 | **+0.33** |
| `text-log-stream` | 0.55 | 0.61 | ~+0.06 |
| `text-relabel` | 0.25 | 0.34 | **+0.09** |
| `scroll-uniform-list` | 0.44 | 5.19 | ~+4.75 |
| `scroll-variable-list` | 1.91 | -9.41 | -11.31 |
| `scroll-div-cards` | 0.44 | -0.92 | -1.36 |
| `scroll-data-table` | 0.09 | 7.47 | **+7.38** |
| `hover-grid` | 0.47 | 0.41 | ~-0.06 |
| `tree-toggle` | 0.33 | 0.14 | ~-0.19 |
| `animated-dots` | 0.50 | 0.41 | ~-0.09 |
| `effects-cards` | 0.30 | 0.41 | ~+0.11 |

**Peak live Rust heap** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 25.0 | 26.0 | **+3.7%** |
| `dashboard-one-panel` | 25.1 | 26.3 | **+4.7%** |
| `dashboard-all-panels` | 25.4 | 27.7 | **+8.9%** |
| `table-stream` | 21.3 | 22.4 | **+4.8%** |
| `text-typing` | 16.1 | 17.5 | **+8.8%** |
| `text-log-stream` | 16.1 | 16.3 | ~+1.5% |
| `text-relabel` | 35.4 | 37.1 | **+4.9%** |
| `scroll-uniform-list` | 12.8 | 17.9 | **+40%** |
| `scroll-variable-list` | 18.1 | 21.8 | **+21%** |
| `scroll-div-cards` | 40.1 | 58.1 | **+45%** |
| `scroll-data-table` | 22.8 | 39.1 | **+72%** |
| `hover-grid` | 48.0 | 50.4 | **+4.9%** |
| `tree-toggle` | 25.1 | 28.4 | **+13%** |
| `animated-dots` | 13.8 | 14.5 | **+5.1%** |
| `effects-cards` | 21.0 | 21.0 | ~-0.1% |

**Heap allocations** (per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 24816 | 209 | -99% |
| `dashboard-one-panel` | 24929 | 793 | -97% |
| `dashboard-all-panels` | 30179 | 24620 | -18% |
| `table-stream` | 17241 | 2341 | -86% |
| `text-typing` | 3118 | 499 | -84% |
| `text-log-stream` | 3983 | 4666 | **+17%** |
| `text-relabel` | 34697 | 28052 | -19% |
| `scroll-uniform-list` | 2573 | 757 | -71% |
| `scroll-variable-list` | 754 | 424 | -44% |
| `scroll-div-cards` | 94460 | 15056 | -84% |
| `scroll-data-table` | 8334 | 1848 | -78% |
| `hover-grid` | 35474 | 21245 | -40% |
| `tree-toggle` | 28145 | 17290 | -39% |
| `animated-dots` | 2031 | 2114 | **+4.1%** |
| `effects-cards` | 20821 | 553 | -97% |

**Bytes allocated** (KB per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 15356 | 378 | -98% |
| `dashboard-one-panel` | 15397 | 1344 | -91% |
| `dashboard-all-panels` | 17330 | 9646 | -44% |
| `table-stream` | 5667 | 593 | -90% |
| `text-typing` | 2210 | 712 | -68% |
| `text-log-stream` | 2677 | 2671 | ~-0.2% |
| `text-relabel` | 15475 | 4385 | -72% |
| `scroll-uniform-list` | 1199 | 686 | -43% |
| `scroll-variable-list` | 408 | 816 | **+100%** |
| `scroll-div-cards` | 65198 | 5202 | -92% |
| `scroll-data-table` | 3588 | 2349 | -35% |
| `hover-grid` | 18496 | 10810 | -42% |
| `tree-toggle` | 16788 | 7806 | -54% |
| `animated-dots` | 2921 | 2993 | ~+2.5% |
| `effects-cards` | 12363 | 637 | -95% |

**Startup: window opened to first frame presented** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 75.8 | 74.7 | ~-1.4% |
| `dashboard-one-panel` | 70.9 | 71.8 | ~+1.2% |
| `dashboard-all-panels` | 73.1 | 78.2 | ~+7.0% |
| `table-stream` | 67.6 | 66.0 | ~-2.4% |
| `text-typing` | 68.0 | 77.2 | ~+13% |
| `text-log-stream` | 66.1 | 69.9 | ~+5.8% |
| `text-relabel` | 76.1 | 70.3 | ~-7.7% |
| `scroll-uniform-list` | 67.3 | 66.0 | ~-2.0% |
| `scroll-variable-list` | 66.3 | 65.6 | ~-1.1% |
| `scroll-div-cards` | 99.3 | 90.9 | -8.5% |
| `scroll-data-table` | 67.7 | 66.6 | ~-1.6% |
| `hover-grid` | 77.3 | 75.4 | ~-2.4% |
| `tree-toggle` | 72.0 | 75.5 | ~+4.8% |
| `animated-dots` | 60.6 | 71.3 | ~+18% |
| `effects-cards` | 69.6 | 67.3 | ~-3.4% |

**Startup: the first frame's draw** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 9.7 | 10.3 | **+6.5%** |
| `dashboard-one-panel` | 9.7 | 10.2 | **+4.9%** |
| `dashboard-all-panels` | 10.2 | 10.7 | **+4.9%** |
| `table-stream` | 7.6 | 8.2 | **+7.1%** |
| `text-typing` | 10.2 | 11.0 | **+7.5%** |
| `text-log-stream` | 4.6 | 4.7 | ~+0.5% |
| `text-relabel` | 10.6 | 11.0 | ~+4.0% |
| `scroll-uniform-list` | 3.9 | 3.9 | ~+0.8% |
| `scroll-variable-list` | 4.7 | 4.8 | ~+1.7% |
| `scroll-div-cards` | 28.9 | 29.8 | ~+3.1% |
| `scroll-data-table` | 5.9 | 6.2 | ~+5.1% |
| `hover-grid` | 10.0 | 10.5 | ~+5.0% |
| `tree-toggle` | 8.8 | 9.8 | ~+11% |
| `animated-dots` | 1.4 | 1.5 | **+13%** |
| `effects-cards` | 8.2 | 8.1 | ~-1.1% |
