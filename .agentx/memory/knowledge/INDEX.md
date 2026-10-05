---
agentx:
  kind: index
  scope: knowledge
  status: current
  created: 2026-10-05T23:00:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - gpux
    - taxonomy
  supersedes: []
  superseded_by: []
---
# Gpux Knowledge Index

Current source, tests, manifests, and `docs/` establish behavior. This tree records durable routing, rules, and rationale for gpux, the RayX-maintained fork of Zed's GPUI crate closure. Decisions live in the [decision index](../decisions/INDEX.md).

## Domains

- [gpui](gpui/INDEX.md): the `gpux` framework crate: layout and style, frame pipeline, positioning and scrolling, input, text, frame effects.
- [renderer](renderer/INDEX.md): the shared `gpui_wgpu` renderer and the `gpux_color` paint contract.
- [platform](platform/INDEX.md): browser, desktop, and mobile platform crates.
- [repository](repository/INDEX.md): upstream imports, crates.io packaging and the wgpu fork, validation, and AgentX governance.

## Source Anchors

- [Workspace manifest](../../../Cargo.toml)
- [README crate table](../../../README.md#crates)
- [Crates](../../../crates)
- [Upstream provenance](../../../docs/history/upstream-provenance.md)
- [gpux skill](../../../.claude/skills/gpux/SKILL.md)
