---
agentx:
  kind: knowledge
  scope: knowledge/gpui/frame-pipeline
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-17T10:00:00+02:00
  topics:
    - scene
    - bounds-tree
    - arena
    - element-id
    - element-state
    - accessibility
    - telemetry
  supersedes: []
  superseded_by: []
---
# Frame Pipeline Knowledge

- Warmed wide-DataTable frame (both DPRs): nine batches, 12 queue-buffer writes, 10 bind groups, two render passes, zero atlas uploads and restarts, one submission.
- Monochrome sprites are densely disordered before the stable sort (684 of 1,514 adjacent pairs at DPR 1, 666 of 1,512 at DPR 2), which rejects a usually-sorted fast path. Subpixel and polychrome families are empty in that workload.
- Bounds tree per warmed frame: 504 insertions, seven max-leaf hits (1.39%), 495 slow searches, 14,351 insertion child evaluations, 576 retained nodes in at most three trees. Early intersection rejection cut search-node visits from 25,720 to 4,440 (82.74%) without changing shape or results; single propagation removes at least 429 duplicate bounds unions per frame. A deferred-intersection algorithm regressed and was retired.
- Global element ids: 485 creations containing 8,668 path elements per warmed frame, average depth 17.87, maximum 23; only 3/15/178 paths fit within 4/8/16 elements, which rejects small bounded inline path storage. All 485 match a preceding-frame hash and 24 repeat within the frame.
- Element-state finish previously scanned 1,344 access-history keys per warmed frame with zero successful transfers; tracking pending replay keys separately dropped failed removals to zero and improved acknowledged throughput about 6-8% and paced p95 about 25%.
- `Arena` destructor gating removed 908 bytes from the multithreaded WASM atomic prefix with neutral throughput.
- GPUI dispatch nodes are not a one-to-one rendered element hierarchy: several elements can share one dispatch node and one focus handle. Accessibility nodes are keyed by global element id, and when nested elements share a focus handle the innermost node holds the focus.
- `Drawable::prepaint` has computed bounds and dispatch parentage, and `Drawable::paint` can refresh key/action listener counts before the frame swaps into `rendered_frame`, which makes the element lifecycle the right layer for inspection.
