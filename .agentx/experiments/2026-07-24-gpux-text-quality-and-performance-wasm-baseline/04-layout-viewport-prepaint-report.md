# Rejected viewport-bounded editable-text prepaint

The experiment passed 70/70 page-isolated headed WASM/WebGPU samples and preserved the visible
TextArea scroll output. It did not demonstrate a sufficiently stable direct improvement: DPR 1
improved from 534.002/593.299 ms to 484.844/529.776 ms p50/p95, while DPR 2 p50 regressed from
497.957 ms to 501.996 ms despite a p95 improvement to 522.796 ms.

The source change was restored to checkpoint `5367ab27d` immediately. The raw run remains for
future diagnosis, but this hypothesis contributes no accepted production change.
