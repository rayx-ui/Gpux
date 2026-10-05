---
agentx:
  kind: state
  scope: knowledge/gpui/text
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - text-system
    - line-layout-cache
    - text-layout
    - editable-text
    - selection
    - ime
  supersedes: []
  superseded_by: []
---
# Text State

## Purpose And Usage

Read before changing text shaping, line layout caches, `TextLayout`, or `editable_text`.

## Current Technical Shape

`TextSystem::shape_text` seeds each line's decoration vector from `min(input_text_run_count, 32)`. All four font-run scratch callers share a thread-local pool whose `RefCell` borrow covers only pop/push, so nested calls are safe and the old shared mutex is gone. `TextLayout` takes a pooled `LineWrapperHandle` only inside its truncation branch and keeps its local-size cache for repeated measurement at the same width.

The ordinary line-layout cache keeps two generations (current and previous frame) with exact unused-entry eviction. Each borrowed request computes one deterministic Fx hash over all identity fields; owned keys retain that scalar, so lookup, previous-frame removal, and promotion hash only the scalar while equality still compares the hash and every text/layout field.

`editable_text` is GPUI's public, policy-neutral editing foundation: revisioned UTF-8 buffers and snapshots, selection, history, IME, single-line and multiline modes, and `TextLayoutMap`. The layout map maps buffer offsets, visual rows, and pixel geometry for soft-wrap and no-wrap text, retains unaffected shaped rows across incremental edits, and invalidates for text, width, DPR, font metrics/features/fallbacks, tabs, and font-affecting style runs; paint-only color changes reuse shaping. `EditableText` uses it for vertical and page movement, wrapped selection, horizontal and vertical viewport scrolling, drag auto-scroll, resize, and IME geometry. Selection paint intersects the normalized byte range with each visible visual row (including empty and newline rows). Platform `secondary()+Shift+Up/Down` extends to logical-line boundaries independent of soft wrap. The shared epoch-guarded drag loop keeps selecting and scrolling with a stationary pointer outside the viewport and stops after release.

`EditableText` registers its backing state entity with `Window::set_view_id` during prepaint, so state notifications invalidate cached ancestors. `EditableTextState::cancel_pointer_selection` ends only GPUI's native pointer-drag session and auto-scroll epoch for an aggregate selection owner that claimed the drag. `EditableTextDisplayTransform::Projected` gives owners such as code editors a buffer/display projection without fold policy; projected `EditableTextStyleRuns` keep font-affecting attributes in shaping while color, background, underline, strikethrough, and fade flow through paint-time decoration runs, and layout keys restrict each row to its overlapping style spans. Manual multi-line wheel scroll persists across ordinary redraws; only initial layout, viewport changes, edits, and explicit selection movement reveal the caret.

## Backlinks

- [Text system](../../../../../crates/gpui/src/text_system.rs)
- [Line layout cache](../../../../../crates/gpui/src/text_system/line_layout.rs)
- [Line wrapper](../../../../../crates/gpui/src/text_system/line_wrapper.rs)
- [Text element](../../../../../crates/gpui/src/elements/text.rs)
- [Editable text](../../../../../crates/gpui/src/elements/editable_text/mod.rs)
- [Editable text element](../../../../../crates/gpui/src/elements/editable_text/element.rs)
