# macOS performance and memory loop: report of gains, 2026-10-15 to 2026-10-16

Branch `perf/agentx-macos-loop` (worktree `Gpux-agentx-loop`), 70 commits since the opening state: **local only, not pushed or merged, native macOS only**
(Apple M5, macOS 26.6.2, Metal, 120 Hz). Four AgentX improvement-loop runs, 31 experiments plus 4 baselines: 19 kept, 12 reverted (one kept experiment, two drawables, was undone by a later one).
The user stopped the loop during experiment 3 of part 4; that experiment was reverted untested. About 35 of the 50 iterations were used.

## 1. How the numbers were taken

The headline tables compare the **opening state** (commit `d5cb2e1`: PR #3's benchmark plus PR #7's scroll-layer memory bounds,
before any loop change) with the **final state** (this branch's head, `bcdcf81`). Both binaries were built from their commits and run
**alternated in one session** (old, new, old, new...) so frequency and thermal drift hit both alike: 15 scenarios, fast mode off and on,
3 repetitions each, 600 measured frames after 120 warm-up frames. Cells are the median of the three. Raw data:
[results/2026-10-16-opening-vs-final/](results/2026-10-16-opening-vs-final/) (summary.json, 180 result files).

Caveats that apply to every table:
- `main_cpu_ms` is thread CPU time. On this machine it moves with pacing and the CPU's frequency choice (the same build read 1.8 or 2.6 ms
  on one scenario from one run to the next), so only the alternated comparison here and the per-experiment A/Bs are evidence; two separate
  runs hours apart are not (the earlier closing runs differ from this one by up to 40% on scenarios nothing touched).
- Allocation counts and bytes are deterministic. Heap peak is nearly so. Footprint moves by a few MB from run to run.
- Absolute `Window::draw` figures also differ between sessions; compare the before/after columns of one table.

## 2. Summary

| | Fast mode off | Fast mode on |
|---|---|---|
| CPU per frame, geometric mean over 15 scenarios | **-12%** | **-14%** |
| `Window::draw` median, geometric mean | -11% | -18% |
| Heap allocations per frame, geometric mean | **-26%** | **-64%** |
| Bytes allocated per frame, geometric mean | -21% | -23% |
| Peak live Rust heap, geometric mean | -12% | -14% |
| Memory footprint at the end, geometric mean | -2% | -2% |

Largest wins (before > after):
- `text-relabel`: CPU 10.8 > 6.6 ms off, 10.7 > 6.1 ms on (**-39% / -43%**); it now holds 120 fps where it ran at 97.
- `hover-grid`: CPU 7.2 > 5.3 off (-26%), 6.6 > 4.6 on (**-31%**); allocations 37,235 > 14,839 on (-60%).
- `dashboard-all-panels`: CPU 7.0 > 5.0 off (-29%), 6.5 > 4.6 on (-30%).
- `scroll-div-cards`, fast on: CPU 5.0 > 3.2 ms (-35%), `Window::draw` 4.5 > 2.1 ms (**-52%**), allocations 27,445 > 5,865 (**-79%**), peak heap 68.8 > 56.9 MB.
- `dashboard-clock`, fast on: allocations 2,811 > 191 per frame (**-93%**), CPU 3.2 > 2.6 ms (-19%).
- `scroll-uniform-list`, fast on: CPU 2.4 > 1.8 ms (-25%), `Window::draw` -39%.
- `tree-toggle`, fast on: CPU -17%, `Window::draw` -23%, allocations -45%, peak heap 37.7 > 26.5 MB (-30%).
- Peak Rust heap fell 10-30% in 10 of 15 scenarios with fast mode on, and 3-8% in four more (`scroll-data-table` 48.7 > 39.0, `hover-grid` 62.7 > 49.4).

