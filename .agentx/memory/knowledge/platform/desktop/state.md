---
agentx:
  kind: state
  scope: knowledge/platform/desktop
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-17T02:30:00+02:00
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

Lane D proves the desktop platforms in real windows through `gpux-conformance`'s `desktop_host`: OS input through `SendInput` on Windows and XTest on X11, in-app input on Wayland and macOS (hosted macOS grants no accessibility permission), and scale changes through `WM_DPICHANGED` on Windows. On macOS the host activates the app (AppKit starts a window's display link only while it is visible, so windows behind the active app never draw) and moves the OS cursor to every injected pointer and touch position with `CGWarpMouseCursorPosition` through `AppHost::with_cursor_sync`, because the active window re-reads the OS cursor every frame; local runs need an awake, uncovered display. CoreText lays out a run of Lilex at 144.00002 px where Cosmic Text and DirectWrite give 144, and the text element ceils widths, so cross-platform size expectations allow one pixel. Proven on an Apple M5 (2026-10-09): lanes A 72/72, C 79/79 on the `metal` goldens, D 38 passed and 2 skipped. macOS proof runs through lanes C (Metal, with committed `metal` goldens) and D; macOS windows need `gpui_platform`'s `font-kit` feature for the CoreText text system; without it they get `NoopTextSystem` and render no text. Wayland's lane D is local only: weston's headless backend has no seat.

AccessKit: Windows owns an `accesskit_windows::Adapter` and answers `WM_GETOBJECT` without holding a re-entrant borrow while returning results or raising UI Automation events; macOS owns a `SubclassingAdapter` and updates native view focus; Wayland and X11 own `accesskit_unix::Adapter`s, X11 publishes scaled root bounds, and Unix adapters forward deactivation. Adapters forward activation trees and actions back through GPUI's accessibility channels.

Cosmic Text recognizes normalized `Noto Color Emoji`, `Apple Color Emoji`, and `Segoe UI Emoji` family names as color emoji and leaves `Segoe UI Symbol` outside that set.

macOS window composition (`gpui_macos/src/fast/composition.rs`, ported from gpui-fast): every GPUI surface but the base gets a `GPUIOverlayView` whose `CAMetalLayer` renderer (`gpui_apple` `new_overlay`, `opaque = NO`) shares the window renderer's device, instance buffers and atlas; native surfaces are layer-backed container `NSView`s clipping their content. `set_composition_order` re-mounts views only when the tree or order changes and otherwise updates frames; scale and drawable-size changes reach every surface. The overlay's `hitTest:` takes a point only inside its input region (logical, top-left) and forwards events to the window's view. Lane D proves it with `composition-macos-order-bounds`, `-input` and `-shadow` (the shadow check is skipped without screen-capture permission); lane D needs an unlocked, awake screen, since a locked session presents no frames.

Linux window composition (`gpui_linux/src/fast/composition/`, ported from gpui-fast): on Wayland, GPUI overlays are synchronized `wl_subsurface`s drawn by `new_sharing` renderers and committed before the base, with a `wp_viewport` (only GPUI's own subsurfaces get one); native surfaces are desynchronized subsurfaces handing out `RawWindowHandle::Wayland` that their producer sizes. An overlay's input region is its content (`layer_input_region`), and pointer input on it resolves to its window (`get_input_window`); a leave and enter between a window's surfaces in one `wl_pointer.frame` is a move, not an exit. Placements, geometry and input regions are logged at debug level under `gpux::composition`, which lane D's `composition-wayland-*` checks read. On X11, native surfaces are child windows with the GPUI content above them cut out of their bounding shape (SHAPE), or, under a compositing manager with an ARGB visual, an override-redirect overlay window (transient for the window, typed dropdown menu, input shape = its content) whose crossings with the window are moves; `composition-x11-*` read the server's tree, geometry, shapes and hit testing.

Windows window composition (`gpui_windows/src/fast/composition.rs`, gpux's own): the window keeps its wgpu DX12 HWND flip swap chain (`DXGI_SCALING_NONE`); composition adds a gpux-owned topmost DirectComposition target (`DCompositionCreateDevice2`, `CreateTargetForHwnd(hwnd, true)`) whose root holds native surface visuals (rectangle clip, opacity for visibility, handed out as `IUnknown` for WebView2's `SetRootVisualTarget`) and GPUI overlay visuals whose premultiplied swap chains wgpu creates from `SurfaceTargetUnsafe::CompositionVisual` (`WgpuRenderer::new_sharing_target`). gpux commits after every composed frame, so producer changes show with the next frame. Restacks remove only the visuals gpux nested, never a producer's children. A recovered renderer or `WM_GPUI_GPU_DEVICE_LOST` rebuilds the tree on a new device and calls `compositor_recreated`. `translate_accelerator` leaves key downs of non-GPUI windows (`is_gpui_window`) to them. Visuals take no input: hosts forward it (WebView2 `SendMouseInput`). Lane D's `composition-windows-*` run on CI (`lane D (windows-2025)`, WARP) and read GDI screenshots; the resize spike showed both designs unstretched, the visual-hosted base exposing black until redrawn, so the HWND base stays.

A macOS window AppKit does not present (fully behind another app's window, minimized, on another Space, on a sleeping display) gets no frames: `windowDidChangeOcclusionState` without `NSWindowOcclusionStateVisible` stops its display-link subscription, and the next visible notification restarts it (covering and uncovering resumes; 2026-10-17, gpux-a11y-devtools). AppKit may still present a window ordered in under another app's window. The benchmark's occasional "never draws" runs were its window covered for the rest of a run; `gpux-bench` now ends a run whose window is hidden with status 2 (`benchmark/tools/hidden-window.sh`), and `run.py` retries it once.

## Backlinks

- [Windows window](../../../../../crates/gpui_windows/src/window.rs)
- [Windows events](../../../../../crates/gpui_windows/src/events.rs)
- [macOS platform crate](../../../../../crates/gpui_macos)
- [macOS composition](../../../../../crates/gpui_macos/src/fast/composition.rs)
- [Wayland composition](../../../../../crates/gpui_linux/src/fast/composition/wayland.rs)
- [X11 composition](../../../../../crates/gpui_linux/src/fast/composition/x11.rs)
- [Windows composition](../../../../../crates/gpui_windows/src/fast/composition.rs)
- [Desktop conformance host](../../../../../crates/gpux_conformance/examples/desktop_host)
- [Wayland client](../../../../../crates/gpui_linux/src/linux/wayland/client.rs)
- [Wayland popup](../../../../../crates/gpui_linux/src/linux/wayland/popup.rs)
- [X11 window](../../../../../crates/gpui_linux/src/linux/x11/window.rs)
- [Linux foreground tasks](../../../../../crates/gpui_linux/src/linux/foreground_tasks.rs)
- [Cosmic Text system](../../../../../crates/gpui_wgpu/src/cosmic_text_system.rs)
- 2026-07-08 current [Windows WGPU DX12 policy](../../../decisions/platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dx12-policy.md)
- 2026-07-08 current [Windows HWND DXGI scaling](../../../decisions/platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dxgi-scaling.md)
