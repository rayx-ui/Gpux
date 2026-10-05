---
agentx:
  kind: state
  scope: knowledge/platform
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - platform
    - gpui-platform
    - accessibility
  supersedes: []
  superseded_by: []
---
# Platform State

## Purpose And Usage

Read before changing platform selection or behavior shared across platform crates.

## Current Technical Shape

`gpui_platform` selects the platform for the current target: `application()` maps WASM to the multithreaded `WebPlatform::new(true)`, and `web_init` performs one-time browser initialization. Every native desktop platform builds GPUI's frame-built AccessKit tree end to end; WASM accessibility is unsupported (`gpui_web` builds no AccessKit adapter or DOM mirror). Anchored popups (`WindowKind::AnchoredPopup`) exist only on Wayland; other backends return `PopupNotSupportedError`.

## Backlinks

- [Platform selection](../../../../crates/gpui_platform/src/gpui_platform.rs)
- [Platform traits](../../../../crates/gpui/src/platform.rs)
- [Popup contract](../../../../crates/gpui/src/platform/popup.rs)
