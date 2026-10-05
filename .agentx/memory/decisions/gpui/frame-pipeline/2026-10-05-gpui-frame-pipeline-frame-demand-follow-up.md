---
agentx:
  kind: decision
  scope: decisions/gpui/frame-pipeline
  status: current
  created: 2026-10-05T23:30:00+02:00
  updated: 2026-10-05T23:30:00+02:00
  topics:
    - gpux
    - frame-scheduling
    - thermal-state
    - wasm
  supersedes: []
  superseded_by: []
---

# Frame-Demand Follow-Up Without App Re-Entry

## Decision: Keep the cached thermal state and request upstream's follow-up frame from frame demand

### Context: Zed's frame callback reads the thermal state through an App update whose effect flush schedules a follow-up frame while the window has frame demand (dirty, awaiting presentation, or pending next-frame callbacks), and it added an unconditional trailing App update. gpux reads the [cached thermal state](2026-07-24-gpui-frame-pipeline-cached-thermal-frame-throttling.md) (2026-07-24, current), so it lost that follow-up, and gpux's continuous browser animation-frame loop would enter App twice on every idle frame.

### Alternatives considered: Restore Zed's per-frame thermal App update; keep the cache and take Zed's trailing update unconditionally; keep the cache and change the upstream test's expectation.

### Reasoning: The frame callback records whether the window had frame demand at frame start and enters App for the trailing schedule only when that demand existed or remains. This reproduces Zed's scheduling, including one follow-up frame after a frame with work, while idle frames never enter App, which the browser loop and the active-borrow startup path require.

### Trade-offs accepted: The frame callback keeps a small gpux delta beside Zed's code in `Window::new`; merges must preserve the frame-start demand check and the guarded trailing schedule.

### Supersedes: None.
