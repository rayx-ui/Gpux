---
agentx:
  kind: state
  scope: knowledge/repository/validation
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - validation
    - focused-checks
    - test-suite
    - runtime-proof
    - wasm
    - performance-method
  supersedes: []
  superseded_by: []
---
# Validation State

## Purpose And Usage

Read before choosing how to prove a gpux change. Command details live in [AGENTS.md](../../../../../AGENTS.md#validation) and the gpux skill's validation section.

## Current Technical Shape

Iteration runs the touched package's focused checks from the Gpux root: `cargo fmt --check`, `cargo check -p <package> --all-targets`, `cargo test -p <package> <filter>`, and warning-denying `cargo clippy -p <package> --all-targets --no-deps`. Web crates also run `cargo check -p gpux-web --target wasm32-unknown-unknown --no-default-features` (the default multithreaded build needs nightly `-Z build-std`, which RayX's xtask provides); Android uses `cargo ndk -t arm64-v8a check -p gpux-mobile`. Manifest changes run `cargo package --workspace`.

Runtime proof lives in RayX (`../RayX`): its Test Suite app drives gpux in headed WASM/WebGPU, for example `cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu --no-default-features --features rayx_diagnostics --playwright-test test_suite/<spec>`, natively on Windows DX12 (`app apps/test_suite run windows`), and on Android (`app apps/test_suite build android --debug`). RayX forbids Lab as a test or diagnostics target. Rendering, input, text, platform, or wgpu changes need that proof. Headed pages verify WebGPU startup, DPR 1 and 2 output, published-frame timing, owner-work counters, and physical-pixel action changes without retained trace hooks.

Performance work uses the warmed wide-DataTable fixture with three headed processes per DPR and accepts a change only when output and every established telemetry counter stay exact; transient evidence goes to ignored `artifacts-temp/`.

## Backlinks

- [Repository instructions](../../../../../AGENTS.md)
- [gpux skill](../../../../../.claude/skills/gpux/SKILL.md)
- 2026-10-05 current [RayX validation memory](../../../../../../RayX/.agentx/memory/knowledge/tooling/validation/state.md)
