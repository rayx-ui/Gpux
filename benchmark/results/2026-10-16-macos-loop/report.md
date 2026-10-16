# gpux benchmark run `2026-10-16-macos-loop`

- Machine: Apple M5, 24 GB, macOS 26.6.2 (macOS-26.6.2-arm64-arm-64bit)
- gpux commit: `2905114aa`
- Every run: 120 warm-up frames, then 600 measured frames; 3 repetitions, interleaved
- `off` and `on` are the same build with every fast-mode mechanism off or on (`Window::set_fast_mode`).
- A cell is the median over the repetitions. A change marked `~` is within the repetitions' spread or under 3% (noise); **bold** marks a change for the worse.

## Summary

Geometric means over the scenarios of the ratio of medians.

| Target | Metric | on / off | wins | losses | within noise |
|---|---|---:|---:|---:|---:|
| macOS native (Metal) | CPU ms per frame | 0.758 (-24.2%) | 10 | 2 | 3 |
| macOS native (Metal) | Window::draw p50 | 0.674 (-32.6%) | 10 | 1 | 4 |

## macOS native (Metal)

viewport 1280×801 @2x; display interval ≈8.3 ms; 90 of 90 runs reported, 9.5 min

**Frame cost: CPU** (ms of main-thread CPU per frame, mean)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.38 | 2.61 | -40% |
| `dashboard-one-panel` | 4.44 | 3.29 | -26% |
| `dashboard-all-panels` | 5.40 | 4.59 | -15% |
| `table-stream` | 3.45 | 2.72 | -21% |
| `text-typing` | 2.92 | 3.11 | **+6.6%** |
| `text-log-stream` | 2.76 | 2.91 | ~+5.3% |
| `text-relabel` | 6.89 | 6.30 | -8.5% |
| `scroll-uniform-list` | 3.47 | 1.92 | -45% |
| `scroll-variable-list` | 2.92 | 1.49 | -49% |
| `scroll-div-cards` | 12.6 | 3.21 | -75% |
| `scroll-data-table` | 2.95 | 3.62 | **+23%** |
| `hover-grid` | 5.98 | 4.98 | -17% |
| `tree-toggle` | 4.40 | 4.02 | -8.8% |
| `animated-dots` | 2.99 | 3.18 | ~+6.1% |
| `effects-cards` | 3.64 | 2.63 | ~-28% |

**Window::draw, median** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.22 | 2.45 | -42% |
| `dashboard-one-panel` | 4.28 | 3.09 | -28% |
| `dashboard-all-panels` | 5.23 | 4.41 | -16% |
| `table-stream` | 3.29 | 2.68 | -19% |
| `text-typing` | 2.78 | 2.90 | ~+4.1% |
| `text-log-stream` | 2.72 | 2.88 | ~+6.0% |
| `text-relabel` | 6.69 | 6.09 | -9.0% |
| `scroll-uniform-list` | 3.38 | 1.36 | -60% |
| `scroll-variable-list` | 2.67 | 0.80 | -70% |
| `scroll-div-cards` | 12.3 | 2.18 | -82% |
| `scroll-data-table` | 2.75 | 2.83 | ~+3.1% |
| `hover-grid` | 5.63 | 4.71 | -16% |
| `tree-toggle` | 4.28 | 2.94 | -31% |
| `animated-dots` | 2.47 | 2.63 | **+6.5%** |
| `effects-cards` | 3.49 | 2.41 | ~-31% |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.34 | 2.82 | -35% |
| `dashboard-one-panel` | 4.42 | 3.89 | -12% |
| `dashboard-all-panels` | 5.35 | 4.71 | -12% |
| `table-stream` | 3.53 | 3.22 | -8.9% |
| `text-typing` | 3.41 | 3.31 | ~-2.9% |
| `text-log-stream` | 3.06 | 3.31 | ~+7.9% |
| `text-relabel` | 6.83 | 6.29 | -8.0% |
| `scroll-uniform-list` | 4.06 | 1.78 | -56% |
| `scroll-variable-list` | 3.33 | 1.83 | -45% |
| `scroll-div-cards` | 12.5 | 2.88 | -77% |
| `scroll-data-table` | 3.07 | 3.93 | **+28%** |
| `hover-grid` | 5.79 | 4.80 | -17% |
| `tree-toggle` | 4.59 | 6.12 | **+33%** |
| `animated-dots` | 3.83 | 4.05 | **+5.9%** |
| `effects-cards` | 3.69 | 2.74 | -26% |

