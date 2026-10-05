---
agentx:
  kind: rules
  scope: knowledge/repository/agentx-memory
  status: current
  created: 2026-10-05T23:00:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - agentx-memory
    - epics
    - experiments
  supersedes: []
  superseded_by: []
---
# AgentX Memory Rules

- Keep `.agentx/` metadata-only; raw diagnostics go to ignored `artifacts-temp/agentx-tasks/` or `artifacts-temp/agentx-experiments/`.
- Treat transplanted epics and loops as read-only history. Resume an unfinished one only through an explicit AgentX `migrate` or a fresh plan that names Gpux as the primary repository and RayX as the runtime-proof repository.
- Place new memory by crate ownership: `crates/gpui` → `gpui`, `gpui_wgpu`/`gpux_color` → `renderer`, platform crates → `platform`, repository workflows → `repository`. Add a domain or subdomain only with a decision.
- Keep RayX's `runtime/gpux-foundation` limited to RayX's consumer view; record gpux behavior here and link across instead of duplicating.
