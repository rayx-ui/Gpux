---
agentx:
  kind: decision
  scope: decisions/repository/packaging
  status: superseded
  created: 2026-08-30T22:55:00+02:00
  updated: 2026-10-05T18:00:00+02:00
  topics:
    - gpux
    - dependencies
    - provenance
    - repository-size
  supersedes:
    - ../../gpui/layout-style/2026-08-30-gpui-layout-style-taffy-inline-children.md
  superseded_by:
    - 2026-10-05-repository-packaging-crates-io-packaging.md
---

## Decision: Prefer upstream dependency sources over full vendoring

### Context: A measured two-slot Taffy child patch improved the wide DataTable benchmark, but carrying the tiny patch required adding the complete 24,000-line published crate plus a one-off update guide to RayX. The repository cost and maintenance policy were disproportionate to the patch.

### Alternatives considered: Keep every patched crate fully vendored; always use crates.io and forgo unreleased fixes; allow exact-revision Git forks for essential small patches; choose the source mechanism by concrete offline, stability, and patch-depth requirements.

### Reasoning: Normal crates.io releases minimize source and maintenance surface. A maintained Git fork pinned to an immutable commit keeps a tiny essential patch reviewable without copying an entire crate, while its lock entry supplies provenance. Full vendoring remains appropriate for offline builds, unstable upstreams, or deep modifications such as the existing wgpu integration. Dedicated skill references should exist only for complex or recurring update/replay procedures.

### Trade-offs accepted: Removing the Taffy vendor initially removed the proven inline-child optimization: registry 0.12.2 measured 16.60% and 17.51% lower median acknowledged throughput at DPR 1 and 2 versus the inline patch. The subsequent registry 0.14.0 upgrade recovered that cost and slightly beat the patch on all four medians, so a fork is not currently justified. Git forks would make fresh builds network-dependent and transfer availability responsibility to the fork owner; crates.io releases may still lag a future essential fix.

### Supersedes: [Vendored Taffy with two inline child slots](../../gpui/layout-style/2026-08-30-gpui-layout-style-taffy-inline-children.md)
