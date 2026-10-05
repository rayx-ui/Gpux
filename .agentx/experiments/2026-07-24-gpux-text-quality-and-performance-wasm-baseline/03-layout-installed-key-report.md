# Installed-key TextLayoutMap fast path

## Scope confirmation

This experiment added only the exact installed-key return. It compares the buffer revision and
layout style kept by `TextLayoutCache`, then compares the retained normalized source style runs
and display projection. The return is before `snapshot.text()`, `logical_lines(...)`, and
`reusable_rows(...)`; therefore unchanged updates do no whole-text materialization, logical-line
vector build, row-cache clone, boundary build, or shaping.

## WASM measurement

The focused TextArea Test Suite page passed 70/70 headed Chromium/WebGPU samples: 35 at DPR 1 and
35 at DPR 2, with five warmups and thirty measured samples in each group. There were no browser or
runtime failure diagnostics. The final screenshots showed the visible 2,000-line fixture scrolled
near its final rows and every action changed physical pixels.

| DPR | Baseline p50/p95 | After p50/p95 | Delta p50/p95 |
| --- | --- | --- | --- |
| 1 | 534.002 / 593.299 ms | 501.607 / 557.942 ms | -6.1% / -6.0% |
| 2 | 497.957 / 561.084 ms | 382.557 / 494.546 ms | -23.2% / -11.9% |

The result is a direct improvement on both measured scales, but does not yet meet the epic's
75-percent reduction target. The remaining all-row rebuild path is reserved for a separate,
atomic incremental-layout experiment.