**Frame rate** (frames per second)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 120 | 120 | ~+0.0% |
| `dashboard-one-panel` | 120 | 120 | ~+0.0% |
| `dashboard-all-panels` | 120 | 120 | ~+0.0% |
| `table-stream` | 120 | 120 | ~+0.0% |
| `text-typing` | 120 | 120 | ~-0.0% |
| `text-log-stream` | 120 | 120 | ~+0.0% |
| `text-relabel` | 120 | 120 | ~+0.0% |
| `scroll-uniform-list` | 120 | 120 | ~+0.0% |
| `scroll-variable-list` | 120 | 120 | ~-0.0% |
| `scroll-div-cards` | 79.0 | 119 | +50% |
| `scroll-data-table` | 120 | 120 | ~-0.0% |
| `hover-grid` | 120 | 120 | ~+0.0% |
| `tree-toggle` | 120 | 120 | ~+0.0% |
| `animated-dots` | 120 | 120 | ~+0.0% |
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
| `scroll-div-cards` | 0 | 11.0 | **+11.00** |
| `scroll-data-table` | 0 | 5 | **+5.00** |
| `hover-grid` | 0 | 0 | ~+0.00 |
| `tree-toggle` | 0 | 0 | ~+0.00 |
| `animated-dots` | 0 | 0 | ~+0.00 |
| `effects-cards` | 0 | 0 | ~+0.00 |

**Process CPU** (% of one core, all threads)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 54.8 | 37.1 | -32% |
| `dashboard-one-panel` | 55.5 | 44.8 | -19% |
| `dashboard-all-panels` | 67.5 | 57.9 | -14% |
| `table-stream` | 44.0 | 36.8 | -16% |
| `text-typing` | 38.8 | 42.8 | **+10%** |
| `text-log-stream` | 37.6 | 39.1 | ~+4.0% |
| `text-relabel` | 84.8 | 78.2 | -7.8% |
| `scroll-uniform-list` | 46.8 | 30.7 | -34% |
| `scroll-variable-list` | 40.7 | 26.1 | -36% |
| `scroll-div-cards` | 101 | 42.3 | -58% |
| `scroll-data-table` | 38.5 | 50.3 | **+31%** |
| `hover-grid` | 74.8 | 63.0 | -16% |
| `tree-toggle` | 55.0 | 51.4 | -6.6% |
| `animated-dots` | 39.5 | 41.9 | ~+6.1% |
| `effects-cards` | 46.2 | 37.6 | ~-19% |

**Memory footprint at the end** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 136 | 138 | ~+1.6% |
| `dashboard-one-panel` | 136 | 139 | ~+2.1% |
| `dashboard-all-panels` | 136 | 138 | ~+1.5% |
| `table-stream` | 133 | 135 | ~+1.7% |
| `text-typing` | 130 | 131 | ~+1.2% |
| `text-log-stream` | 130 | 130 | ~+0.1% |
| `text-relabel` | 146 | 147 | ~+0.7% |
| `scroll-uniform-list` | 126 | 163 | **+30%** |
| `scroll-variable-list` | 134 | 169 | **+26%** |
| `scroll-div-cards` | 138 | 212 | **+54%** |
| `scroll-data-table` | 136 | 189 | **+39%** |
| `hover-grid` | 157 | 160 | ~+1.8% |
| `tree-toggle` | 138 | 144 | **+4.3%** |
| `animated-dots` | 124 | 126 | ~+1.2% |
| `effects-cards` | 133 | 136 | ~+1.9% |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | -1.41 | -1.47 | ~-0.06 |
| `dashboard-one-panel` | -1.44 | -1.36 | ~+0.08 |
| `dashboard-all-panels` | -1.52 | -1.50 | ~+0.02 |
| `table-stream` | -1.08 | -1.03 | ~+0.05 |
| `text-typing` | -0.11 | 0.30 | ~+0.41 |
| `text-log-stream` | 0.33 | 0.50 | ~+0.17 |
| `text-relabel` | -0.81 | -3.55 | -2.73 |
| `scroll-uniform-list` | 0.06 | 4.66 | **+4.59** |
| `scroll-variable-list` | 1.88 | -13.9 | -15.80 |
| `scroll-div-cards` | -4.34 | 1.03 | ~+5.38 |
| `scroll-data-table` | 0.11 | -0.33 | ~-0.44 |
| `hover-grid` | -0.42 | -0.47 | ~-0.05 |
| `tree-toggle` | 0.33 | 0.16 | ~-0.17 |
| `animated-dots` | -1.09 | -0.50 | ~+0.59 |
| `effects-cards` | -0.92 | -0.66 | ~+0.27 |

