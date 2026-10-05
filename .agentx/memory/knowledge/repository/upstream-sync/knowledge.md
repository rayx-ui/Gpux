---
agentx:
  kind: knowledge
  scope: knowledge/repository/upstream-sync
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - upstream-sync
    - provenance
  supersedes: []
  superseded_by: []
---
# Upstream Sync Knowledge

- Zed upstream is `https://github.com/zed-industries/zed` and gpui-mobile is `https://github.com/itsbalamurali/gpui-mobile`; both are fetched into a temporary or cache location, never kept as local fork checkouts.
- Gpux patches are not limited to `crates/gpui`: platform and renderer sibling crates and shader files carry local behavior that a closure import can silently revert.
- The archive branch and tag are separate refs and must be pushed explicitly so fresh clones receive the forensic graph.
