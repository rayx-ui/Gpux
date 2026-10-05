---
agentx:
  kind: decision
  scope: decisions/renderer/wgpu
  status: current
  created: 2026-07-04T00:00:00+02:00
  updated: 2026-07-04T00:00:00+02:00
  topics:
    - runtime
    - gpux-foundation
    - shader-effects
    - renderer-capabilities
  supersedes: []
  superseded_by: []
---

## Decision: WGPU owns first shader-effect support and non-WGPU backends stay capability-false

### Context: The shader-effects-and-screen-transitions epic added gpux element effects, screen ripples, and snapshot transitions. WGPU covers WASM and WGPU-backed mobile, while DirectX and Metal do not yet have native shader pipelines for these effects.

### Alternatives considered: Implement WGPU first and leave unsupported backends capability-false; add visual approximations in DirectX/Metal; claim native support while silently skipping shader batches.

### Reasoning: The requirements explicitly reject fake native backend parity. WGPU gives validated browser/mobile shader behavior and can share the renderer-managed snapshot architecture. DirectX/Metal should not report support until they have real shader resources, bind layouts, and runtime validation for the same effect contracts.

### Trade-offs accepted: Native non-WGPU desktop backends can run apps without crashing but do not render these shader effects yet. Future native parity work must implement real backend-specific shaders and validation instead of toggling capability flags.

### Supersedes: none
