---
agentx:
  kind: decision
  scope: decisions/gpui/positioning-scroll
  status: current
  created: 2026-07-31T01:00:00+02:00
  updated: 2026-07-31T01:00:00+02:00
  topics:
    - gpux
    - capture-scroll-wheel
    - opaque-hit-test
    - event-propagation
  supersedes: []
  superseded_by: []
---

# Opaque Scroll Capture Owner

## Decision: Capture wheel policy belongs on the opaque hit-test owner

### Context: `DraggableScrollableSheet` must suppress nested scroll delivery while the sheet expands or collapses, then allow the real child scrollable to receive the same input path at maximum extent. Its opaque `GestureDetector` creates a GPUI `BlockMouse` hit-test boundary.

### Alternatives considered: Capture wheel input on an ancestor outside the detector; install competing bubble handlers on the sheet and child; add a sheet-specific wheel router below GPUI.

### Reasoning: GPUI capture listeners arbitrate an event only along its actual dispatch path. Installing the listener on the opaque detector keeps ownership local, allows the sheet to stop propagation before child listeners during `DraggingSheet`, and allows normal child delivery during `ScrollingChild` without a parallel routing system.

### Trade-offs accepted: Components that create opaque hit-test boundaries must explicitly expose and use capture-phase handlers on that surface; ancestor capture is not a universal interception mechanism.

### Supersedes: None
