---
agentx:
  kind: decision
  scope: decisions/repository/validation
  status: current
  created: 2026-10-09T17:00:00+02:00
  updated: 2026-10-19T14:27:00+02:00
  topics:
    - gpux
    - validation
    - rayx
    - runtime-proof
  supersedes: []
  superseded_by: []
---

## Decision: gpux owns its runtime proof; RayX is only a consumer

### Context: gpux's guidance said runtime proof happened in RayX (its Test Suite, xtask and CI). The owner rejected an epic that relied on RayX tests (2026-10-07): a library published to crates.io must be verifiable from its own repository.

### Alternatives considered: Keep RayX's Test Suite as gpux's runtime proof; share a test harness crate between the two repositories.

### Reasoning: No gpux test, tool, workflow, document or skill step requires a RayX checkout, app, DevKit, xtask or CI; `gpux-testkit` and `gpux-tests-conformance` replace them. RayX stays a consumer: a change to an API RayX uses still ships with the matching RayX change, whose checks prove the consumer still works, not gpux. RayX-specific follow-ups belong to a later RayX epic.

### Trade-offs accepted: RayX's broad application workloads no longer count as gpux evidence, so gpux maintains its own scene catalog and lanes.

### Supersedes: None.
