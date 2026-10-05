---
agentx:
  kind: decision
  scope: decisions/platform/web
  status: current
  created: 2026-06-24T19:28:00+02:00
  updated: 2026-06-24T19:28:00+02:00
  topics:
    - gpux
    - wasm
    - multithreading
  supersedes: []
  superseded_by: []
---

## Decision: RayX gpux browser apps are multithreaded by default

### Context: why this came up

The Rust-only browser path no longer runs through the old managed host, and RayX needs serious WASM multithreading for gpux browser apps.

### Alternatives considered: what else was on the table

Keep `gpui_web` single-threaded with `default-features = false`; keep a shared-memory binary but call `single_threaded_web()`; or make `gpui_web` multithreaded by default and verify both the binary and runtime path.

### Reasoning: why this option won

`gpui_web` already exposes `default = ["multithreaded"]`, and `gpui_platform::application()` maps WASM to `WebPlatform::new(true)`. The RayX WASM build can satisfy browser threading requirements with nightly `build-std`, atomics/shared-memory linker flags, `wasm-bindgen 0.2.125`, COOP/COEP/CORP serving headers, and headed Playwright counters.

### Trade-offs accepted: what you gave up

RayX browser builds use nightly Rust for the WASM target and must preserve cross-origin isolation headers. The validation harness must inspect binary/shared-memory evidence and runtime worker evidence, not screenshots alone.

### Supersedes: link to prior decision, if replacing

None.
