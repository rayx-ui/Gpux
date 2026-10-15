# gpux benchmark run `2026-10-15-pr3`

- Machine: Apple M5, 24 GB, macOS 26.6.2 (macOS-26.6.2-arm64-arm-64bit)
- gpux commit: `0d343b5fa`
- Every run: 120 warm-up frames, then 600 measured frames; 3 repetitions, interleaved
- `off` and `on` are the same build with every fast-mode mechanism off or on (`Window::set_fast_mode`).
- A cell is the median over the repetitions. A change marked `~` is within the repetitions' spread or under 3% (noise); **bold** marks a change for the worse.

## Summary

Geometric means over the scenarios of the ratio of medians.

| Target | Metric | on / off | wins | losses | within noise |
|---|---|---:|---:|---:|---:|
| iOS Simulator (Metal) | CPU ms per frame | 0.821 (-17.9%) | 8 | 0 | 7 |
| iOS Simulator (Metal) | Window::draw p50 | 0.673 (-32.7%) | 12 | 1 | 2 |
| macOS native (Metal) | CPU ms per frame | 0.814 (-18.6%) | 7 | 2 | 6 |
| macOS native (Metal) | Window::draw p50 | 0.777 (-22.3%) | 8 | 1 | 6 |
| WASM in Chromium, WebGL2 | CPU ms per frame | 0.645 (-35.5%) | 11 | 0 | 4 |
| WASM in Chromium, WebGL2 | Window::draw p50 | 0.561 (-43.9%) | 11 | 0 | 4 |
| WASM in Chromium, WebGPU | CPU ms per frame | 0.741 (-25.9%) | 11 | 1 | 3 |
| WASM in Chromium, WebGPU | Window::draw p50 | 0.684 (-31.6%) | 11 | 2 | 2 |

## iOS Simulator (Metal)

viewport 402×874 @3x; display interval ≈16.1 ms; iPhone 17 Pro (iOS-26-5); 90 of 90 runs reported

**Frame cost: CPU** (ms of main-thread CPU per frame, mean)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 5.65 | 3.17 | -44% |
| `dashboard-one-panel` | 5.71 | 4.02 | -30% |
| `dashboard-all-panels` | 7.51 | 7.32 | ~-2.6% |
| `table-stream` | 4.89 | 4.50 | -7.9% |
| `text-typing` | 4.36 | 3.41 | -22% |
| `text-log-stream` | 4.04 | 4.15 | ~+2.7% |
| `text-relabel` | 10.7 | 10.5 | ~-1.7% |
| `scroll-uniform-list` | 4.57 | 2.97 | -35% |
| `scroll-variable-list` | 2.92 | 2.96 | ~+1.3% |
| `scroll-div-cards` | 14.1 | 6.48 | -54% |
| `scroll-data-table` | 4.42 | 4.11 | ~-7.1% |
| `hover-grid` | 10.3 | 9.41 | -9.1% |
| `tree-toggle` | 6.02 | 5.79 | ~-3.9% |
| `animated-dots` | 4.31 | 4.30 | ~-0.2% |
| `effects-cards` | 4.80 | 3.63 | -24% |

**Window::draw, median** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 5.09 | 2.17 | -57% |
| `dashboard-one-panel` | 5.15 | 3.38 | -34% |
| `dashboard-all-panels` | 7.04 | 6.82 | -3.1% |
| `table-stream` | 4.42 | 3.87 | -12% |
| `text-typing` | 3.68 | 2.43 | -34% |
| `text-log-stream` | 2.93 | 3.16 | **+8.0%** |
| `text-relabel` | 10.2 | 10.0 | ~-2.0% |
| `scroll-uniform-list` | 3.75 | 1.36 | -64% |
| `scroll-variable-list` | 1.54 | 0.77 | -50% |
| `scroll-div-cards` | 13.1 | 5.12 | -61% |
| `scroll-data-table` | 3.80 | 2.93 | -23% |
| `hover-grid` | 7.76 | 6.59 | -15% |
| `tree-toggle` | 5.25 | 4.35 | -17% |
| `animated-dots` | 3.61 | 3.67 | ~+1.6% |
| `effects-cards` | 4.13 | 1.71 | -59% |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 5.37 | 2.57 | -52% |
| `dashboard-one-panel` | 5.35 | 3.77 | -30% |
| `dashboard-all-panels` | 7.19 | 7.00 | ~-2.7% |
| `table-stream` | 4.70 | 4.54 | ~-3.4% |
| `text-typing` | 3.98 | 2.81 | -29% |
| `text-log-stream` | 3.32 | 3.45 | ~+3.9% |
| `text-relabel` | 10.6 | 10.5 | ~-0.9% |
| `scroll-uniform-list` | 4.74 | 3.18 | -33% |
| `scroll-variable-list` | 2.36 | 2.46 | ~+4.2% |
| `scroll-div-cards` | 13.6 | 5.75 | -58% |
| `scroll-data-table` | 4.17 | 4.05 | ~-2.8% |
| `hover-grid` | 8.09 | 6.96 | -14% |
| `tree-toggle` | 5.83 | 7.41 | **+27%** |
| `animated-dots` | 4.23 | 4.59 | **+8.6%** |
| `effects-cards` | 4.36 | 2.09 | -52% |

**Frame rate** (frames per second)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 60.0 | 60.0 | ~-0.0% |
| `dashboard-one-panel` | 60.0 | 60.0 | ~+0.0% |
| `dashboard-all-panels` | 60.0 | 60.0 | ~-0.0% |
| `table-stream` | 60.0 | 60.0 | ~+0.0% |
| `text-typing` | 60.0 | 60.0 | ~-0.0% |
| `text-log-stream` | 60.0 | 60.0 | ~+0.0% |
| `text-relabel` | 60.0 | 60.0 | ~+0.0% |
| `scroll-uniform-list` | 60.0 | 59.9 | ~-0.2% |
| `scroll-variable-list` | 60.0 | 60.0 | ~-0.0% |
| `scroll-div-cards` | 60.0 | 60.0 | ~+0.0% |
| `scroll-data-table` | 60.0 | 60.0 | ~+0.0% |
| `hover-grid` | 60.0 | 60.0 | ~+0.0% |
| `tree-toggle` | 60.0 | 60.0 | ~-0.0% |
| `animated-dots` | 60.0 | 60.0 | ~-0.0% |
| `effects-cards` | 60.0 | 60.0 | ~+0.0% |

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
| `scroll-uniform-list` | 0 | 1 | ~+1.00 |
| `scroll-variable-list` | 0 | 0 | ~+0.00 |
| `scroll-div-cards` | 0 | 0 | ~+0.00 |
| `scroll-data-table` | 0 | 1 | **+1.00** |
| `hover-grid` | 0 | 0 | ~+0.00 |
| `tree-toggle` | 0 | 0 | ~+0.00 |
| `animated-dots` | 0 | 0 | ~+0.00 |
| `effects-cards` | 0 | 0 | ~+0.00 |

**Process CPU** (% of one core, all threads)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 37.5 | 26.3 | -30% |
| `dashboard-one-panel` | 37.9 | 30.0 | -21% |
| `dashboard-all-panels` | 48.9 | 47.7 | ~-2.3% |
| `table-stream` | 33.4 | 32.7 | ~-2.1% |
| `text-typing` | 31.3 | 28.0 | -11% |
| `text-log-stream` | 30.3 | 30.7 | ~+1.3% |
| `text-relabel` | 67.5 | 66.5 | ~-1.6% |
| `scroll-uniform-list` | 33.8 | 24.2 | -28% |
| `scroll-variable-list` | 25.9 | 26.4 | ~+1.7% |
| `scroll-div-cards` | 87.8 | 43.4 | -51% |
| `scroll-data-table` | 31.1 | 29.9 | ~-4.1% |
| `hover-grid` | 67.5 | 61.9 | -8.3% |
| `tree-toggle` | 39.6 | 38.2 | ~-3.5% |
| `animated-dots` | 30.8 | 30.6 | ~-0.7% |
| `effects-cards` | 32.9 | 31.7 | ~-3.5% |

