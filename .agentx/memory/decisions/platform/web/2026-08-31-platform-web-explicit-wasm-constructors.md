---
agentx:
  kind: decision
  scope: decisions/platform/web
  status: current
  created: 2026-08-31T10:05:00+02:00
  updated: 2026-08-31T10:05:00+02:00
  topics:
    - gpux
    - wasm
    - startup
    - performance
  supersedes: []
  superseded_by: []
---

## Decision: Initialize WASM constructors explicitly in gpui_platform

### Context: The threaded Test Suite binary was classified as command-style because no exported call path invoked `__wasm_call_ctors`. wasm-ld consequently inserted the complete constructor traversal before every export, and a warmed profile attributed 506.85/464.90 ms of DPR 1/2 self time to idempotent inventory registration.

### Alternatives considered: Keep linker-injected wrappers; change or remove inventory-backed actions; patch wasm-bindgen or wasm-thread; call the synthesized constructor routine from gpui_platform's existing browser initialization boundary.

### Reasoning: `gpui_platform::web_init` is already the required once-before-use browser boundary. Calling `__wasm_call_ctors` there gives wasm-ld a reactor-style reachable call without adding a new host protocol. Final disassembly contains exactly one call there and no command wrappers on application, worker, or allocator exports. Fresh profiles contain zero inventory/constructor samples, while three-process throughput improved 20.73%/20.61% and paced p95 improved 11.77%/11.29% at DPR 1/2 with exact runtime invariants.

### Trade-offs accepted: The call is a small target-gated unsafe FFI boundary to a linker-synthesized symbol, and browser consumers must honor the documented `web_init`-once contract. Final-binary validation must catch any future toolchain change that restores command wrappers or removes the synthesized symbol.

### Supersedes: None.
