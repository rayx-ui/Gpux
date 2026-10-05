# Visual-row bounded editable prepaint

All 70 focused headed WASM/WebGPU samples passed. The DPR 2 screenshot shows the expected
scrolled populated TextArea. Prepaint now uses two partition-point searches over sorted row
metadata and paints only viewport-intersecting rows plus one visual row of overscan.

| DPR | Baseline p50/p95 | After p50/p95 | Reduction p50/p95 |
| --- | --- | --- | --- |
| 1 | 534.002 / 593.299 ms | 112.869 / 116.588 ms | 78.9% / 80.4% |
| 2 | 497.957 / 561.084 ms | 112.852 / 124.215 ms | 77.3% / 77.9% |

Accept the result: both cells meet the epic TextArea threshold of 25% of baseline.
