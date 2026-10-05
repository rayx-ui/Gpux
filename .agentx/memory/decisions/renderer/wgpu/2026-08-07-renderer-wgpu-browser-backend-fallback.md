---
agentx:
  kind: decision
  scope: decisions/renderer/wgpu
  status: current
  created: 2026-08-07T19:41:00+02:00
  updated: 2026-08-07T19:41:00+02:00
  topics:
    - gpux
    - wasm
    - webgpu
    - webgl
    - renderer-abi
  supersedes:
    - 2026-08-07-renderer-wgpu-webgpu-only-browser-renderer.md
  superseded_by: []
---

## Decision: Prefer WebGPU and fall back to a full-ABI WebGL2 transport

### Context: RayX must run on browsers without usable WebGPU while retaining renderer extensions beyond upstream Zed's packed scene ABI.

### Alternatives considered: Remain WebGPU-only; import Zed's narrower WebGL transport; or implement WebGL2 transport parity for every RayX record family and make it the fallback.

### Reasoning: Exact word-addressable `Rgba32Uint` textures preserve every 32-bit record field without WebGL storage buffers. Surface-bound adapter selection, replacement-canvas cleanup, and paired headed Test Suite coverage make fallback behavior explicit while WebGPU remains preferred.

### Trade-offs accepted: The renderer maintains two binding transports and WebGL2 cannot use dual-source subpixel blending. Every future packed-record or shader binding change must update both transports and their ABI assertions.

### Supersedes: [WebGPU-only browser renderer](2026-08-07-renderer-wgpu-webgpu-only-browser-renderer.md).
