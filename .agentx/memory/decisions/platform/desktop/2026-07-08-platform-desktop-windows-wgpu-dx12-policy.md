---
agentx:
  kind: decision
  scope: decisions/platform/desktop
  status: current
  created: 2026-07-08T02:42:00+02:00
  updated: 2026-07-08T08:20:00+02:00
  topics:
    - gpux
    - wgpu
    - windows
    - dx12
  supersedes: []
  superseded_by: []
---

## Decision: Use WGPU DX12 as the Windows native backend policy

### Context: The Windows platform renderer is being migrated from the legacy `DirectXRenderer` to the shared `WgpuRenderer`. The initial epic plan requested a native WGPU path but had inherited the earlier native helper policy of Vulkan plus GL. During runtime validation, the user clarified that Windows should use the latest native Windows graphics API rather than Vulkan or GL.

### Alternatives considered: Keep the WGPU native helper on Vulkan plus GL for Windows; restore or preserve the legacy `DirectXRenderer`; request WGPU's DX12 backend on Windows while keeping `WgpuRenderer` as the common renderer abstraction.

### Reasoning: WGPU DX12 provides the native Windows DirectX backend without keeping the obsolete Windows-only renderer path. It aligns Windows with the platform-native graphics stack, avoids depending on Vulkan or OpenGL driver availability on Windows, and keeps the epic's cross-platform renderer architecture centered on `WgpuRenderer`.

### Trade-offs accepted: Windows validation must prove the DX12 WGPU path specifically, and any future backend-specific issue must be diagnosed through WGPU/DX12 rather than the removed legacy renderer. Other native targets keep their existing backend policy until they receive their own platform-specific renderer work.

### Supersedes: The initial `gpux-wgpu-platform-renderers` epic design assumption that Windows WGPU should use the generic Vulkan plus GL native helper policy.
