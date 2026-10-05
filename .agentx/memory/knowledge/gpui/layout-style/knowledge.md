---
agentx:
  kind: knowledge
  scope: knowledge/gpui/layout-style
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - taffy
    - layout
    - style
    - measured-layout
    - telemetry
  supersedes: []
  superseded_by: []
---
# Layout And Style Knowledge

- Taffy 0.14's `TaffyTree::clear()` clears nodes, children, and parents but not the sparse `node_context_data` map; pooling nodes with empty context slots avoids per-frame sparse-map removal and insertion (229 `SecondaryMap` entries per warmed wide-DataTable frame).
- GPUI layout requests are post-order: every child `LayoutId` exists before its parent is requested, so pooled reuse can compare retained child ids directly with the current slice.
- Taffy's `set_style` replaces the whole style and marks the node dirty unconditionally; skipping it on exact equality keeps valid caches for unchanged unmeasured leaves.
- Warmed wide-DataTable tree shape (both DPRs): 1,123 nodes, 341 leaves, 782 containers, 1,041 child edges; fanout averages 1.33 and peaks at 25, with 92.84% of containers at ≤2 children and 99.74% at ≤8; 405 flex, 377 block, no grid containers; 82 root layout calls. Registry Taffy 0.14.0 beat 0.12.2 by about 20% acknowledged throughput and also beat the removed vendored two-inline-children patch, so no fork is justified.
- Node-request diagnostics on that frame: all 1,123 requests reuse pooled nodes with equal converted styles and ordered topologies; zero new nodes, style writes, or topology writes; 229 measured leaves refill retained slots with no extra dirty marks; all 1,041 child-edge inserts are unique. The remaining request cost is equality verification and its upstream `Style::to_taffy` conversion.
- Layout-bounds telemetry: 3,661 calls, 2,574 cache hits, 1,005 recursive parent-origin hits, zero parent-origin misses per warmed frame.
- Measured-leaf stability: all 882 measurement calls match a prior-frame ordinal with identical inputs and snapped output, and text owns all 882 (list, uniform-list, and custom measurement own none). Closures can still depend on state Taffy does not see, so this is a lead, not a cache contract.
- Style-phase counts per warmed frame: 875 request-layout computations, 845 initial prepaint, zero post-sticky recomputes, 845 paint, 729 structurally static and runtime-safe reuse candidates (86.27%). 757 of 875 request styles are structurally static; 742 come from gpux-owned elements and 15 from external `Interactivity` consumers (RayX Sliver and VirtualList); 716 gpux-owned and 13 external candidates stay eligible at prepaint. The draw-local store holds about 519 KiB for those 729 styles.
- Only 13 of 875 request-layout computations (1.49%) keep an exactly default `Interactivity::base_style`, so deferring allocation for default base refinements is not a material optimization.
- An earlier parallel topology-shadow experiment regressed tail latency and was reverted.