**Peak live Rust heap** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 25.0 | 26.0 | **+3.7%** |
| `dashboard-one-panel` | 25.1 | 26.3 | **+4.7%** |
| `dashboard-all-panels` | 25.4 | 27.7 | **+8.9%** |
| `table-stream` | 21.4 | 22.4 | **+4.3%** |
| `text-typing` | 16.1 | 17.5 | **+8.9%** |
| `text-log-stream` | 16.1 | 16.3 | ~+1.5% |
| `text-relabel` | 35.4 | 37.1 | **+4.9%** |
| `scroll-uniform-list` | 12.8 | 17.9 | **+40%** |
| `scroll-variable-list` | 18.1 | 21.9 | **+21%** |
| `scroll-div-cards` | 41.1 | 59.4 | **+44%** |
| `scroll-data-table` | 23.0 | 39.1 | **+70%** |
| `hover-grid` | 48.0 | 50.4 | **+4.9%** |
| `tree-toggle` | 25.2 | 28.6 | **+13%** |
| `animated-dots` | 13.8 | 14.5 | **+5.0%** |
| `effects-cards` | 20.9 | 21.0 | ~+0.5% |

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
| `scroll-uniform-list` | 2572 | 756 | -71% |
| `scroll-variable-list` | 753 | 423 | -44% |
| `scroll-div-cards` | 94459 | 6017 | -94% |
| `scroll-data-table` | 8333 | 1847 | -78% |
| `hover-grid` | 35415 | 21210 | -40% |
| `tree-toggle` | 28145 | 17302 | -39% |
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
| `scroll-div-cards` | 65198 | 3944 | -94% |
| `scroll-data-table` | 3588 | 2349 | -35% |
| `hover-grid` | 18468 | 10795 | -42% |
| `tree-toggle` | 16788 | 7841 | -53% |
| `animated-dots` | 2921 | 2993 | ~+2.5% |
| `effects-cards` | 12363 | 637 | -95% |

**Startup: window opened to first frame presented** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 70.9 | 73.4 | ~+3.6% |
| `dashboard-one-panel` | 74.5 | 72.8 | ~-2.3% |
| `dashboard-all-panels` | 71.4 | 66.1 | ~-7.4% |
| `table-stream` | 67.0 | 65.0 | ~-2.9% |
| `text-typing` | 67.0 | 73.9 | ~+10% |
| `text-log-stream` | 71.8 | 65.1 | ~-9.4% |
| `text-relabel` | 71.7 | 72.6 | ~+1.3% |
| `scroll-uniform-list` | 66.6 | 58.0 | ~-13% |
| `scroll-variable-list` | 67.1 | 65.3 | ~-2.7% |
| `scroll-div-cards` | 91.4 | 90.3 | ~-1.2% |
| `scroll-data-table` | 64.7 | 68.3 | ~+5.5% |
| `hover-grid` | 78.3 | 74.0 | ~-5.5% |
| `tree-toggle` | 64.9 | 71.3 | ~+10% |
| `animated-dots` | 68.9 | 64.5 | ~-6.4% |
| `effects-cards` | 69.9 | 64.6 | ~-7.5% |

**Startup: the first frame's draw** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 9.7 | 10.5 | **+7.5%** |
| `dashboard-one-panel` | 10.3 | 10.5 | ~+1.5% |
| `dashboard-all-panels` | 9.8 | 10.6 | **+8.5%** |
| `table-stream` | 7.7 | 8.2 | **+6.7%** |
| `text-typing` | 10.3 | 11.0 | **+6.8%** |
| `text-log-stream` | 4.7 | 4.6 | ~-0.4% |
| `text-relabel` | 10.7 | 11.1 | ~+3.4% |
| `scroll-uniform-list` | 3.9 | 4.1 | **+5.5%** |
| `scroll-variable-list` | 4.6 | 4.7 | ~+1.9% |
| `scroll-div-cards` | 28.8 | 29.8 | ~+3.5% |
| `scroll-data-table` | 5.7 | 6.2 | **+7.7%** |
| `hover-grid` | 9.9 | 10.2 | ~+3.8% |
| `tree-toggle` | 8.9 | 9.5 | ~+7.4% |
| `animated-dots` | 1.4 | 1.6 | **+19%** |
| `effects-cards` | 7.8 | 8.3 | ~+6.3% |
