---
agentx:
  kind: decision
  scope: decisions/gpui/positioning-scroll
  status: current
  created: 2026-07-04T15:28:46+02:00
  updated: 2026-07-04T15:28:46+02:00
  topics:
    - runtime
    - gpux-foundation
    - fixed-sticky-positioning
    - scene-order
    - hit-testing
  supersedes: []
  superseded_by: []
---

## Decision: Order fixed and sticky divs through shared positioned paint and hit-test lanes

### Context: The fixed/sticky implementation already resolved correct visual bounds, masks, hitboxes, and scroll contexts, but scene primitive ordering still allowed later ordinary content to paint over fixed or sticky elements after scroll. Hit testing also used stored prepaint insertion order, so later ordinary hitboxes could remain topmost over visually elevated fixed/sticky elements.

### Alternatives considered: Give sticky elements a sticky-only paint lane, add app-level overlay wrappers, special-case only the Lab story, reorder all normal scene primitives globally, sort the stored frame hitbox vector, or add shared positioned paint and hit-test lanes for both fixed and sticky behavior.

### Reasoning: Fixed and sticky are both positioned chrome relative to ordinary flow. They must keep the normal GPUI paint path so backgrounds, borders, text, children, masks, transforms, and opacity continue to behave like any other `div()`, but their primitives need a higher scene order than later ordinary siblings. A shared positioned scene layer gives fixed/sticky content a high draw-order lane while using a separate `BoundsTree` to preserve overlap ordering among positioned elements; later overlapping positioned elements still paint above earlier ones. Hitboxes need the same topmost semantics, but cached prepaint reuse stores hitbox index ranges, so the stored hitbox vector must not be globally sorted. High positioned `HitboxId` values plus point-local sorting of matching hitboxes preserves reuse ranges while making fixed/sticky hitboxes topmost over ordinary hitboxes.

### Trade-offs accepted: This is a GPUI positioned-order rule, not a full CSS stacking-context or `z-index` implementation. Hit testing now sorts the hitboxes matching the queried point instead of relying solely on reverse vector order. More advanced stacking semantics can be added later, but ordinary fixed/sticky correctness should not depend on app-authored overlays or proxy elements.

### Supersedes: None
