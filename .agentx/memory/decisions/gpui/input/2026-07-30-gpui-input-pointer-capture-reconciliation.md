---
agentx:
  kind: decision
  scope: decisions/gpui/input
  status: superseded
  created: 2026-07-30T21:05:00+02:00
  updated: 2026-10-05T23:30:00+02:00
  topics:
    - gpux
    - pointer-capture
    - pointer-interaction-owner
    - reconciliation
  supersedes: []
  superseded_by:
    - 2026-10-05-gpui-input-upstream-input-model.md
---

# Per-pointer Capture Reconciliation

## Decision: Keep pointer ownership per PointerId and refresh capture after reconciliation

### Context: A terminal pointer event can trigger render/update reconciliation before raw dispatch. Reconciliation may preserve the active typed action owner while replacing its painted hitbox, leaving a pre-reconciliation captured hitbox stale.

### Alternatives considered: Retain one window-global owner; dispatch through the original captured hitbox; or cancel every interaction whenever its exact hitbox changes.

### Reasoning: PointerId is already the independent input lifecycle identity. Keeping owner and capture maps per pointer preserves concurrent pointer sequences, while refreshing the captured hitbox after reconciliation delivers Up or Cancel through the current dispatch tree without losing the established typed action owner.

### Trade-offs accepted: Terminal dispatch performs one additional per-pointer capture lookup after reconciliation, and owner-preserving hitbox rebinds remain part of the Window lifecycle contract.

### Supersedes: None.
