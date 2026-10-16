# macOS fast-mode performance and memory loop, 2026-10-15 to 2026-10-16

Branch `perf/agentx-macos-loop` (worktree `Gpux-agentx-loop`), 70 commits since the opening state: **local only, not pushed or merged, native macOS only**
(Apple M5, macOS 26.6.2, Metal, 120 Hz). Five AgentX improvement-loop runs, 33 experiments plus 5 baselines: 21 kept, 12 reverted.
The user stopped the loop during experiment 3 of part 4; that experiment was reverted untested. Part 5 then investigated the two apparent regressions, found that they were a measurement artifact (the CPU clock effect described below), added `--hold-clocks` to the benchmark and, with it, re-judged the two-drawable change that part 1 had reverted on the same artifact, and adopted it.

## 1. How the numbers were taken

The headline tables compare the **opening state** (commit `d5cb2e1`: PR #3's benchmark plus PR #7's scroll-layer memory bounds,
before any loop change) with the **final state** (this branch's head, `f0b35b8`). Both binaries were built from their commits and run
**alternated in one session** (old, new, old, new...) so frequency and thermal drift hit both alike: 15 scenarios, fast mode off and on,
3 repetitions each, 600 measured frames after 120 warm-up frames. Cells are the median of the three. Raw data:
[benchmark/results/2026-10-16-opening-vs-final/](../../benchmark/results/2026-10-16-opening-vs-final/) (summary.json, 180 result files).

**Clock state matters, and part 5 of the loop found it.** The tables of sections 2 and 6 were first taken with the CPU clusters idle,
which made two fast-on scenarios look 21% slower than the opening. A frame that needs less work runs at a lower clock, so its thread
takes more CPU time per unit of work (the same `scroll-data-table` main-thread work reads 2.9 ms with the clusters idle and 1.0 ms with
three busy processes beside it). The comparison was therefore repeated with `run.py --hold-clocks 3`, which keeps the clocks up
([benchmark/results/2026-10-16-opening-vs-final-held-clocks/](../../benchmark/results/2026-10-16-opening-vs-final-held-clocks/), same 180 runs).
The held-clock numbers measure the work and are the ones to read; the idle-clock numbers are kept because they are what an unloaded
machine shows, and they understate the gains. Allocations, bytes and heap do not depend on the clock.

Caveats that apply to every table:
- `main_cpu_ms` is thread CPU time. On this machine it moves with pacing and the CPU's frequency choice (the same build read 1.8 or 2.6 ms
  on one scenario from one run to the next), so only the alternated comparison here and the per-experiment A/Bs are evidence; two separate
  runs hours apart are not (the earlier closing runs differ from this one by up to 40% on scenarios nothing touched).
- Allocation counts and bytes are deterministic. Heap peak is nearly so. Footprint moves by a few MB from run to run.
- Absolute `Window::draw` figures also differ between sessions; compare the before/after columns of one table.

## 2. Summary

| | Fast mode off | Fast mode on |
|---|---|---|
| CPU per frame, geometric mean over 15 scenarios, clocks held | **-17%** | **-31%** |
| CPU per frame, same, clocks idle (understates) | -12% | -14% |
| `Window::draw` median, geometric mean, clocks held | -17% | -34% |
| `Window::draw` median, same, clocks idle | -11% | -18% |
| Heap allocations per frame, geometric mean | **-26%** | **-64%** |
| Bytes allocated per frame, geometric mean | -21% | -23% |
| Peak live Rust heap, geometric mean | -12% | -14% |
| Memory footprint at the end, geometric mean (two drawables, section 3 part 5) | **-13%** | **-13%** |

