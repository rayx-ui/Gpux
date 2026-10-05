# Prepaint snapshot emptiness

The focused headed WASM/WebGPU TextArea fixture passed all 70 samples and its DPR 2 screenshot
showed the expected scrolled populated text. Replacing the placeholder check's whole-snapshot
string allocation produced a mixed timing result, so it is not retained.

| DPR | Prior p50/p95 | Candidate p50/p95 | Change p50/p95 |
| --- | --- | --- | --- |
| 1 | 129.493 / 141.935 ms | 137.592 / 144.960 ms | +6.3% / +2.1% |
| 2 | 137.290 / 143.619 ms | 133.411 / 145.400 ms | -2.8% / +1.2% |

The candidate is rejected because it regresses DPR 1 and DPR 2 p95. The raw output preserves all
timings and screenshots; the source is restored to the checkpoint.
