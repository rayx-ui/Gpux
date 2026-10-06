---
agentx:
  kind: knowledge
  scope: knowledge/gpui/text
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-06T03:20:00+02:00
  topics:
    - text-system
    - line-layout-cache
    - text-layout
    - telemetry
  supersedes: []
  superseded_by: []
---
# Text Knowledge

- `TextLayout::index_for_position` treats lines as half-open bands: a point exactly on a line boundary belongs to the line below. Upstream routes it into the line above, whose wrapped layout has no row there and returns offset 0, so a drag ending on a boundary collapsed CodeView selections to their start.
- A multi-line `EditableText` sizes its paint element to 100% of its parent, so it needs a definite-height container. A parent sized only by `min_h` leaves it zero-height: the text is clipped away and the editable has no hit area. Give the container a height or a flex basis.

Measured on the warmed wide DataTable at both DPRs:

- 882 text measurements: 562 (63.72%) return from `TextLayout`'s local-size cache and 320 run full layout. The 320 split into 229 empty first-use states (one per measured leaf) and 91 first wrap-width transitions, with no truncation invalidation and no transition back, so a multi-width local cache is unjustified.
- Every full layout issues one wrapped-layout request: 19 current-frame hits, 301 previous-frame hits, zero misses, no unwrapped requests, and zero platform shaping. Remaining warm cost is upper-layer measurement plus previous-frame lookup/promotion, not glyph shaping.
- Before prehashing, those 320 requests executed 921 full-key hashes over 17,178 text bytes; prehashing cut that to 320 hashes (-65%) for one retained `u64` per owned key.
- `shape_text` runs 320 times with exactly one decoration run per line; the fixed 32-slot reservation wasted 9,920 of 10,240 slots per frame.
- The font-run pool served 320 takes with zero misses; the old shared mutex cost 640 uncontended acquisitions per frame. The truncation-only wrapper checkout removed 320 unused checkouts and returns, each of which locked the shared wrapper pool.
- Full-run aggregation tells a different story than the warmed final frame: over a complete interaction run, about 9.5% (DPR 1) and 10.5% (DPR 2) of wrapped requests miss, and each miss makes one nested unwrapped request that misses both caches. Final-frame-only telemetry hides this cold/transition population.
