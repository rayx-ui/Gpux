---
agentx:
  kind: rules
  scope: knowledge/repository/validation
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - validation
    - test-suite
    - runtime-proof
  supersedes: []
  superseded_by: []
---
# Validation Rules

- Test only what changed: the touched package's focused tests plus the Test Suite specs that drive the touched behavior. Run a whole suite only when the change affects the whole codebase, and say why first.
- Start agent-driven WASM proof through `cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu --playwright-test test_suite/<focused-spec>.spec.ts` from `../RayX`; never automate Lab.
- Verify threaded WASM with both the generated `*_bg.wasm` binary (shared memory, `wasm_thread_entry_point`) and headed runtime counters.
- Prove WGSL, packed-record, and backend-selection changes in headed WebGPU and, for transport changes, forced WebGL2.
- Do not close iOS or Metal runtime work from Windows checks.
- Keep raw logs, screenshots, traces, and profiles in ignored `artifacts-temp/`, never in `.agentx/` or commits.