Largest wins (before > after; CPU and draw figures here are the first, idle-clock comparison and understate the gains: section 6.1 has the clock-held ones):
- `text-relabel`: CPU 10.8 > 6.6 ms off, 10.7 > 6.1 ms on (**-39% / -43%**); it now holds 120 fps where it ran at 97.
- `hover-grid`: CPU 7.2 > 5.3 off (-26%), 6.6 > 4.6 on (**-31%**); allocations 37,235 > 14,839 on (-60%).
- `dashboard-all-panels`: CPU 7.0 > 5.0 off (-29%), 6.5 > 4.6 on (-30%).
- `scroll-div-cards`, fast on: CPU 5.0 > 3.2 ms (-35%), `Window::draw` 4.5 > 2.1 ms (**-52%**), allocations 27,445 > 5,865 (**-79%**), peak heap 68.8 > 56.9 MB.
- `dashboard-clock`, fast on: allocations 2,811 > 191 per frame (**-93%**), CPU 3.2 > 2.6 ms (-19%).
- `scroll-uniform-list`, fast on: CPU 2.4 > 1.8 ms (-25%), `Window::draw` -39%.
- `tree-toggle`, fast on: CPU -17%, `Window::draw` -23%, allocations -45%, peak heap 37.7 > 26.5 MB (-30%).
- Peak Rust heap fell 10-30% in 10 of 15 scenarios with fast mode on, and 3-8% in four more (`scroll-data-table` 48.7 > 39.0, `hover-grid` 62.7 > 49.4).

- Footprint (two drawables): `scroll-div-cards` fast on 215 > 185 MB, `scroll-data-table` 199 > 166, `hover-grid` 172 > 142, `text-relabel` 152 > 134; fast off 11-31 MB lower in 14 of 15 scenarios.

**With the clocks held, every scenario improved and none regressed** (section 6.1): geometric mean CPU per frame
**-17% off / -31% on**, `Window::draw` median -17% / -34%, 95th percentile -17% / -34%. Largest, fast on: `scroll-div-cards` CPU
5.06 > 1.20 ms (-76%, draw -82%), `text-relabel` -46%, `hover-grid` -38%, `dashboard-all-panels` -36%, `table-stream` -35%,
`scroll-data-table` -31% (draw -36%), `dashboard-one-panel` -28%, `dashboard-clock` -28%. The idle-clock run had shown
`scroll-data-table` (+21%) and `dashboard-one-panel` (+21%) as slower: that was the clock effect, not a regression, and part 5 of the
loop recorded it as such. Only `scroll-variable-list` fast on is flat to slightly up in draw (0.07 > 0.08 ms, at the measurement floor).

## 3. What changed (all kept changes, in order)

Part 1, memory and CPU, 19 experiments:
| Commit | Change | Measured effect (fast on unless noted) |
|---|---|---|
| `36a3b4a` | Text leaves are settled in linear time (a set, not a scan per leaf) | div-cards CPU -5%, hover-grid -5% |
| `f611fd4` | Per-element DevKit nodes, semantics and property setters exist only with the new `devtools` Cargo feature (default on; the conformance crate asks for it) | dashboard-clock CPU -16%, allocations -62%; div-cards CPU -9% |
| `c1ca4f4` | Text leaves are measured through an enum variant, not a boxed closure; one-pass keeping of retained layout nodes | dashboard-clock allocations 1,078 > 211 per frame |
| `c05d5bd` | List layers hold 0.15 viewports of overscan (was 0.5) | data-table peak heap -13%, uniform-list -12%, footprint -3 to -4% |
| `80a4f49` | Sized Core Text fonts are kept per (font, size) | text-relabel CPU -40% (92-97 > 120 fps), off or on |
| `0ecef68`, `69eece6` | `Rgba` converts to its `Color` through a per-thread cache; one shared transparent default | hover-grid CPU -19%, allocations -43% |
| `5bed338` | Kept layout styles are compared field by field, not hashed | div-cards CPU -3%, dashboard-all-panels -3% |
| `7dfa2e4` | Three Metal drawables restored after trying two (two saved 16-22 MB but made effects-cards read ~1.5x the CPU with idle clocks). Superseded in part 5 | |

Part 2, 1 experiment:
| `f7a22c7` | A composited scroll container keeps its children's layout and requests it in prepaint only if the layer is painted after all | div-cards `Window::draw` 3.2 > 2.2 ms (-31%), allocations -60% |