**Not a gain, found by this comparison:** two fast-on scenarios are slower in CPU than the opening state, consistently across all three
repetitions: `scroll-data-table` (2.94 > 3.54 ms, +21%; `Window::draw` p50 +7%) and `dashboard-one-panel` (2.83 > 3.43 ms, +21%; draw +11%).
The data-table cost is known and was accepted at the time: keeping sized Core Text fonts speeds text-relabel by 40% and costs the
data-table about 10% (experiment 13 of part 1, a same-binary check), and the shorter list overscan makes lists render more rows while
scrolling in exchange for 13% less heap. The `dashboard-one-panel` regression is **unexplained**: I did not bisect it. A later check that
the closing build of part 3 and the final build are equal (both about 3.0-3.5 ms there) shows it entered earlier. Treat it as an open item.
Also with fast mode on, `animated-dots` (+1%), `text-log-stream` draw (+4%) and, with it off, `text-typing` draw (+12%, CPU -2%) did not improve.


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
| `7dfa2e4` | Three Metal drawables restored after trying two (two saved 16-22 MB but made effects-cards take ~1.5x the CPU) | effects-cards back to 1.5 ms |

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

Part 4, 1 experiment kept (user stopped the loop):
| `bcdcf81` | The unit transformation skips the matrix when a primitive's bounds are taken | hover-grid draw -0.9%, div-cards draw -2% (minor) |

## 4. What was tried and reverted (12)

