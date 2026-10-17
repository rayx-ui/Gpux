# gpux benchmark run `2026-10-16-macos-loop-part-3`

- Machine: Apple M5, 24 GB, macOS 26.6.2 (macOS-26.6.2-arm64-arm-64bit)
- gpux commit: `d805fcd72`
- Every run: 120 warm-up frames, then 600 measured frames; 3 repetitions, interleaved
- `off` and `on` are the same build with every fast-mode mechanism off or on (`Window::set_fast_mode`).
- A cell is the median over the repetitions. A change marked `~` is within the repetitions' spread or under 3% (noise); **bold** marks a change for the worse.

## Summary

Geometric means over the scenarios of the ratio of medians.

| Target | Metric | on / off | wins | losses | within noise |
|---|---|---:|---:|---:|---:|
| macOS native (Metal) | CPU ms per frame | 0.699 (-30.1%) | 12 | 0 | 3 |
| macOS native (Metal) | Window::draw p50 | 0.597 (-40.3%) | 13 | 0 | 2 |

## macOS native (Metal)

viewport 1280×801 @2x; display interval ≈8.3 ms; 90 of 90 runs reported, 9.5 min

**Frame cost: CPU** (ms of main-thread CPU per frame, mean)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.20 | 1.77 | -58% |
| `dashboard-one-panel` | 4.22 | 2.63 | -38% |
| `dashboard-all-panels` | 5.19 | 4.64 | -11% |
| `table-stream` | 3.29 | 2.84 | -14% |
| `text-typing` | 2.84 | 2.04 | -28% |
| `text-log-stream` | 2.82 | 2.78 | ~-1.4% |
| `text-relabel` | 6.68 | 6.10 | -8.7% |
| `scroll-uniform-list` | 2.22 | 1.39 | -37% |
| `scroll-variable-list` | 1.74 | 1.11 | -36% |
| `scroll-div-cards` | 11.9 | 3.30 | -72% |
| `scroll-data-table` | 2.84 | 2.78 | ~-2.0% |
| `hover-grid` | 5.54 | 4.56 | -18% |
| `tree-toggle` | 4.22 | 3.71 | -12% |
| `animated-dots` | 2.89 | 2.92 | ~+1.0% |
| `effects-cards` | 3.55 | 1.59 | -55% |

**Window::draw, median** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.04 | 1.32 | -67% |
| `dashboard-one-panel` | 4.06 | 2.24 | -45% |
| `dashboard-all-panels` | 4.99 | 4.48 | -10% |
| `table-stream` | 3.20 | 2.52 | -21% |
| `text-typing` | 2.77 | 1.61 | -42% |
| `text-log-stream` | 2.57 | 2.68 | ~+4.3% |
| `text-relabel` | 6.48 | 5.89 | -9.2% |
| `scroll-uniform-list` | 1.92 | 0.88 | -54% |
| `scroll-variable-list` | 1.39 | 0.61 | -56% |
| `scroll-div-cards` | 11.5 | 2.18 | -81% |
| `scroll-data-table` | 2.76 | 2.07 | -25% |
| `hover-grid` | 5.19 | 4.27 | -18% |
| `tree-toggle` | 4.07 | 2.72 | -33% |
| `animated-dots` | 2.61 | 2.71 | ~+4.1% |
| `effects-cards` | 3.43 | 1.31 | -62% |

