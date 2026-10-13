---
agentx:
  kind: decision
  scope: decisions/gpui/frame-pipeline
  status: current
  created: 2026-10-13T10:00:00+02:00
  updated: 2026-10-13T10:00:00+02:00
  topics:
    - gpux
    - fast-mode
    - oracle
    - testing
  supersedes: []
  superseded_by: []
---

# Hold every fast-mode frame equal to a frame drawn from scratch

## Decision: Hold every fast-mode frame equal to a frame drawn from scratch

### Context: Retention, splice and layers reuse what an earlier frame produced; any difference from the frame a window drawing from scratch would draw is a bug the user sees only as a stale or wrong pixel.

### Alternatives considered: Compare pixels only; test each mechanism against hand-written expectations.

### Reasoning: A randomized oracle (`fast::tests::oracle*`) draws an incremental window and a from-scratch window in lockstep after every change and compares the whole frame description (`Window::describe_rendered_frame`: scene families, layers expanded, hitboxes, listeners, cursors, tooltips, tab stops, dispatch tree, element state, DevKit nodes); deterministic counter tests (`fast_mode_attribution`) pin which mechanism does which work; lane A/C parity groups compare the conformance catalog with fast mode on and off. A frame that differs is fixed in the mechanism, never in the oracle.

### Trade-offs accepted: The oracle needs a deterministic test platform and a glyph-box text system; mechanisms that depend on a real renderer (tiles) add lane C/B/E/F groups beside it. Randomized tests with every mechanism on (`randomized_element_tree_matches_full_render_in_fast_mode`) found the splice state leak.

### Supersedes: None.
