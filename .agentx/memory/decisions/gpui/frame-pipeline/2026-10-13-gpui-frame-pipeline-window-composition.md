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
    - composition
    - native-views
  supersedes: []
  superseded_by: []
---

# Window composition is an opt-in surface tree with a base and an overlay

## Decision: Window composition is an opt-in surface tree with a base and an overlay

### Context: Native views (web views, video, camera) must sit between GPUI's base content and GPUI's overlays (deferred draws, prompts, tooltips, top-layer scrollbars) on every platform, as open Zed PR #62379 proposes.

### Alternatives considered: Split by paint phase only; keep native views above everything; per-platform APIs.

### Reasoning: `Window::enable_window_composition` returns a controller of a surface tree with a base and an overlay GPUI surface; the overlay takes the top-layer band and the overlay range of deferred draws, prompts, drag, tooltips and the inspector; `native_surface` elements place native surfaces and mark their scroll content non-layerable. With composition disabled the scene, draw call and allocations are unchanged. Backends: macOS NSView overlay, Wayland subsurfaces, X11 child windows with shape cut-outs, Windows DirectComposition above the HWND swap chain (keeping `DXGI_SCALING_NONE`), the browser's overlay canvas on WebGPU (WebGL2 reports unsupported), iOS UIView containers above `GPUIMetalView`, Android attached panel windows with a translucent overlay (GL draws the whole scene in its own window and cuts the overlay out of each container).

### Trade-offs accepted: Android's embedded host is type-checked, not run; Windows evidence is lane D in CI; the choice of the Windows base placement and the Android window mechanism were taken by spike (`docs/upstream/gpui-fast.md`).

### Supersedes: None.
