---
agentx:
  kind: decision
  scope: decisions/gpui/frame-pipeline
  status: current
  created: 2026-07-24T01:35:00+02:00
  updated: 2026-07-24T01:35:00+02:00
  topics:
    - gpux
    - wasm
    - frame-scheduling
    - thermal-state
  supersedes: []
  superseded_by: []
---

## Decision: Cache thermal state for frame throttling

### Context: The WASM request-animation-frame callback re-entered `AnyWindowHandle::update` only to read `App::thermal_state()`. When a frame was requested while startup already held the app `RefCell`, browser console logging reported `RefCell already borrowed` every frame.

### Alternatives considered: Keep the re-entrant lookup and suppress its error; disable thermal throttling on WASM; add a browser-only special case; or keep a window-owned snapshot updated by the existing thermal observer mechanism.

### Reasoning: Thermal state is infrequently updated and naturally observable. A window-owned `Rc<Cell<ThermalState>>` plus retained `App::on_thermal_state_change` subscription makes frame throttling read-only and avoids re-entering the app from a platform frame callback on every target.

### Trade-offs accepted: The cached value changes through the platform notification rather than a synchronous per-frame query, which is appropriate because throttling is advisory and thermal transitions are not frame-rate events.

### Supersedes: None.
