---
agentx:
  kind: decision
  scope: decisions/renderer/wgpu
  status: superseded
  created: 2026-08-07T15:14:15+02:00
  updated: 2026-08-07T19:41:00+02:00
  topics:
    - gpux
    - wasm
    - webgpu
    - webgl
    - renderer-abi
  supersedes: []
  superseded_by:
    - 2026-08-07-renderer-wgpu-browser-backend-fallback.md
---

## Decision: Keep RayX browser rendering WebGPU-only

### Context: Zed added a WebGL renderer transport while RayX's gpux fork has a larger packed scene ABI for effects, HTML islands, and other renderer extensions.

### Alternatives considered: Import Zed's WebGL path as-is; maintain parallel WebGPU and WebGL transports immediately; keep the proven WebGPU path exclusive until complete parity exists.

### Reasoning: Advertising the upstream WebGL fallback without transporting every RayX scene field would create a backend that compiles but renders incomplete or misaligned data. The WebGPU path preserves the full RayX ABI and is exercised by headed Test Suite validation.

### Trade-offs accepted: Browsers without usable WebGPU cannot run RayX. A future WebGL backend must deliberately map the complete RayX renderer ABI and pass the same runtime coverage before it can be enabled.

### Supersedes: None.
