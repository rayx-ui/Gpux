---
agentx:
  kind: knowledge
  scope: knowledge/repository/agentx-memory
  status: current
  created: 2026-10-05T23:00:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - agentx-memory
    - transplant
    - commit-mapping
  supersedes: []
  superseded_by: []
---
# AgentX Memory Knowledge

- Transplanted epic and experiment JSON is unmodified: commit hashes are RayX commits, `repositories` entries named `main` with path `.` meant the RayX root, and source paths under `crates/gpux/` correspond to the same paths at the Gpux root (`crates/gpux/crates/gpui/src/window.rs` is `crates/gpui/src/window.rs`).
- `git filter-repo` rewrote every hash during extraction and no RayX-to-Gpux commit map was kept for that step. Filtered commits keep author date and message, so find the Gpux equivalent of RayX commit `X` by matching `git -C ../RayX log -1 --format='%ad %s' X` against `git log --format='%h %ad %s'` here; RayX `9aff311f` is Gpux `e2f8568`.
- The old-to-new decision filename map is in the [transplant decision](../../../decisions/repository/agentx-memory/2026-10-05-repository-agentx-memory-taxonomy-transplant.md); old names followed `YYYY-MM-DD-runtime-gpux-foundation-<topic>.md` and kept their topic slug.
