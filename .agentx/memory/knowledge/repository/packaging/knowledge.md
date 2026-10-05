---
agentx:
  kind: knowledge
  scope: knowledge/repository/packaging
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - crates-io
    - packaging
    - calloop
  supersedes: []
  superseded_by: []
---
# Packaging Knowledge

- The crates.io calloop release lacks Zed's fork change that bounds idle callbacks to about 10 ms per loop iteration; `gpui_linux` reproduces it with its own `ForegroundTasks` queue instead of a calloop fork.
- Because the cycle-edge dev-dependencies are path-only, the published crates' own tests do not build from the registry tarball.
- Publishing to crates.io and the GitHub repository setup remain manual steps.
