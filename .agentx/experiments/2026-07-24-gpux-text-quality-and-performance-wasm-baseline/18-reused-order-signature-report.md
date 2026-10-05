## Reused SelectionArea geometry signature buffer

Rejected and reverted. The candidate reused an area-owned scratch vector while calculating the next geometry signature. The focused DPR 1/DPR 2 fixture and the 70 repeated post-change runs passed, but DPR 1 p95 input-to-published-frame duration regressed from 95.91 ms to 104.31 ms (8.8%), exceeding the experiment's five-percent gate.

The candidate source has been restored to the checkpoint before this result is recorded. The retained direct stable-order fast path remains the accepted dormant-frame allocation reduction.