**GPU time per frame** (ms, Metal command buffer start to end (macOS only))

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 3.17 | 2.55 | ~-19% |
| `dashboard-one-panel` | 3.24 | 3.14 | ~-3.1% |
| `dashboard-all-panels` | 2.02 | 2.61 | **+30%** |
| `table-stream` | 2.52 | 2.86 | ~+14% |
| `text-typing` | 1.62 | 1.97 | ~+22% |
| `text-log-stream` | 2.81 | 2.84 | ~+1.2% |
| `text-relabel` | 2.08 | 1.78 | -14% |
| `scroll-uniform-list` | 1.67 | 2.75 | **+65%** |
| `scroll-variable-list` | 2.07 | 3.00 | **+45%** |
| `scroll-div-cards` | 2.07 | 2.83 | **+36%** |
| `scroll-data-table` | 1.93 | 3.17 | **+64%** |
| `hover-grid` | 3.87 | 2.85 | ~-26% |
| `tree-toggle` | 1.80 | 1.69 | ~-6.1% |
| `animated-dots` | 1.57 | 1.69 | ~+7.9% |
| `effects-cards` | 3.89 | 5.37 | **+38%** |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.22 | 2.16 | -49% |
| `dashboard-one-panel` | 4.21 | 3.34 | ~-21% |
| `dashboard-all-panels` | 5.29 | 4.66 | -12% |
| `table-stream` | 3.35 | 3.58 | ~+6.8% |
| `text-typing` | 3.29 | 2.51 | ~-24% |
| `text-log-stream` | 3.15 | 3.12 | ~-1.0% |
| `text-relabel` | 6.77 | 6.27 | -7.4% |
| `scroll-uniform-list` | 2.29 | 1.49 | ~-35% |
| `scroll-variable-list` | 1.97 | 1.21 | -38% |
| `scroll-div-cards` | 12.0 | 2.99 | -75% |
| `scroll-data-table` | 2.95 | 2.85 | ~-3.3% |
| `hover-grid` | 5.41 | 4.37 | -19% |
| `tree-toggle` | 4.44 | 5.62 | **+27%** |
| `animated-dots` | 3.20 | 3.46 | **+8.3%** |
| `effects-cards` | 3.51 | 1.53 | -56% |

**Frame rate** (frames per second)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 120 | 120 | ~-0.0% |
| `dashboard-one-panel` | 120 | 120 | ~+0.0% |
| `dashboard-all-panels` | 120 | 120 | ~+0.0% |
| `table-stream` | 120 | 120 | ~-0.0% |
| `text-typing` | 120 | 120 | ~+0.0% |
| `text-log-stream` | 120 | 120 | ~+0.0% |
| `text-relabel` | 120 | 120 | ~-0.0% |
| `scroll-uniform-list` | 120 | 120 | ~-0.0% |
| `scroll-variable-list` | 120 | 120 | ~-0.0% |
| `scroll-div-cards` | 84.2 | 119 | +42% |
| `scroll-data-table` | 120 | 120 | ~+0.0% |
| `hover-grid` | 120 | 120 | ~+0.0% |
| `tree-toggle` | 120 | 120 | ~+0.0% |
| `animated-dots` | 120 | 120 | ~-0.1% |
| `effects-cards` | 120 | 120 | ~+0.0% |

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
| `scroll-variable-list` | 1 | 1 | ~+0.00 |
| `scroll-div-cards` | 0 | 11.0 | **+11.00** |
| `scroll-data-table` | 0 | 2 | ~+2.00 |
| `hover-grid` | 0 | 0 | ~+0.00 |
| `tree-toggle` | 0 | 0 | ~+0.00 |
| `animated-dots` | 0 | 1 | ~+1.00 |
| `effects-cards` | 0 | 0 | ~+0.00 |

**Process CPU** (% of one core, all threads)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 52.6 | 26.5 | -50% |
| `dashboard-one-panel` | 52.8 | 37.0 | -30% |
| `dashboard-all-panels` | 65.0 | 58.4 | -10% |
| `table-stream` | 41.8 | 38.9 | -7.0% |
| `text-typing` | 37.8 | 30.3 | -20% |
| `text-log-stream` | 38.9 | 37.6 | ~-3.3% |
| `text-relabel` | 82.2 | 75.6 | -8.1% |
| `scroll-uniform-list` | 32.2 | 23.5 | -27% |
| `scroll-variable-list` | 26.5 | 20.7 | -22% |
| `scroll-div-cards` | 101 | 43.9 | -57% |
| `scroll-data-table` | 36.8 | 40.3 | **+10%** |
| `hover-grid` | 69.5 | 57.7 | -17% |
| `tree-toggle` | 52.8 | 47.2 | -11% |
| `animated-dots` | 39.1 | 38.6 | ~-1.3% |
| `effects-cards` | 44.9 | 24.0 | -46% |

