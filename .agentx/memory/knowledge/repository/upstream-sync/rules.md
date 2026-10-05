---
agentx:
  kind: rules
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
# Upstream Sync Rules

- Follow the matching gpux skill reference before any Zed, gpui-mobile, or wgpu update; treat its patch-surface checklist as the audit list.
- Resolve the prior Zed base and pre-migration patches from `docs/history/` and the archive tag; never require a predecessor RayX checkout.
- Reconcile Zed closure updates semantically from the last represented commit and audit upstream and gpux-local deltas independently before accepting either side.
- Import only GPUI and GPUI-dependent crates, never the Zed application tree, and audit platform/renderer siblings and shader files.
- After every validated import, update `zed-imports.tsv`, the represented commit in `upstream-provenance.md`, and this memory in the same change, and record import and replay provenance in commit bodies.
- Re-apply the publishing manifest shape after imports (see [packaging rules](../packaging/rules.md)).
