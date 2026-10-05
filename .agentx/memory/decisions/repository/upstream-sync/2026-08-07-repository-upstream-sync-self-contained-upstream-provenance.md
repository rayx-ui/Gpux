---
agentx:
  kind: decision
  scope: decisions/repository/upstream-sync
  status: current
  created: 2026-08-07T14:45:00+02:00
  updated: 2026-08-07T14:45:00+02:00
  topics:
    - gpux
    - upstream-sync
    - provenance
    - history
  supersedes: []
  superseded_by: []
---

## Decision: Keep gpux upstream provenance self-contained in RayX

### Context: The Rust RayX migration copied the current gpux tree but did not retain the earlier Zed import markers in current-branch ancestry. Future updates otherwise depended on the old `RayX.NET/RayX` checkout to recover the last Zed base and pre-migration patch history.

### Alternatives considered: Continue using the predecessor checkout; keep only a prose base SHA; import the predecessor's unrelated full history; preserve a gpux-only archive graph plus checked-in ledgers.

### Reasoning: A filtered 72-commit gpux graph preserves inspectable patch diffs and provenance bodies without importing unrelated managed/product history. Checked-in ledgers make the latest Zed base and hash mappings discoverable even before archive refs are inspected. Current RayX history remains authoritative after the Rust migration boundary.

### Trade-offs accepted: The archive branch and tag must be pushed explicitly because they are separate refs. Each validated Zed import must update both commit-body provenance and the checked-in ledger, adding a small maintenance obligation.

### Supersedes: None