**Memory footprint at the end** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 134 | 138 | ~+2.7% |
| `dashboard-one-panel` | 135 | 138 | ~+2.7% |
| `dashboard-all-panels` | 135 | 139 | **+3.1%** |
| `table-stream` | 134 | 136 | ~+2.1% |
| `text-typing` | 130 | 131 | ~+1.2% |
| `text-log-stream` | 130 | 130 | ~-0.1% |
| `text-relabel` | 146 | 130 | ~-11% |
| `scroll-uniform-list` | 126 | 160 | **+27%** |
| `scroll-variable-list` | 135 | 169 | **+26%** |
| `scroll-div-cards` | 151 | 210 | **+39%** |
| `scroll-data-table` | 134 | 192 | **+43%** |
| `hover-grid` | 157 | 157 | ~+0.4% |
| `tree-toggle` | 136 | 140 | **+3.4%** |
| `animated-dots` | 125 | 125 | ~+0.7% |
| `effects-cards` | 133 | 136 | ~+2.3% |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | -1.62 | -1.50 | ~+0.12 |
| `dashboard-one-panel` | -1.06 | -1.12 | ~-0.06 |
| `dashboard-all-panels` | -1.08 | -0.69 | ~+0.39 |
| `table-stream` | -0.67 | -0.41 | ~+0.27 |
| `text-typing` | 0.11 | 0.03 | ~-0.08 |
| `text-log-stream` | 0.56 | 0.39 | ~-0.17 |
| `text-relabel` | 0.16 | -3.77 | -3.92 |
| `scroll-uniform-list` | 0.06 | 2.30 | **+2.23** |
| `scroll-variable-list` | 1.98 | -14.2 | -16.17 |
| `scroll-div-cards` | 10.9 | 0.97 | ~-9.89 |
| `scroll-data-table` | -1.47 | 1.42 | ~+2.89 |
| `hover-grid` | -0.22 | -2.27 | ~-2.05 |
| `tree-toggle` | -0.47 | -0.39 | ~+0.08 |
| `animated-dots` | -0.92 | -0.81 | ~+0.11 |
| `effects-cards` | -1.06 | 0.09 | ~+1.16 |

**Peak live Rust heap** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 24.3 | 25.1 | **+3.4%** |
| `dashboard-one-panel` | 24.2 | 25.4 | **+4.9%** |
| `dashboard-all-panels` | 24.5 | 26.8 | **+9.1%** |
| `table-stream` | 21.3 | 22.4 | **+4.8%** |
| `text-typing` | 16.1 | 17.5 | **+8.7%** |
| `text-log-stream` | 16.1 | 16.3 | ~+1.5% |
| `text-relabel` | 34.4 | 36.1 | **+5.0%** |
| `scroll-uniform-list` | 12.7 | 17.8 | **+40%** |
| `scroll-variable-list` | 18.4 | 22.2 | **+21%** |
| `scroll-div-cards` | 38.6 | 56.9 | **+47%** |
| `scroll-data-table` | 22.0 | 39.0 | **+77%** |
| `hover-grid` | 47.3 | 49.4 | **+4.5%** |
| `tree-toggle` | 23.2 | 26.5 | **+14%** |
| `animated-dots` | 14.3 | 15.0 | **+5.0%** |
| `effects-cards` | 21.0 | 21.0 | ~-0.2% |

