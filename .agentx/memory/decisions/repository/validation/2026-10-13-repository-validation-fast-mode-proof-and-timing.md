---
agentx:
  kind: decision
  scope: decisions/repository/validation
  status: current
  created: 2026-10-13T10:00:00+02:00
  updated: 2026-10-13T10:00:00+02:00
  topics:
    - gpux
    - fast-mode
    - testing
    - performance
  supersedes: []
  superseded_by: []
---

# Fast mode is proved in its own lanes; timings are lane G's and macOS-only

## Decision: Fast mode is proved in its own lanes; timings are lane G's and macOS-only

### Context: Fast mode changes how every frame is drawn, so its proof must cover behaviour, pixels and each renderer, while timings on shared or software runners mean nothing.

### Alternatives considered: Assert timings in CI; run the benchmarks everywhere.

### Reasoning: Correctness is lane A (oracle, counters, parity), lane C (pixel parity and tile groups), lane B (`scroll-layers` specs on WebGPU and WebGL2), lane D (composition and windows), lanes E and F (devices, resume, memory release). Timing is lane G only (`gpux-testkit perf fast-mode`, the `@timing` specs, the `scroll-layers-*-timing` host checks); CI asserts deterministic counters instead. The `fast_mode` suite presents to the headless Metal renderer and runs on macOS only; the final comparison is `docs/fast-mode.md`.

### Trade-offs accepted: No Linux GPU timing exists (Windows DX12 was timed later: [PERF-4 wgpu-desktop](2026-10-14-repository-validation-wgpu-desktop-perf-4.md)); mobile figures come from an emulator and a Simulator. Lane G on macOS needs the display awake.

### Supersedes: None.