**Memory footprint at the end** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 37.9 | 40.7 | **+7.5%** |
| `dashboard-one-panel` | 37.9 | 40.6 | **+7.1%** |
| `dashboard-all-panels` | 38.2 | 40.1 | **+5.0%** |
| `table-stream` | 36.9 | 37.8 | ~+2.5% |
| `text-typing` | 33.4 | 34.5 | ~+3.3% |
| `text-log-stream` | 31.6 | 31.9 | ~+1.0% |
| `text-relabel` | 43.1 | 45.2 | **+4.8%** |
| `scroll-uniform-list` | 30.8 | 50.2 | **+63%** |
| `scroll-variable-list` | 32.6 | 35.9 | **+10%** |
| `scroll-div-cards` | 57.8 | 66.7 | **+15%** |
| `scroll-data-table` | 35.2 | 102 | **+189%** |
| `hover-grid` | 51.9 | 52.5 | ~+1.2% |
| `tree-toggle` | 42.2 | 46.5 | **+10%** |
| `animated-dots` | 33.2 | 33.0 | ~-0.6% |
| `effects-cards` | 36.9 | 37.0 | ~+0.1% |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 0.05 | -0.22 | ~-0.27 |
| `dashboard-one-panel` | 0.00 | -0.11 | ~-0.11 |
| `dashboard-all-panels` | -0.06 | -0.16 | ~-0.09 |
| `table-stream` | -0.09 | -0.11 | ~-0.02 |
| `text-typing` | -0.11 | -0.14 | ~-0.03 |
| `text-log-stream` | -0.09 | -0.12 | ~-0.03 |
| `text-relabel` | -0.62 | -0.67 | ~-0.05 |
| `scroll-uniform-list` | -0.02 | 9.28 | **+9.30** |
| `scroll-variable-list` | 0.17 | 1.36 | **+1.19** |
| `scroll-div-cards` | -0.08 | -0.25 | ~-0.17 |
| `scroll-data-table` | 0.03 | 37.6 | **+37.53** |
| `hover-grid` | 0.30 | -0.08 | -0.38 |
| `tree-toggle` | -0.12 | -2.20 | -2.08 |
| `animated-dots` | -0.12 | -0.08 | ~+0.05 |
| `effects-cards` | 0.16 | -0.03 | ~-0.19 |

**Peak live Rust heap** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 28.0 | 30.0 | **+7.2%** |
| `dashboard-one-panel` | 28.2 | 30.2 | **+7.1%** |
| `dashboard-all-panels` | 28.5 | 31.3 | **+10%** |
| `table-stream` | 22.1 | 23.0 | **+4.2%** |
| `text-typing` | 16.1 | 17.5 | **+9.0%** |
| `text-log-stream` | 15.4 | 16.0 | **+3.9%** |
| `text-relabel` | 34.4 | 36.4 | **+5.8%** |
| `scroll-uniform-list` | 13.8 | 47.7 | **+246%** |
| `scroll-variable-list` | 15.5 | 27.9 | **+80%** |
| `scroll-div-cards` | 50.2 | 63.3 | **+26%** |
| `scroll-data-table` | 22.5 | 124 | **+450%** |
| `hover-grid` | 53.0 | 55.3 | **+4.5%** |
| `tree-toggle` | 34.5 | 39.7 | **+15%** |
| `animated-dots` | 17.0 | 17.7 | **+4.2%** |
| `effects-cards` | 22.9 | 23.2 | ~+1.2% |

