---
agentx:
  kind: rules
  scope: knowledge/repository
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - repository
    - commits
  supersedes: []
  superseded_by: []
---
# Repository Rules

- Use Conventional Commits with meaningful scopes; upstream imports keep the provenance lines the gpux skill describes.
- Keep a gpux commit that changes an API RayX uses paired with the matching RayX commit.
- Keep Zed's crate boundaries and directory names; do not merge or split upstream crates.
