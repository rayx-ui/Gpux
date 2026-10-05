---
agentx:
  kind: decision
  scope: decisions/repository/packaging
  status: current
  created: 2026-10-05T12:00:00+02:00
  updated: 2026-10-05T12:00:00+02:00
  topics:
    - gpux
    - ztracing
    - zlog
    - licensing
  supersedes: []
  superseded_by: []
---

## Decision: gpux does not carry `zlog`, `ztracing` or `ztracing_macro`

### Context: The copies were imported while Zed licensed them GPL-3.0-or-later (Zed relicensed them Apache-2.0 on 2026-09-01). RayX never builds with `--cfg ztracing`, so their instrumentation expanded to no-ops, and they blocked publishing gpux.

### Alternatives considered: Re-import the Apache-2.0 versions; replace the call sites with `tracing`; remove the crates and their no-op call sites.

### Reasoning: Removal keeps behavior identical (gpux workspace checks and `sum_tree` tests pass) and leaves three fewer crates to publish. The gpux skill's Zed-import guide strips `ztracing`/`zlog` usages on import; the `profiling` crate covers instrumentation.

### Trade-offs accepted: Opt-in Tracy profiling through `--cfg ztracing` is gone. Future Zed imports need the strip step.

### Supersedes: None.
