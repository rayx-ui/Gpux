---
agentx:
  kind: index
  scope: knowledge/repository
  status: current
  created: 2026-10-05T23:00:00+02:00
  updated: 2026-10-07T23:55:00+02:00
  topics:
    - repository
    - upstream-sync
    - packaging
    - validation
    - agentx-memory
  supersedes: []
  superseded_by: []
---
# Repository Domain Index

How gpux is maintained as a repository: upstream imports, publishing, validation routing, and AgentX memory governance.

- [state](state.md) · [knowledge](knowledge.md) · [rules](rules.md) · [hypotheses](hypotheses.md)

## Subdomains

- [upstream-sync](upstream-sync/INDEX.md): Zed GPUI closure, Zed PR, gpui-mobile, and wgpu imports with provenance.
- [packaging](packaging/INDEX.md): `gpux-*` crates.io packaging, dependency policy, and the `gpux-wgpu` fork.
- [validation](validation/INDEX.md): focused crate checks and runtime proof in gpux's own lanes, run through `gpux-testkit`.
- [agentx-memory](agentx-memory/INDEX.md): this memory tree's taxonomy and its transplant from RayX.
