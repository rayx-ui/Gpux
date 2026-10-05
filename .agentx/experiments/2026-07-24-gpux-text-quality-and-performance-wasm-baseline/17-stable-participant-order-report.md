## Stable SelectionArea participant-order fast path

Accepted. `SelectionAreaState::end_frame` first compares the current and next participant sequences directly. When they are identical, no participant ID vectors or temporary membership `HashSet`s are created. A changed sequence retains the established set comparison to preserve membership semantics.

The focused WASM/WebGPU fixture passed at DPR 1 and DPR 2, then passed 70 repeated runs. Input-to-published-frame p50/p95 was 92.52/96.55 ms at DPR 1 versus the 88.19/95.91 ms baseline, and 90.84/98.86 ms at DPR 2 versus 89.56/106.01 ms. The small DPR 1 p95 variation remains below the five-percent rejection threshold; DPR 2 p95 improved. A fresh DPR 1 screenshot retains the expected participant rendering after the scroll action.

Decision: retain the direct stable-order check. It is the smallest way to remove dormant participant-sized allocations without adding map-maintenance work to every traversal.
