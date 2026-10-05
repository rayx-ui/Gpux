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
    - resize
    - dxgi
  supersedes: []
  superseded_by: []
---

## Decision: Use non-stretch DXGI scaling for Windows WGPU HWND surfaces

### Context: Fast live resizing on the Windows WGPU/DX12 path can briefly show stale presented content while the window bounds are changing faster than new frames are produced. The legacy Windows DirectX HWND path used `DXGI_SCALING_NONE`, while the vendored WGPU DX12 HWND surface creation used `DXGI_SCALING_STRETCH`.

### Alternatives considered: Keep WGPU's default stretch scaling for HWND swap chains; compensate only in GPUI resize scheduling; patch all DX12 surface targets to non-stretch scaling; patch only native HWND surfaces to non-stretch scaling while preserving stretch behavior for composition targets.

### Reasoning: Native HWND windows should not ask DXGI/DWM to stretch the last swap-chain buffer during live resize. Keeping the patch scoped to `SurfaceTarget::WndHandle(_)` restores the prior HWND behavior without changing composition, SwapChainPanel, or visual-hosted targets that may rely on stretch semantics.

### Trade-offs accepted: The repository now carries a vendored WGPU DX12 surface patch and must preserve it during WGPU upgrades until upstream exposes an equivalent configuration point or changes HWND scaling behavior. Resize validation must include fast height-only native Windows resizing because slower scripted screenshots can miss sub-frame compositor artifacts.

### Supersedes: None.
