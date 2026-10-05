---
agentx:
  kind: state
  scope: knowledge/platform/desktop
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - windows
    - dx12
    - dxgi-scaling
    - subpixel-text
    - macos
    - metal
    - linux
    - wayland
    - x11
    - accesskit
    - popups
    - emoji-fonts
  supersedes: []
  superseded_by: []
---
# Desktop State

## Purpose And Usage

Read before changing a native desktop platform crate, its renderer hookup, AccessKit adapters, popups, or desktop text systems.

## Current Technical Shape

Windows renders through `gpui_wgpu::WgpuRenderer` on DX12 only; the old DirectX renderer and device path is gone. The platform owns a shared `GpuContext`, creates WGPU surfaces from a raw HWND wrapper, and uses `gpui_wgpu::CosmicTextSystem::new("Segoe UI")` for non-headless text. HWND swap chains use `DXGI_SCALING_NONE`, which needs the `gpux-wgpu` fork. `WindowsWindowState` snapshots `supports_dual_source_blending()` when the renderer is created and answers `is_subpixel_rendering_supported` from that snapshot, so an adapter without the feature uses grayscale text. `WM_SIZE` drawable changes are coalesced into scheduled frames.

macOS keeps GPUI's Metal renderer. `quad_fragment` applies the same border edge coverage correction as WGPU through hard-coded `BORDER_GAMMA_RATIOS`/`BORDER_ENHANCED_CONTRAST` (Metal has no gamma uniform); that port has not been compiled or run. Metal skips visual-effect batches and reports effects unsupported.

Linux (`gpui_linux`) renders Wayland and X11 through `gpui_wgpu`. Wayland implements `WindowKind::AnchoredPopup` with explicit parent lookup, xdg/layer-shell parent attachment, positioner anchor/gravity/constraint/offset mapping, optional input-serial grabs, configure/ack, reposition tokens, non-blocking popup children, `popup_done` closure, and protocol-order teardown. The Wayland `client.rs`/`window.rs` and X11 `window.rs` were re-imported from Zed `c24358d` on 2026-10-05 (restoring default-feature builds, X11 `request_attention` urgency, AccessKit focus updates, and window-relative `mouse_position`). Wayland omits Zed's app-initiated external drag source because gpux has no drag-to-platform handoff or `FileDropEvent::Ended`. Without Zed's calloop fork, `gpui_linux` bounds idle work with its own budgeted `ForegroundTasks` queue.

AccessKit: Windows owns an `accesskit_windows::Adapter` and answers `WM_GETOBJECT` without holding a re-entrant borrow while returning results or raising UI Automation events; macOS owns a `SubclassingAdapter` and updates native view focus; Wayland and X11 own `accesskit_unix::Adapter`s, X11 publishes scaled root bounds, and Unix adapters forward deactivation. Adapters forward activation trees and actions back through GPUI's accessibility channels.

Cosmic Text recognizes normalized `Noto Color Emoji`, `Apple Color Emoji`, and `Segoe UI Emoji` family names as color emoji and leaves `Segoe UI Symbol` outside that set.

## Backlinks

- [Windows window](../../../../../crates/gpui_windows/src/window.rs)
- [Windows events](../../../../../crates/gpui_windows/src/events.rs)
- [macOS platform crate](../../../../../crates/gpui_macos)
- [Wayland client](../../../../../crates/gpui_linux/src/linux/wayland/client.rs)
- [Wayland popup](../../../../../crates/gpui_linux/src/linux/wayland/popup.rs)
- [X11 window](../../../../../crates/gpui_linux/src/linux/x11/window.rs)
- [Linux foreground tasks](../../../../../crates/gpui_linux/src/linux/foreground_tasks.rs)
- [Cosmic Text system](../../../../../crates/gpui_wgpu/src/cosmic_text_system.rs)
- 2026-07-08 current [Windows WGPU DX12 policy](../../../decisions/platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dx12-policy.md)
- 2026-07-08 current [Windows HWND DXGI scaling](../../../decisions/platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dxgi-scaling.md)
