---
agentx:
  kind: state
  scope: knowledge/repository
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - repository
    - sibling-repositories
    - rayx-consumer
  supersedes: []
  superseded_by: []
---
# Repository State

## Purpose And Usage

Read before repository-level work: imports, manifests, publishing, validation routing, or memory maintenance.

## Current Technical Shape

gpux lives in its own repository, checked out beside RayX (`../RayX`). Its history was extracted from RayX `crates/gpux` on 2026-10-05; Zed's crate boundaries and directory names are kept so upstream imports stay reviewable. RayX consumes the crates by relative path (`../Gpux/crates/...`) for co-development, so an API change RayX uses needs a paired RayX commit, and RayX hosts all runtime proof. Repository instructions live in [AGENTS.md](../../../../AGENTS.md) and [CLAUDE.md](../../../../CLAUDE.md); recurring workflows live in the [gpux skill](../../../../.claude/skills/gpux/SKILL.md).

## Backlinks

- [Workspace manifest](../../../../Cargo.toml)
- [README](../../../../README.md)
- [Installation and usage](../../../../docs/installation-and-usage.md)
