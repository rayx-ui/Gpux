---
agentx:
  kind: decision
  scope: decisions/repository/validation
  status: current
  created: 2026-10-14T12:00:00+02:00
  updated: 2026-10-14T12:00:00+02:00
  topics:
    - gpux
    - wsl
    - lane-c
    - lane-d
    - vulkan
  supersedes: []
  superseded_by: []
---

# Under WSL only lanes C and D use the GPU, through a locally built Mesa dzn, and prove no timing

## Decision: WSL runs lanes C and D on the GPU through a Mesa `dzn` built locally; it is never timing proof

### Context: WSL 2 exposes the GPU to Linux only as D3D12 and CUDA. Ubuntu 24.04's `mesa-vulkan-drivers` ships no `dzn`, and Chromium on WSL gets only software adapters.

### Alternatives considered: Treating WSL as a Linux GPU for lane G; skipping the Linux GPU entirely.

### Reasoning: Mesa built from source with `-Dvulkan-drivers=microsoft-experimental` (selected with `VK_DRIVER_FILES`) plus [`WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER`](../../renderer/wgpu/2026-10-14-renderer-wgpu-noncompliant-adapter-variable.md) puts the GPU in front of lane C (188 of 188) and lane D (43 of 43, 13 skipped), which proves pixels and windows on Vulkan. The adapter is Vulkan translated to D3D12 on a non-conformant driver, not a native Linux driver, so its results are not timing. Lane B and the browser timings cannot run there; the setup is in `LINUX-WSL.md`.

### Trade-offs accepted: No browser timing and no `fast_mode` table exist on a Linux GPU.

### Supersedes: None.