Part 3, 11 experiments (6 kept):
| Commit | Change | Measured effect |
|---|---|---|
| `b83d0b2` | An element's 21 listener and style lists are allocated on first use (`LazyVec`) | div-cards draw -10%, dashboard-all-panels -6%; geomean CPU -2.3% |
| `09577bb` | An element's aria properties are allocated on first use (`LazyAria`); `Interactivity` shrinks 1,368 > about 740 bytes | geomean CPU -0.7%, div-cards heap -1.7% |
| `75c4c07` | **Benchmark only:** reports GPU time per frame from Metal command buffers (`gpu_ms`) | div-cards composition costs 2.6 ms of GPU against 2.1 ms with fast mode off |
| `04ee65e` | Taffy is built without its `std` feature, so its node contexts sit in a dense indexed map, not a SipHash map | dashboard-clock draw -11%, div-cards -5%, hover-grid -2% |
| `0bc274d` | Each color keeps its fully transparent twin, which every filled or bordered quad pairs with its fill | hover-grid CPU -6%, allocations -15% |
| `e438919` | The empty font features of a default text style are one shared list per thread | hover-grid allocations -18%, text-relabel -11% |
| `73f34be` | A text's single default run is held on the stack, not in a `Vec`, when kept leaves compare and shape | allocations -14% to -20% on text-relabel, dashboard-all-panels, tree-toggle |

Part 5, 3 experiments (2 kept, 1 baseline):

| Commit | Change | Measured effect |
|---|---|---|
| `6d33e77` | **Benchmark only:** `run.py --hold-clocks N` runs N busy processes beside the desktop runs so the CPU clocks stay up; the README documents why | exposed that the apparent fast-on regressions of `scroll-data-table` and `dashboard-one-panel` (+21%) were -31% and -28% with the clocks held; no product code involved |
| `f0b35b8` | Metal layers hold two drawables again (`maximumDrawableCount = 2`) | footprint -12 to -22 MB (about 10%) in every scenario at no CPU cost with the clocks held (effects-cards 0.33 ms on both); frames still 120 fps; maximum frame interval 7 > 9 ms; missed frames in 24 runs: 2 runs with one or two, against 1 with three drawables. One line restores three |

Part 4, 1 experiment kept (user stopped the loop):
| `bcdcf81` | The unit transformation skips the matrix when a primitive's bounds are taken | hover-grid draw -0.9%, div-cards draw -2% (minor) |

## 4. What was tried and reverted (12)

