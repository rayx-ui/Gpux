---
agentx:
  kind: rules
  scope: knowledge/repository
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-16T09:00:00+02:00
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
- Benchmark CPU per frame (`main_cpu_ms`) drifts with the CPU's frequency and core choice (a 1.8 ms scenario can read 2.7 ms in the same build), so judge a small effect by alternating one binary with an environment switch, or by allocations and `sample` profiles, not by two builds' medians; `effects-cards` fast-on is bimodal.
