# Snapshot-range editable text layout iteration

All 70 focused headed WASM/WebGPU samples passed with no diagnostics failures and visible,
nonzero TextArea scroll pixels. Unprojected layout preparation no longer materializes the full Rope
or creates a logical-line vector; it slices validated source line ranges one at a time. Projected
text streams its already-owned display string without a line vector.

| DPR | Baseline p50/p95 | After p50/p95 | Reduction p50/p95 |
| --- | --- | --- | --- |
| 1 | 534.002 / 593.299 ms | 129.493 / 141.935 ms | 75.8% / 76.1% |
| 2 | 497.957 / 561.084 ms | 137.290 / 143.619 ms | 72.4% / 74.4% |

The result is accepted. DPR 2 remains narrowly above the epic p50 target, so the next atomic
experiment must address a measured residual without weakening fixtures or thresholds.
