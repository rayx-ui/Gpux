---
agentx:
  kind: decision
  scope: decisions/gpui/frame-pipeline
  status: current
  created: 2026-10-13T10:00:00+02:00
  updated: 2026-10-13T10:00:00+02:00
  topics:
    - gpux
    - fast-mode
    - gpui-fast
  supersedes: []
  superseded_by: []
---

# Fast mode: per-window runtime switches, on by default, off is stock gpux

## Decision: Fast mode: per-window runtime switches, on by default, off is stock gpux

### Context: Fast mode brings gpui-fast's layout retention, view retention, splice, scroll layers and window composition into gpux, which had to run them where gpui-fast does and still draw exactly as before when they are off; RayX consumes gpux by path and would otherwise run each mechanism before its proof was complete.

### Alternatives considered: Cargo features per mechanism; process-wide switches; one per-window value read at window creation.

### Reasoning: One `FastMode` per window (`Window::fast_mode`/`set_fast_mode`), set from compiled defaults and `GPUX_FAST_MODE`, `GPUX_LAYOUT_RETENTION`, `GPUX_VIEW_RETENTION`, `GPUX_SCROLL_LAYERS` (gpui-fast's `GPUI_*` names as aliases), normalized so nothing is on without what it builds on and scroll layers only where a renderer rasterizes tiles. Mechanisms landed dark and the defaults went on in the epic's last sprint, once every mechanism was held equal to a frame drawn from scratch; composition stays opt-in per window. Behaviour gpui-fast applies unconditionally (snapping of scroll offsets, glyph origins and paths) is tied to the scroll-layers switch, so `GPUX_FAST_MODE=0` restores today's output. Every mechanism lives in a `fast/` module named after the gpui-fast file it adapts, and code outside `fast/` only calls in.

### Trade-offs accepted: Turning the defaults on found four bugs the opt-in tests had not (splice carrying removed element state, paint-only ancestor contexts, scrollbar-drag activity, text-policy changes) and costs scenes where every view changes every frame 11–15% (`all-panels`); such windows opt out. Tests of stock behaviour must open their window off.

### Supersedes: None.