**Heap allocations** (per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 28171 | 2964 | -89% |
| `dashboard-one-panel` | 28284 | 3643 | -87% |
| `dashboard-all-panels` | 33534 | 28834 | -14% |
| `table-stream` | 17693 | 4881 | -72% |
| `text-typing` | 3577 | 1107 | -69% |
| `text-log-stream` | 4622 | 5462 | **+18%** |
| `text-relabel` | 43243 | 39569 | -8.5% |
| `scroll-uniform-list` | 3423 | 5099 | **+49%** |
| `scroll-variable-list` | 661 | 1110 | **+68%** |
| `scroll-div-cards` | 104257 | 28050 | -73% |
| `scroll-data-table` | 10608 | 13805 | **+30%** |
| `hover-grid` | 49958 | 38916 | -22% |
| `tree-toggle` | 32193 | 22622 | -30% |
| `animated-dots` | 3163 | 3243 | ~+2.5% |
| `effects-cards` | 22964 | 2021 | -91% |

**Bytes allocated** (KB per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 17236 | 356 | -98% |
| `dashboard-one-panel` | 17277 | 1367 | -92% |
| `dashboard-all-panels` | 19210 | 11517 | -40% |
| `table-stream` | 5827 | 651 | -89% |
| `text-typing` | 2306 | 808 | -65% |
| `text-log-stream` | 2932 | 2924 | ~-0.3% |
| `text-relabel` | 15880 | 4939 | -69% |
| `scroll-uniform-list` | 1456 | 1040 | -29% |
| `scroll-variable-list` | 290 | 848 | **+193%** |
| `scroll-div-cards` | 70781 | 10691 | -85% |
| `scroll-data-table` | 4029 | 2198 | -45% |
| `hover-grid` | 33422 | 25771 | -23% |
| `tree-toggle` | 18202 | 9111 | -50% |
| `animated-dots` | 4319 | 4386 | ~+1.6% |
| `effects-cards` | 13642 | 615 | -95% |

**Startup: window opened to first frame presented** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 73.5 | 65.3 | ~-11% |
| `dashboard-one-panel` | 65.5 | 73.5 | ~+12% |
| `dashboard-all-panels` | 71.8 | 68.7 | ~-4.3% |
| `table-stream` | 70.5 | 65.3 | ~-7.3% |
| `text-typing` | 58.5 | 68.6 | **+17%** |
| `text-log-stream` | 71.2 | 69.3 | ~-2.7% |
| `text-relabel` | 68.7 | 74.0 | ~+7.6% |
| `scroll-uniform-list` | 73.6 | 66.4 | ~-10% |
| `scroll-variable-list` | 63.7 | 62.4 | ~-2.0% |
| `scroll-div-cards` | 88.6 | 91.2 | ~+3.0% |
| `scroll-data-table` | 74.1 | 67.9 | ~-8.4% |
| `hover-grid` | 69.8 | 67.2 | ~-3.8% |
| `tree-toggle` | 73.4 | 69.2 | ~-5.7% |
| `animated-dots` | 64.8 | 70.3 | ~+8.6% |
| `effects-cards` | 64.5 | 65.6 | ~+1.7% |

**Startup: the first frame's draw** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 11.4 | 13.7 | **+20%** |
| `dashboard-one-panel` | 10.6 | 11.8 | **+11%** |
| `dashboard-all-panels` | 10.8 | 12.1 | **+11%** |
| `table-stream` | 9.8 | 10.1 | ~+3.2% |
| `text-typing` | 12.8 | 10.8 | ~-16% |
| `text-log-stream` | 6.3 | 5.4 | ~-14% |
| `text-relabel` | 15.4 | 15.5 | ~+0.6% |
| `scroll-uniform-list` | 4.3 | 4.6 | ~+7.2% |
| `scroll-variable-list` | 4.3 | 4.2 | ~-4.1% |
| `scroll-div-cards` | 34.5 | 35.3 | ~+2.3% |
| `scroll-data-table` | 7.8 | 8.4 | ~+6.5% |
| `hover-grid` | 12.4 | 12.9 | ~+4.0% |
| `tree-toggle` | 10.6 | 11.7 | ~+10% |
| `animated-dots` | 1.6 | 1.8 | ~+14% |
| `effects-cards` | 11.3 | 8.9 | ~-21% |

## macOS native (Metal)

viewport 1280×801 @2x; display interval ≈8.3 ms; 90 of 90 runs reported

**Frame cost: CPU** (ms of main-thread CPU per frame, mean)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.51 | 3.12 | -31% |
| `dashboard-one-panel` | 4.58 | 2.87 | -37% |
| `dashboard-all-panels` | 7.11 | 6.64 | -6.6% |
| `table-stream` | 3.74 | 2.70 | -28% |
| `text-typing` | 2.90 | 3.20 | **+10%** |
| `text-log-stream` | 2.71 | 2.91 | ~+7.3% |
| `text-relabel` | 11.0 | 11.0 | ~-0.5% |
| `scroll-uniform-list` | 3.14 | 2.87 | ~-8.6% |
| `scroll-variable-list` | 2.90 | 1.77 | -39% |
| `scroll-div-cards` | 13.8 | 5.14 | -63% |
| `scroll-data-table` | 3.18 | 3.42 | **+7.8%** |
| `hover-grid` | 7.62 | 6.88 | -10% |
| `tree-toggle` | 4.60 | 4.56 | ~-0.9% |
| `animated-dots` | 3.11 | 2.91 | ~-6.4% |
| `effects-cards` | 3.64 | 2.58 | ~-29% |

**Window::draw, median** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.34 | 2.93 | -33% |
| `dashboard-one-panel` | 4.43 | 2.60 | -41% |
| `dashboard-all-panels` | 6.88 | 6.40 | -7.0% |
| `table-stream` | 3.58 | 2.57 | -28% |
| `text-typing` | 2.72 | 2.95 | **+8.3%** |
| `text-log-stream` | 2.43 | 2.81 | ~+16% |
| `text-relabel` | 10.8 | 10.8 | ~-0.2% |
| `scroll-uniform-list` | 2.74 | 2.62 | ~-4.3% |
| `scroll-variable-list` | 2.62 | 1.10 | -58% |
| `scroll-div-cards` | 13.2 | 4.67 | -65% |
| `scroll-data-table` | 2.98 | 3.04 | ~+2.2% |
| `hover-grid` | 6.96 | 6.06 | -13% |
| `tree-toggle` | 4.38 | 3.60 | -18% |
| `animated-dots` | 2.69 | 2.64 | ~-1.9% |
| `effects-cards` | 3.57 | 2.38 | ~-33% |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 4.49 | 3.70 | -17% |
| `dashboard-one-panel` | 4.57 | 3.37 | -26% |
| `dashboard-all-panels` | 7.13 | 6.67 | -6.4% |
| `table-stream` | 3.86 | 3.16 | ~-18% |
| `text-typing` | 3.45 | 3.60 | ~+4.5% |
| `text-log-stream` | 3.21 | 3.33 | ~+3.7% |
| `text-relabel` | 11.3 | 11.1 | ~-2.0% |
| `scroll-uniform-list` | 4.07 | 3.77 | -7.2% |
| `scroll-variable-list` | 3.42 | 2.42 | -29% |
| `scroll-div-cards` | 13.7 | 4.91 | -64% |
| `scroll-data-table` | 3.35 | 4.05 | **+21%** |
| `hover-grid` | 7.25 | 6.39 | -12% |
| `tree-toggle` | 4.81 | 6.38 | **+33%** |
| `animated-dots` | 3.58 | 3.30 | -8.0% |
| `effects-cards` | 3.70 | 2.89 | -22% |

**Frame rate** (frames per second)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 120 | 120 | ~-0.0% |
| `dashboard-one-panel` | 120 | 120 | ~-0.0% |
| `dashboard-all-panels` | 120 | 120 | ~+0.0% |
| `table-stream` | 120 | 120 | ~-0.1% |
| `text-typing` | 120 | 120 | ~+0.1% |
| `text-log-stream` | 120 | 120 | ~+0.1% |
| `text-relabel` | 90.7 | 91.2 | ~+0.5% |
| `scroll-uniform-list` | 120 | 120 | ~-0.2% |
| `scroll-variable-list` | 120 | 120 | ~-0.2% |
| `scroll-div-cards` | 72.5 | 120 | +66% |
| `scroll-data-table` | 120 | 119 | ~-0.6% |
| `hover-grid` | 120 | 120 | ~-0.0% |
| `tree-toggle` | 120 | 120 | ~+0.0% |
| `animated-dots` | 120 | 120 | ~-0.0% |
| `effects-cards` | 120 | 120 | ~-0.0% |

**Missed frames** (intervals over 1.5× the display's, of the measured frames; changes in frames)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 1 | 1 | ~+0.00 |
| `dashboard-one-panel` | 0 | 1 | ~+1.00 |
| `dashboard-all-panels` | 1 | 1 | ~+0.00 |
| `table-stream` | 0 | 1 | ~+1.00 |
| `text-typing` | 1 | 2 | ~+1.00 |
| `text-log-stream` | 1 | 2 | ~+1.00 |
| `text-relabel` | 0 | 0 | ~+0.00 |
| `scroll-uniform-list` | 1 | 2 | ~+1.00 |
| `scroll-variable-list` | 1 | 1 | ~+0.00 |
| `scroll-div-cards` | 0 | 1 | **+1.00** |
| `scroll-data-table` | 1 | 3 | ~+2.00 |
| `hover-grid` | 1 | 0 | ~-1.00 |
| `tree-toggle` | 0 | 0 | ~+0.00 |
| `animated-dots` | 1 | 1 | ~+0.00 |
| `effects-cards` | 0 | 1 | **+1.00** |

**Process CPU** (% of one core, all threads)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 56.3 | 43.8 | -22% |
| `dashboard-one-panel` | 57.0 | 39.3 | -31% |
| `dashboard-all-panels` | 87.9 | 82.4 | -6.3% |
| `table-stream` | 47.5 | 36.0 | -24% |
| `text-typing` | 38.7 | 45.2 | **+17%** |
| `text-log-stream` | 36.8 | 39.4 | ~+6.9% |
| `text-relabel` | 102 | 101 | ~-0.1% |
| `scroll-uniform-list` | 44.1 | 41.7 | ~-5.4% |
| `scroll-variable-list` | 41.7 | 29.3 | -30% |
| `scroll-div-cards` | 101 | 64.2 | -37% |
| `scroll-data-table` | 41.2 | 45.2 | **+10%** |
| `hover-grid` | 94.4 | 85.6 | -9.3% |
| `tree-toggle` | 57.4 | 57.0 | ~-0.7% |
| `animated-dots` | 41.8 | 38.2 | ~-8.6% |
| `effects-cards` | 45.9 | 36.3 | ~-21% |

**Memory footprint at the end** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 139 | 149 | **+7.1%** |
| `dashboard-one-panel` | 141 | 150 | **+5.8%** |
| `dashboard-all-panels` | 141 | 144 | ~+2.0% |
| `table-stream` | 143 | 137 | ~-3.6% |
| `text-typing` | 130 | 139 | **+6.7%** |
| `text-log-stream` | 131 | 139 | ~+6.1% |
| `text-relabel` | 151 | 151 | ~+0.4% |
| `scroll-uniform-list` | 133 | 237 | **+78%** |
| `scroll-variable-list` | 141 | 221 | **+56%** |
| `scroll-div-cards` | 165 | 256 | **+55%** |
| `scroll-data-table` | 142 | 317 | **+124%** |
| `hover-grid` | 170 | 175 | ~+2.7% |
| `tree-toggle` | 145 | 150 | **+3.6%** |
| `animated-dots` | 128 | 129 | ~+1.1% |
| `effects-cards` | 135 | 143 | **+5.8%** |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | -1.47 | -1.38 | ~+0.09 |
| `dashboard-one-panel` | 0.39 | -3.42 | ~-3.81 |
| `dashboard-all-panels` | -0.67 | -0.27 | ~+0.41 |
| `table-stream` | 6.31 | -0.95 | -7.27 |
| `text-typing` | 0.00 | 2.09 | ~+2.09 |
| `text-log-stream` | 0.41 | 7.44 | ~+7.03 |
| `text-relabel` | 0.16 | -1.88 | ~-2.03 |
| `scroll-uniform-list` | 7.11 | 18.2 | **+11.08** |
| `scroll-variable-list` | 5.94 | 3.14 | ~-2.80 |
| `scroll-div-cards` | 0.11 | 3.50 | **+3.39** |
| `scroll-data-table` | 5.69 | 54.7 | **+49.03** |
| `hover-grid` | -0.69 | 0.97 | ~+1.66 |
| `tree-toggle` | -0.20 | -2.34 | -2.14 |
| `animated-dots` | 1.58 | -0.08 | ~-1.66 |
| `effects-cards` | -0.81 | -3.45 | -2.64 |

**Peak live Rust heap** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 29.4 | 30.8 | **+5.0%** |
| `dashboard-one-panel` | 29.5 | 31.1 | **+5.2%** |
| `dashboard-all-panels` | 29.9 | 32.6 | **+9.0%** |
| `table-stream` | 23.4 | 24.3 | **+3.9%** |
| `text-typing` | 16.6 | 18.1 | **+8.7%** |
| `text-log-stream` | 16.8 | 17.2 | ~+2.4% |
| `text-relabel` | 39.0 | 40.8 | **+4.5%** |
| `scroll-uniform-list` | 13.2 | 49.9 | **+277%** |
| `scroll-variable-list` | 18.1 | 42.6 | **+136%** |
| `scroll-div-cards` | 49.7 | 68.8 | **+38%** |
| `scroll-data-table` | 24.8 | 143 | **+476%** |
| `hover-grid` | 60.4 | 62.9 | **+4.1%** |
| `tree-toggle` | 32.4 | 37.7 | **+17%** |
| `animated-dots` | 14.7 | 15.4 | **+4.9%** |
| `effects-cards` | 23.5 | 23.4 | ~-0.5% |

**Heap allocations** (per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 27949 | 2811 | -90% |
| `dashboard-one-panel` | 28062 | 3421 | -88% |
| `dashboard-all-panels` | 33312 | 28611 | -14% |
| `table-stream` | 18952 | 4725 | -75% |
| `text-typing` | 3421 | 950 | -72% |
| `text-log-stream` | 4468 | 5306 | **+19%** |
| `text-relabel` | 37700 | 32545 | -14% |
| `scroll-uniform-list` | 3022 | 4185 | **+38%** |
| `scroll-variable-list` | 839 | 1037 | **+24%** |
| `scroll-div-cards` | 104062 | 27441 | -74% |
| `scroll-data-table` | 9604 | 12174 | **+27%** |
| `hover-grid` | 49794 | 37173 | -25% |
| `tree-toggle` | 32035 | 22611 | -29% |
| `animated-dots` | 3034 | 3114 | ~+2.6% |
| `effects-cards` | 22749 | 1998 | -91% |

**Bytes allocated** (KB per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 17277 | 403 | -98% |
| `dashboard-one-panel` | 17319 | 1408 | -92% |
| `dashboard-all-panels` | 19252 | 11557 | -40% |
| `table-stream` | 5847 | 627 | -89% |
| `text-typing` | 2284 | 785 | -66% |
| `text-log-stream` | 2911 | 2900 | ~-0.4% |
| `text-relabel` | 15492 | 4385 | -72% |
| `scroll-uniform-list` | 1331 | 1016 | -24% |
| `scroll-variable-list` | 445 | 706 | **+59%** |
| `scroll-div-cards` | 70760 | 10713 | -85% |
| `scroll-data-table` | 3705 | 2622 | -29% |
| `hover-grid` | 33768 | 26081 | -23% |
| `tree-toggle` | 18180 | 9187 | -49% |
| `animated-dots` | 4292 | 4360 | ~+1.6% |
| `effects-cards` | 13693 | 658 | -95% |

**Startup: window opened to first frame presented** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 68.4 | 67.3 | ~-1.6% |
| `dashboard-one-panel` | 71.5 | 66.9 | ~-6.3% |
| `dashboard-all-panels` | 74.0 | 65.8 | ~-11% |
| `table-stream` | 68.0 | 71.4 | ~+4.9% |
| `text-typing` | 80.8 | 73.1 | ~-10% |
| `text-log-stream` | 66.3 | 66.7 | ~+0.6% |
| `text-relabel` | 69.0 | 65.6 | ~-4.9% |
| `scroll-uniform-list` | 65.7 | 63.4 | ~-3.5% |
| `scroll-variable-list` | 76.3 | 69.3 | ~-9.2% |
| `scroll-div-cards` | 95.9 | 97.8 | ~+1.9% |
| `scroll-data-table` | 69.3 | 67.9 | ~-2.0% |
| `hover-grid` | 65.7 | 70.6 | ~+7.4% |
| `tree-toggle` | 65.0 | 66.3 | ~+1.9% |
| `animated-dots` | 61.2 | 59.5 | ~-2.8% |
| `effects-cards` | 66.8 | 68.2 | ~+2.1% |

**Startup: the first frame's draw** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 12.4 | 12.5 | ~+0.8% |
| `dashboard-one-panel` | 11.8 | 12.7 | ~+7.1% |
| `dashboard-all-panels` | 11.9 | 12.8 | ~+6.8% |
| `table-stream` | 10.3 | 11.1 | **+7.4%** |
| `text-typing` | 11.5 | 11.5 | ~+0.1% |
| `text-log-stream` | 5.8 | 5.8 | ~+0.2% |
| `text-relabel` | 15.5 | 16.1 | ~+3.4% |
| `scroll-uniform-list` | 4.8 | 4.8 | ~+0.9% |
| `scroll-variable-list` | 5.5 | 5.3 | ~-3.0% |
| `scroll-div-cards` | 35.4 | 36.2 | ~+2.3% |
| `scroll-data-table` | 8.2 | 8.6 | ~+4.1% |
| `hover-grid` | 12.4 | 13.1 | ~+6.3% |
| `tree-toggle` | 9.9 | 10.7 | ~+7.5% |
| `animated-dots` | 1.5 | 1.8 | **+14%** |
| `effects-cards` | 9.0 | 9.3 | ~+3.4% |

## WASM in Chromium, WebGL2

viewport 1280×800 @2x; display interval ≈9.0 ms; Chromium 147.0.7727.15, backend gl; 90 of 90 runs reported

**Frame cost: CPU** (ms of Chrome main-thread task time per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 5.75 | 2.39 | ~-58% |
| `dashboard-one-panel` | 5.81 | 2.27 | -61% |
| `dashboard-all-panels` | 8.09 | 6.96 | -14% |
| `table-stream` | 3.51 | 2.07 | -41% |
| `text-typing` | 2.49 | 1.99 | -20% |
| `text-log-stream` | 2.12 | 2.11 | ~-0.5% |
| `text-relabel` | 8.57 | 7.95 | -7.3% |
| `scroll-uniform-list` | 2.11 | 1.62 | -23% |
| `scroll-variable-list` | 2.02 | 1.31 | -35% |
| `scroll-div-cards` | 18.0 | 6.16 | -66% |
| `scroll-data-table` | 3.06 | 3.11 | ~+1.6% |
| `hover-grid` | 11.4 | 10.6 | -6.5% |
| `tree-toggle` | 5.59 | 5.24 | -6.3% |
| `animated-dots` | 2.42 | 2.54 | ~+4.9% |
| `effects-cards` | 5.31 | 0.77 | -86% |

**Window::draw, median** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 5.34 | 1.88 | ~-65% |
| `dashboard-one-panel` | 5.39 | 1.91 | -65% |
| `dashboard-all-panels` | 7.37 | 6.03 | -18% |
| `table-stream` | 3.21 | 1.78 | -45% |
| `text-typing` | 2.27 | 1.46 | -36% |
| `text-log-stream` | 1.85 | 1.90 | ~+2.4% |
| `text-relabel` | 8.10 | 7.41 | -8.5% |
| `scroll-uniform-list` | 1.70 | 1.10 | -35% |
| `scroll-variable-list` | 1.43 | 0.55 | -61% |
| `scroll-div-cards` | 17.3 | 5.17 | -70% |
| `scroll-data-table` | 2.80 | 2.57 | ~-8.1% |
| `hover-grid` | 9.63 | 8.84 | -8.2% |
| `tree-toggle` | 5.33 | 3.89 | -27% |
| `animated-dots` | 2.11 | 2.27 | ~+7.6% |
| `effects-cards` | 4.90 | 0.51 | -90% |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 5.49 | 2.44 | ~-56% |
| `dashboard-one-panel` | 5.89 | 2.49 | -58% |
| `dashboard-all-panels` | 8.57 | 8.42 | ~-1.7% |
| `table-stream` | 3.40 | 2.08 | -39% |
| `text-typing` | 2.45 | 2.25 | -8.0% |
| `text-log-stream` | 2.13 | 2.17 | ~+2.0% |
| `text-relabel` | 8.32 | 7.73 | -7.0% |
| `scroll-uniform-list` | 2.28 | 1.48 | -35% |
| `scroll-variable-list` | 2.20 | 1.30 | -41% |
| `scroll-div-cards` | 17.7 | 5.56 | -69% |
| `scroll-data-table` | 2.99 | 3.04 | ~+1.9% |
| `hover-grid` | 11.2 | 9.59 | -14% |
| `tree-toggle` | 5.76 | 6.96 | **+21%** |
| `animated-dots` | 2.44 | 2.59 | ~+6.2% |
| `effects-cards` | 5.11 | 0.69 | -86% |

**Frame rate** (frames per second)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 104 | 104 | ~-0.0% |
| `dashboard-one-panel` | 103 | 103 | ~-0.0% |
| `dashboard-all-panels` | 103 | 103 | ~-0.0% |
| `table-stream` | 120 | 120 | ~+0.0% |
| `text-typing` | 120 | 120 | ~+0.0% |
| `text-log-stream` | 120 | 120 | ~+0.0% |
| `text-relabel` | 116 | 120 | +3.3% |
| `scroll-uniform-list` | 120 | 120 | ~-0.0% |
| `scroll-variable-list` | 120 | 120 | ~+0.0% |
| `scroll-div-cards` | 55.3 | 120 | +117% |
| `scroll-data-table` | 117 | 119 | ~+2.3% |
| `hover-grid` | 87.5 | 93.8 | +7.2% |
| `tree-toggle` | 120 | 120 | ~-0.0% |
| `animated-dots` | 120 | 120 | ~+0.0% |
| `effects-cards` | 62.5 | 62.5 | ~-0.0% |

**Missed frames** (intervals over 1.5× the display's, of the measured frames; changes in frames)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 0 | 0 | ~+0.00 |
| `dashboard-one-panel` | 0 | 0 | ~+0.00 |
| `dashboard-all-panels` | 0 | 1 | ~+1.00 |
| `table-stream` | 0 | 0 | ~+0.00 |
| `text-typing` | 0 | 0 | ~+0.00 |
| `text-log-stream` | 0 | 0 | ~+0.00 |
| `text-relabel` | 0 | 0 | ~+0.00 |
| `scroll-uniform-list` | 0 | 1 | ~+1.00 |
| `scroll-variable-list` | 0 | 1 | **+1.00** |
| `scroll-div-cards` | 0 | 11.0 | **+11.00** |
| `scroll-data-table` | 0 | 2 | **+2.00** |
| `hover-grid` | 0 | 0 | ~+0.00 |
| `tree-toggle` | 0 | 0 | ~+0.00 |
| `animated-dots` | 0 | 0 | ~+0.00 |
| `effects-cards` | 0 | 0 | ~+0.00 |

**WASM linear memory at the end** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 53.6 | 56.1 | **+4.7%** |
| `dashboard-one-panel` | 54.4 | 57.0 | **+4.7%** |
| `dashboard-all-panels` | 54.7 | 57.4 | **+5.0%** |
| `table-stream` | 49.1 | 50.8 | **+3.4%** |
| `text-typing` | 44.2 | 45.4 | ~+2.5% |
| `text-log-stream` | 42.5 | 42.9 | ~+0.9% |
| `text-relabel` | 63.9 | 65.9 | **+3.1%** |
| `scroll-uniform-list` | 40.4 | 69.7 | **+73%** |
| `scroll-variable-list` | 43.4 | 59.2 | **+37%** |
| `scroll-div-cards` | 68.1 | 86.2 | **+27%** |
| `scroll-data-table` | 54.2 | 169 | **+213%** |
| `hover-grid` | 85.6 | 87.8 | ~+2.5% |
| `tree-toggle` | 54.0 | 59.8 | **+11%** |
| `animated-dots` | 41.2 | 42.1 | ~+2.1% |
| `effects-cards` | 48.9 | 49.8 | ~+1.8% |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 0.00 | 0.00 | ~+0.00 |
| `dashboard-one-panel` | 1.00 | 0.75 | ~-0.25 |
| `dashboard-all-panels` | 0.50 | 0.75 | ~+0.25 |
| `table-stream` | 0.00 | 0.31 | ~+0.31 |
| `text-typing` | 0.31 | 0.31 | ~+0.00 |
| `text-log-stream` | 0.19 | 0.56 | **+0.38** |
| `text-relabel` | 0.00 | 0.00 | ~+0.00 |
| `scroll-uniform-list` | 0.00 | 15.4 | **+15.44** |
| `scroll-variable-list` | 0.00 | 9.00 | **+9.00** |
| `scroll-div-cards` | 0.38 | 1.06 | **+0.69** |
| `scroll-data-table` | 2.62 | 65.4 | **+62.75** |
| `hover-grid` | 0.50 | 0.44 | ~-0.06 |
| `tree-toggle` | 0.00 | 0.00 | ~+0.00 |
| `animated-dots` | 0.31 | 0.62 | ~+0.31 |
| `effects-cards` | 0.31 | 0.38 | ~+0.06 |

**Peak live Rust heap** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 46.9 | 48.3 | **+3.0%** |
| `dashboard-one-panel` | 46.9 | 48.5 | **+3.4%** |
| `dashboard-all-panels` | 47.4 | 50.1 | **+5.5%** |
| `table-stream` | 42.3 | 43.7 | **+3.2%** |
| `text-typing` | 37.9 | 38.7 | ~+2.2% |
| `text-log-stream` | 36.2 | 36.2 | ~+0.1% |
| `text-relabel` | 55.0 | 56.9 | **+3.3%** |
| `scroll-uniform-list` | 33.8 | 63.4 | **+88%** |
| `scroll-variable-list` | 37.0 | 53.0 | **+43%** |
| `scroll-div-cards` | 61.4 | 75.8 | **+23%** |
| `scroll-data-table` | 45.8 | 141 | **+208%** |
| `hover-grid` | 78.4 | 81.0 | **+3.3%** |
| `tree-toggle` | 46.9 | 52.1 | **+11%** |
| `animated-dots` | 34.9 | 35.5 | ~+1.9% |
| `effects-cards` | 42.7 | 42.3 | ~-0.9% |

**Heap allocations** (per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 28213 | 3075 | -89% |
| `dashboard-one-panel` | 28517 | 3876 | -86% |
| `dashboard-all-panels` | 42682 | 37981 | -11% |
| `table-stream` | 20713 | 6486 | -69% |
| `text-typing` | 3907 | 1436 | -63% |
| `text-log-stream` | 4791 | 5630 | **+17%** |
| `text-relabel` | 63901 | 58746 | -8.1% |
| `scroll-uniform-list` | 3366 | 5622 | **+67%** |
| `scroll-variable-list` | 1217 | 2438 | **+100%** |
| `scroll-div-cards` | 104386 | 28797 | -72% |
| `scroll-data-table` | 10682 | 14272 | **+34%** |
| `hover-grid` | 53537 | 40895 | -24% |
| `tree-toggle` | 32266 | 22698 | -30% |
| `animated-dots` | 3218 | 3298 | ~+2.5% |
| `effects-cards` | 23330 | 2579 | -89% |

**Bytes allocated** (KB per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 16930 | 693 | -96% |
| `dashboard-one-panel` | 17001 | 1708 | -90% |
| `dashboard-all-panels` | 20269 | 12401 | -39% |
| `table-stream` | 6130 | 1217 | -80% |
| `text-typing` | 2919 | 1473 | -50% |
| `text-log-stream` | 3285 | 3246 | ~-1.2% |
| `text-relabel` | 19627 | 8407 | -57% |
| `scroll-uniform-list` | 1456 | 1026 | -30% |
| `scroll-variable-list` | 902 | 807 | -10% |
| `scroll-div-cards` | 69206 | 9718 | -86% |
| `scroll-data-table` | 4135 | 2591 | -37% |
| `hover-grid` | 34553 | 26984 | -22% |
| `tree-toggle` | 17504 | 8434 | -52% |
| `animated-dots` | 4247 | 4313 | ~+1.6% |
| `effects-cards` | 13442 | 947 | -93% |

**Startup: window opened to first frame presented** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 185 | 163 | ~-12% |
| `dashboard-one-panel` | 187 | 184 | ~-1.9% |
| `dashboard-all-panels` | 170 | 172 | ~+0.9% |
| `table-stream` | 183 | 152 | ~-17% |
| `text-typing` | 161 | 155 | ~-4.0% |
| `text-log-stream` | 130 | 130 | ~+0.6% |
| `text-relabel` | 139 | 143 | ~+3.4% |
| `scroll-uniform-list` | 126 | 126 | ~-0.1% |
| `scroll-variable-list` | 128 | 129 | ~+0.7% |
| `scroll-div-cards` | 184 | 189 | ~+2.7% |
| `scroll-data-table` | 130 | 133 | ~+2.0% |
| `hover-grid` | 146 | 161 | ~+11% |
| `tree-toggle` | 150 | 151 | ~+0.1% |
| `animated-dots` | 106 | 108 | ~+1.5% |
| `effects-cards` | 135 | 136 | ~+0.9% |

**Startup: the first frame's draw** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 55.0 | 57.1 | ~+3.9% |
| `dashboard-one-panel` | 55.0 | 55.8 | ~+1.4% |
| `dashboard-all-panels` | 50.7 | 53.0 | ~+4.5% |
| `table-stream` | 45.5 | 42.3 | ~-7.1% |
| `text-typing` | 59.7 | 55.5 | ~-6.9% |
| `text-log-stream` | 32.0 | 33.3 | **+3.9%** |
| `text-relabel` | 41.8 | 44.1 | **+5.6%** |
| `scroll-uniform-list` | 28.2 | 29.8 | **+5.6%** |
| `scroll-variable-list` | 30.2 | 31.8 | **+5.1%** |
| `scroll-div-cards` | 86.8 | 90.4 | ~+4.2% |
| `scroll-data-table` | 33.1 | 35.4 | **+6.7%** |
| `hover-grid` | 43.9 | 49.5 | ~+13% |
| `tree-toggle` | 43.0 | 44.6 | ~+3.7% |
| `animated-dots` | 9.2 | 10.9 | **+18%** |
| `effects-cards` | 37.2 | 39.8 | **+7.1%** |

## WASM in Chromium, WebGPU

viewport 1280×800 @2x; display interval ≈7.9 ms; Chromium 147.0.7727.15, backend webgpu; 90 of 90 runs reported

**Frame cost: CPU** (ms of Chrome main-thread task time per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 5.52 | 2.28 | -59% |
| `dashboard-one-panel` | 5.52 | 2.34 | -58% |
| `dashboard-all-panels` | 7.08 | 6.19 | -13% |
| `table-stream` | 3.96 | 2.35 | -41% |
| `text-typing` | 2.76 | 2.30 | -17% |
| `text-log-stream` | 2.38 | 2.40 | ~+0.7% |
| `text-relabel` | 8.30 | 7.66 | -7.7% |
| `scroll-uniform-list` | 2.22 | 2.14 | ~-3.4% |
| `scroll-variable-list` | 2.11 | 1.85 | -12% |
| `scroll-div-cards` | 17.6 | 5.88 | -67% |
| `scroll-data-table` | 3.35 | 4.01 | **+20%** |
| `hover-grid` | 10.1 | 9.16 | -9.0% |
| `tree-toggle` | 5.52 | 5.21 | -5.7% |
| `animated-dots` | 2.63 | 2.75 | ~+4.6% |
| `effects-cards` | 4.09 | 2.15 | -47% |

**Window::draw, median** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 5.20 | 1.86 | -64% |
| `dashboard-one-panel` | 5.22 | 2.02 | -61% |
| `dashboard-all-panels` | 6.70 | 5.81 | -13% |
| `table-stream` | 3.74 | 2.05 | -45% |
| `text-typing` | 2.50 | 1.79 | -29% |
| `text-log-stream` | 2.13 | 2.21 | ~+3.6% |
| `text-relabel` | 7.99 | 7.26 | -9.1% |
| `scroll-uniform-list` | 1.84 | 1.69 | ~-8.1% |
| `scroll-variable-list` | 1.59 | 1.16 | -27% |
| `scroll-div-cards` | 17.0 | 5.11 | -70% |
| `scroll-data-table` | 3.04 | 3.56 | **+17%** |
| `hover-grid` | 9.05 | 8.11 | -10% |
| `tree-toggle` | 5.36 | 3.90 | -27% |
| `animated-dots` | 2.28 | 2.40 | **+5.2%** |
| `effects-cards` | 3.80 | 1.78 | -53% |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 5.56 | 2.57 | -54% |
| `dashboard-one-panel` | 5.40 | 2.49 | -54% |
| `dashboard-all-panels` | 6.89 | 6.09 | -12% |
| `table-stream` | 3.93 | 2.58 | -34% |
| `text-typing` | 2.82 | 2.57 | ~-8.9% |
| `text-log-stream` | 2.51 | 2.54 | ~+1.2% |
| `text-relabel` | 8.10 | 7.59 | -6.3% |
| `scroll-uniform-list` | 2.53 | 2.30 | ~-9.1% |
| `scroll-variable-list` | 2.30 | 2.00 | -13% |
| `scroll-div-cards` | 17.4 | 5.55 | -68% |
| `scroll-data-table` | 3.35 | 4.38 | **+31%** |
| `hover-grid` | 9.28 | 8.34 | -10% |
| `tree-toggle` | 5.82 | 7.07 | **+22%** |
| `animated-dots` | 2.64 | 2.79 | ~+5.7% |
| `effects-cards` | 4.11 | 2.30 | -44% |

**Frame rate** (frames per second)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 120 | 120 | ~-0.0% |
| `dashboard-one-panel` | 120 | 120 | ~-0.0% |
| `dashboard-all-panels` | 120 | 120 | ~+0.0% |
| `table-stream` | 120 | 120 | ~-0.0% |
| `text-typing` | 120 | 120 | ~-0.0% |
| `text-log-stream` | 120 | 120 | ~-0.0% |
| `text-relabel` | 120 | 120 | ~-0.0% |
| `scroll-uniform-list` | 120 | 120 | ~-0.0% |
| `scroll-variable-list` | 120 | 120 | ~-0.2% |
| `scroll-div-cards` | 56.6 | 120 | +112% |
| `scroll-data-table` | 120 | 119 | ~-0.7% |
| `hover-grid` | 99.2 | 109 | +9.5% |
| `tree-toggle` | 120 | 120 | ~+0.0% |
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
| `scroll-uniform-list` | 0 | 1 | **+1.00** |
| `scroll-variable-list` | 0 | 1 | ~+1.00 |
| `scroll-div-cards` | 0 | 11.0 | **+11.00** |
| `scroll-data-table` | 0 | 2 | **+2.00** |
| `hover-grid` | 0 | 0 | ~+0.00 |
| `tree-toggle` | 0 | 0 | ~+0.00 |
| `animated-dots` | 0 | 0 | ~+0.00 |
| `effects-cards` | 0 | 0 | ~+0.00 |

**WASM linear memory at the end** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 50.4 | 53.4 | **+5.8%** |
| `dashboard-one-panel` | 50.4 | 53.3 | **+5.7%** |
| `dashboard-all-panels` | 51.1 | 53.7 | **+5.0%** |
| `table-stream` | 48.1 | 49.6 | **+3.3%** |
| `text-typing` | 42.8 | 43.1 | ~+0.7% |
| `text-log-stream` | 39.9 | 40.9 | ~+2.3% |
| `text-relabel` | 61.5 | 62.8 | ~+2.0% |
| `scroll-uniform-list` | 38.8 | 71.4 | **+84%** |
| `scroll-variable-list` | 41.1 | 58.0 | **+41%** |
| `scroll-div-cards` | 67.8 | 84.8 | **+25%** |
| `scroll-data-table` | 49.6 | 168 | **+238%** |
| `hover-grid` | 77.9 | 80.6 | **+3.5%** |
| `tree-toggle` | 53.1 | 58.2 | **+10%** |
| `animated-dots` | 39.6 | 40.2 | ~+1.4% |
| `effects-cards` | 46.4 | 47.1 | ~+1.5% |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 0.19 | 0.00 | -0.19 |
| `dashboard-one-panel` | 0.00 | 0.00 | ~+0.00 |
| `dashboard-all-panels` | 0.00 | 0.00 | ~+0.00 |
| `table-stream` | 0.00 | 0.00 | ~+0.00 |
| `text-typing` | 0.00 | 0.12 | ~+0.12 |
| `text-log-stream` | 0.00 | 0.00 | ~+0.00 |
| `text-relabel` | 0.00 | 0.00 | ~+0.00 |
| `scroll-uniform-list` | 0.00 | 18.6 | **+18.62** |
| `scroll-variable-list` | 0.00 | 9.31 | **+9.31** |
| `scroll-div-cards` | 0.00 | 1.75 | **+1.75** |
| `scroll-data-table` | 2.12 | 67.6 | **+65.50** |
| `hover-grid` | 0.00 | 0.00 | ~+0.00 |
| `tree-toggle` | 0.00 | 0.00 | ~+0.00 |
| `animated-dots` | 0.31 | 0.31 | ~+0.00 |
| `effects-cards` | 0.38 | 0.38 | ~+0.00 |

**Peak live Rust heap** (MB)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 44.3 | 45.5 | ~+2.7% |
| `dashboard-one-panel` | 44.3 | 45.9 | **+3.6%** |
| `dashboard-all-panels` | 44.8 | 47.4 | **+5.8%** |
| `table-stream` | 40.9 | 41.9 | ~+2.2% |
| `text-typing` | 36.0 | 36.9 | ~+2.6% |
| `text-log-stream` | 33.9 | 34.2 | ~+1.1% |
| `text-relabel` | 53.3 | 55.0 | **+3.2%** |
| `scroll-uniform-list` | 32.3 | 62.4 | **+93%** |
| `scroll-variable-list` | 35.0 | 52.1 | **+49%** |
| `scroll-div-cards` | 60.3 | 74.7 | **+24%** |
| `scroll-data-table` | 41.4 | 140 | **+238%** |
| `hover-grid` | 71.8 | 74.1 | **+3.2%** |
| `tree-toggle` | 46.0 | 51.1 | **+11%** |
| `animated-dots` | 33.4 | 34.0 | ~+2.0% |
| `effects-cards` | 40.1 | 39.6 | ~-1.1% |

**Heap allocations** (per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 27989 | 2851 | -90% |
| `dashboard-one-panel` | 28293 | 3652 | -87% |
| `dashboard-all-panels` | 42458 | 37757 | -11% |
| `table-stream` | 20480 | 6253 | -69% |
| `text-typing` | 3674 | 1203 | -67% |
| `text-log-stream` | 4558 | 5397 | **+18%** |
| `text-relabel` | 63657 | 58502 | -8.1% |
| `scroll-uniform-list` | 3142 | 4286 | **+36%** |
| `scroll-variable-list` | 984 | 1128 | **+15%** |
| `scroll-div-cards` | 104071 | 27472 | -74% |
| `scroll-data-table` | 10359 | 12845 | **+24%** |
| `hover-grid` | 49962 | 37320 | -25% |
| `tree-toggle` | 32042 | 22474 | -30% |
| `animated-dots` | 3040 | 3120 | ~+2.6% |
| `effects-cards` | 22764 | 2013 | -91% |

**Bytes allocated** (KB per frame)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 16620 | 382 | -98% |
| `dashboard-one-panel` | 16689 | 1397 | -92% |
| `dashboard-all-panels` | 19954 | 12087 | -39% |
| `table-stream` | 5692 | 779 | -86% |
| `text-typing` | 2479 | 1034 | -58% |
| `text-log-stream` | 2864 | 2825 | ~-1.4% |
| `text-relabel` | 18673 | 7454 | -60% |
| `scroll-uniform-list` | 1251 | 783 | -37% |
| `scroll-variable-list` | 516 | 560 | **+8.5%** |
| `scroll-div-cards` | 69012 | 9480 | -86% |
| `scroll-data-table` | 3553 | 2285 | -36% |
| `hover-grid` | 31899 | 24331 | -24% |
| `tree-toggle` | 17435 | 8366 | -52% |
| `animated-dots` | 4016 | 4082 | ~+1.6% |
| `effects-cards` | 13130 | 635 | -95% |

**Startup: window opened to first frame presented** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 60.2 | 58.1 | ~-3.5% |
| `dashboard-one-panel` | 54.9 | 57.8 | **+5.2%** |
| `dashboard-all-panels` | 55.4 | 58.6 | ~+5.8% |
| `table-stream` | 49.8 | 51.3 | ~+3.0% |
| `text-typing` | 63.2 | 65.7 | ~+3.9% |
| `text-log-stream` | 43.1 | 44.0 | ~+2.2% |
| `text-relabel` | 53.0 | 54.6 | ~+3.1% |
| `scroll-uniform-list` | 39.5 | 40.8 | ~+3.3% |
| `scroll-variable-list` | 41.6 | 43.7 | **+5.0%** |
| `scroll-div-cards` | 98.1 | 104 | **+5.6%** |
| `scroll-data-table` | 44.4 | 46.4 | ~+4.7% |
| `hover-grid` | 55.1 | 57.9 | **+5.2%** |
| `tree-toggle` | 51.4 | 54.5 | **+6.1%** |
| `animated-dots` | 20.0 | 21.7 | **+8.9%** |
| `effects-cards` | 49.9 | 50.4 | ~+1.0% |

**Startup: the first frame's draw** (ms)

| Scenario | off | on | on vs off |
|---|---:|---:|---:|
| `dashboard-clock` | 46.0 | 47.5 | ~+3.3% |
| `dashboard-one-panel` | 44.0 | 47.4 | **+7.8%** |
| `dashboard-all-panels` | 44.8 | 47.7 | ~+6.5% |
| `table-stream` | 39.5 | 40.7 | ~+3.2% |
| `text-typing` | 53.1 | 55.1 | ~+3.9% |
| `text-log-stream` | 32.5 | 33.9 | **+4.5%** |
| `text-relabel` | 42.6 | 44.4 | **+4.2%** |
| `scroll-uniform-list` | 29.0 | 30.6 | **+5.6%** |
| `scroll-variable-list` | 30.8 | 32.8 | **+6.6%** |
| `scroll-div-cards` | 87.2 | 92.1 | **+5.6%** |
| `scroll-data-table` | 34.5 | 36.2 | ~+4.9% |
| `hover-grid` | 44.6 | 47.5 | **+6.5%** |
| `tree-toggle` | 41.0 | 43.9 | **+7.3%** |
| `animated-dots` | 9.7 | 11.3 | **+17%** |
| `effects-cards` | 39.7 | 40.2 | ~+1.3% |

## Fast-mode work counters: macOS native (Metal)

One untimed run per scenario in a build with gpux's diagnostics counters (`--stats`): the work per measured frame.

| Scenario | mode | views built | views reused | views spliced | cached views replayed | style writes | nodes claimed | lines shaped | primitives painted | primitives copied | layers composited |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | off | 51.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 1.0 | 2009 | 0.0 | 0.0 |
| `dashboard-clock` | on | 1.0 | 0.0 | 2.0 | 0.0 | 0.0 | 3.0 | 1.0 | 43.0 | 1966 | 0.0 |
| `dashboard-one-panel` | off | 51.0 | 0.0 | 0.0 | 0.0 | 15.8 | 0.0 | 9.8 | 2017 | 0.0 | 0.0 |
| `dashboard-one-panel` | on | 2.0 | 0.0 | 2.0 | 0.0 | 15.8 | 58.0 | 9.8 | 170 | 1847 | 0.0 |
| `dashboard-all-panels` | off | 51.0 | 0.0 | 0.0 | 0.0 | 759 | 0.0 | 416 | 2043 | 0.0 | 0.0 |
| `dashboard-all-panels` | on | 49.0 | 0.0 | 2.0 | 0.0 | 759 | 2643 | 416 | 2018 | 25.0 | 0.0 |
| `table-stream` | off | 62.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 69.1 | 3237 | 0.0 | 0.0 |
| `table-stream` | on | 5.8 | 0.0 | 2.0 | 0.0 | 0.0 | 156 | 69.1 | 300 | 2937 | 0.0 |
| `text-typing` | off | 152 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 1.0 | 3883 | 0.0 | 0.0 |
| `text-typing` | on | 3.5 | 0.0 | 2.0 | 0.0 | 0.0 | 6.9 | 1.0 | 681 | 3202 | 0.0 |
| `text-log-stream` | off | 2.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 2.0 | 3797 | 0.0 | 0.0 |
| `text-log-stream` | on | 1.0 | 0.0 | 1.0 | 0.0 | 0.0 | 401 | 2.0 | 3796 | 1.0 | 0.0 |
| `text-relabel` | off | 2.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 1190 | 7350 | 0.0 | 0.0 |
| `text-relabel` | on | 1.0 | 0.0 | 1.0 | 0.0 | 0.0 | 3051 | 1190 | 7349 | 1.0 | 0.0 |
| `scroll-uniform-list` | off | 2.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 5.1 | 1456 | 0.0 | 0.0 |
| `scroll-uniform-list` | on | 1.0 | 0.0 | 1.0 | 0.0 | 13.0 | 20.5 | 3.0 | 20.6 | 1.0 | 1.0 |
| `scroll-variable-list` | off | 2.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 2.2 | 3303 | 0.0 | 0.0 |
| `scroll-variable-list` | on | 1.0 | 0.0 | 1.0 | 0.0 | 3.0 | 6.2 | 1.0 | 20.3 | 1.0 | 1.0 |
| `scroll-div-cards` | off | 2.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 1280 | 0.0 | 0.0 |
| `scroll-div-cards` | on | 1.0 | 0.0 | 1.0 | 0.0 | 0.0 | 6802 | 0.0 | 20.6 | 1.0 | 1.0 |
| `scroll-data-table` | off | 2.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 34.9 | 4186 | 0.0 | 0.0 |
| `scroll-data-table` | on | 1.0 | 0.0 | 1.0 | 0.0 | 60.6 | 98.7 | 29.4 | 139 | 1.0 | 1.0 |
| `hover-grid` | off | 2.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 11041 | 0.0 | 0.0 |
| `hover-grid` | on | 1.0 | 0.0 | 1.0 | 0.0 | 0.0 | 3241 | 0.0 | 11040 | 1.0 | 0.0 |
| `tree-toggle` | off | 2.0 | 0.0 | 0.0 | 0.0 | 201 | 0.0 | 0.0 | 320 | 0.0 | 0.0 |
| `tree-toggle` | on | 1.0 | 0.0 | 1.0 | 0.0 | 217 | 3583 | 0.0 | 319 | 1.0 | 0.0 |
| `animated-dots` | off | 2.0 | 0.0 | 0.0 | 0.0 | 987 | 0.0 | 0.0 | 1001 | 0.0 | 0.0 |
| `animated-dots` | on | 1.0 | 0.0 | 1.0 | 0.0 | 987 | 1001 | 0.0 | 1000 | 1.0 | 0.0 |
| `effects-cards` | off | 3.0 | 0.0 | 0.0 | 0.0 | 1.0 | 0.0 | 0.0 | 1555 | 0.0 | 0.0 |
| `effects-cards` | on | 1.0 | 0.0 | 2.0 | 0.0 | 1.0 | 2.0 | 0.0 | 2.0 | 1553 | 0.0 |
