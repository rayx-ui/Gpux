---
agentx:
  kind: knowledge
  scope: knowledge/platform/mobile
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - android
    - ios
    - runtime-proof
  supersedes: []
  superseded_by: []
---
# Mobile Knowledge

- Android shader-effect runtime proof (DevKit plus app-scoped logcat on an emulator with `backend=gl`) showed all WGPU effect capability flags true, `visual_effect_count=2`, `screen_effect_region_count=1`, no missing regions or snapshots, and no renderer/shader/pipeline/surface/validation/panic log lines.
- Foreground callbacks may query or mutate `AndroidPlatform` state, so draining them under the state lock deadlocks real startup.
- Checking iOS from Windows (`cargo check --target aarch64-apple-ios`) needs local Clang, `SDKROOT`, and `llvm-ar` overrides and proves only compilation, never simulator or device rendering.
