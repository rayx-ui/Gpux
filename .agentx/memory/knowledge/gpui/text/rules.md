---
agentx:
  kind: rules
  scope: knowledge/gpui/text
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - text-system
    - line-layout-cache
    - text-layout
    - editable-text
  supersedes: []
  superseded_by: []
---
# Text Rules

- Preserve `TextLayout`'s local-size reuse path; measure cache tiers and platform shaping before adding a content/style key every callback must build.
- Do not target glyph shaping for warm-frame text cost; measure previous-frame lookup/promotion or sound upper-layer invalidation instead.
- Keep the line-layout cache's prehashed-key contract: hash every identity field once per borrowed request, retain it in owned keys, hash only the scalar for lookup/removal/promotion, compare every field after the hash check, and keep two-generation rollover and exact eviction.
- Size `shape_text` decoration vectors from `min(input_text_run_count, 32)`; do not restore a fixed 32-slot allocation.
- Keep font-run scratch in the shared thread-local pool, borrowing it only during pop/push; do not restore the `TextSystem` mutex.
- Acquire a pooled `LineWrapperHandle` only after truncation is known to be active.
- Use full-run aggregation, not final-frame telemetry, for text-cache ownership experiments.
- Keep editable-text projection policy-neutral: expose buffer/display transforms and projected style painting, and leave folds, search, gutters, diagnostics, syntax languages, and editor configuration to consumer crates.
- Register a `RenderOnce` editable backing entity with `Window::set_view_id` during prepaint; do not compensate with parent-notify subscriptions or disabled caching.
- When an aggregate selection owner claims an editable drag, cancel GPUI's native pointer-selection session without changing the selection; do not swallow general pointer propagation.
- Preserve manual editable-text scroll across ordinary redraws; reveal the caret only after initial/bounds layout or an edit/selection operation.
- Apply editable paint attributes through decoration runs, independent of shaping-cache keys, and limit each row's shaping work to its overlapping style spans.
