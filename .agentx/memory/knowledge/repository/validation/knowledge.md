---
agentx:
  kind: knowledge
  scope: knowledge/repository/validation
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-07T23:55:00+02:00
  topics:
    - validation
    - shader-validation
    - ios
    - performance-method
  supersedes: []
  superseded_by: []
---
# Validation Knowledge

- Compile checks do not exercise Dawn/WebGPU layout validation; WGSL and packed-record changes need a lane B run (`gpux-testkit lane b`), where Dawn validates them.
- Windows can only compile-check iOS; simulator or device rendering needs macOS with Xcode, XcodeGen, and xcrun.
- Process-to-process variance on the reference workload is a few percent, so single-run deltas below that are noise; telemetry counters, not timings, establish behavioral equivalence.
