---
agentx:
  kind: rules
  scope: knowledge/gpui/frame-pipeline
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-07T16:45:00+02:00
  topics:
    - scene
    - arena
    - element-id
    - deferred
    - content-mask
    - thermal-state
    - devtools
  supersedes: []
  superseded_by: []
---
# Frame Pipeline Rules

- Preserve equal-key scene order for sprites. Use cached-key stable sorting only for the hot monochrome family; keep direct stable sorting for cold or empty families unless separate measurements justify the extra allocation path.
- Register arena drop records only when `std::mem::needs_drop::<T>()` is true.
- Keep `GlobalElementId` on direct `Arc<[ElementId]>` ownership with a cached `collections::FxHasher` hash and full-path equality. Keep reuse diagnostics hash-only behind `diagnostics`; do not add production interning without a measured design that improves throughput and tail latency.
- Keep the full accessed-state history for replay ranges and rollback; restrict finish-time removal to separately tracked replay keys.
- Keep deferred prepaint entries at stable arena indices; process them in place rather than moving completed entries behind cached ranges.
- Keep `gpux::deferred(...)` content-mask-preserving by default; use `Deferred::unclipped()` only for intentional top-level overlays.
- Preserve zero-width and zero-height content masks before device-pixel cover rounding; do not floor and ceil an empty mask independently.
- Elide inherited opacity transformation only when the factor is exactly `1.0`.
- Frame callbacks must not re-enter `AnyWindowHandle::update` to read app-owned platform snapshots; cache them through an owned subscription before scheduling frames.
- Remap inspection occurrence ids, parents, dispatch ids, focus, and property setters on cached prepaint replay, and truncate them on rollback.
