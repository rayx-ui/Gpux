# Identity display-transform snapshot guard

All 70 headed WASM/WebGPU samples passed and the DPR 2 screenshot retained the expected scrolled
TextArea. The identity-only guard is rejected because the full fixed benchmark is mixed.

| DPR | Prior p50/p95 | Candidate p50/p95 | Change p50/p95 |
| --- | --- | --- | --- |
| 1 | 129.493 / 141.935 ms | 133.356 / 143.637 ms | +3.0% / +1.2% |
| 2 | 137.290 / 143.619 ms | 133.718 / 143.870 ms | -2.6% / +0.2% |

The source is restored to the checkpoint and raw artifacts are retained.