Boxing the devtools semantics (more allocations); a directional list overscan (two layer tests failed); 256 px tiles (footprint +4%);
a digit glyph table (18% slower than Core Text's warm path in a same-binary A/B; a cross-build measurement had suggested -32%);
boxing the div interactivity (allocations +8%); inlined length and edge comparisons (0%); two Metal drawables (reverted in part 1, adopted again in part 5); a one-pass
claim of kept layout nodes, tried twice (within noise); recycling the base style boxes (allocations -67% but div-cards draw +6%);
deciding "emoji" once per glyph run in the Core Text shaper (0%); halving the share of list-row layout nodes that linger
(started, stopped before measuring). Each failure is recorded with its numbers in `.agentx/experiments/`.

## 5. Targets from the opportunity list, and where they stand

| Target | Result |
|---|---|
| Scroll-layer memory (partly met) | Peak heap down 11-22% on the scroll scenarios (list overscan, lazy element data). Footprint is 13% lower than at the opening (geometric mean, fast on or off; two drawables in part 5), but with fast mode on it is still 1.28-1.40x the footprint with it off on the scroll scenarios (data-table 166 vs 119 MB, div-cards 185 vs 135): the remainder is the visible tile textures (private Metal storage, about the window area) and the window's drawables, which a `malloc_history` check confirmed are not Rust heap. **Not met.** |
| Spliced frame copy cost | `dashboard-clock` fast on: allocations -93%, CPU 0.44 > 0.32 ms and draw p50 0.39 > 0.27 ms with the clocks held, which is **below the 1 ms target** (with idle clocks it reads 2.4 ms, again the clock effect). The work per retained element is about 0.9 us across claiming, copying records and replaying primitives. |
| Skip layout and prepaint in composited scroll layers | Done for layout (`f7a22c7`): div-cards draw p50 4.52 > 0.81 ms with the clocks held (-82%; 2.1 ms with idle clocks), target 2 ms **met**. Prepaint is still needed. |
| scroll-data-table with fast mode | Clocks held: CPU 0.83 > 0.58 ms (-31%), draw -36%, heap -20%, footprint -7%. (An idle-clock run had read +21%; that was the clock effect.) |
| Adaptive layout retention | text-relabel, hover-grid, tree-toggle all improved a lot (-43%, -31%, -17% CPU). |
| text-typing on macOS | Fast on: CPU -7%, allocations -51%; off: unchanged. |
| Hover granularity | **Not done, and it is a design limit**: splicing reuses per nested view, so a hover inside one view (hover-grid's 1,600 cells) builds that whole view again. Per-cell views would splice; element-level reuse inside a view was not attempted. |
| Scroll-frame allocations | Div-cards fast on 27,445 > 5,865 per frame (-79%); data-table -66%; uniform list -58%. |
| Text shaping | Sized font cache (-40% on relabel), shared defaults and runs (-11% to -20% allocations). Core Text's own typesetting is now about half of text-relabel's frame and was not touched. |
| Benchmark coverage (GPU time) | Added `gpu_ms` to the JSON and the report (macOS). Lanes B-G (web, Android, iOS, real GPUs) were not run. |

## 6. Full results

Sections 6.1-6.3 compare the opening state with the final build (head `f0b35b8`: two Metal drawables) with the CPU clocks held (`run.py --hold-clocks 3`), 15 scenarios, 3 alternated repetitions, medians, [data](../../benchmark/results/2026-10-16-opening-vs-final-held-clocks/). Section 6.4 is the first comparison, made with idle clocks and three drawables ([data](../../benchmark/results/2026-10-16-opening-vs-bcdcf81-idle-clocks/)); it understates the CPU gains. A held-clock run of the build before the drawable change is in [results/2026-10-16-opening-vs-bcdcf81-held-clocks/](../../benchmark/results/2026-10-16-opening-vs-bcdcf81-held-clocks/).

### 6.1 CPU and draw time, clocks held

### CPU per frame (`main_cpu_ms`, mean, ms)

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 1.05 > 0.87 | -18% | 1.12 > 0.92 | -18% |
| dashboard-all-panels | 7.30 > 5.18 | -29% | 6.76 > 4.33 | -36% |
| dashboard-clock | 4.15 > 3.60 | -13% | 0.44 > 0.32 | -27% |
| dashboard-one-panel | 4.23 > 3.66 | -13% | 0.67 > 0.50 | -25% |
| effects-cards | 2.99 > 2.62 | -13% | 0.39 > 0.31 | -20% |
| hover-grid | 7.68 > 5.40 | -30% | 6.80 > 4.23 | -38% |
| scroll-data-table | 1.84 > 1.56 | -15% | 0.83 > 0.57 | -32% |
| scroll-div-cards | 14.29 > 12.21 | -15% | 5.05 > 1.24 | -76% |
| scroll-uniform-list | 0.62 > 0.54 | -13% | 0.26 > 0.21 | -20% |
| scroll-variable-list | 0.41 > 0.38 | -6% | 0.18 > 0.18 | -2% |
| table-stream | 2.76 > 2.30 | -17% | 1.05 > 0.68 | -35% |
| text-log-stream | 0.85 > 0.77 | -9% | 0.89 > 0.80 | -11% |
| text-relabel | 11.88 > 6.97 | -41% | 11.58 > 6.20 | -47% |
| text-typing | 0.97 > 0.92 | -5% | 0.45 > 0.41 | -10% |
| tree-toggle | 4.19 > 3.64 | -13% | 3.95 > 2.85 | -28% |
| **geometric mean** | | **-17%** | | **-31%** |

### `Window::draw` median (ms)

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 0.96 > 0.78 | -19% | 1.04 > 0.83 | -20% |
| dashboard-all-panels | 7.10 > 5.03 | -29% | 6.55 > 4.19 | -36% |
| dashboard-clock | 3.98 > 3.46 | -13% | 0.39 > 0.27 | -31% |
| dashboard-one-panel | 4.06 > 3.54 | -13% | 0.62 > 0.45 | -28% |
| effects-cards | 2.85 > 2.49 | -13% | 0.34 > 0.27 | -22% |
| hover-grid | 7.08 > 5.07 | -28% | 6.21 > 3.94 | -37% |
| scroll-data-table | 1.74 > 1.48 | -15% | 0.63 > 0.40 | -36% |
| scroll-div-cards | 13.78 > 11.94 | -13% | 4.51 > 0.85 | -81% |
| scroll-uniform-list | 0.57 > 0.50 | -13% | 0.19 > 0.12 | -36% |
| scroll-variable-list | 0.36 > 0.33 | -8% | 0.07 > 0.08 | +20% |
| table-stream | 2.67 > 2.21 | -17% | 1.01 > 0.63 | -38% |
| text-log-stream | 0.79 > 0.71 | -9% | 0.84 > 0.73 | -12% |
| text-relabel | 11.71 > 6.80 | -42% | 11.41 > 6.04 | -47% |
| text-typing | 0.91 > 0.86 | -5% | 0.39 > 0.35 | -12% |
| tree-toggle | 4.08 > 3.56 | -13% | 3.10 > 2.03 | -35% |
| **geometric mean** | | **-17%** | | **-34%** |

### `Window::draw` 95th percentile (ms)

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 0.99 > 0.80 | -19% | 1.07 > 0.85 | -20% |
| dashboard-all-panels | 7.34 > 5.19 | -29% | 6.79 > 4.30 | -37% |
| dashboard-clock | 4.04 > 3.51 | -13% | 0.42 > 0.31 | -26% |
| dashboard-one-panel | 4.15 > 3.59 | -13% | 0.66 > 0.50 | -25% |
| effects-cards | 2.92 > 2.56 | -12% | 0.38 > 0.30 | -22% |
| hover-grid | 7.34 > 5.35 | -27% | 6.39 > 4.03 | -37% |
| scroll-data-table | 1.88 > 1.56 | -17% | 0.82 > 0.49 | -41% |
| scroll-div-cards | 14.20 > 12.16 | -14% | 4.69 > 0.96 | -79% |
| scroll-uniform-list | 0.61 > 0.52 | -14% | 0.26 > 0.18 | -33% |
| scroll-variable-list | 0.46 > 0.42 | -9% | 0.24 > 0.19 | -21% |
| table-stream | 2.72 > 2.26 | -17% | 1.09 > 0.71 | -35% |
| text-log-stream | 0.81 > 0.75 | -8% | 0.88 > 0.78 | -11% |
| text-relabel | 12.08 > 7.06 | -42% | 11.79 > 6.33 | -46% |
| text-typing | 0.96 > 0.91 | -5% | 0.45 > 0.40 | -11% |
| tree-toggle | 4.45 > 3.91 | -12% | 5.44 > 4.22 | -22% |
| **geometric mean** | | **-17%** | | **-34%** |

### 6.2 Allocations and heap (independent of the clock)

### Heap allocations per frame

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 3,034 > 1,030 | -66% | 3,114 > 1,110 | -64% |
| dashboard-all-panels | 33,312 > 27,529 | -17% | 28,611 > 17,563 | -39% |
| dashboard-clock | 27,949 > 22,166 | -21% | 2,811 > 191 | -93% |
| dashboard-one-panel | 28,062 > 22,279 | -21% | 3,421 > 628 | -82% |
| effects-cards | 22,749 > 18,655 | -18% | 1,998 > 543 | -73% |
| hover-grid | 49,877 > 29,073 | -42% | 37,235 > 14,839 | -60% |
| scroll-data-table | 9,605 > 7,073 | -26% | 4,657 > 1,601 | -66% |
| scroll-div-cards | 104,063 > 86,458 | -17% | 27,445 > 5,865 | -79% |
| scroll-uniform-list | 3,023 > 2,159 | -29% | 1,662 > 698 | -58% |
| scroll-variable-list | 840 > 680 | -19% | 588 > 408 | -31% |
| table-stream | 18,952 > 15,622 | -18% | 4,725 > 1,687 | -64% |
| text-log-stream | 4,468 > 3,661 | -18% | 5,306 > 3,623 | -32% |
| text-relabel | 37,700 > 31,696 | -16% | 32,545 > 20,556 | -37% |
| text-typing | 3,421 > 2,816 | -18% | 950 > 470 | -51% |
| tree-toggle | 32,035 > 25,262 | -21% | 22,611 > 12,422 | -45% |
| **geometric mean** | | **-26%** | | **-64%** |

### Bytes allocated per frame (KB)

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 4,292 > 1,553 | -64% | 4,360 > 1,625 | -63% |
| dashboard-all-panels | 19,252 > 16,012 | -17% | 11,557 > 7,855 | -32% |
| dashboard-clock | 17,277 > 14,037 | -19% | 403 > 376 | -7% |
| dashboard-one-panel | 17,318 > 14,078 | -19% | 1,408 > 1,304 | -7% |
| effects-cards | 13,693 > 10,693 | -22% | 658 > 631 | -4% |
| hover-grid | 33,822 > 13,995 | -59% | 26,121 > 6,310 | -76% |
| scroll-data-table | 3,705 > 3,465 | -6% | 1,682 > 2,325 | +38% |
| scroll-div-cards | 70,760 > 61,697 | -13% | 10,731 > 3,880 | -64% |
| scroll-uniform-list | 1,331 > 1,103 | -17% | 548 > 677 | +23% |
| scroll-variable-list | 445 > 391 | -12% | 640 > 815 | +27% |
| table-stream | 5,847 > 5,562 | -5% | 627 > 529 | -16% |
| text-log-stream | 2,911 > 2,662 | -9% | 2,900 > 2,579 | -11% |
| text-relabel | 15,492 > 15,356 | -1% | 4,385 > 3,776 | -14% |
| text-typing | 2,284 > 2,197 | -4% | 785 > 711 | -9% |
| tree-toggle | 18,180 > 16,674 | -8% | 9,187 > 7,510 | -18% |
| **geometric mean** | | **-21%** | | **-23%** |

### Peak live Rust heap (MB)

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 14.6 > 14.1 | -4% | 15.2 > 14.8 | -3% |
| dashboard-all-panels | 29.8 > 24.7 | -17% | 32.6 > 26.9 | -17% |
| dashboard-clock | 29.4 > 24.3 | -17% | 30.8 > 25.1 | -19% |
| dashboard-one-panel | 29.4 > 24.3 | -17% | 31.1 > 25.3 | -18% |
| effects-cards | 23.5 > 20.9 | -11% | 23.4 > 21.0 | -10% |
| hover-grid | 60.4 > 47.3 | -22% | 62.7 > 49.4 | -21% |
| scroll-data-table | 24.7 > 21.9 | -11% | 48.7 > 38.8 | -20% |
| scroll-div-cards | 49.7 > 38.6 | -22% | 68.8 > 56.8 | -17% |
| scroll-uniform-list | 13.0 > 12.5 | -4% | 21.2 > 17.8 | -16% |
| scroll-variable-list | 18.0 > 18.3 | +2% | 23.6 > 22.2 | -6% |
| table-stream | 23.3 > 21.4 | -8% | 24.2 > 22.3 | -8% |
| text-log-stream | 16.6 > 16.1 | -3% | 17.1 > 16.2 | -5% |
| text-relabel | 39.0 > 34.4 | -12% | 40.8 > 36.2 | -11% |
| text-typing | 16.5 > 16.1 | -3% | 18.1 > 17.4 | -4% |
| tree-toggle | 32.3 > 23.3 | -28% | 37.7 > 26.4 | -30% |
| **geometric mean** | | **-12%** | | **-14%** |

### 6.3 Memory footprint (process physical footprint at the end, MB)

### Footprint

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 125 > 109 | -13% | 111 > 109 | -1% |
| dashboard-all-panels | 141 > 120 | -15% | 143 > 123 | -14% |
| dashboard-clock | 140 > 120 | -14% | 142 > 122 | -14% |
| dashboard-one-panel | 140 > 119 | -15% | 142 > 123 | -14% |
| effects-cards | 137 > 118 | -14% | 139 > 120 | -13% |
| hover-grid | 171 > 141 | -17% | 172 > 142 | -18% |
| scroll-data-table | 136 > 119 | -13% | 199 > 166 | -16% |
| scroll-div-cards | 149 > 135 | -9% | 215 > 185 | -14% |
| scroll-uniform-list | 110 > 110 | -0% | 162 > 144 | -11% |
| scroll-variable-list | 134 > 119 | -11% | 167 > 152 | -9% |
| table-stream | 136 > 118 | -13% | 136 > 120 | -12% |
| text-log-stream | 128 > 114 | -11% | 129 > 114 | -11% |
| text-relabel | 150 > 131 | -13% | 152 > 134 | -12% |
| text-typing | 130 > 114 | -12% | 132 > 115 | -13% |
| tree-toggle | 145 > 121 | -17% | 149 > 124 | -16% |
| **geometric mean** | | **-13%** | | **-13%** |

### 6.4 First comparison: clocks idle, three drawables

### CPU per frame, clocks idle (ms)

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 3.04 > 2.96 | -3% | 3.03 > 3.05 | +1% |
| dashboard-all-panels | 7.03 > 5.00 | -29% | 6.52 > 4.58 | -30% |
| dashboard-clock | 4.53 > 4.14 | -9% | 3.19 > 2.58 | -19% |
| dashboard-one-panel | 4.60 > 4.20 | -9% | 2.83 > 3.43 | +21% |
| effects-cards | 3.79 > 3.67 | -3% | 1.79 > 1.76 | -1% |
| hover-grid | 7.17 > 5.30 | -26% | 6.61 > 4.55 | -31% |
| scroll-data-table | 3.08 > 2.87 | -7% | 2.94 > 3.54 | +21% |
| scroll-div-cards | 13.56 > 11.76 | -13% | 5.01 > 3.23 | -35% |
| scroll-uniform-list | 3.65 > 3.52 | -4% | 2.44 > 1.83 | -25% |
| scroll-variable-list | 2.99 > 2.85 | -5% | 1.59 > 1.51 | -5% |
| table-stream | 3.66 > 3.34 | -9% | 3.08 > 2.85 | -7% |
| text-log-stream | 2.92 > 2.79 | -4% | 2.96 > 2.83 | -5% |
| text-relabel | 10.79 > 6.61 | -39% | 10.73 > 6.06 | -43% |
| text-typing | 2.98 > 2.91 | -2% | 3.19 > 2.96 | -7% |
| tree-toggle | 4.56 > 4.25 | -7% | 4.56 > 3.77 | -17% |
| **geometric mean** | | **-12%** | | **-14%** |

### `Window::draw` median, clocks idle (ms)

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 2.55 > 2.76 | +8% | 2.73 > 2.76 | +1% |
| dashboard-all-panels | 6.84 > 4.85 | -29% | 6.32 > 4.42 | -30% |
| dashboard-clock | 4.35 > 3.98 | -9% | 2.97 > 2.38 | -20% |
| dashboard-one-panel | 4.43 > 4.03 | -9% | 2.90 > 3.21 | +11% |
| effects-cards | 3.65 > 3.47 | -5% | 1.67 > 1.53 | -8% |
| hover-grid | 6.59 > 5.00 | -24% | 5.99 > 4.26 | -29% |
| scroll-data-table | 2.97 > 2.79 | -6% | 2.65 > 2.83 | +7% |
| scroll-div-cards | 13.11 > 11.45 | -13% | 4.47 > 2.14 | -52% |
| scroll-uniform-list | 3.43 > 3.38 | -2% | 2.09 > 1.28 | -39% |
| scroll-variable-list | 2.77 > 2.60 | -6% | 0.86 > 0.84 | -2% |
| table-stream | 3.52 > 3.25 | -8% | 2.75 > 2.72 | -1% |
| text-log-stream | 2.92 > 2.67 | -8% | 2.67 > 2.78 | +4% |
| text-relabel | 10.61 > 6.44 | -39% | 10.55 > 5.86 | -44% |
| text-typing | 2.57 > 2.88 | +12% | 3.05 > 2.81 | -8% |
| tree-toggle | 4.37 > 4.09 | -6% | 3.59 > 2.75 | -23% |
| **geometric mean** | | **-11%** | | **-18%** |

## 7. Where the remaining time is

Profiles of the final build (`sample`, main thread) are flat: no function is above about 4% of a frame. The largest shares are
`memmove` of large structs (the 224-byte `Quad` and 232-byte `Primitive` are copied several times per primitive, an element's `Style`
and `Div` are moved into arenas), per-primitive scene work (`insert_primitive`, bounds tree, sort, `Scene::clear`), the comparison of kept
styles (`Edges<Length>` equality, about 1.4%), Taffy's own flexbox allocations in normal mode, and Core Text typesetting. Each is 1-4% and
would need a structural change (primitives by reference, one copy of each primitive, grouping rare `Interactivity` fields behind one pointer)
to move. That is why the loop's gains per experiment fell from 10-40% in part 1 to 1-6% in parts 3 and 4.

## 8. Method and records

Loop records (hypothesis, plan, numbers, decision per experiment): `.agentx/experiments/2026-10-15-...-perf-and-memory{,-part-2}`,
`2026-10-16-...-part-3`, `...-part-4`. Durable rules learned are in `.agentx/memory/knowledge/{gpui,platform,repository}/rules.md`.
Detail per part: [benchmark/LOOP-2026-10-16.md](../../benchmark/LOOP-2026-10-16.md).
