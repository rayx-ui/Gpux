# gpux rendering benchmarks: PR #3 (fast mode)

Measured on 2026-10-15 with the benchmark app in this directory ([README.md](README.md) explains
how it works and how to run it). Raw data: [results/2026-10-15-pr3/](results/2026-10-15-pr3/)
(`raw/` holds one JSON report per run, `meta.json` the commits and settings, `report.md` the
generated tables reproduced below).

| | |
|---|---|
| `pre` | gpux before [PR #3](https://github.com/rayx-ui/Gpux/pull/3): `aefaea3` |
| `off` | `main` after PR #3 (`0d343b5`; the later commits are CI and test only), every fast-mode mechanism off (`Window::set_fast_mode(FastMode::OFF)`) |
| `on` | the same build, every mechanism on (`FastMode::ALL`: layout retention, view retention, splice, scroll layers; window composition stays opt-in) |
| Machine | Apple M5, 24 GB, macOS 26.6.2, display at 120 Hz |
| Targets | macOS native (Metal window 1280×800 @2x) · iOS Simulator (iPhone 17 Pro, iOS 26.5, 402×874 @3x, 60 Hz) · WASM in Chromium 147 on WebGPU and on WebGL2 (1280×800 @2x) |
| Runs | 15 scenarios × 3 configurations × 3 repetitions × 4 targets = 540 runs, all reported, none with an unfocused window; each run 120 warm-up and 600 measured frames in its own process (page) |

How to read the tables: a cell is the median over the three repetitions. A change is marked `~`
when it is within the repetitions' own spread or under 3% (noise); **bold** marks a change for the
worse. CPU per frame is the main thread's CPU time per frame (step, draw, present and the
platform's work around them); in the browser it is Chrome's main-thread task time per frame.

## Answers

**1. Did PR #3 slow down normal (non-fast) mode?** No. With fast mode off, `main` costs the same
or less CPU per frame than `pre` on every target: the geometric mean over the 15 scenarios is
**−5.6% on macOS, −1.6% on the iOS Simulator, −4.9% on WebGPU and −4.8% on WebGL2**, and no
scenario regresses beyond noise on the native targets. Two scenarios get much faster without fast
mode: `hover-grid` (−23% to −47%; on macOS it goes from 69 to 120 fps) and `text-relabel` (−3% to
−15%). The only consistent cost is small: about **+3% CPU on large trees rebuilt whole every
frame** (`scroll-div-cards` +2.8% to +3.9%, `tree-toggle` +0.5% to +3.4%, `dashboard-clock` up to
+3.4% in the browser), at the noise threshold. Memory with fast mode off is slightly higher: the
peak Rust heap is +1.5% to +21% (+0.6 to +7.3 MB) and the footprint −3% to +10%.

**2. Is fast mode really faster?** Yes, in most UIs, with a clear memory cost and a few losses.
CPU per frame, geometric mean over the scenarios, `on` against `off`: **−18.6% on macOS, −17.9%
on the iOS Simulator, −25.9% on WebGPU, −35.5% on WebGL2**; `Window::draw` −22% to −44%. It wins
big where few views change (dashboards −30% to −61%, a streaming table −8% to −41%, decorated
pages −24% to −86%) and where content scrolls through scroll layers (a plain scrolling `div` of
400 cards −54% to −67%, which lifts it from 55–73 fps to a steady 120 fps on macOS and in the
browser; the chat `list` up to −39%). It does not help when one view rebuilds everything every frame (`text-log-stream`,
`animated-dots`, `tree-toggle`, `text-relabel`: within ±8%) and it **loses on
`scroll-data-table`** (+8% CPU on macOS, +20% on WebGPU) and on `text-typing` on macOS (+10%,
while every other target gains 17–22%). Its price is **memory**: −4% to +11% footprint in the
other scenarios, and **+10% to +240% in the four scrolling scenarios** (+55% to +124% on macOS),
held by scroll layers' tiles and kept rows, with the footprint still growing during the measured
frames in `scroll-data-table` (+38 to +68 MB in 600 frames).

## Summary

Geometric means over the 15 scenarios of the ratio of medians; a regression or win is a
scenario whose change is beyond the noise.

| Target | Metric | off / pre | regressions (off vs pre) | on / off | on wins / losses (vs off) |
|---|---|---:|---:|---:|---:|
| iOS Simulator (Metal) | CPU ms per frame | 0.984 (-1.6%) | 0 of 15 | 0.821 (-17.9%) | 8 / 0 of 15 |
| iOS Simulator (Metal) | draw p50 | 0.994 (-0.6%) | 1 of 15 | 0.673 (-32.7%) | 12 / 1 of 15 |
| macOS native (Metal) | CPU ms per frame | 0.944 (-5.6%) | 0 of 15 | 0.814 (-18.6%) | 7 / 2 of 15 |
| macOS native (Metal) | draw p50 | 0.935 (-6.5%) | 0 of 15 | 0.777 (-22.3%) | 8 / 1 of 15 |
| WASM in Chromium, WebGL2 | CPU ms per frame | 0.952 (-4.8%) | 1 of 15 | 0.645 (-35.5%) | 11 / 0 of 15 |
| WASM in Chromium, WebGL2 | draw p50 | 0.936 (-6.4%) | 2 of 15 | 0.561 (-43.9%) | 11 / 0 of 15 |
| WASM in Chromium, WebGPU | CPU ms per frame | 0.951 (-4.9%) | 3 of 15 | 0.741 (-25.9%) | 11 / 1 of 15 |
| WASM in Chromium, WebGPU | draw p50 | 0.936 (-6.4%) | 2 of 15 | 0.684 (-31.6%) | 11 / 2 of 15 |

## At a glance: every target

Cells are `pre / off / on (on vs off)`.

**CPU per frame** (ms; in the browser, Chrome's main-thread task time)

| Scenario | macOS (Metal) | iOS Simulator | WASM · WebGPU | WASM · WebGL2 |
|---|---:|---:|---:|---:|
| `dashboard-clock` | 4.56 / 4.51 / 3.12 (-31%) | 5.52 / 5.65 / 3.17 (-44%) | 5.34 / 5.52 / 2.28 (-59%) | 5.67 / 5.75 / 2.39 (-58%) |
| `dashboard-one-panel` | 4.61 / 4.58 / 2.87 (-37%) | 5.58 / 5.71 / 4.02 (-30%) | 5.43 / 5.52 / 2.34 (-58%) | 5.76 / 5.81 / 2.27 (-61%) |
| `dashboard-all-panels` | 7.10 / 7.11 / 6.64 (-7%) | 7.45 / 7.51 / 7.32 (-3%) | 6.98 / 7.08 / 6.19 (-13%) | 7.91 / 8.09 / 6.96 (-14%) |
| `table-stream` | 3.88 / 3.74 / 2.70 (-28%) | 4.94 / 4.89 / 4.50 (-8%) | 4.07 / 3.96 / 2.35 (-41%) | 3.68 / 3.51 / 2.07 (-41%) |
| `text-typing` | 2.82 / 2.90 / 3.20 (+10%) | 4.26 / 4.36 / 3.41 (-22%) | 2.79 / 2.76 / 2.30 (-17%) | 2.58 / 2.49 / 1.99 (-20%) |
| `text-log-stream` | 2.87 / 2.71 / 2.91 (+7%) | 3.99 / 4.04 / 4.15 (+3%) | 2.41 / 2.38 / 2.40 (+1%) | 2.12 / 2.12 / 2.11 (-1%) |
| `text-relabel` | 12.48 / 11.01 / 10.96 (-1%) | 11.06 / 10.69 / 10.51 (-2%) | 9.74 / 8.30 / 7.66 (-8%) | 9.99 / 8.57 / 7.95 (-7%) |
| `scroll-uniform-list` | 3.29 / 3.14 / 2.87 (-9%) | 4.38 / 4.57 / 2.97 (-35%) | 2.32 / 2.22 / 2.14 (-3%) | 2.11 / 2.11 / 1.62 (-23%) |
| `scroll-variable-list` | 3.05 / 2.90 / 1.77 (-39%) | 3.21 / 2.92 / 2.96 (+1%) | 2.17 / 2.11 / 1.85 (-12%) | 2.05 / 2.02 / 1.31 (-35%) |
| `scroll-div-cards` | 13.41 / 13.79 / 5.14 (-63%) | 13.57 / 14.10 / 6.48 (-54%) | 17.04 / 17.64 / 5.88 (-67%) | 17.53 / 18.03 / 6.16 (-66%) |
| `scroll-data-table` | 3.26 / 3.18 / 3.42 (+8%) | 4.37 / 4.42 / 4.11 (-7%) | 3.46 / 3.35 / 4.01 (+20%) | 3.25 / 3.06 / 3.11 (+2%) |
| `hover-grid` | 14.41 / 7.62 / 6.88 (-10%) | 13.42 / 10.34 / 9.41 (-9%) | 17.54 / 10.06 / 9.16 (-9%) | 18.82 / 11.38 / 10.64 (-6%) |
| `tree-toggle` | 4.54 / 4.60 / 4.56 (-1%) | 5.99 / 6.02 / 5.79 (-4%) | 5.34 / 5.52 / 5.21 (-6%) | 5.41 / 5.59 / 5.24 (-6%) |
| `animated-dots` | 2.93 / 3.11 / 2.91 (-6%) | 4.64 / 4.31 / 4.30 (-0%) | 2.73 / 2.63 / 2.75 (+5%) | 2.55 / 2.42 / 2.54 (+5%) |
| `effects-cards` | 3.65 / 3.64 / 2.58 (-29%) | 4.60 / 4.80 / 3.63 (-24%) | 4.03 / 4.09 / 2.15 (-47%) | 5.19 / 5.31 / 0.77 (-86%) |

**Frame rate** (frames per second; the display paces 120 Hz, the Simulator 60 Hz)

| Scenario | macOS (Metal) | iOS Simulator | WASM · WebGPU | WASM · WebGL2 |
|---|---:|---:|---:|---:|
| `dashboard-clock` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 104 / 104 / 104 |
| `dashboard-one-panel` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 104 / 103 / 103 |
| `dashboard-all-panels` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 104 / 103 / 103 |
| `table-stream` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 120 / 120 / 120 |
| `text-typing` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 120 / 120 / 120 |
| `text-log-stream` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 120 / 120 / 120 |
| `text-relabel` | 80 / 91 / 91 | 60 / 60 / 60 | 102 / 120 / 120 | 100 / 116 / 120 |
| `scroll-uniform-list` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 120 / 120 / 120 |
| `scroll-variable-list` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 120 / 120 / 120 |
| `scroll-div-cards` | 74 / 72 / 120 | 60 / 60 / 60 | 58 / 57 / 120 | 57 / 55 / 120 |
| `scroll-data-table` | 120 / 120 / 119 | 60 / 60 / 60 | 120 / 120 / 119 | 118 / 117 / 119 |
| `hover-grid` | 69 / 120 / 120 | 60 / 60 / 60 | 57 / 99 / 109 | 53 / 88 / 94 |
| `tree-toggle` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 120 / 120 / 120 |
| `animated-dots` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 120 / 120 / 120 |
| `effects-cards` | 120 / 120 / 120 | 60 / 60 / 60 | 120 / 120 / 120 | 63 / 62 / 62 |

**Memory footprint at the end of the run** (MB; Apple targets `phys_footprint`, GPU allocations
included; in the browser, the WASM linear memory)

| Scenario | macOS (Metal) | iOS Simulator | WASM · WebGPU | WASM · WebGL2 |
|---|---:|---:|---:|---:|
| `dashboard-clock` | 137 / 139 / 149 (+7%) | 38 / 38 / 41 (+7%) | 49 / 50 / 53 (+6%) | 51 / 54 / 56 (+5%) |
| `dashboard-one-panel` | 137 / 141 / 150 (+6%) | 38 / 38 / 41 (+7%) | 49 / 50 / 53 (+6%) | 52 / 54 / 57 (+5%) |
| `dashboard-all-panels` | 138 / 141 / 144 (+2%) | 39 / 38 / 40 (+5%) | 49 / 51 / 54 (+5%) | 52 / 55 / 57 (+5%) |
| `table-stream` | 133 / 143 / 137 (-4%) | 35 / 37 / 38 (+3%) | 45 / 48 / 50 (+3%) | 46 / 49 / 51 (+3%) |
| `text-typing` | 129 / 130 / 139 (+7%) | 33 / 33 / 34 (+3%) | 40 / 43 / 43 (+1%) | 42 / 44 / 45 (+3%) |
| `text-log-stream` | 128 / 131 / 139 (+6%) | 31 / 32 / 32 (+1%) | 38 / 40 / 41 (+2%) | 40 / 42 / 43 (+1%) |
| `text-relabel` | 146 / 151 / 151 (+0%) | 42 / 43 / 45 (+5%) | 56 / 62 / 63 (+2%) | 58 / 64 / 66 (+3%) |
| `scroll-uniform-list` | 132 / 133 / 237 (+78%) | 31 / 31 / 50 (+63%) | 38 / 39 / 71 (+84%) | 39 / 40 / 70 (+73%) |
| `scroll-variable-list` | 130 / 141 / 221 (+56%) | 32 / 33 / 36 (+10%) | 38 / 41 / 58 (+41%) | 41 / 43 / 59 (+37%) |
| `scroll-div-cards` | 161 / 165 / 256 (+55%) | 56 / 58 / 67 (+15%) | 66 / 68 / 85 (+25%) | 67 / 68 / 86 (+27%) |
| `scroll-data-table` | 133 / 142 / 317 (+124%) | 33 / 35 / 102 (+189%) | 46 / 50 / 168 (+238%) | 51 / 54 / 169 (+213%) |
| `hover-grid` | 169 / 170 / 175 (+3%) | 47 / 52 / 53 (+1%) | 76 / 78 / 81 (+3%) | 81 / 86 / 88 (+2%) |
| `tree-toggle` | 145 / 145 / 150 (+4%) | 41 / 42 / 46 (+10%) | 52 / 53 / 58 (+10%) | 53 / 54 / 60 (+11%) |
| `animated-dots` | 132 / 128 / 129 (+1%) | 33 / 33 / 33 (-1%) | 39 / 40 / 40 (+1%) | 40 / 41 / 42 (+2%) |
| `effects-cards` | 134 / 135 / 143 (+6%) | 35 / 37 / 37 (+0%) | 43 / 46 / 47 (+1%) | 46 / 49 / 50 (+2%) |

**Peak live Rust heap** (MB)

| Scenario | macOS (Metal) | iOS Simulator | WASM · WebGPU | WASM · WebGL2 |
|---|---:|---:|---:|---:|
| `dashboard-clock` | 27.0 / 29.4 / 30.8 (+5%) | 26.8 / 28.0 / 30.0 (+7%) | 42.1 / 44.3 / 45.5 (+3%) | 44.7 / 46.9 / 48.3 (+3%) |
| `dashboard-one-panel` | 27.1 / 29.5 / 31.1 (+5%) | 26.8 / 28.2 / 30.2 (+7%) | 42.2 / 44.3 / 45.9 (+4%) | 44.7 / 46.9 / 48.5 (+3%) |
| `dashboard-all-panels` | 27.5 / 29.9 / 32.6 (+9%) | 27.0 / 28.5 / 31.3 (+10%) | 42.4 / 44.8 / 47.4 (+6%) | 45.0 / 47.4 / 50.1 (+6%) |
| `table-stream` | 20.7 / 23.4 / 24.3 (+4%) | 20.1 / 22.1 / 23.0 (+4%) | 38.1 / 40.9 / 41.9 (+2%) | 39.3 / 42.3 / 43.7 (+3%) |
| `text-typing` | 13.7 / 16.6 / 18.1 (+9%) | 14.4 / 16.1 / 17.5 (+9%) | 33.5 / 36.0 / 36.9 (+3%) | 35.4 / 37.9 / 38.7 (+2%) |
| `text-log-stream` | 13.9 / 16.8 / 17.2 (+2%) | 13.6 / 15.4 / 16.0 (+4%) | 31.5 / 33.9 / 34.2 (+1%) | 33.5 / 36.2 / 36.2 (+0%) |
| `text-relabel` | 34.1 / 39.0 / 40.8 (+4%) | 31.7 / 34.4 / 36.4 (+6%) | 48.1 / 53.3 / 55.0 (+3%) | 49.8 / 55.0 / 56.9 (+3%) |
| `scroll-uniform-list` | 11.5 / 13.2 / 49.9 (+277%) | 12.3 / 13.8 / 47.7 (+246%) | 30.7 / 32.3 / 62.4 (+93%) | 32.1 / 33.8 / 63.4 (+88%) |
| `scroll-variable-list` | 15.4 / 18.1 / 42.6 (+136%) | 13.8 / 15.5 / 27.9 (+80%) | 32.6 / 35.0 / 52.1 (+49%) | 34.5 / 37.0 / 53.0 (+43%) |
| `scroll-div-cards` | 46.5 / 49.7 / 68.8 (+38%) | 47.9 / 50.2 / 63.3 (+26%) | 57.8 / 60.3 / 74.7 (+24%) | 58.8 / 61.4 / 75.8 (+23%) |
| `scroll-data-table` | 21.4 / 24.8 / 142.6 (+476%) | 19.8 / 22.5 / 123.6 (+450%) | 37.8 / 41.4 / 139.9 (+238%) | 42.3 / 45.8 / 141.2 (+208%) |
| `hover-grid` | 57.2 / 60.4 / 62.9 (+4%) | 48.9 / 53.0 / 55.3 (+5%) | 67.5 / 71.8 / 74.1 (+3%) | 71.1 / 78.4 / 81.0 (+3%) |
| `tree-toggle` | 31.5 / 32.4 / 37.7 (+17%) | 33.5 / 34.5 / 39.7 (+15%) | 45.4 / 46.0 / 51.1 (+11%) | 46.2 / 46.9 / 52.1 (+11%) |
| `animated-dots` | 14.2 / 14.7 / 15.4 (+5%) | 16.3 / 17.0 / 17.7 (+4%) | 32.7 / 33.4 / 34.0 (+2%) | 34.1 / 34.9 / 35.5 (+2%) |
| `effects-cards` | 20.2 / 23.5 / 23.4 (-1%) | 21.0 / 22.9 / 23.2 (+1%) | 37.0 / 40.1 / 39.6 (-1%) | 39.5 / 42.7 / 42.3 (-1%) |

## Full results

Every table per target: CPU, `Window::draw` p50 and p95, frame rate, missed frames, process CPU,
memory footprint, memory growth over the measured frames, peak heap, allocations, startup, and on
macOS fast mode's own work counters.

### iOS Simulator (Metal)

viewport 402×874 @3x; display interval ≈16.1 ms; iPhone 17 Pro (iOS-26-5); 135 of 135 runs reported, 32.3 min

**Frame cost: CPU** (ms of main-thread CPU per frame, mean)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 5.52 | 5.65 | 3.17 | ~+2.5% | -44% | -43% |
| `dashboard-one-panel` | 5.58 | 5.71 | 4.02 | ~+2.2% | -30% | -28% |
| `dashboard-all-panels` | 7.45 | 7.51 | 7.32 | ~+0.7% | ~-2.6% | ~-1.9% |
| `table-stream` | 4.94 | 4.89 | 4.50 | ~-1.0% | -7.9% | -8.9% |
| `text-typing` | 4.26 | 4.36 | 3.41 | ~+2.4% | -22% | -20% |
| `text-log-stream` | 3.99 | 4.04 | 4.15 | ~+1.3% | ~+2.7% | ~+4.0% |
| `text-relabel` | 11.1 | 10.7 | 10.5 | -3.4% | ~-1.7% | ~-5.0% |
| `scroll-uniform-list` | 4.38 | 4.57 | 2.97 | ~+4.3% | -35% | ~-32% |
| `scroll-variable-list` | 3.21 | 2.92 | 2.96 | ~-8.9% | ~+1.3% | ~-7.8% |
| `scroll-div-cards` | 13.6 | 14.1 | 6.48 | ~+4.0% | -54% | -52% |
| `scroll-data-table` | 4.37 | 4.42 | 4.11 | ~+1.1% | ~-7.1% | ~-6.1% |
| `hover-grid` | 13.4 | 10.3 | 9.41 | -23% | -9.1% | -30% |
| `tree-toggle` | 5.99 | 6.02 | 5.79 | ~+0.6% | ~-3.9% | ~-3.4% |
| `animated-dots` | 4.64 | 4.31 | 4.30 | ~-7.1% | ~-0.2% | ~-7.2% |
| `effects-cards` | 4.60 | 4.80 | 3.63 | ~+4.3% | -24% | -21% |

**Window::draw, median** (ms per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 5.00 | 5.09 | 2.17 | ~+1.7% | -57% | -57% |
| `dashboard-one-panel` | 5.02 | 5.15 | 3.38 | ~+2.5% | -34% | -33% |
| `dashboard-all-panels` | 6.96 | 7.04 | 6.82 | ~+1.1% | -3.1% | ~-2.1% |
| `table-stream` | 4.48 | 4.42 | 3.87 | ~-1.3% | -12% | -14% |
| `text-typing` | 3.43 | 3.68 | 2.43 | ~+7.3% | -34% | -29% |
| `text-log-stream` | 2.98 | 2.93 | 3.16 | ~-1.8% | **+8.0%** | **+6.1%** |
| `text-relabel` | 10.6 | 10.2 | 10.0 | -3.1% | ~-2.0% | ~-5.0% |
| `scroll-uniform-list` | 3.04 | 3.75 | 1.36 | ~+23% | -64% | -55% |
| `scroll-variable-list` | 1.80 | 1.54 | 0.77 | ~-14% | -50% | -57% |
| `scroll-div-cards` | 12.6 | 13.1 | 5.12 | **+3.9%** | -61% | -60% |
| `scroll-data-table` | 3.57 | 3.80 | 2.93 | ~+6.4% | -23% | -18% |
| `hover-grid` | 10.7 | 7.76 | 6.59 | -28% | -15% | -39% |
| `tree-toggle` | 5.20 | 5.25 | 4.35 | ~+0.9% | -17% | -16% |
| `animated-dots` | 3.79 | 3.61 | 3.67 | ~-4.6% | ~+1.6% | ~-3.0% |
| `effects-cards` | 3.90 | 4.13 | 1.71 | ~+6.0% | -59% | -56% |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 5.21 | 5.37 | 2.57 | ~+3.0% | -52% | -51% |
| `dashboard-one-panel` | 5.25 | 5.35 | 3.77 | ~+2.0% | -30% | -28% |
| `dashboard-all-panels` | 7.18 | 7.19 | 7.00 | ~+0.2% | ~-2.7% | ~-2.5% |
| `table-stream` | 4.70 | 4.70 | 4.54 | ~+0.0% | ~-3.4% | ~-3.4% |
| `text-typing` | 3.80 | 3.98 | 2.81 | ~+4.6% | -29% | -26% |
| `text-log-stream` | 3.33 | 3.32 | 3.45 | ~-0.2% | ~+3.9% | ~+3.6% |
| `text-relabel` | 11.1 | 10.6 | 10.5 | ~-4.6% | ~-0.9% | ~-5.4% |
| `scroll-uniform-list` | 4.46 | 4.74 | 3.18 | ~+6.1% | -33% | -29% |
| `scroll-variable-list` | 2.61 | 2.36 | 2.46 | ~-10% | ~+4.2% | ~-5.8% |
| `scroll-div-cards` | 13.1 | 13.6 | 5.75 | **+3.8%** | -58% | -56% |
| `scroll-data-table` | 4.00 | 4.17 | 4.05 | ~+4.2% | ~-2.8% | ~+1.3% |
| `hover-grid` | 11.0 | 8.09 | 6.96 | -27% | -14% | -37% |
| `tree-toggle` | 5.74 | 5.83 | 7.41 | ~+1.6% | **+27%** | **+29%** |
| `animated-dots` | 4.53 | 4.23 | 4.59 | -6.7% | **+8.6%** | ~+1.4% |
| `effects-cards` | 4.12 | 4.36 | 2.09 | ~+6.0% | -52% | -49% |

**Frame rate** (frames per second)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 60.0 | 60.0 | 60.0 | ~+0.0% | ~-0.0% | ~-0.0% |
| `dashboard-one-panel` | 60.0 | 60.0 | 60.0 | ~+0.0% | ~+0.0% | ~+0.0% |
| `dashboard-all-panels` | 60.0 | 60.0 | 60.0 | ~+0.0% | ~-0.0% | ~-0.0% |
| `table-stream` | 60.0 | 60.0 | 60.0 | ~+0.0% | ~+0.0% | ~+0.0% |
| `text-typing` | 60.0 | 60.0 | 60.0 | ~-0.0% | ~-0.0% | ~-0.0% |
| `text-log-stream` | 60.0 | 60.0 | 60.0 | ~+0.0% | ~+0.0% | ~+0.0% |
| `text-relabel` | 60.0 | 60.0 | 60.0 | ~-0.0% | ~+0.0% | ~-0.0% |
| `scroll-uniform-list` | 60.0 | 60.0 | 59.9 | ~-0.0% | ~-0.2% | ~-0.2% |
| `scroll-variable-list` | 60.0 | 60.0 | 60.0 | ~-0.0% | ~-0.0% | ~-0.0% |
| `scroll-div-cards` | 60.0 | 60.0 | 60.0 | ~-0.0% | ~+0.0% | ~-0.0% |
| `scroll-data-table` | 60.0 | 60.0 | 60.0 | ~-0.0% | ~+0.0% | ~+0.0% |
| `hover-grid` | 60.0 | 60.0 | 60.0 | ~-0.0% | ~+0.0% | ~-0.0% |
| `tree-toggle` | 60.0 | 60.0 | 60.0 | ~+0.0% | ~-0.0% | ~+0.0% |
| `animated-dots` | 60.0 | 60.0 | 60.0 | ~+0.0% | ~-0.0% | ~+0.0% |
| `effects-cards` | 60.0 | 60.0 | 60.0 | ~-0.0% | ~+0.0% | ~-0.0% |

**Missed frames** (intervals over 1.5× the display's, of the measured frames; changes in frames)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `dashboard-one-panel` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `dashboard-all-panels` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `table-stream` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-typing` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-log-stream` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-relabel` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `scroll-uniform-list` | 0 | 0 | 1 | ~+0.00 | ~+1.00 | ~+1.00 |
| `scroll-variable-list` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `scroll-div-cards` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `scroll-data-table` | 0 | 0 | 1 | ~+0.00 | **+1.00** | **+1.00** |
| `hover-grid` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `tree-toggle` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `animated-dots` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `effects-cards` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |

**Process CPU** (% of one core, all threads)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 36.8 | 37.5 | 26.3 | ~+2.1% | -30% | -29% |
| `dashboard-one-panel` | 37.0 | 37.9 | 30.0 | ~+2.5% | -21% | -19% |
| `dashboard-all-panels` | 48.6 | 48.9 | 47.7 | ~+0.6% | ~-2.3% | ~-1.7% |
| `table-stream` | 33.4 | 33.4 | 32.7 | ~-0.2% | ~-2.1% | ~-2.2% |
| `text-typing` | 31.1 | 31.3 | 28.0 | ~+0.8% | -11% | -10% |
| `text-log-stream` | 30.0 | 30.3 | 30.7 | ~+1.1% | ~+1.3% | ~+2.4% |
| `text-relabel` | 69.7 | 67.5 | 66.5 | -3.1% | ~-1.6% | ~-4.6% |
| `scroll-uniform-list` | 32.7 | 33.8 | 24.2 | ~+3.3% | -28% | ~-26% |
| `scroll-variable-list` | 27.7 | 25.9 | 26.4 | ~-6.4% | ~+1.7% | ~-4.8% |
| `scroll-div-cards` | 84.7 | 87.8 | 43.4 | **+3.7%** | -51% | -49% |
| `scroll-data-table` | 30.8 | 31.1 | 29.9 | ~+1.1% | ~-4.1% | ~-3.1% |
| `hover-grid` | 85.6 | 67.5 | 61.9 | -21% | -8.3% | -28% |
| `tree-toggle` | 39.4 | 39.6 | 38.2 | ~+0.5% | ~-3.5% | ~-3.0% |
| `animated-dots` | 33.0 | 30.8 | 30.6 | ~-6.6% | ~-0.7% | ~-7.2% |
| `effects-cards` | 31.4 | 32.9 | 31.7 | ~+4.6% | ~-3.5% | ~+0.9% |

**Memory footprint at the end** (MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 38.5 | 37.9 | 40.7 | ~-1.6% | **+7.5%** | **+5.7%** |
| `dashboard-one-panel` | 38.3 | 37.9 | 40.6 | ~-1.1% | **+7.1%** | **+5.9%** |
| `dashboard-all-panels` | 38.5 | 38.2 | 40.1 | ~-0.9% | **+5.0%** | **+4.0%** |
| `table-stream` | 34.9 | 36.9 | 37.8 | **+5.6%** | ~+2.5% | **+8.3%** |
| `text-typing` | 32.9 | 33.4 | 34.5 | ~+1.6% | ~+3.3% | ~+4.9% |
| `text-log-stream` | 31.0 | 31.6 | 31.9 | ~+1.9% | ~+1.0% | ~+2.9% |
| `text-relabel` | 42.0 | 43.1 | 45.2 | ~+2.7% | **+4.8%** | **+7.6%** |
| `scroll-uniform-list` | 30.7 | 30.8 | 50.2 | ~+0.5% | **+63%** | **+64%** |
| `scroll-variable-list` | 31.7 | 32.6 | 35.9 | ~+3.0% | **+10%** | **+13%** |
| `scroll-div-cards` | 55.8 | 57.8 | 66.7 | **+3.7%** | **+15%** | **+20%** |
| `scroll-data-table` | 33.5 | 35.2 | 102 | **+5.2%** | **+189%** | **+204%** |
| `hover-grid` | 47.1 | 51.9 | 52.5 | **+10%** | ~+1.2% | **+11%** |
| `tree-toggle` | 41.3 | 42.2 | 46.5 | ~+2.0% | **+10%** | **+12%** |
| `animated-dots` | 32.5 | 33.2 | 33.0 | ~+2.1% | ~-0.6% | ~+1.4% |
| `effects-cards` | 35.1 | 36.9 | 37.0 | **+5.1%** | ~+0.1% | **+5.2%** |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | -0.08 | 0.05 | -0.22 | ~+0.12 | ~-0.27 | ~-0.14 |
| `dashboard-one-panel` | -0.22 | 0.00 | -0.11 | ~+0.22 | ~-0.11 | ~+0.11 |
| `dashboard-all-panels` | -0.16 | -0.06 | -0.16 | ~+0.09 | ~-0.09 | ~+0.00 |
| `table-stream` | -0.06 | -0.09 | -0.11 | ~-0.03 | ~-0.02 | ~-0.05 |
| `text-typing` | 0.06 | -0.11 | -0.14 | ~-0.17 | ~-0.03 | ~-0.20 |
| `text-log-stream` | -0.05 | -0.09 | -0.12 | ~-0.05 | ~-0.03 | ~-0.08 |
| `text-relabel` | -0.70 | -0.62 | -0.67 | ~+0.08 | ~-0.05 | ~+0.03 |
| `scroll-uniform-list` | -0.03 | -0.02 | 9.28 | ~+0.02 | **+9.30** | **+9.31** |
| `scroll-variable-list` | 0.05 | 0.17 | 1.36 | ~+0.12 | **+1.19** | **+1.31** |
| `scroll-div-cards` | -0.09 | -0.08 | -0.25 | ~+0.02 | ~-0.17 | ~-0.16 |
| `scroll-data-table` | -0.03 | 0.03 | 37.6 | ~+0.06 | **+37.53** | **+37.59** |
| `hover-grid` | -0.12 | 0.30 | -0.08 | **+0.42** | -0.38 | ~+0.05 |
| `tree-toggle` | -0.31 | -0.12 | -2.20 | ~+0.19 | -2.08 | -1.89 |
| `animated-dots` | -0.08 | -0.12 | -0.08 | ~-0.05 | ~+0.05 | ~+0.00 |
| `effects-cards` | -0.05 | 0.16 | -0.03 | ~+0.20 | ~-0.19 | ~+0.02 |

**Peak live Rust heap** (MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 26.8 | 28.0 | 30.0 | **+4.7%** | **+7.2%** | **+12%** |
| `dashboard-one-panel` | 26.8 | 28.2 | 30.2 | **+5.2%** | **+7.1%** | **+13%** |
| `dashboard-all-panels` | 27.0 | 28.5 | 31.3 | **+5.5%** | **+10%** | **+16%** |
| `table-stream` | 20.1 | 22.1 | 23.0 | **+10%** | **+4.2%** | **+14%** |
| `text-typing` | 14.4 | 16.1 | 17.5 | **+12%** | **+9.0%** | **+22%** |
| `text-log-stream` | 13.6 | 15.4 | 16.0 | **+13%** | **+3.9%** | **+18%** |
| `text-relabel` | 31.7 | 34.4 | 36.4 | **+8.4%** | **+5.8%** | **+15%** |
| `scroll-uniform-list` | 12.3 | 13.8 | 47.7 | **+12%** | **+246%** | **+287%** |
| `scroll-variable-list` | 13.8 | 15.5 | 27.9 | **+12%** | **+80%** | **+102%** |
| `scroll-div-cards` | 47.9 | 50.2 | 63.3 | **+4.9%** | **+26%** | **+32%** |
| `scroll-data-table` | 19.8 | 22.5 | 124 | **+13%** | **+450%** | **+523%** |
| `hover-grid` | 48.9 | 53.0 | 55.3 | **+8.3%** | **+4.5%** | **+13%** |
| `tree-toggle` | 33.5 | 34.5 | 39.7 | ~+2.9% | **+15%** | **+18%** |
| `animated-dots` | 16.3 | 17.0 | 17.7 | **+4.5%** | **+4.2%** | **+8.9%** |
| `effects-cards` | 21.0 | 22.9 | 23.2 | **+9.2%** | ~+1.2% | **+10%** |

**Heap allocations** (per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 28166 | 28171 | 2964 | ~+0.0% | -89% | -89% |
| `dashboard-one-panel` | 28279 | 28284 | 3643 | ~+0.0% | -87% | -87% |
| `dashboard-all-panels` | 33529 | 33534 | 28834 | ~+0.0% | -14% | -14% |
| `table-stream` | 17687 | 17693 | 4881 | ~+0.0% | -72% | -72% |
| `text-typing` | 3570 | 3577 | 1107 | ~+0.2% | -69% | -69% |
| `text-log-stream` | 4616 | 4622 | 5462 | ~+0.1% | **+18%** | **+18%** |
| `text-relabel` | 43237 | 43243 | 39569 | ~+0.0% | -8.5% | -8.5% |
| `scroll-uniform-list` | 3416 | 3423 | 5099 | ~+0.2% | **+49%** | **+49%** |
| `scroll-variable-list` | 656 | 661 | 1110 | ~+0.8% | **+68%** | **+69%** |
| `scroll-div-cards` | 104209 | 104257 | 28050 | ~+0.0% | -73% | -73% |
| `scroll-data-table` | 10602 | 10608 | 13805 | ~+0.1% | **+30%** | **+30%** |
| `hover-grid` | 49949 | 49958 | 38916 | ~+0.0% | -22% | -22% |
| `tree-toggle` | 32191 | 32193 | 22622 | ~+0.0% | -30% | -30% |
| `animated-dots` | 3164 | 3163 | 3243 | ~-0.0% | ~+2.5% | ~+2.5% |
| `effects-cards` | 22974 | 22964 | 2021 | ~-0.0% | -91% | -91% |

**Bytes allocated** (KB per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 17253 | 17236 | 356 | ~-0.1% | -98% | -98% |
| `dashboard-one-panel` | 17294 | 17277 | 1367 | ~-0.1% | -92% | -92% |
| `dashboard-all-panels` | 19228 | 19210 | 11517 | ~-0.1% | -40% | -40% |
| `table-stream` | 5855 | 5827 | 651 | ~-0.5% | -89% | -89% |
| `text-typing` | 2322 | 2306 | 808 | ~-0.7% | -65% | -65% |
| `text-log-stream` | 2952 | 2932 | 2924 | ~-0.7% | ~-0.3% | ~-1.0% |
| `text-relabel` | 15914 | 15880 | 4939 | ~-0.2% | -69% | -69% |
| `scroll-uniform-list` | 1462 | 1456 | 1040 | ~-0.4% | -29% | -29% |
| `scroll-variable-list` | 305 | 290 | 848 | -4.9% | **+193%** | **+178%** |
| `scroll-div-cards` | 70787 | 70781 | 10691 | ~-0.0% | -85% | -85% |
| `scroll-data-table` | 4068 | 4029 | 2198 | ~-0.9% | -45% | -46% |
| `hover-grid` | 34451 | 33422 | 25771 | ~-3.0% | -23% | -25% |
| `tree-toggle` | 18206 | 18202 | 9111 | ~-0.0% | -50% | -50% |
| `animated-dots` | 4538 | 4319 | 4386 | -4.8% | ~+1.6% | -3.3% |
| `effects-cards` | 13730 | 13642 | 615 | ~-0.6% | -95% | -96% |

**Startup: window opened to first frame presented** (ms)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 75.7 | 73.5 | 65.3 | ~-2.9% | ~-11% | ~-14% |
| `dashboard-one-panel` | 70.6 | 65.5 | 73.5 | ~-7.1% | ~+12% | **+4.2%** |
| `dashboard-all-panels` | 66.9 | 71.8 | 68.7 | ~+7.2% | ~-4.3% | ~+2.6% |
| `table-stream` | 65.7 | 70.5 | 65.3 | ~+7.4% | ~-7.3% | ~-0.5% |
| `text-typing` | 78.5 | 58.5 | 68.6 | ~-25% | **+17%** | ~-13% |
| `text-log-stream` | 67.7 | 71.2 | 69.3 | ~+5.3% | ~-2.7% | ~+2.5% |
| `text-relabel` | 71.0 | 68.7 | 74.0 | ~-3.2% | ~+7.6% | ~+4.2% |
| `scroll-uniform-list` | 67.9 | 73.6 | 66.4 | ~+8.5% | ~-10% | ~-2.1% |
| `scroll-variable-list` | 72.4 | 63.7 | 62.4 | ~-12% | ~-2.0% | ~-14% |
| `scroll-div-cards` | 86.5 | 88.6 | 91.2 | ~+2.5% | ~+3.0% | ~+5.5% |
| `scroll-data-table` | 63.5 | 74.1 | 67.9 | **+17%** | ~-8.4% | ~+7.0% |
| `hover-grid` | 75.8 | 69.8 | 67.2 | ~-7.8% | ~-3.8% | ~-11% |
| `tree-toggle` | 67.6 | 73.4 | 69.2 | ~+8.5% | ~-5.7% | ~+2.3% |
| `animated-dots` | 72.0 | 64.8 | 70.3 | ~-10% | ~+8.6% | ~-2.3% |
| `effects-cards` | 78.5 | 64.5 | 65.6 | -18% | ~+1.7% | -16% |

**Startup: the first frame's draw** (ms)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 13.1 | 11.4 | 13.7 | ~-13% | **+20%** | ~+4.2% |
| `dashboard-one-panel` | 11.0 | 10.6 | 11.8 | ~-3.0% | **+11%** | **+7.7%** |
| `dashboard-all-panels` | 10.8 | 10.8 | 12.1 | ~+0.6% | **+11%** | **+12%** |
| `table-stream` | 9.7 | 9.8 | 10.1 | ~+1.9% | ~+3.2% | ~+5.1% |
| `text-typing` | 13.0 | 12.8 | 10.8 | ~-1.7% | ~-16% | ~-17% |
| `text-log-stream` | 6.9 | 6.3 | 5.4 | ~-10% | ~-14% | ~-23% |
| `text-relabel` | 16.0 | 15.4 | 15.5 | ~-3.5% | ~+0.6% | ~-2.9% |
| `scroll-uniform-list` | 4.2 | 4.3 | 4.6 | ~+2.8% | ~+7.2% | ~+10% |
| `scroll-variable-list` | 4.7 | 4.3 | 4.2 | ~-6.8% | ~-4.1% | ~-11% |
| `scroll-div-cards` | 34.8 | 34.5 | 35.3 | ~-0.8% | ~+2.3% | ~+1.4% |
| `scroll-data-table` | 10.4 | 7.8 | 8.4 | -25% | ~+6.5% | -20% |
| `hover-grid` | 15.1 | 12.4 | 12.9 | -18% | ~+4.0% | -14% |
| `tree-toggle` | 17.0 | 10.6 | 11.7 | ~-37% | ~+10% | ~-31% |
| `animated-dots` | 4.2 | 1.6 | 1.8 | -62% | ~+14% | ~-57% |
| `effects-cards` | 11.3 | 11.3 | 8.9 | ~+0.4% | ~-21% | ~-21% |

### macOS native (Metal)

viewport 1280×801 @2x; display interval ≈8.3 ms; 135 of 135 runs reported, 14.9 min

**Frame cost: CPU** (ms of main-thread CPU per frame, mean)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 4.56 | 4.51 | 3.12 | ~-1.0% | -31% | -32% |
| `dashboard-one-panel` | 4.61 | 4.58 | 2.87 | ~-0.7% | -37% | -38% |
| `dashboard-all-panels` | 7.10 | 7.11 | 6.64 | ~+0.2% | -6.6% | -6.4% |
| `table-stream` | 3.88 | 3.74 | 2.70 | -3.7% | -28% | -30% |
| `text-typing` | 2.82 | 2.90 | 3.20 | ~+2.9% | **+10%** | **+14%** |
| `text-log-stream` | 2.87 | 2.71 | 2.91 | ~-5.4% | ~+7.3% | ~+1.4% |
| `text-relabel` | 12.5 | 11.0 | 11.0 | -12% | ~-0.5% | -12% |
| `scroll-uniform-list` | 3.29 | 3.14 | 2.87 | ~-4.3% | ~-8.6% | ~-13% |
| `scroll-variable-list` | 3.05 | 2.90 | 1.77 | ~-5.2% | -39% | -42% |
| `scroll-div-cards` | 13.4 | 13.8 | 5.14 | ~+2.8% | -63% | -62% |
| `scroll-data-table` | 3.26 | 3.18 | 3.42 | ~-2.7% | **+7.8%** | **+4.9%** |
| `hover-grid` | 14.4 | 7.62 | 6.88 | -47% | -10% | -52% |
| `tree-toggle` | 4.54 | 4.60 | 4.56 | ~+1.4% | ~-0.9% | ~+0.6% |
| `animated-dots` | 2.93 | 3.11 | 2.91 | ~+6.3% | ~-6.4% | ~-0.6% |
| `effects-cards` | 3.65 | 3.64 | 2.58 | ~-0.1% | ~-29% | ~-29% |

**Window::draw, median** (ms per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 4.39 | 4.34 | 2.93 | ~-1.1% | -33% | -33% |
| `dashboard-one-panel` | 4.45 | 4.43 | 2.60 | ~-0.5% | -41% | -42% |
| `dashboard-all-panels` | 6.88 | 6.88 | 6.40 | ~+0.0% | -7.0% | -7.0% |
| `table-stream` | 3.75 | 3.58 | 2.57 | -4.5% | -28% | -31% |
| `text-typing` | 2.69 | 2.72 | 2.95 | ~+1.2% | **+8.3%** | ~+10% |
| `text-log-stream` | 2.64 | 2.43 | 2.81 | ~-7.9% | ~+16% | ~+6.7% |
| `text-relabel` | 12.3 | 10.8 | 10.8 | -12% | ~-0.2% | -12% |
| `scroll-uniform-list` | 2.79 | 2.74 | 2.62 | ~-1.9% | ~-4.3% | ~-6.1% |
| `scroll-variable-list` | 2.82 | 2.62 | 1.10 | ~-7.1% | -58% | -61% |
| `scroll-div-cards` | 12.9 | 13.2 | 4.67 | ~+2.9% | -65% | -64% |
| `scroll-data-table` | 3.11 | 2.98 | 3.04 | ~-4.3% | ~+2.2% | ~-2.2% |
| `hover-grid` | 13.8 | 6.96 | 6.06 | -49% | -13% | -56% |
| `tree-toggle` | 4.34 | 4.38 | 3.60 | ~+0.8% | -18% | -17% |
| `animated-dots` | 2.64 | 2.69 | 2.64 | ~+2.1% | ~-1.9% | ~+0.2% |
| `effects-cards` | 3.51 | 3.57 | 2.38 | ~+1.6% | ~-33% | ~-32% |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 4.51 | 4.49 | 3.70 | ~-0.6% | -17% | -18% |
| `dashboard-one-panel` | 4.57 | 4.57 | 3.37 | ~+0.0% | -26% | -26% |
| `dashboard-all-panels` | 7.04 | 7.13 | 6.67 | ~+1.2% | -6.4% | -5.2% |
| `table-stream` | 4.00 | 3.86 | 3.16 | -3.4% | ~-18% | -21% |
| `text-typing` | 3.47 | 3.45 | 3.60 | ~-0.5% | ~+4.5% | ~+3.9% |
| `text-log-stream` | 3.36 | 3.21 | 3.33 | ~-4.4% | ~+3.7% | ~-0.9% |
| `text-relabel` | 12.7 | 11.3 | 11.1 | -11% | ~-2.0% | -12% |
| `scroll-uniform-list` | 4.12 | 4.07 | 3.77 | ~-1.2% | -7.2% | -8.4% |
| `scroll-variable-list` | 3.71 | 3.42 | 2.42 | -7.9% | -29% | -35% |
| `scroll-div-cards` | 13.3 | 13.7 | 4.91 | ~+2.6% | -64% | -63% |
| `scroll-data-table` | 3.44 | 3.35 | 4.05 | ~-2.7% | **+21%** | **+18%** |
| `hover-grid` | 14.1 | 7.25 | 6.39 | -49% | -12% | -55% |
| `tree-toggle` | 4.71 | 4.81 | 6.38 | ~+2.1% | **+33%** | **+35%** |
| `animated-dots` | 3.69 | 3.58 | 3.30 | ~-3.1% | -8.0% | -11% |
| `effects-cards` | 3.73 | 3.70 | 2.89 | ~-0.8% | -22% | -22% |

**Frame rate** (frames per second)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 120 | 120 | 120 | ~-0.0% | ~-0.0% | ~-0.0% |
| `dashboard-one-panel` | 120 | 120 | 120 | ~-0.0% | ~-0.0% | ~-0.0% |
| `dashboard-all-panels` | 120 | 120 | 120 | ~-0.0% | ~+0.0% | ~+0.0% |
| `table-stream` | 120 | 120 | 120 | ~+0.0% | ~-0.1% | ~-0.1% |
| `text-typing` | 120 | 120 | 120 | ~-0.1% | ~+0.1% | ~-0.0% |
| `text-log-stream` | 120 | 120 | 120 | ~-0.1% | ~+0.1% | ~-0.0% |
| `text-relabel` | 80.0 | 90.7 | 91.2 | +13% | ~+0.5% | +14% |
| `scroll-uniform-list` | 120 | 120 | 120 | ~+0.0% | ~-0.2% | ~-0.2% |
| `scroll-variable-list` | 120 | 120 | 120 | ~-0.0% | ~-0.2% | ~-0.2% |
| `scroll-div-cards` | 74.5 | 72.5 | 120 | ~-2.7% | +66% | +61% |
| `scroll-data-table` | 120 | 120 | 119 | ~+0.0% | ~-0.6% | ~-0.6% |
| `hover-grid` | 69.2 | 120 | 120 | +73% | ~-0.0% | +73% |
| `tree-toggle` | 120 | 120 | 120 | ~-0.0% | ~+0.0% | ~-0.0% |
| `animated-dots` | 120 | 120 | 120 | ~+0.0% | ~-0.0% | ~+0.0% |
| `effects-cards` | 120 | 120 | 120 | ~+0.0% | ~-0.0% | ~+0.0% |

**Missed frames** (intervals over 1.5× the display's, of the measured frames; changes in frames)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 0 | 1 | 1 | ~+1.00 | ~+0.00 | ~+1.00 |
| `dashboard-one-panel` | 0 | 0 | 1 | ~+0.00 | ~+1.00 | ~+1.00 |
| `dashboard-all-panels` | 1 | 1 | 1 | ~+0.00 | ~+0.00 | ~+0.00 |
| `table-stream` | 0 | 0 | 1 | ~+0.00 | ~+1.00 | ~+1.00 |
| `text-typing` | 1 | 1 | 2 | ~+0.00 | ~+1.00 | ~+1.00 |
| `text-log-stream` | 1 | 1 | 2 | ~+0.00 | ~+1.00 | ~+1.00 |
| `text-relabel` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `scroll-uniform-list` | 1 | 1 | 2 | ~+0.00 | ~+1.00 | ~+1.00 |
| `scroll-variable-list` | 0 | 1 | 1 | ~+1.00 | ~+0.00 | ~+1.00 |
| `scroll-div-cards` | 0 | 0 | 1 | ~+0.00 | **+1.00** | **+1.00** |
| `scroll-data-table` | 0 | 1 | 3 | ~+1.00 | ~+2.00 | **+3.00** |
| `hover-grid` | 0 | 1 | 0 | **+1.00** | ~-1.00 | ~+0.00 |
| `tree-toggle` | 1 | 0 | 0 | ~-1.00 | ~+0.00 | ~-1.00 |
| `animated-dots` | 2 | 1 | 1 | ~-1.00 | ~+0.00 | ~-1.00 |
| `effects-cards` | 0 | 0 | 1 | ~+0.00 | **+1.00** | **+1.00** |

**Process CPU** (% of one core, all threads)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 56.8 | 56.3 | 43.8 | ~-0.9% | -22% | -23% |
| `dashboard-one-panel` | 57.4 | 57.0 | 39.3 | ~-0.7% | -31% | -31% |
| `dashboard-all-panels` | 87.6 | 87.9 | 82.4 | ~+0.3% | -6.3% | -5.9% |
| `table-stream` | 48.9 | 47.5 | 36.0 | ~-2.9% | -24% | -26% |
| `text-typing` | 37.1 | 38.7 | 45.2 | ~+4.1% | **+17%** | **+22%** |
| `text-log-stream` | 38.7 | 36.8 | 39.4 | ~-4.8% | ~+6.9% | ~+1.7% |
| `text-relabel` | 101 | 102 | 101 | ~+0.2% | ~-0.1% | ~+0.1% |
| `scroll-uniform-list` | 45.9 | 44.1 | 41.7 | ~-4.0% | ~-5.4% | ~-9.2% |
| `scroll-variable-list` | 43.5 | 41.7 | 29.3 | ~-4.0% | -30% | -33% |
| `scroll-div-cards` | 101 | 101 | 64.2 | ~-0.0% | -37% | -37% |
| `scroll-data-table` | 41.9 | 41.2 | 45.2 | ~-1.7% | **+10%** | **+7.9%** |
| `hover-grid` | 101 | 94.4 | 85.6 | -6.9% | -9.3% | -16% |
| `tree-toggle` | 56.8 | 57.4 | 57.0 | ~+1.0% | ~-0.7% | ~+0.4% |
| `animated-dots` | 38.9 | 41.8 | 38.2 | ~+7.4% | ~-8.6% | ~-1.9% |
| `effects-cards` | 45.9 | 45.9 | 36.3 | ~+0.1% | ~-21% | ~-21% |

**Memory footprint at the end** (MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 137 | 139 | 149 | ~+1.5% | **+7.1%** | **+8.7%** |
| `dashboard-one-panel` | 137 | 141 | 150 | ~+2.8% | **+5.8%** | **+8.8%** |
| `dashboard-all-panels` | 138 | 141 | 144 | ~+2.3% | ~+2.0% | **+4.3%** |
| `table-stream` | 133 | 143 | 137 | **+7.1%** | ~-3.6% | **+3.2%** |
| `text-typing` | 129 | 130 | 139 | ~+1.3% | **+6.7%** | **+8.1%** |
| `text-log-stream` | 128 | 131 | 139 | ~+2.0% | ~+6.1% | **+8.3%** |
| `text-relabel` | 146 | 151 | 151 | **+3.4%** | ~+0.4% | **+3.8%** |
| `scroll-uniform-list` | 132 | 133 | 237 | ~+1.4% | **+78%** | **+80%** |
| `scroll-variable-list` | 130 | 141 | 221 | **+9.1%** | **+56%** | **+70%** |
| `scroll-div-cards` | 161 | 165 | 256 | ~+2.1% | **+55%** | **+59%** |
| `scroll-data-table` | 133 | 142 | 317 | **+6.3%** | **+124%** | **+138%** |
| `hover-grid` | 169 | 170 | 175 | ~+0.7% | ~+2.7% | **+3.4%** |
| `tree-toggle` | 145 | 145 | 150 | ~+0.4% | **+3.6%** | **+4.0%** |
| `animated-dots` | 132 | 128 | 129 | ~-3.2% | ~+1.1% | ~-2.2% |
| `effects-cards` | 134 | 135 | 143 | ~+1.2% | **+5.8%** | **+7.1%** |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | -0.19 | -1.47 | -1.38 | ~-1.28 | ~+0.09 | ~-1.19 |
| `dashboard-one-panel` | -0.17 | 0.39 | -3.42 | ~+0.56 | ~-3.81 | -3.25 |
| `dashboard-all-panels` | -0.34 | -0.67 | -0.27 | ~-0.33 | ~+0.41 | ~+0.08 |
| `table-stream` | -0.53 | 6.31 | -0.95 | ~+6.84 | -7.27 | ~-0.42 |
| `text-typing` | 0.39 | 0.00 | 2.09 | ~-0.39 | ~+2.09 | ~+1.70 |
| `text-log-stream` | 0.33 | 0.41 | 7.44 | ~+0.08 | ~+7.03 | **+7.11** |
| `text-relabel` | -3.64 | 0.16 | -1.88 | **+3.80** | ~-2.03 | ~+1.77 |
| `scroll-uniform-list` | 4.59 | 7.11 | 18.2 | ~+2.52 | **+11.08** | **+13.59** |
| `scroll-variable-list` | 0.33 | 5.94 | 3.14 | ~+5.61 | ~-2.80 | ~+2.81 |
| `scroll-div-cards` | -2.19 | 0.11 | 3.50 | **+2.30** | **+3.39** | **+5.69** |
| `scroll-data-table` | -0.28 | 5.69 | 54.7 | ~+5.97 | **+49.03** | **+55.00** |
| `hover-grid` | -2.19 | -0.69 | 0.97 | ~+1.50 | ~+1.66 | ~+3.16 |
| `tree-toggle` | 0.36 | -0.20 | -2.34 | ~-0.56 | -2.14 | ~-2.70 |
| `animated-dots` | -0.42 | 1.58 | -0.08 | ~+2.00 | ~-1.66 | ~+0.34 |
| `effects-cards` | 0.09 | -0.81 | -3.45 | ~-0.91 | -2.64 | -3.55 |

**Peak live Rust heap** (MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 27.0 | 29.4 | 30.8 | **+8.7%** | **+5.0%** | **+14%** |
| `dashboard-one-panel` | 27.1 | 29.5 | 31.1 | **+9.1%** | **+5.2%** | **+15%** |
| `dashboard-all-panels` | 27.5 | 29.9 | 32.6 | **+8.6%** | **+9.0%** | **+18%** |
| `table-stream` | 20.7 | 23.4 | 24.3 | **+13%** | **+3.9%** | **+18%** |
| `text-typing` | 13.7 | 16.6 | 18.1 | **+21%** | **+8.7%** | **+32%** |
| `text-log-stream` | 13.9 | 16.8 | 17.2 | **+20%** | ~+2.4% | **+23%** |
| `text-relabel` | 34.1 | 39.0 | 40.8 | **+14%** | **+4.5%** | **+19%** |
| `scroll-uniform-list` | 11.5 | 13.2 | 49.9 | **+15%** | **+277%** | **+332%** |
| `scroll-variable-list` | 15.4 | 18.1 | 42.6 | **+18%** | **+136%** | **+177%** |
| `scroll-div-cards` | 46.5 | 49.7 | 68.8 | **+6.8%** | **+38%** | **+48%** |
| `scroll-data-table` | 21.4 | 24.8 | 143 | **+16%** | **+476%** | **+565%** |
| `hover-grid` | 57.2 | 60.4 | 62.9 | **+5.5%** | **+4.1%** | **+10%** |
| `tree-toggle` | 31.5 | 32.4 | 37.7 | ~+2.8% | **+17%** | **+20%** |
| `animated-dots` | 14.2 | 14.7 | 15.4 | **+4.0%** | **+4.9%** | **+9.0%** |
| `effects-cards` | 20.2 | 23.5 | 23.4 | **+17%** | ~-0.5% | **+16%** |

**Heap allocations** (per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 27941 | 27949 | 2811 | ~+0.0% | -90% | -90% |
| `dashboard-one-panel` | 28054 | 28062 | 3421 | ~+0.0% | -88% | -88% |
| `dashboard-all-panels` | 33304 | 33312 | 28611 | ~+0.0% | -14% | -14% |
| `table-stream` | 18945 | 18952 | 4725 | ~+0.0% | -75% | -75% |
| `text-typing` | 3415 | 3421 | 950 | ~+0.2% | -72% | -72% |
| `text-log-stream` | 4461 | 4468 | 5306 | ~+0.2% | **+19%** | **+19%** |
| `text-relabel` | 37701 | 37700 | 32545 | ~-0.0% | -14% | -14% |
| `scroll-uniform-list` | 3019 | 3022 | 4185 | ~+0.1% | **+38%** | **+39%** |
| `scroll-variable-list` | 834 | 839 | 1037 | ~+0.7% | **+24%** | **+24%** |
| `scroll-div-cards` | 104055 | 104062 | 27441 | ~+0.0% | -74% | -74% |
| `scroll-data-table` | 9598 | 9604 | 12174 | ~+0.1% | **+27%** | **+27%** |
| `hover-grid` | 49879 | 49794 | 37173 | ~-0.2% | -25% | -25% |
| `tree-toggle` | 32036 | 32035 | 22611 | ~-0.0% | -29% | -29% |
| `animated-dots` | 3035 | 3034 | 3114 | ~-0.0% | ~+2.6% | ~+2.6% |
| `effects-cards` | 22823 | 22749 | 1998 | ~-0.3% | -91% | -91% |

**Bytes allocated** (KB per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 17373 | 17277 | 403 | ~-0.6% | -98% | -98% |
| `dashboard-one-panel` | 17415 | 17319 | 1408 | ~-0.6% | -92% | -92% |
| `dashboard-all-panels` | 19348 | 19252 | 11557 | ~-0.5% | -40% | -40% |
| `table-stream` | 5881 | 5847 | 627 | ~-0.6% | -89% | -89% |
| `text-typing` | 2326 | 2284 | 785 | ~-1.8% | -66% | -66% |
| `text-log-stream` | 2952 | 2911 | 2900 | ~-1.4% | ~-0.4% | ~-1.8% |
| `text-relabel` | 15578 | 15492 | 4385 | ~-0.6% | -72% | -72% |
| `scroll-uniform-list` | 1356 | 1331 | 1016 | ~-1.9% | -24% | -25% |
| `scroll-variable-list` | 484 | 445 | 706 | -8.0% | **+59%** | **+46%** |
| `scroll-div-cards` | 70782 | 70760 | 10713 | ~-0.0% | -85% | -85% |
| `scroll-data-table` | 3781 | 3705 | 2622 | ~-2.0% | -29% | -31% |
| `hover-grid` | 35583 | 33768 | 26081 | -5.1% | -23% | -27% |
| `tree-toggle` | 18183 | 18180 | 9187 | ~-0.0% | -49% | -49% |
| `animated-dots` | 4511 | 4292 | 4360 | -4.9% | ~+1.6% | -3.4% |
| `effects-cards` | 14052 | 13693 | 658 | ~-2.6% | -95% | -95% |

**Startup: window opened to first frame presented** (ms)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 74.3 | 68.4 | 67.3 | ~-8.0% | ~-1.6% | ~-9.5% |
| `dashboard-one-panel` | 72.6 | 71.5 | 66.9 | ~-1.5% | ~-6.3% | ~-7.8% |
| `dashboard-all-panels` | 67.4 | 74.0 | 65.8 | ~+10% | ~-11% | ~-2.3% |
| `table-stream` | 73.0 | 68.0 | 71.4 | ~-6.8% | ~+4.9% | ~-2.2% |
| `text-typing` | 71.4 | 80.8 | 73.1 | ~+13% | ~-10% | ~+2.3% |
| `text-log-stream` | 70.8 | 66.3 | 66.7 | ~-6.4% | ~+0.6% | ~-5.8% |
| `text-relabel` | 77.4 | 69.0 | 65.6 | ~-11% | ~-4.9% | ~-15% |
| `scroll-uniform-list` | 63.8 | 65.7 | 63.4 | ~+3.0% | ~-3.5% | ~-0.6% |
| `scroll-variable-list` | 71.9 | 76.3 | 69.3 | ~+6.1% | ~-9.2% | ~-3.6% |
| `scroll-div-cards` | 96.5 | 95.9 | 97.8 | ~-0.6% | ~+1.9% | ~+1.3% |
| `scroll-data-table` | 77.2 | 69.3 | 67.9 | ~-10% | ~-2.0% | ~-12% |
| `hover-grid` | 79.2 | 65.7 | 70.6 | ~-17% | ~+7.4% | -11% |
| `tree-toggle` | 69.7 | 65.0 | 66.3 | ~-6.7% | ~+1.9% | ~-4.9% |
| `animated-dots` | 58.5 | 61.2 | 59.5 | ~+4.7% | ~-2.8% | ~+1.7% |
| `effects-cards` | 64.3 | 66.8 | 68.2 | ~+4.0% | ~+2.1% | ~+6.1% |

**Startup: the first frame's draw** (ms)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 13.3 | 12.4 | 12.5 | ~-6.8% | ~+0.8% | ~-6.0% |
| `dashboard-one-panel` | 12.3 | 11.8 | 12.7 | ~-3.5% | ~+7.1% | ~+3.4% |
| `dashboard-all-panels` | 11.8 | 11.9 | 12.8 | ~+1.5% | ~+6.8% | **+8.4%** |
| `table-stream` | 11.2 | 10.3 | 11.1 | ~-8.0% | **+7.4%** | ~-1.2% |
| `text-typing` | 11.3 | 11.5 | 11.5 | ~+1.5% | ~+0.1% | ~+1.6% |
| `text-log-stream` | 6.1 | 5.8 | 5.8 | ~-4.8% | ~+0.2% | ~-4.6% |
| `text-relabel` | 17.3 | 15.5 | 16.1 | ~-10% | ~+3.4% | -6.9% |
| `scroll-uniform-list` | 5.0 | 4.8 | 4.8 | ~-4.4% | ~+0.9% | ~-3.5% |
| `scroll-variable-list` | 6.4 | 5.5 | 5.3 | ~-14% | ~-3.0% | ~-16% |
| `scroll-div-cards` | 34.3 | 35.4 | 36.2 | ~+3.1% | ~+2.3% | ~+5.5% |
| `scroll-data-table` | 9.5 | 8.2 | 8.6 | ~-13% | ~+4.1% | ~-9.4% |
| `hover-grid` | 18.6 | 12.4 | 13.1 | -34% | ~+6.3% | -29% |
| `tree-toggle` | 9.7 | 9.9 | 10.7 | ~+1.8% | ~+7.5% | **+9.5%** |
| `animated-dots` | 1.8 | 1.5 | 1.8 | ~-13% | **+14%** | ~-1.2% |
| `effects-cards` | 9.8 | 9.0 | 9.3 | ~-7.8% | ~+3.4% | ~-4.7% |

### WASM in Chromium, WebGL2

viewport 1280×800 @2x; display interval ≈9.0 ms; Chromium 147.0.7727.15, backend gl; 135 of 135 runs reported, 16.8 min

**Frame cost: CPU** (ms of Chrome main-thread task time per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 5.67 | 5.75 | 2.39 | ~+1.4% | ~-58% | ~-58% |
| `dashboard-one-panel` | 5.76 | 5.81 | 2.27 | ~+0.8% | -61% | -61% |
| `dashboard-all-panels` | 7.91 | 8.09 | 6.96 | ~+2.3% | -14% | -12% |
| `table-stream` | 3.68 | 3.51 | 2.07 | -4.7% | -41% | -44% |
| `text-typing` | 2.58 | 2.49 | 1.99 | ~-3.6% | -20% | -23% |
| `text-log-stream` | 2.12 | 2.12 | 2.11 | ~-0.0% | ~-0.5% | ~-0.6% |
| `text-relabel` | 9.99 | 8.57 | 7.95 | -14% | -7.3% | -20% |
| `scroll-uniform-list` | 2.11 | 2.11 | 1.62 | ~+0.3% | -23% | -23% |
| `scroll-variable-list` | 2.05 | 2.02 | 1.31 | ~-1.7% | -35% | -36% |
| `scroll-div-cards` | 17.5 | 18.0 | 6.16 | ~+2.9% | -66% | -65% |
| `scroll-data-table` | 3.25 | 3.06 | 3.11 | -5.9% | ~+1.6% | -4.4% |
| `hover-grid` | 18.8 | 11.4 | 10.6 | -40% | -6.5% | -43% |
| `tree-toggle` | 5.41 | 5.59 | 5.24 | **+3.2%** | -6.3% | -3.3% |
| `animated-dots` | 2.55 | 2.42 | 2.54 | ~-5.0% | ~+4.9% | ~-0.3% |
| `effects-cards` | 5.19 | 5.31 | 0.77 | ~+2.4% | -86% | -85% |

**Window::draw, median** (ms per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 5.28 | 5.34 | 1.88 | ~+1.2% | ~-65% | ~-64% |
| `dashboard-one-panel` | 5.35 | 5.39 | 1.91 | ~+0.6% | -65% | -64% |
| `dashboard-all-panels` | 7.27 | 7.37 | 6.03 | ~+1.5% | -18% | -17% |
| `table-stream` | 3.40 | 3.21 | 1.78 | -5.6% | -45% | -48% |
| `text-typing` | 2.37 | 2.27 | 1.46 | -4.4% | -36% | -38% |
| `text-log-stream` | 1.91 | 1.85 | 1.90 | -3.2% | ~+2.4% | ~-0.8% |
| `text-relabel` | 9.52 | 8.10 | 7.41 | -15% | -8.5% | -22% |
| `scroll-uniform-list` | 1.75 | 1.70 | 1.10 | ~-2.8% | -35% | -37% |
| `scroll-variable-list` | 1.50 | 1.43 | 0.55 | ~-5.0% | -61% | -63% |
| `scroll-div-cards` | 16.8 | 17.3 | 5.17 | **+3.1%** | -70% | -69% |
| `scroll-data-table` | 2.96 | 2.80 | 2.57 | ~-5.6% | ~-8.1% | -13% |
| `hover-grid` | 17.4 | 9.63 | 8.84 | -45% | -8.2% | -49% |
| `tree-toggle` | 5.15 | 5.33 | 3.89 | **+3.4%** | -27% | -25% |
| `animated-dots` | 2.32 | 2.11 | 2.27 | ~-8.8% | ~+7.6% | ~-1.9% |
| `effects-cards` | 4.79 | 4.90 | 0.51 | ~+2.3% | -90% | -89% |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 5.42 | 5.49 | 2.44 | ~+1.3% | ~-56% | ~-55% |
| `dashboard-one-panel` | 5.96 | 5.89 | 2.49 | ~-1.3% | -58% | -58% |
| `dashboard-all-panels` | 8.46 | 8.57 | 8.42 | ~+1.3% | ~-1.7% | ~-0.5% |
| `table-stream` | 3.59 | 3.40 | 2.08 | ~-5.1% | -39% | -42% |
| `text-typing` | 2.50 | 2.45 | 2.25 | ~-2.2% | -8.0% | -10% |
| `text-log-stream` | 2.15 | 2.13 | 2.17 | ~-0.9% | ~+2.0% | ~+1.1% |
| `text-relabel` | 9.80 | 8.32 | 7.73 | -15% | -7.0% | -21% |
| `scroll-uniform-list` | 2.24 | 2.28 | 1.48 | ~+1.7% | -35% | -34% |
| `scroll-variable-list` | 2.18 | 2.20 | 1.30 | ~+1.1% | -41% | -40% |
| `scroll-div-cards` | 17.1 | 17.7 | 5.56 | ~+3.3% | -69% | -68% |
| `scroll-data-table` | 3.16 | 2.99 | 3.04 | -5.5% | ~+1.9% | -3.8% |
| `hover-grid` | 18.7 | 11.2 | 9.59 | -40% | -14% | -49% |
| `tree-toggle` | 5.57 | 5.76 | 6.96 | **+3.5%** | **+21%** | **+25%** |
| `animated-dots` | 2.64 | 2.44 | 2.59 | ~-7.6% | ~+6.2% | ~-1.9% |
| `effects-cards` | 5.02 | 5.11 | 0.69 | ~+1.7% | -86% | -86% |

**Frame rate** (frames per second)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 104 | 104 | 104 | ~-0.3% | ~-0.0% | ~-0.3% |
| `dashboard-one-panel` | 104 | 103 | 103 | ~-0.5% | ~-0.0% | ~-0.5% |
| `dashboard-all-panels` | 104 | 103 | 103 | ~-1.3% | ~-0.0% | ~-1.3% |
| `table-stream` | 120 | 120 | 120 | ~+0.0% | ~+0.0% | ~+0.0% |
| `text-typing` | 120 | 120 | 120 | ~+0.0% | ~+0.0% | ~+0.0% |
| `text-log-stream` | 120 | 120 | 120 | ~-0.0% | ~+0.0% | ~-0.0% |
| `text-relabel` | 99.7 | 116 | 120 | +17% | +3.3% | +20% |
| `scroll-uniform-list` | 120 | 120 | 120 | ~+0.0% | ~-0.0% | ~-0.0% |
| `scroll-variable-list` | 120 | 120 | 120 | ~+0.0% | ~+0.0% | ~+0.0% |
| `scroll-div-cards` | 56.9 | 55.3 | 120 | ~-2.8% | +117% | +111% |
| `scroll-data-table` | 118 | 117 | 119 | ~-0.8% | ~+2.3% | ~+1.4% |
| `hover-grid` | 53.0 | 87.5 | 93.8 | +65% | +7.2% | +77% |
| `tree-toggle` | 120 | 120 | 120 | ~+0.0% | ~-0.0% | ~-0.0% |
| `animated-dots` | 120 | 120 | 120 | ~+0.0% | ~+0.0% | ~+0.0% |
| `effects-cards` | 63.4 | 62.5 | 62.5 | ~-1.4% | ~-0.0% | ~-1.4% |

**Missed frames** (intervals over 1.5× the display's, of the measured frames; changes in frames)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `dashboard-one-panel` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `dashboard-all-panels` | 0 | 0 | 1 | ~+0.00 | ~+1.00 | ~+1.00 |
| `table-stream` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-typing` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-log-stream` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-relabel` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `scroll-uniform-list` | 0 | 0 | 1 | ~+0.00 | ~+1.00 | ~+1.00 |
| `scroll-variable-list` | 0 | 0 | 1 | ~+0.00 | **+1.00** | **+1.00** |
| `scroll-div-cards` | 0 | 0 | 11.0 | ~+0.00 | **+11.00** | **+11.00** |
| `scroll-data-table` | 0 | 0 | 2 | ~+0.00 | **+2.00** | ~+2.00 |
| `hover-grid` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `tree-toggle` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `animated-dots` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `effects-cards` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |

**WASM linear memory at the end** (MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 51.4 | 53.6 | 56.1 | **+4.4%** | **+4.7%** | **+9.2%** |
| `dashboard-one-panel` | 52.4 | 54.4 | 57.0 | **+3.9%** | **+4.7%** | **+8.8%** |
| `dashboard-all-panels` | 52.4 | 54.7 | 57.4 | **+4.4%** | **+5.0%** | **+10%** |
| `table-stream` | 46.5 | 49.1 | 50.8 | **+5.5%** | **+3.4%** | **+9.1%** |
| `text-typing` | 41.6 | 44.2 | 45.4 | **+6.3%** | ~+2.5% | **+9.0%** |
| `text-log-stream` | 40.1 | 42.5 | 42.9 | **+5.9%** | ~+0.9% | **+6.9%** |
| `text-relabel` | 58.0 | 63.9 | 65.9 | **+10%** | **+3.1%** | **+14%** |
| `scroll-uniform-list` | 38.8 | 40.4 | 69.7 | **+4.0%** | **+73%** | **+80%** |
| `scroll-variable-list` | 40.8 | 43.4 | 59.2 | **+6.3%** | **+37%** | **+45%** |
| `scroll-div-cards` | 67.4 | 68.1 | 86.2 | ~+1.0% | **+27%** | **+28%** |
| `scroll-data-table` | 50.7 | 54.2 | 169 | **+6.9%** | **+213%** | **+234%** |
| `hover-grid` | 81.1 | 85.6 | 87.8 | **+5.6%** | ~+2.5% | **+8.2%** |
| `tree-toggle` | 53.2 | 54.0 | 59.8 | ~+1.4% | **+11%** | **+12%** |
| `animated-dots` | 40.5 | 41.2 | 42.1 | ~+1.9% | ~+2.1% | **+4.0%** |
| `effects-cards` | 45.8 | 48.9 | 49.8 | **+6.8%** | ~+1.8% | **+8.7%** |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 0.00 | 0.00 | 0.00 | ~+0.00 | ~+0.00 | ~+0.00 |
| `dashboard-one-panel` | 0.75 | 1.00 | 0.75 | ~+0.25 | ~-0.25 | ~+0.00 |
| `dashboard-all-panels` | 0.75 | 0.50 | 0.75 | ~-0.25 | ~+0.25 | ~+0.00 |
| `table-stream` | 0.00 | 0.00 | 0.31 | ~+0.00 | ~+0.31 | ~+0.31 |
| `text-typing` | 0.00 | 0.31 | 0.31 | **+0.31** | ~+0.00 | ~+0.31 |
| `text-log-stream` | 0.50 | 0.19 | 0.56 | ~-0.31 | **+0.38** | ~+0.06 |
| `text-relabel` | 0.00 | 0.00 | 0.00 | ~+0.00 | ~+0.00 | ~+0.00 |
| `scroll-uniform-list` | 0.00 | 0.00 | 15.4 | ~+0.00 | **+15.44** | **+15.44** |
| `scroll-variable-list` | 0.38 | 0.00 | 9.00 | ~-0.38 | **+9.00** | **+8.62** |
| `scroll-div-cards` | 0.00 | 0.38 | 1.06 | ~+0.38 | **+0.69** | **+1.06** |
| `scroll-data-table` | 2.62 | 2.62 | 65.4 | ~+0.00 | **+62.75** | **+62.75** |
| `hover-grid` | 1.50 | 0.50 | 0.44 | -1.00 | ~-0.06 | -1.06 |
| `tree-toggle` | 0.00 | 0.00 | 0.00 | ~+0.00 | ~+0.00 | ~+0.00 |
| `animated-dots` | 0.31 | 0.31 | 0.62 | ~+0.00 | ~+0.31 | ~+0.31 |
| `effects-cards` | 0.38 | 0.31 | 0.38 | ~-0.06 | ~+0.06 | ~+0.00 |

**Peak live Rust heap** (MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 44.7 | 46.9 | 48.3 | **+4.9%** | **+3.0%** | **+8.1%** |
| `dashboard-one-panel` | 44.7 | 46.9 | 48.5 | **+4.9%** | **+3.4%** | **+8.5%** |
| `dashboard-all-panels` | 45.0 | 47.4 | 50.1 | **+5.4%** | **+5.5%** | **+11%** |
| `table-stream` | 39.3 | 42.3 | 43.7 | **+7.6%** | **+3.2%** | **+11%** |
| `text-typing` | 35.4 | 37.9 | 38.7 | **+7.1%** | ~+2.2% | **+9.5%** |
| `text-log-stream` | 33.5 | 36.2 | 36.2 | **+8.0%** | ~+0.1% | **+8.1%** |
| `text-relabel` | 49.8 | 55.0 | 56.9 | **+11%** | **+3.3%** | **+14%** |
| `scroll-uniform-list` | 32.1 | 33.8 | 63.4 | **+5.2%** | **+88%** | **+97%** |
| `scroll-variable-list` | 34.5 | 37.0 | 53.0 | **+7.3%** | **+43%** | **+54%** |
| `scroll-div-cards` | 58.8 | 61.4 | 75.8 | **+4.5%** | **+23%** | **+29%** |
| `scroll-data-table` | 42.3 | 45.8 | 141 | **+8.3%** | **+208%** | **+234%** |
| `hover-grid` | 71.1 | 78.4 | 81.0 | **+10%** | **+3.3%** | **+14%** |
| `tree-toggle` | 46.2 | 46.9 | 52.1 | ~+1.6% | **+11%** | **+13%** |
| `animated-dots` | 34.1 | 34.9 | 35.5 | ~+2.1% | ~+1.9% | **+4.0%** |
| `effects-cards` | 39.5 | 42.7 | 42.3 | **+7.9%** | ~-0.9% | **+7.0%** |

**Heap allocations** (per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 28205 | 28213 | 3075 | ~+0.0% | -89% | -89% |
| `dashboard-one-panel` | 28509 | 28517 | 3876 | ~+0.0% | -86% | -86% |
| `dashboard-all-panels` | 42674 | 42682 | 37981 | ~+0.0% | -11% | -11% |
| `table-stream` | 20706 | 20713 | 6486 | ~+0.0% | -69% | -69% |
| `text-typing` | 3902 | 3907 | 1436 | ~+0.1% | -63% | -63% |
| `text-log-stream` | 4785 | 4791 | 5630 | ~+0.1% | **+17%** | **+18%** |
| `text-relabel` | 63902 | 63901 | 58746 | ~-0.0% | -8.1% | -8.1% |
| `scroll-uniform-list` | 3363 | 3366 | 5622 | ~+0.1% | **+67%** | **+67%** |
| `scroll-variable-list` | 1212 | 1217 | 2438 | ~+0.4% | **+100%** | **+101%** |
| `scroll-div-cards` | 104379 | 104386 | 28797 | ~+0.0% | -72% | -72% |
| `scroll-data-table` | 10677 | 10682 | 14272 | ~+0.1% | **+34%** | **+34%** |
| `hover-grid` | 53539 | 53537 | 40895 | ~-0.0% | -24% | -24% |
| `tree-toggle` | 32267 | 32266 | 22698 | ~-0.0% | -30% | -30% |
| `animated-dots` | 3219 | 3218 | 3298 | ~-0.0% | ~+2.5% | ~+2.5% |
| `effects-cards` | 23382 | 23330 | 2579 | ~-0.2% | -89% | -89% |

**Bytes allocated** (KB per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 17018 | 16930 | 693 | ~-0.5% | -96% | -96% |
| `dashboard-one-panel` | 17088 | 17001 | 1708 | ~-0.5% | -90% | -90% |
| `dashboard-all-panels` | 20357 | 20269 | 12401 | ~-0.4% | -39% | -39% |
| `table-stream` | 6165 | 6130 | 1217 | ~-0.6% | -80% | -80% |
| `text-typing` | 2955 | 2919 | 1473 | ~-1.2% | -50% | -50% |
| `text-log-stream` | 3319 | 3285 | 3246 | ~-1.0% | ~-1.2% | ~-2.2% |
| `text-relabel` | 19713 | 19627 | 8407 | ~-0.4% | -57% | -57% |
| `scroll-uniform-list` | 1479 | 1456 | 1026 | ~-1.6% | -30% | -31% |
| `scroll-variable-list` | 933 | 902 | 807 | -3.3% | -10% | -13% |
| `scroll-div-cards` | 69226 | 69206 | 9718 | ~-0.0% | -86% | -86% |
| `scroll-data-table` | 4207 | 4135 | 2591 | ~-1.7% | -37% | -38% |
| `hover-grid` | 36059 | 34553 | 26984 | -4.2% | -22% | -25% |
| `tree-toggle` | 17507 | 17504 | 8434 | ~-0.0% | -52% | -52% |
| `animated-dots` | 4434 | 4247 | 4313 | -4.2% | ~+1.6% | ~-2.7% |
| `effects-cards` | 13756 | 13442 | 947 | ~-2.3% | -93% | -93% |

**Startup: window opened to first frame presented** (ms)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 149 | 185 | 163 | **+24%** | ~-12% | ~+10% |
| `dashboard-one-panel` | 175 | 187 | 184 | ~+7.0% | ~-1.9% | ~+4.9% |
| `dashboard-all-panels` | 169 | 170 | 172 | ~+0.8% | ~+0.9% | ~+1.7% |
| `table-stream` | 142 | 183 | 152 | **+29%** | ~-17% | ~+7.0% |
| `text-typing` | 148 | 161 | 155 | ~+8.5% | ~-4.0% | ~+4.2% |
| `text-log-stream` | 128 | 130 | 130 | ~+1.6% | ~+0.6% | ~+2.2% |
| `text-relabel` | 137 | 139 | 143 | ~+1.3% | ~+3.4% | **+4.7%** |
| `scroll-uniform-list` | 121 | 126 | 126 | **+3.9%** | ~-0.1% | **+3.8%** |
| `scroll-variable-list` | 123 | 128 | 129 | **+4.3%** | ~+0.7% | **+5.0%** |
| `scroll-div-cards` | 178 | 184 | 189 | ~+3.1% | ~+2.7% | **+5.9%** |
| `scroll-data-table` | 124 | 130 | 133 | **+5.0%** | ~+2.0% | **+7.2%** |
| `hover-grid` | 150 | 146 | 161 | ~-3.1% | ~+11% | ~+7.5% |
| `tree-toggle` | 140 | 150 | 151 | ~+7.6% | ~+0.1% | ~+7.7% |
| `animated-dots` | 103 | 106 | 108 | ~+3.0% | ~+1.5% | **+4.5%** |
| `effects-cards` | 132 | 135 | 136 | ~+2.1% | ~+0.9% | ~+3.0% |

**Startup: the first frame's draw** (ms)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 45.1 | 55.0 | 57.1 | **+22%** | ~+3.9% | **+27%** |
| `dashboard-one-panel` | 44.7 | 55.0 | 55.8 | ~+23% | ~+1.4% | **+25%** |
| `dashboard-all-panels` | 48.7 | 50.7 | 53.0 | ~+4.2% | ~+4.5% | ~+8.9% |
| `table-stream` | 35.4 | 45.5 | 42.3 | **+29%** | ~-7.1% | **+19%** |
| `text-typing` | 51.1 | 59.7 | 55.5 | ~+17% | ~-6.9% | ~+8.7% |
| `text-log-stream` | 30.7 | 32.0 | 33.3 | ~+4.3% | **+3.9%** | **+8.4%** |
| `text-relabel` | 42.3 | 41.8 | 44.1 | ~-1.3% | **+5.6%** | **+4.2%** |
| `scroll-uniform-list` | 25.0 | 28.2 | 29.8 | **+13%** | **+5.6%** | **+19%** |
| `scroll-variable-list` | 27.5 | 30.2 | 31.8 | **+10%** | **+5.1%** | **+16%** |
| `scroll-div-cards` | 84.3 | 86.8 | 90.4 | ~+3.0% | ~+4.2% | **+7.2%** |
| `scroll-data-table` | 30.2 | 33.1 | 35.4 | **+10%** | **+6.7%** | **+17%** |
| `hover-grid` | 51.3 | 43.9 | 49.5 | ~-14% | ~+13% | ~-3.6% |
| `tree-toggle` | 40.2 | 43.0 | 44.6 | ~+7.0% | ~+3.7% | ~+11% |
| `animated-dots` | 8.4 | 9.2 | 10.9 | **+10%** | **+18%** | **+30%** |
| `effects-cards` | 34.6 | 37.2 | 39.8 | **+7.4%** | **+7.1%** | **+15%** |

### WASM in Chromium, WebGPU

viewport 1280×800 @2x; display interval ≈7.9 ms; Chromium 147.0.7727.15, backend webgpu; 135 of 135 runs reported, 15.3 min

**Frame cost: CPU** (ms of Chrome main-thread task time per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 5.34 | 5.52 | 2.28 | **+3.2%** | -59% | -57% |
| `dashboard-one-panel` | 5.43 | 5.52 | 2.34 | ~+1.6% | -58% | -57% |
| `dashboard-all-panels` | 6.98 | 7.08 | 6.19 | ~+1.4% | -13% | -11% |
| `table-stream` | 4.07 | 3.96 | 2.35 | ~-2.7% | -41% | -42% |
| `text-typing` | 2.79 | 2.76 | 2.30 | ~-0.8% | -17% | -18% |
| `text-log-stream` | 2.41 | 2.38 | 2.40 | ~-1.1% | ~+0.7% | ~-0.5% |
| `text-relabel` | 9.74 | 8.30 | 7.66 | -15% | -7.7% | -21% |
| `scroll-uniform-list` | 2.32 | 2.22 | 2.14 | ~-4.2% | ~-3.4% | ~-7.6% |
| `scroll-variable-list` | 2.17 | 2.11 | 1.85 | ~-2.6% | -12% | -14% |
| `scroll-div-cards` | 17.0 | 17.6 | 5.88 | **+3.5%** | -67% | -65% |
| `scroll-data-table` | 3.46 | 3.35 | 4.01 | -3.2% | **+20%** | **+16%** |
| `hover-grid` | 17.5 | 10.1 | 9.16 | -43% | -9.0% | -48% |
| `tree-toggle` | 5.34 | 5.52 | 5.21 | **+3.5%** | -5.7% | ~-2.4% |
| `animated-dots` | 2.73 | 2.63 | 2.75 | ~-3.5% | ~+4.6% | ~+0.9% |
| `effects-cards` | 4.03 | 4.09 | 2.15 | ~+1.5% | -47% | -47% |

**Window::draw, median** (ms per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 5.06 | 5.20 | 1.86 | ~+2.8% | -64% | -63% |
| `dashboard-one-panel` | 5.15 | 5.22 | 2.02 | ~+1.3% | -61% | -61% |
| `dashboard-all-panels` | 6.60 | 6.70 | 5.81 | ~+1.5% | -13% | -12% |
| `table-stream` | 3.87 | 3.74 | 2.05 | -3.3% | -45% | -47% |
| `text-typing` | 2.53 | 2.50 | 1.79 | ~-1.2% | -29% | -29% |
| `text-log-stream` | 2.22 | 2.13 | 2.21 | ~-3.9% | ~+3.6% | ~-0.5% |
| `text-relabel` | 9.42 | 7.99 | 7.26 | -15% | -9.1% | -23% |
| `scroll-uniform-list` | 1.94 | 1.84 | 1.69 | ~-5.4% | ~-8.1% | -13% |
| `scroll-variable-list` | 1.72 | 1.59 | 1.16 | ~-7.9% | -27% | -33% |
| `scroll-div-cards` | 16.4 | 17.0 | 5.11 | **+3.5%** | -70% | -69% |
| `scroll-data-table` | 3.21 | 3.04 | 3.56 | -5.1% | **+17%** | **+11%** |
| `hover-grid` | 16.6 | 9.05 | 8.11 | -45% | -10% | -51% |
| `tree-toggle` | 5.16 | 5.36 | 3.90 | **+4.0%** | -27% | -24% |
| `animated-dots` | 2.42 | 2.28 | 2.40 | -5.8% | **+5.2%** | ~-0.8% |
| `effects-cards` | 3.84 | 3.80 | 1.78 | ~-1.0% | -53% | -54% |

**Window::draw, 95th percentile** (ms per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 5.29 | 5.56 | 2.57 | ~+5.1% | -54% | -51% |
| `dashboard-one-panel` | 5.36 | 5.40 | 2.49 | ~+0.8% | -54% | -54% |
| `dashboard-all-panels` | 6.83 | 6.89 | 6.09 | ~+0.8% | -12% | -11% |
| `table-stream` | 4.05 | 3.93 | 2.58 | -3.1% | -34% | -36% |
| `text-typing` | 2.85 | 2.82 | 2.57 | ~-1.2% | ~-8.9% | ~-10% |
| `text-log-stream` | 2.55 | 2.51 | 2.54 | ~-1.4% | ~+1.2% | ~-0.2% |
| `text-relabel` | 9.63 | 8.10 | 7.59 | -16% | -6.3% | -21% |
| `scroll-uniform-list` | 2.58 | 2.53 | 2.30 | ~-1.7% | ~-9.1% | ~-11% |
| `scroll-variable-list` | 2.43 | 2.30 | 2.00 | ~-5.1% | -13% | -18% |
| `scroll-div-cards` | 16.9 | 17.4 | 5.55 | ~+2.5% | -68% | -67% |
| `scroll-data-table` | 3.43 | 3.35 | 4.38 | ~-2.3% | **+31%** | **+28%** |
| `hover-grid` | 17.0 | 9.28 | 8.34 | -45% | -10% | -51% |
| `tree-toggle` | 5.66 | 5.82 | 7.07 | ~+2.8% | **+22%** | **+25%** |
| `animated-dots` | 2.74 | 2.64 | 2.79 | ~-3.7% | ~+5.7% | ~+1.8% |
| `effects-cards` | 4.04 | 4.11 | 2.30 | ~+1.8% | -44% | -43% |

**Frame rate** (frames per second)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 120 | 120 | 120 | ~+0.0% | ~-0.0% | ~-0.0% |
| `dashboard-one-panel` | 120 | 120 | 120 | ~-0.0% | ~-0.0% | ~-0.0% |
| `dashboard-all-panels` | 120 | 120 | 120 | ~-0.0% | ~+0.0% | ~+0.0% |
| `table-stream` | 120 | 120 | 120 | ~-0.0% | ~-0.0% | ~-0.0% |
| `text-typing` | 120 | 120 | 120 | ~+0.0% | ~-0.0% | ~+0.0% |
| `text-log-stream` | 120 | 120 | 120 | ~+0.0% | ~-0.0% | ~+0.0% |
| `text-relabel` | 102 | 120 | 120 | +17% | ~-0.0% | +17% |
| `scroll-uniform-list` | 120 | 120 | 120 | ~+0.0% | ~-0.0% | ~-0.0% |
| `scroll-variable-list` | 120 | 120 | 120 | ~-0.0% | ~-0.2% | ~-0.2% |
| `scroll-div-cards` | 58.5 | 56.6 | 120 | **-3.2%** | +112% | +105% |
| `scroll-data-table` | 120 | 120 | 119 | ~-0.0% | ~-0.7% | ~-0.7% |
| `hover-grid` | 56.9 | 99.2 | 109 | +74% | +9.5% | +91% |
| `tree-toggle` | 120 | 120 | 120 | ~-0.0% | ~+0.0% | ~+0.0% |
| `animated-dots` | 120 | 120 | 120 | ~+0.0% | ~-0.0% | ~-0.0% |
| `effects-cards` | 120 | 120 | 120 | ~-0.0% | ~-0.0% | ~-0.0% |

**Missed frames** (intervals over 1.5× the display's, of the measured frames; changes in frames)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `dashboard-one-panel` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `dashboard-all-panels` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `table-stream` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-typing` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-log-stream` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-relabel` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `scroll-uniform-list` | 0 | 0 | 1 | ~+0.00 | **+1.00** | **+1.00** |
| `scroll-variable-list` | 0 | 0 | 1 | ~+0.00 | ~+1.00 | **+1.00** |
| `scroll-div-cards` | 0 | 0 | 11.0 | ~+0.00 | **+11.00** | **+11.00** |
| `scroll-data-table` | 0 | 0 | 2 | ~+0.00 | **+2.00** | **+2.00** |
| `hover-grid` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `tree-toggle` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `animated-dots` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |
| `effects-cards` | 0 | 0 | 0 | ~+0.00 | ~+0.00 | ~+0.00 |

**WASM linear memory at the end** (MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 49.1 | 50.4 | 53.4 | ~+2.7% | **+5.8%** | **+8.7%** |
| `dashboard-one-panel` | 49.1 | 50.4 | 53.3 | ~+2.7% | **+5.7%** | **+8.5%** |
| `dashboard-all-panels` | 49.1 | 51.1 | 53.7 | **+4.1%** | **+5.0%** | **+9.3%** |
| `table-stream` | 45.1 | 48.1 | 49.6 | **+6.5%** | **+3.3%** | **+10%** |
| `text-typing` | 39.6 | 42.8 | 43.1 | **+7.9%** | ~+0.7% | **+8.7%** |
| `text-log-stream` | 37.6 | 39.9 | 40.9 | **+6.1%** | ~+2.3% | **+8.6%** |
| `text-relabel` | 56.2 | 61.5 | 62.8 | **+9.5%** | ~+2.0% | **+12%** |
| `scroll-uniform-list` | 37.6 | 38.8 | 71.4 | **+3.3%** | **+84%** | **+90%** |
| `scroll-variable-list` | 38.4 | 41.1 | 58.0 | **+7.0%** | **+41%** | **+51%** |
| `scroll-div-cards` | 66.5 | 67.8 | 84.8 | ~+1.9% | **+25%** | **+28%** |
| `scroll-data-table` | 46.2 | 49.6 | 168 | **+7.3%** | **+238%** | **+262%** |
| `hover-grid` | 75.7 | 77.9 | 80.6 | ~+2.9% | **+3.5%** | **+6.4%** |
| `tree-toggle` | 52.2 | 53.1 | 58.2 | ~+1.7% | **+10%** | **+11%** |
| `animated-dots` | 38.8 | 39.6 | 40.2 | ~+2.3% | ~+1.4% | **+3.7%** |
| `effects-cards` | 43.4 | 46.4 | 47.1 | **+6.9%** | ~+1.5% | **+8.5%** |

**Memory growth over the measured frames** (MB; changes in MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 0.00 | 0.19 | 0.00 | **+0.19** | -0.19 | ~+0.00 |
| `dashboard-one-panel` | 0.00 | 0.00 | 0.00 | ~+0.00 | ~+0.00 | ~+0.00 |
| `dashboard-all-panels` | 0.00 | 0.00 | 0.00 | ~+0.00 | ~+0.00 | ~+0.00 |
| `table-stream` | 0.00 | 0.00 | 0.00 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-typing` | 0.00 | 0.00 | 0.12 | ~+0.00 | ~+0.12 | ~+0.12 |
| `text-log-stream` | 0.00 | 0.00 | 0.00 | ~+0.00 | ~+0.00 | ~+0.00 |
| `text-relabel` | 0.00 | 0.00 | 0.00 | ~+0.00 | ~+0.00 | ~+0.00 |
| `scroll-uniform-list` | 0.00 | 0.00 | 18.6 | ~+0.00 | **+18.62** | **+18.62** |
| `scroll-variable-list` | 0.12 | 0.00 | 9.31 | -0.12 | **+9.31** | **+9.19** |
| `scroll-div-cards` | 0.00 | 0.00 | 1.75 | ~+0.00 | **+1.75** | **+1.75** |
| `scroll-data-table` | 2.12 | 2.12 | 67.6 | ~+0.00 | **+65.50** | **+65.50** |
| `hover-grid` | 0.00 | 0.00 | 0.00 | ~+0.00 | ~+0.00 | ~+0.00 |
| `tree-toggle` | 0.00 | 0.00 | 0.00 | ~+0.00 | ~+0.00 | ~+0.00 |
| `animated-dots` | 0.00 | 0.31 | 0.31 | ~+0.31 | ~+0.00 | ~+0.31 |
| `effects-cards` | 0.00 | 0.38 | 0.38 | **+0.38** | ~+0.00 | **+0.38** |

**Peak live Rust heap** (MB)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 42.1 | 44.3 | 45.5 | **+5.1%** | ~+2.7% | **+7.9%** |
| `dashboard-one-panel` | 42.2 | 44.3 | 45.9 | **+5.1%** | **+3.6%** | **+8.9%** |
| `dashboard-all-panels` | 42.4 | 44.8 | 47.4 | **+5.6%** | **+5.8%** | **+12%** |
| `table-stream` | 38.1 | 40.9 | 41.9 | **+7.6%** | ~+2.2% | **+10%** |
| `text-typing` | 33.5 | 36.0 | 36.9 | **+7.3%** | ~+2.6% | **+10%** |
| `text-log-stream` | 31.5 | 33.9 | 34.2 | **+7.6%** | ~+1.1% | **+8.9%** |
| `text-relabel` | 48.1 | 53.3 | 55.0 | **+11%** | **+3.2%** | **+14%** |
| `scroll-uniform-list` | 30.7 | 32.3 | 62.4 | **+5.3%** | **+93%** | **+103%** |
| `scroll-variable-list` | 32.6 | 35.0 | 52.1 | **+7.5%** | **+49%** | **+60%** |
| `scroll-div-cards` | 57.8 | 60.3 | 74.7 | **+4.3%** | **+24%** | **+29%** |
| `scroll-data-table` | 37.8 | 41.4 | 140 | **+9.4%** | **+238%** | **+270%** |
| `hover-grid` | 67.5 | 71.8 | 74.1 | **+6.3%** | **+3.2%** | **+10%** |
| `tree-toggle` | 45.4 | 46.0 | 51.1 | ~+1.5% | **+11%** | **+13%** |
| `animated-dots` | 32.7 | 33.4 | 34.0 | ~+2.0% | ~+2.0% | **+4.1%** |
| `effects-cards` | 37.0 | 40.1 | 39.6 | **+8.4%** | ~-1.1% | **+7.2%** |

**Heap allocations** (per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 27981 | 27989 | 2851 | ~+0.0% | -90% | -90% |
| `dashboard-one-panel` | 28285 | 28293 | 3652 | ~+0.0% | -87% | -87% |
| `dashboard-all-panels` | 42450 | 42458 | 37757 | ~+0.0% | -11% | -11% |
| `table-stream` | 20473 | 20480 | 6253 | ~+0.0% | -69% | -69% |
| `text-typing` | 3669 | 3674 | 1203 | ~+0.1% | -67% | -67% |
| `text-log-stream` | 4552 | 4558 | 5397 | ~+0.1% | **+18%** | **+19%** |
| `text-relabel` | 63658 | 63657 | 58502 | ~-0.0% | -8.1% | -8.1% |
| `scroll-uniform-list` | 3139 | 3142 | 4286 | ~+0.1% | **+36%** | **+37%** |
| `scroll-variable-list` | 979 | 984 | 1128 | ~+0.5% | **+15%** | **+15%** |
| `scroll-div-cards` | 104064 | 104071 | 27472 | ~+0.0% | -74% | -74% |
| `scroll-data-table` | 10354 | 10359 | 12845 | ~+0.1% | **+24%** | **+24%** |
| `hover-grid` | 49964 | 49962 | 37320 | ~-0.0% | -25% | -25% |
| `tree-toggle` | 32043 | 32042 | 22474 | ~-0.0% | -30% | -30% |
| `animated-dots` | 3041 | 3040 | 3120 | ~-0.0% | ~+2.6% | ~+2.6% |
| `effects-cards` | 22816 | 22764 | 2013 | ~-0.2% | -91% | -91% |

**Bytes allocated** (KB per frame)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 16708 | 16620 | 382 | ~-0.5% | -98% | -98% |
| `dashboard-one-panel` | 16777 | 16689 | 1397 | ~-0.5% | -92% | -92% |
| `dashboard-all-panels` | 20042 | 19954 | 12087 | ~-0.4% | -39% | -40% |
| `table-stream` | 5727 | 5692 | 779 | ~-0.6% | -86% | -86% |
| `text-typing` | 2515 | 2479 | 1034 | ~-1.4% | -58% | -59% |
| `text-log-stream` | 2898 | 2864 | 2825 | ~-1.2% | ~-1.4% | ~-2.5% |
| `text-relabel` | 18759 | 18673 | 7454 | ~-0.5% | -60% | -60% |
| `scroll-uniform-list` | 1274 | 1251 | 783 | ~-1.8% | -37% | -39% |
| `scroll-variable-list` | 547 | 516 | 560 | -5.7% | **+8.5%** | ~+2.3% |
| `scroll-div-cards` | 69032 | 69012 | 9480 | ~-0.0% | -86% | -86% |
| `scroll-data-table` | 3625 | 3553 | 2285 | ~-2.0% | -36% | -37% |
| `hover-grid` | 33410 | 31899 | 24331 | -4.5% | -24% | -27% |
| `tree-toggle` | 17439 | 17435 | 8366 | ~-0.0% | -52% | -52% |
| `animated-dots` | 4204 | 4016 | 4082 | -4.5% | ~+1.6% | ~-2.9% |
| `effects-cards` | 13444 | 13130 | 635 | ~-2.3% | -95% | -95% |

**Startup: window opened to first frame presented** (ms)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 52.1 | 60.2 | 58.1 | **+15%** | ~-3.5% | **+11%** |
| `dashboard-one-panel` | 53.8 | 54.9 | 57.8 | ~+2.2% | **+5.2%** | ~+7.4% |
| `dashboard-all-panels` | 54.5 | 55.4 | 58.6 | ~+1.7% | ~+5.8% | ~+7.6% |
| `table-stream` | 44.6 | 49.8 | 51.3 | **+12%** | ~+3.0% | **+15%** |
| `text-typing` | 61.8 | 63.2 | 65.7 | ~+2.3% | ~+3.9% | **+6.3%** |
| `text-log-stream` | 40.8 | 43.1 | 44.0 | **+5.5%** | ~+2.2% | **+7.8%** |
| `text-relabel` | 52.7 | 53.0 | 54.6 | ~+0.5% | ~+3.1% | ~+3.6% |
| `scroll-uniform-list` | 35.2 | 39.5 | 40.8 | **+12%** | ~+3.3% | **+16%** |
| `scroll-variable-list` | 38.3 | 41.6 | 43.7 | **+8.6%** | **+5.0%** | **+14%** |
| `scroll-div-cards` | 94.8 | 98.1 | 104 | ~+3.4% | **+5.6%** | **+9.2%** |
| `scroll-data-table` | 40.9 | 44.4 | 46.4 | ~+8.5% | ~+4.7% | ~+14% |
| `hover-grid` | 59.6 | 55.1 | 57.9 | -7.7% | **+5.2%** | ~-2.9% |
| `tree-toggle` | 49.7 | 51.4 | 54.5 | ~+3.5% | **+6.1%** | **+10%** |
| `animated-dots` | 18.6 | 20.0 | 21.7 | **+7.4%** | **+8.9%** | **+17%** |
| `effects-cards` | 47.2 | 49.9 | 50.4 | **+5.9%** | ~+1.0% | ~+6.9% |

**Startup: the first frame's draw** (ms)

| Scenario | pre | off | on | off vs pre | on vs off | on vs pre |
|---|---:|---:|---:|---:|---:|---:|
| `dashboard-clock` | 42.2 | 46.0 | 47.5 | ~+9.0% | ~+3.3% | **+13%** |
| `dashboard-one-panel` | 43.5 | 44.0 | 47.4 | ~+1.2% | **+7.8%** | ~+9.1% |
| `dashboard-all-panels` | 44.6 | 44.8 | 47.7 | ~+0.5% | ~+6.5% | ~+7.1% |
| `table-stream` | 34.9 | 39.5 | 40.7 | **+13%** | ~+3.2% | **+17%** |
| `text-typing` | 52.2 | 53.1 | 55.1 | ~+1.6% | ~+3.9% | **+5.5%** |
| `text-log-stream` | 31.0 | 32.5 | 33.9 | **+4.8%** | **+4.5%** | **+10%** |
| `text-relabel` | 43.0 | 42.6 | 44.4 | ~-0.9% | **+4.2%** | ~+3.3% |
| `scroll-uniform-list` | 25.5 | 29.0 | 30.6 | **+14%** | **+5.6%** | **+20%** |
| `scroll-variable-list` | 28.0 | 30.8 | 32.8 | **+10%** | **+6.6%** | **+17%** |
| `scroll-div-cards` | 84.6 | 87.2 | 92.1 | ~+3.0% | **+5.6%** | **+8.8%** |
| `scroll-data-table` | 31.3 | 34.5 | 36.2 | ~+10% | ~+4.9% | ~+16% |
| `hover-grid` | 49.6 | 44.6 | 47.5 | -10% | **+6.5%** | -4.2% |
| `tree-toggle` | 39.5 | 41.0 | 43.9 | ~+3.8% | **+7.3%** | **+11%** |
| `animated-dots` | 8.7 | 9.7 | 11.3 | **+11%** | **+17%** | **+30%** |
| `effects-cards` | 36.2 | 39.7 | 40.2 | **+9.5%** | ~+1.3% | **+11%** |

### Fast-mode work counters: macOS native (Metal)

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

## Conclusions

1. **Non-fast mode did not regress.** Across 60 scenario-target pairs, the worst `off` cell is
   +6.3% CPU against `pre` (`animated-dots` on macOS, within its noise), the few changes beyond
   the noise are +3–4%, and the geometric mean is 1.6–5.6% faster. PR #3's standalone
   optimizations (the scene and text changes every mode shares, see docs/fast-mode.md) pay for
   the fast-mode hooks that run with fast mode off: `hover-grid` is up to 47% cheaper and reaches the display's
   rate on macOS. What remains is a ~3% CPU cost on large trees rebuilt whole every frame and
   0.6–7 MB more heap, which an application only notices on its heaviest frames.
2. **Fast mode is a real speed-up for typical application UIs.** On frames where a few views
   change in a big window (dashboards, tables with streaming rows, a document being typed into,
   decorated card pages) it removes 30–60% of the frame's CPU and 50–98% of the bytes allocated
   per frame, on every target. On WebGL2 and WebGPU, where the same work costs more, the gains
   are largest.
3. **Scroll layers are the biggest single win and the biggest cost.** A plain scrolling `div`
   drops from 13–18 ms to 5–6 ms per frame and holds 120 fps, but every scrolling scenario pays
   for tiles and kept rows in memory (macOS `scroll-data-table`: 142 → 317 MB footprint, 25 →
   143 MB peak heap), and `scroll-data-table` gets slower with them on two targets. The memory
   keeps growing during the measured frames there, so it is not yet known whether it is bounded
   (see the first opportunity below).
4. **Fast mode cannot help a view that changes everything each frame** (animations, a log whose
   lines all move, 1,500 labels that all change, a tree whose rows shift). It costs little there
   (−8% to +8%), but its bookkeeping (thousands of layout nodes claimed per frame) is visible in
   `tree-toggle`'s 95th percentile on macOS (+33%).
5. **Some frames are bound by something fast mode does not touch.** `effects-cards` on WebGL2
   runs at 62 fps in every configuration although fast mode cuts its CPU from 5.3 to 0.8 ms: the
   GPU (shadows and gradients) is the limit. `text-relabel` spends 8–11 ms per frame, shaping 1,190 lines,
   in every configuration.
6. **Startup is essentially unchanged.** Window opened to first frame presented is 58–98 ms
   natively, 19–104 ms on WebGPU and 103–189 ms on WebGL2, with no consistent difference between
   the configurations. The first frame's draw is mostly a little slower with fast mode on (up to
   +20%), as that frame also builds the retained state.
7. These are real windows presenting to a 120 Hz display (60 Hz in the Simulator), so the
   numbers include presentation; they differ from lane G's headless `fast_mode` bench (which
   reported `all-panels` +10.7% with fast mode; here `dashboard-all-panels` is 3–14% *faster*).
   The iOS figures come from the Simulator on the Mac, not from a phone.

## Improvement opportunities

Ordered by expected value. Each names the scenario and metric an AgentX improvement loop can use
as its measurement (`python3 benchmark/run.py --target <target> --scenarios <id> --configs off,on`).

1. **Bound scroll layers' memory.** Footprint +55% to +240% in `scroll-uniform-list`,
   `scroll-variable-list`, `scroll-div-cards` and `scroll-data-table`, still growing over 600
   frames in `scroll-data-table` (+38 to +68 MB) and `scroll-uniform-list` (+9 to +19 MB).
   First run 3,000- and 9,000-frame runs of both to see whether it plateaus (this was planned
   here and blocked by the screen locking), then cap the tiles and kept rows a list layer holds
   (evict rows far from the viewport, budget tiles per layer). Measure: `footprint_mb.end`,
   growth and `heap_peak_mb`; target ≤1.3× `off` and no growth after warm-up.
2. **Make a spliced frame cost what changed, not what was copied.** In `dashboard-clock` with fast
   mode on, 43 primitives are painted and 1,966 copied per frame (counters), and that copy is most
   of the remaining 2.9 ms on macOS. Replay retained subtrees by reference (keep their primitive
   ranges in the scene instead of copying them), or composite unchanged sibling views like scroll
   layers do. Measure: `dashboard-clock`, `dashboard-one-panel`, `table-stream` draw p50; target
   under 1 ms for the clock.
3. **Skip layout and prepaint inside a composited scroll layer on scroll-only frames.**
   `scroll-div-cards` with layers composited still claims 6,802 layout nodes per frame and costs
   ~5 ms. Only the offset changed; the layer could be moved without walking its content. Measure:
   `scroll-div-cards` CPU; target under 2 ms.
4. **Fix `scroll-data-table` with fast mode.** +8% CPU on macOS, +20% on WebGPU, p95 +21% on
   macOS, 60 style writes and ~100 node claims per frame, and the largest memory growth. Look at
   how list layers keep and re-lay-out rows that enter the view with many text cells.
5. **Let retention step aside when a view rebuilds everything.** In `text-relabel`, `hover-grid`,
   `tree-toggle` and `text-log-stream`, layout retention claims 400–3,600 nodes per frame and
   reuses almost none (`tree-toggle` p95 +33% on macOS; `text-log-stream` +7%). An adaptive policy
   that turns layout retention off per view after N frames of no reuse (and back on when reuse
   returns) would remove that cost; cheaper keyed claims would help everywhere.
6. **Investigate `text-typing` on macOS.** +10% CPU with fast mode on macOS while iOS, WebGPU and
   WebGL2 gain 17–22%; 3,202 primitives copied per frame around one changed paragraph. Likely the
   same copy cost as (2) meeting macOS's present path; measure `text-typing` on macOS.
7. **Hover over a large single view.** `hover-grid` still paints 11,040 primitives per frame with
   fast mode on (6.9 ms on macOS, 9–11 ms elsewhere): one hovered cell repaints the whole view.
   Retaining hover style changes at element granularity would make it cost one cell.
8. **Allocations in scroll-only frames.** With fast mode on, `scroll-uniform-list`,
   `scroll-variable-list` and `scroll-data-table` make 15–100% more allocations per frame than
   without (layer bookkeeping); pool them per window. Measure: `allocations_per_frame`.
9. **Fast mode off should cost nothing.** With fast mode off the peak heap is 0.6–7 MB above `pre`
   and large full-rebuild trees cost ~3% more CPU; allocate fast-mode state lazily and skip its
   per-element hooks when the window's mode is `FastMode::OFF`. Measure `off` against `pre` on
   `scroll-div-cards`, `tree-toggle` and the heap tables.
10. **Text shaping in label-heavy frames.** `text-relabel` shapes 1,190 lines per frame in every
    configuration (8–11 ms of CPU per frame); caching shaped runs of short numeric strings (or shaping digits
    from a per-font glyph table) would cut the cost every mode pays.
11. **GPU-bound decoration on WebGL2.** `effects-cards` holds 62 fps on WebGL2 in every
    configuration with 0.8–5 ms of CPU: shadows and gradients are fragment-bound there. Cheaper
    shadow rendering on the GL backend is the lever; this benchmark shows it as frame rate, not CPU.
12. **Benchmark coverage.** Add GPU time per frame (Metal command-buffer and WebGPU timestamp
    queries), the Android target (an APK shell like lane E's), runs on Linux and Windows hosts,
    and a physical iPhone, so the loops above can be measured where they matter most.
