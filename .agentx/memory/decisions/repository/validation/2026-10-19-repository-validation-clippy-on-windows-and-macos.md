---
agentx:
  kind: decision
  scope: decisions/repository/validation
  status: current
  created: 2026-10-19T12:58:00+02:00
  updated: 2026-10-19T12:58:00+02:00
  promoted_from: metal-paint-parity
  topics:
    - validation
    - ci
    - clippy
    - macos
  supersedes: []
  superseded_by: []
---

## Decision: CI lints with Clippy on Windows and macOS, and builds `runtime_shaders` on macOS

### Context: `ci.yml`'s `check` job ran Clippy on Windows only, so Apple-only code (`gpux-apple`, `gpux-macos`, `gpux-zed-util`'s darwin command) was never linted: the Metal port needed a `clone_on_copy` fix found by hand, and a darwin test helper failed macOS Clippy unnoticed. The `runtime_shaders` feature of `gpux-apple` was never built.

### Alternatives considered: Clippy on every OS, Linux included; a separate macOS lint job; building `runtime_shaders` inside the workspace check.

### Reasoning: The `check` job's Clippy step runs where `runner.os != 'Linux'`, with the same command (`--workspace --all-targets --no-deps`, the fork excludes, `-D warnings`), reusing the macOS job's build cache. Linux stays unlinted (owner, 2026-10-19). A macOS-only step runs `cargo check -p gpux-apple --features runtime_shaders` on its own, after the workspace check, because feature unification would otherwise switch the precompiled metallib off for the whole workspace check. No job, input or trigger was added.

### Trade-offs accepted: Linux-only code (`gpui_linux`) is linted only where a developer runs Clippy locally; a macOS lint finding now fails `check (macos-15)`, so Apple-only allows need a local reason comment, never a crate- or module-level allow.

### Supersedes: None.
