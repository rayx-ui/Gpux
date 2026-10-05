# Borrowed retained TextLayout row cache

The focused Test Suite TextArea page passed headed WASM/WebGPU at DPR 1 and DPR 2, with visible
scroll deltas and nonempty physical-pixel change. `TextLayoutCache::reusable_rows` now returns an
optional borrow of the retained map. `TextLayoutMap` only clones an individual `Arc` when a row is
installed in the new cache; the former whole-map `self.rows.clone()` no longer exists.

The fixed wheel action follows the exact unchanged-key fast path and therefore does not exercise a
changed-layout cache lookup. Its one-sample timings are recorded as runtime health evidence, not as
an allocation benchmark. The reduction is direct from the removed clone and does not alter atomic
error behavior because the installed map remains owned by the cache until successful replacement.