Boxing the devtools semantics (more allocations); a directional list overscan (two layer tests failed); 256 px tiles (footprint +4%);
a digit glyph table (18% slower than Core Text's warm path in a same-binary A/B; a cross-build measurement had suggested -32%);
boxing the div interactivity (allocations +8%); inlined length and edge comparisons (0%); two Metal drawables (see above); a one-pass
claim of kept layout nodes, tried twice (within noise); recycling the base style boxes (allocations -67% but div-cards draw +6%);
deciding "emoji" once per glyph run in the Core Text shaper (0%); halving the share of list-row layout nodes that linger
(started, stopped before measuring). Each failure is recorded with its numbers in `.agentx/experiments/`.

## 5. Targets from the opportunity list, and where they stand

| Target | Result |
|---|---|
| Scroll-layer memory | Peak heap down 11-22% on the scroll scenarios (list overscan, lazy element data). Footprint with fast mode on is still 1.30-1.46x the footprint with it off, and 3-7% lower than at the opening: the remainder is the visible tile textures (private Metal storage, about the window area) and the three drawables, which a `malloc_history` check confirmed are not Rust heap. |
| Spliced frame copy cost | `dashboard-clock` fast on: allocations -93%, draw p50 -20% (2.97 > 2.38 ms here; 1.4 ms in a quieter session). The 1 ms target is **not met**: the work left is about 0.9 us per retained element across claiming, copying records and replaying primitives. |
| Skip layout and prepaint in composited scroll layers | Done for layout (`f7a22c7`): div-cards draw p50 4.5 > 2.1 ms (target 2 ms, **met within noise**). Prepaint is still needed. |
| scroll-data-table with fast mode | **Worse**: CPU +21% against the opening (see section 2); heap and footprint are better (-20% / -7%). |
| Adaptive layout retention | text-relabel, hover-grid, tree-toggle all improved a lot (-43%, -31%, -17% CPU). |
| text-typing on macOS | Fast on: CPU -7%, allocations -51%; off: unchanged. |
| Hover granularity | **Not done, and it is a design limit**: splicing reuses per nested view, so a hover inside one view (hover-grid's 1,600 cells) builds that whole view again. Per-cell views would splice; element-level reuse inside a view was not attempted. |
| Scroll-frame allocations | Div-cards fast on 27,445 > 5,865 per frame (-79%); data-table -66%; uniform list -58%. |
| Text shaping | Sized font cache (-40% on relabel), shared defaults and runs (-11% to -20% allocations). Core Text's own typesetting is now about half of text-relabel's frame and was not touched. |
| Benchmark coverage (GPU time) | Added `gpu_ms` to the JSON and the report (macOS). Lanes B-G (web, Android, iOS, real GPUs) were not run. |

## 6. Full results

### CPU per frame on the main thread (`main_cpu_ms`, mean, ms)

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

### `Window::draw`, median (ms)

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
| hover-grid | 33,822 > 13,994 | -59% | 26,121 > 6,310 | -76% |
| scroll-data-table | 3,705 > 3,465 | -6% | 1,682 > 2,325 | +38% |
| scroll-div-cards | 70,760 > 61,697 | -13% | 10,731 > 3,880 | -64% |
| scroll-uniform-list | 1,331 > 1,103 | -17% | 549 > 678 | +23% |
| scroll-variable-list | 445 > 391 | -12% | 641 > 815 | +27% |
| table-stream | 5,847 > 5,562 | -5% | 627 > 529 | -16% |
| text-log-stream | 2,911 > 2,662 | -9% | 2,900 > 2,579 | -11% |
| text-relabel | 15,492 > 15,356 | -1% | 4,385 > 3,776 | -14% |
| text-typing | 2,284 > 2,197 | -4% | 785 > 711 | -9% |
| tree-toggle | 18,180 > 16,674 | -8% | 9,187 > 7,510 | -18% |
| **geometric mean** | | **-21%** | | **-23%** |

### Peak live Rust heap (MB)

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 14.7 > 14.3 | -3% | 15.4 > 15.0 | -3% |
| dashboard-all-panels | 29.7 > 24.5 | -17% | 32.5 > 26.8 | -18% |
| dashboard-clock | 29.4 > 24.1 | -18% | 30.8 > 25.1 | -19% |
| dashboard-one-panel | 29.4 > 24.2 | -18% | 31.1 > 25.4 | -18% |
| effects-cards | 23.5 > 21.1 | -11% | 23.4 > 21.0 | -10% |
| hover-grid | 60.4 > 47.1 | -22% | 62.7 > 49.4 | -21% |
| scroll-data-table | 24.7 > 21.9 | -11% | 48.7 > 39.0 | -20% |
| scroll-div-cards | 49.8 > 38.6 | -22% | 68.8 > 56.9 | -17% |
| scroll-uniform-list | 13.2 > 12.8 | -3% | 21.3 > 17.9 | -16% |
| scroll-variable-list | 18.1 > 18.3 | +1% | 23.6 > 22.2 | -6% |
| table-stream | 23.4 > 21.3 | -9% | 24.3 > 22.4 | -8% |
| text-log-stream | 16.7 > 16.1 | -4% | 17.2 > 16.3 | -5% |
| text-relabel | 39.0 > 34.4 | -12% | 40.8 > 36.1 | -11% |
| text-typing | 16.6 > 16.1 | -3% | 18.1 > 17.5 | -3% |
| tree-toggle | 32.3 > 23.3 | -28% | 37.7 > 26.5 | -30% |
| **geometric mean** | | **-12%** | | **-14%** |

### Memory footprint at the end (MB, process physical footprint)

| Scenario | Fast mode off: before > after | Change | Fast mode on: before > after | Change |
|---|---|---|---|---|
| animated-dots | 126 > 124 | -2% | 126 > 125 | -1% |
| dashboard-all-panels | 141 > 136 | -4% | 128 > 138 | +8% |
| dashboard-clock | 139 > 135 | -3% | 142 > 138 | -3% |
| dashboard-one-panel | 141 > 135 | -4% | 142 > 138 | -3% |
| effects-cards | 137 > 133 | -2% | 145 > 136 | -6% |
| hover-grid | 169 > 157 | -7% | 173 > 160 | -8% |
| scroll-data-table | 135 > 133 | -2% | 200 > 187 | -7% |
| scroll-div-cards | 149 > 151 | +2% | 216 > 209 | -3% |
| scroll-uniform-list | 126 > 126 | -0% | 171 > 166 | -3% |
| scroll-variable-list | 135 > 135 | +0% | 175 > 173 | -1% |
| table-stream | 136 > 134 | -1% | 137 > 137 | +0% |
| text-log-stream | 131 > 130 | -0% | 132 > 131 | -1% |
| text-relabel | 151 > 146 | -3% | 150 > 150 | -0% |
| text-typing | 130 > 130 | +0% | 131 > 132 | +0% |
| tree-toggle | 142 > 137 | -4% | 149 > 141 | -6% |
| **geometric mean** | | **-2%** | | **-2%** |


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
Detail per part: [LOOP-2026-10-16.md](LOOP-2026-10-16.md).