**Heap allocations** (per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 22166 | 191 | -99% |
| `dashboard-one-panel` | 22279 | 628 | -97% |
| `dashboard-all-panels` | 27529 | 17563 | -36% |
| `table-stream` | 15622 | 1687 | -89% |
| `text-typing` | 2816 | 470 | -83% |
| `text-log-stream` | 3661 | 3623 | ~-1.0% |
| `text-relabel` | 31696 | 20556 | -35% |
| `scroll-uniform-list` | 2158 | 697 | -68% |
| `scroll-variable-list` | 679 | 407 | -40% |
| `scroll-div-cards` | 86457 | 5864 | -93% |
| `scroll-data-table` | 7072 | 1600 | -77% |
| `hover-grid` | 29025 | 14814 | -49% |
| `tree-toggle` | 25262 | 12422 | -51% |
| `animated-dots` | 1030 | 1110 | **+7.8%** |
| `effects-cards` | 18655 | 543 | -97% |

**Bytes allocated** (KB per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 14037 | 376 | -97% |
| `dashboard-one-panel` | 14078 | 1304 | -91% |
| `dashboard-all-panels` | 16012 | 7855 | -51% |
| `table-stream` | 5562 | 529 | -90% |
| `text-typing` | 2197 | 711 | -68% |
| `text-log-stream` | 2662 | 2579 | -3.1% |
| `text-relabel` | 15356 | 3776 | -75% |
| `scroll-uniform-list` | 1103 | 677 | -39% |
| `scroll-variable-list` | 391 | 815 | **+109%** |
| `scroll-div-cards` | 61697 | 3880 | -94% |
| `scroll-data-table` | 3465 | 2325 | -33% |
| `hover-grid` | 13974 | 6302 | -55% |
| `tree-toggle` | 16674 | 7510 | -55% |
| `animated-dots` | 1553 | 1625 | **+4.7%** |
| `effects-cards` | 10693 | 631 | -94% |

**Startup: window opened to first frame presented** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 82.8 | 74.5 | ~-10% |
| `dashboard-one-panel` | 81.0 | 80.7 | ~-0.3% |
| `dashboard-all-panels` | 79.6 | 78.9 | ~-0.8% |
| `table-stream` | 73.0 | 79.6 | ~+9.0% |
| `text-typing` | 78.5 | 82.7 | ~+5.4% |
| `text-log-stream` | 73.1 | 74.8 | ~+2.4% |
| `text-relabel` | 88.6 | 79.6 | ~-10% |
| `scroll-uniform-list` | 69.6 | 81.0 | **+16%** |
| `scroll-variable-list` | 89.8 | 77.9 | ~-13% |
| `scroll-div-cards` | 99.1 | 105 | ~+5.7% |
| `scroll-data-table` | 73.2 | 70.3 | ~-3.9% |
| `hover-grid` | 82.8 | 86.3 | ~+4.2% |
| `tree-toggle` | 80.3 | 82.8 | ~+3.0% |
| `animated-dots` | 76.7 | 72.5 | ~-5.5% |
| `effects-cards` | 77.3 | 83.3 | ~+7.7% |

**Startup: the first frame's draw** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 10.2 | 10.5 | ~+2.8% |
| `dashboard-one-panel` | 10.0 | 10.3 | ~+3.1% |
| `dashboard-all-panels` | 9.9 | 10.9 | **+10%** |
| `table-stream` | 7.8 | 8.4 | **+6.8%** |
| `text-typing` | 10.7 | 11.1 | ~+3.6% |
| `text-log-stream` | 4.8 | 4.9 | ~+0.8% |
| `text-relabel` | 11.4 | 10.9 | ~-4.5% |
| `scroll-uniform-list` | 4.2 | 4.3 | ~+3.9% |
| `scroll-variable-list` | 5.0 | 5.2 | ~+2.8% |
| `scroll-div-cards` | 29.1 | 30.3 | ~+4.0% |
| `scroll-data-table` | 6.2 | 6.1 | ~-0.6% |
| `hover-grid` | 9.6 | 9.9 | ~+3.2% |
| `tree-toggle` | 9.2 | 9.8 | ~+6.3% |
| `animated-dots` | 1.3 | 1.5 | **+12%** |
| `effects-cards` | 8.0 | 8.3 | **+4.1%** |
