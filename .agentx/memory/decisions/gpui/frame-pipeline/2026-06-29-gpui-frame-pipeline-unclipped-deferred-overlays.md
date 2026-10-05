---
agentx:
  kind: decision
  scope: decisions/gpui/frame-pipeline
  status: current
  created: 2026-06-29T23:36:45+02:00
  updated: 2026-06-29T23:36:45+02:00
  topics:
    - runtime
    - gpux-foundation
    - overlays
  supersedes: []
  superseded_by: []
---

## Decision: Add explicit unclipped deferred overlays

### Context: DatePicker calendars opened inside `Dialog` were correctly deferred and anchored, but still inherited the dialog content mask from `gpux::deferred(...)`, so the calendar was clipped at the dialog bounds.

### Alternatives considered: Remove the dialog body overflow mask; make all deferred draws ignore content masks; move each affected overlay to a different place in the component tree; add an explicit deferred opt-in that passes `content_mask: None` to `Window::defer_draw`.

### Reasoning: Dialog and scroll clipping are legitimate for normal content, and ordinary deferred elements should keep inheriting masks. Top-level overlays are the distinct case: they already use deferred layering and viewport-aware anchors, but must not be clipped by ancestors. An explicit `Deferred::unclipped()` keeps the default safe while giving overlay components a clear, reusable escape hatch.

### Trade-offs accepted: Overlay authors must remember to call `.unclipped()` for floating surfaces. This is preferable to making every deferred draw ignore clipping and risking regressions in non-overlay deferred content.

### Supersedes: none
