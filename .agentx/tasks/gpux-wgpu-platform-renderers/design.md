---
agentx:
  kind: knowledge
  scope: repo:tasks/gpux-wgpu-platform-renderers
  status: current
  created: 2026-07-07T23:00:00+02:00
  updated: 2026-07-07T23:00:00+02:00
  topics:
    - gpux
    - wgpu
    - windows
    - mobile
    - renderer
    - design
  supersedes: []
  superseded_by: []
---

# Design: GPUI WGPU Platform Renderers

## Current Source Shape

`gpui_platform::current_platform` maps Windows to `gpui_windows::WindowsPlatform::new(headless)`, mobile to `gpui_mobile::current_platform(headless)`, and WASM to `gpui_web::WebPlatform`.

`gpui_windows` currently owns a DirectX-first stack:

- `WindowsPlatform::new` creates `DirectXDevices`.
- `DirectWriteTextSystem` is constructed from those DirectX devices.
- `WindowsWindowState` owns `RefCell<DirectXRenderer>`.
- `DirectXRenderer` owns D3D/DXGI/DirectComposition rendering, atlas, device-loss, and present behavior.
- `WindowsWindow` already exposes raw Win32 window/display handles, which are the right shape for WGPU surface creation.

`gpui_wgpu` already owns the shared renderer:

- `WgpuRenderer::new` accepts raw window/display handles and creates/configures a surface.
- `WgpuRenderer::effect_capabilities()` reports the implemented WGPU shader-effect support.
- `WgpuRenderer` has surface replacement, device-loss/recovery, transparency update, render-to-image, and diagnostics-oriented APIs used by other targets.
- Native `WgpuContext::instance` currently enables `wgpu::Backends::VULKAN | wgpu::Backends::GL`.

`gpui_mobile` is already WGPU-backed:

- Android wraps `ANativeWindow` as `RawAndroidWindow`, constructs `WgpuRenderer` with `Mailbox` present mode, and unconfigures/replaces the surface across native window loss/recreation.
- Android exposes `AndroidBackend::{Vulkan,Gles}` preference state, but the current renderer creation path does not feed that preference into WGPU backend selection.
- iOS uses `GPUIMetalView`/`CAMetalLayer`, constructs a `wgpu::Instance` with `Backends::METAL`, pre-populates `GpuContext`, and then constructs `WgpuRenderer`.
- iOS has a CoreText text system and must remain independent from the Windows DirectX text system.

## Target Architecture

Windows native must become a WGPU platform peer of Linux, Web, Android, and iOS. The active Windows platform creates or receives WGPU renderer state, Windows windows own `WgpuRenderer`, and renderer capabilities come from the active renderer.

The clean end-state shape is:

- `gpui_windows` depends on `gpui_wgpu`.
- `WindowsPlatformState` owns WGPU context/backend policy and a WGPU-compatible text system for non-headless operation.
- `WindowsWindowState` owns `RefCell<WgpuRenderer>` or an equivalent direct WGPU renderer owner.
- A focused Win32 raw-window wrapper provides the cloneable/sendable handle object consumed by `WgpuRenderer::new`.
- `WindowsWindow::draw`, resize, transparency update, screenshot/render-to-image, and device-loss paths delegate directly to `WgpuRenderer` using existing GPUI/WGPU patterns.
- DirectX renderer/device/atlas modules are removed from the active rendering path. Any remaining Windows OS integration must be named for its actual responsibility and must not expose a DirectX renderer fallback.

## Windows Backend Policy

The initial Windows WGPU route must keep the current native WGPU backend set, `VULKAN | GL`, because the user goal is avoiding the checked-in DirectX renderer. This policy must be visible in code and diagnostics.

If later validation proves WGPU DX12 is required or desirable, record a decision first and describe it as WGPU-over-DX12. That is not the same as retaining the old `DirectXRenderer`.

## Text And Atlas Design

The Windows WGPU route must use the WGPU-compatible text path instead of `DirectWriteTextSystem` tied to `DirectXDevices`. The expected implementation is to reuse `gpui_wgpu::CosmicTextSystem`, subject to source validation during Sprint 2.

Text acceptance must focus on:

- basic Latin UI text,
- font fallback,
- emoji fallback where currently supported,
- high-DPI glyph scale,
- no D3D device requirement in the WGPU route.

## Capability Reporting

Effect support remains capability-driven:

- WGPU-backed Windows reports WGPU effect capabilities from `WgpuRenderer`.
- Android continues to report `WgpuRenderer` effect capabilities when a renderer exists.
- iOS reports `WgpuRenderer` effect capabilities when a renderer exists.
- Missing-renderer, headless, or non-WGPU states report unsupported capabilities.

Do not copy WGPU capability values into static platform state unless the renderer is actually active.

## Mobile Shader Support Contract

Android and iOS must provide 100% support for the WGPU shader-effect suite advertised by `WgpuRenderer::effect_capabilities()`. The required suite includes element effects, screen effects, and retained snapshot/transition effects.

Android must make the Vulkan/GLES backend policy real before shader support is called complete. Runtime proof must show that the selected Android WGPU backend compiles the shaders, creates the pipelines and bind groups, renders nonblank effect output, survives surface recreation, and reports matching capabilities.

iOS must keep its custom Metal `wgpu::Instance` and must route `PlatformWindow::effect_capabilities()` to the active `WgpuRenderer`. Runtime proof must show that the Metal-backed WGPU path compiles the same shaders, creates the same effect pipelines and bind groups, renders nonblank effect output, and reports matching capabilities.

Any mobile-only missing effect, disabled capability, shader compilation failure, bind-layout mismatch, blank effect output, or unvalidated advertised capability blocks the Android/iOS shader-support tasks.

## Diagnostics And Temporary Traces

Development must prefer targeted runtime state over guesswork. When behavior is unclear, temporarily trace:

- selected renderer kind,
- selected WGPU backend and adapter,
- surface creation/configuration/unconfiguration,
- present mode,
- window logical/physical size and scale factor,
- transparency mode,
- device-loss/recovery state,
- effect capability state.

Temporary traces may be `tracing` logs, DevKit diagnostics, or short-lived debug fields, but they must be removed before task completion unless promoted into a deliberate production diagnostic surface.

## Validation Design

WASM headed WebGPU is the baseline development proof because it exercises the shared WGPU renderer and shader-effect contracts quickly:

```powershell
cargo run -p xtask -- app apps/lab test wasm --headed --webgpu
```

Affected native targets still require their own runtime proof:

- Windows: run the Lab or selected host app on the native Windows platform and inspect DevKit/runtime diagnostics for WGPU renderer state.
- Android: use the repository `xtask` Android path plus `adb`/`logcat` when Android renderer behavior changes.
- iOS: preserve the Metal/WGPU source path and run compile/configuration/runtime proof where host tooling is available; document exact blockers from Windows.

Screenshots can support evidence, but they do not replace diagnostics, logs, and target-specific runtime checks.

## Migration Strategy

1. Build a small Windows WGPU renderer construction slice first and prove first-frame rendering.
2. Move text, atlas, and capability reporting to the WGPU route.
3. Complete lifecycle parity and remove the active DirectX renderer path.
4. Tighten Android/iOS WGPU backend/capability behavior.
5. Run broad validation, remove temporary traces, and update durable memory/docs.

This sequence keeps each sprint vertically testable and avoids carrying a long-lived compatibility renderer layer.
