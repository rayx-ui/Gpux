---
agentx:
  kind: decision
  scope: decisions/repository/validation
  status: current
  created: 2026-10-14T12:00:00+02:00
  updated: 2026-10-14T12:00:00+02:00
  topics:
    - gpux
    - fast-mode
    - performance
    - windows
    - wgpu
  supersedes: []
  superseded_by: []
---

# PERF-4's wgpu-desktop half is closed with stated limits

## Decision: The wgpu-desktop timing run is Windows DX12 plus WSL, and its two gaps stay open and recorded

### Context: PERF-4 asked for lane G on a wgpu desktop. The user ran it on Windows DirectX 12 (RTX 4080 SUPER) and under WSL (`docs/measurements/wgpu-desktop-timings.md`, summarized in `docs/fast-mode.md`). It builds on [fast mode's proof and timing decision](2026-10-13-repository-validation-fast-mode-proof-and-timing.md), whose "no Windows or Linux GPU timing exists" it narrows.

### Alternatives considered: Weakening the `fast_mode` glyph check; waiting for a Linux GPU machine.

### Reasoning: Windows lane G: 418 passed, 1 failed, 10 skipped; the failure is the `fast_mode` bench's own check (the harness draws no glyphs), never weakened. Browser p95 on Windows: the scrolling `div` wins 25% to 39% with layers, the list is +0%, -5%, +3% on WebGPU and +8%, +8%, +4% on WebGL2. The run is quoted only from the measurements document, next to the Mac numbers.

### Trade-offs accepted: Open and recorded, not blockers: browser timings on a Linux GPU (impossible on WSL), and the `fast_mode` per-mode comparison off macOS until the bench harness draws text on Windows and Linux (a follow-up, outside the fast-mode epic).

### Supersedes: None.
