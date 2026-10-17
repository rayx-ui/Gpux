---
agentx:
  kind: decision
  scope: decisions/gpui/text
  status: current
  created: 2026-10-17T12:00:00+02:00
  updated: 2026-10-17T12:00:00+02:00
  promoted_from: gpux-rich-editable-text
  topics:
    - gpux
    - editable-text
    - rich-text
    - formatting
    - history
  supersedes: []
  superseded_by: []
---

# Editable State Owns Rich Formatting

## Decision: `EditableTextState` stores character, paragraph and object formatting beside its text, remapped at its single edit choke point

### Context: gpux has one editable element, `EditableText`. RayX's rich-text editor re-derived style runs by diffing text after every change, which loses formatting across edits and history.

### Alternatives considered: A separate rich editor element; owner-held attributes re-sent as revision-bound style runs each frame; attributes stored in the text buffer.

### Reasoning: Platform editors keep attributes in the editable state (`NSAttributedString`, `Spannable`, `AnnotatedString`, `TextSpan`). Storing sorted, non-overlapping `Arc<TextAttributes>` spans, a paragraph-format table and object attributes in the state lets one remap in `apply_buffer_edit` carry formatting through every edit, lets history share it by `Arc`, and keeps one element for plain and rich fields. Formatting is created on first use, so a plain field allocates nothing and keeps today's paths. `EditableTextEvent` keeps its shape (RayX destructures it irrefutably); formatting changes have their own `format_revision` and `EditableTextFormatEvent`. `AttributedText` is the public snapshot for owners and the HTML codec.

### Trade-offs accepted: Every buffer mutation must pass through the remap; whole-buffer paths restore formatting from history instead. gpux holds no editing policy: toolbars, documents, Markdown, collaboration and code-editor decorations stay with owners, and gutters, folds, search and diagnostics never enter the layout map.

### Supersedes: None.
