## Revisioned SelectionArea participant reconciliation

Rejected and reverted. The candidate replaced temporary participant ID vectors and `HashSet`s with a revisioned area-owned position map. The focused SelectionArea fixture rendered correctly and passed its normal DPR 1/DPR 2 smoke run, but its repeated DPR 1 p95 input-to-published-frame duration regressed from 95.91 ms to 105.40 ms (9.9%). That exceeds the experiment's five-percent limit.

The first baseline pass had one unrelated DPR 2 browser closure during screenshot capture; a narrow DPR 2 retry passed. The accepted 70-run post-change measurement completed with no failures. The source was returned exactly to its checkpoint implementation before recording this result.

Decision: do not retain the map. A later experiment must improve dormant reconciliation without introducing a map-maintenance cost on every participant traversal.
