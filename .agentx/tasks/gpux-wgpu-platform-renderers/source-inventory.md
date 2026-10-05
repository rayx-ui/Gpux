---
agentx:
  kind: knowledge
  scope: repo:tasks/gpux-wgpu-platform-renderers
  status: current
  created: 2026-07-07T23:00:00+02:00
  updated: 2026-07-08T08:16:00+02:00
  topics:
    - gpux
    - wgpu
    - source-inventory
  supersedes: []
  superseded_by: []
---

# Source Inventory: GPUI WGPU Platform Renderers

## Source-Of-Truth Files

| Area | File | Current Evidence | Planned Treatment |
| --- | --- | --- | --- |
| Platform selection | `crates/gpux/crates/gpui_platform/src/gpui_platform.rs` | `current_platform` maps Windows to `gpui_windows::WindowsPlatform::new(headless)`, mobile to `gpui_mobile::current_platform(headless)`, and WASM to `WebPlatform`. | Keep public selector stable while Windows internals move to WGPU. |
| Windows platform state | `crates/gpux/crates/gpui_windows/src/platform.rs` | `WindowsPlatform::new` creates a shared `gpui_wgpu::GpuContext`; non-headless text uses `gpui_wgpu::CosmicTextSystem::new("Segoe UI")`, while headless stays on `NoopTextSystem`. Creation info carries the WGPU context, not DirectX devices. | Keep Windows text/render initialization on the active WGPU-compatible path. Requirements: FR1, FR2, FR4, FR6. |
| Windows window state | `crates/gpux/crates/gpui_windows/src/window.rs` | `WindowsWindowState` owns `RefCell<WgpuRenderer>`, creates WGPU surfaces from `WgpuWin32Window`, delegates draw/render-to-image/atlas/GPU specs/effect capabilities to WGPU, and coalesces live-resize drawable-size changes before draw. | Preserve WGPU ownership, lifecycle, diagnostics, resize, and renderer capability delegation. Requirements: FR1, FR2, FR5, FR6, FR11. |
| Windows event lifecycle | `crates/gpux/crates/gpui_windows/src/events.rs` | Resize/DPI/minimize/drop handling now routes through WGPU surface resize/unconfigure/recovery state; `WM_SIZE` records coalesced drawable-size changes and live-resize frames are scheduled instead of forcing a full render per message. Input, IME, cursor, mouse, keyboard, drag/drop, and window action paths remain platform responsibilities. | Preserve WGPU resize/recovery behavior while keeping non-renderer input/window behavior stable. Requirements: FR1, FR6, FR11. |
| Windows renderer | `crates/gpux/crates/gpui_windows/src/directx_renderer.rs` | Removed from the active source tree; Windows rendering now uses `gpui_wgpu::WgpuRenderer`. | Keep no active DirectX renderer fallback. |
| Windows devices | `crates/gpux/crates/gpui_windows/src/directx_devices.rs` | Removed from the active source tree; shared GPU state lives in `gpui_wgpu::GpuContext`. | Keep no active DirectX device dependency. |
| Windows text | `crates/gpux/crates/gpui_windows/src/direct_write.rs` | Removed from the active source tree; Windows text uses `CosmicTextSystem` from `gpui_wgpu`. | Keep Windows text independent from removed DirectWrite device recovery. |
| Windows package | `crates/gpux/crates/gpui_windows/Cargo.toml` | Depends on `gpui_wgpu`; obsolete DirectX renderer/build shader files have been removed from the active crate. | Preserve WGPU dependency boundary and avoid reintroducing DirectX renderer dependencies. |
| WGPU context | `crates/gpux/crates/gpui_wgpu/src/wgpu_context.rs` | Native `WgpuContext::instance` enables `VULKAN | GL`; browser uses WebGPU/GL; adapter enumeration exists. | Keep Windows non-DirectX backend policy visible; add target-specific factory only if Android preference or Windows policy requires it. |
| WGPU renderer | `crates/gpux/crates/gpui_wgpu/src/wgpu_renderer.rs` | `WgpuRenderer::new` consumes raw-window handles requiring `HasWindowHandle + HasDisplayHandle + Debug + Clone + Send + Sync + 'static`; renderer exposes effect capabilities, drawable-size updates, recovery, surface replacement, surface unconfigure, transparency update, and render-to-image. | Use as the active Windows renderer and source of WGPU capabilities. Requirements: FR1, FR4, FR5, FR6, FR10, FR11. |
| Linux reference | `crates/gpux/crates/gpui_linux/src` | Existing native WGPU-backed platform/window implementation. | Use as reference for native WGPU renderer lifecycle and diagnostics patterns. |
| Android platform | `crates/gpux/crates/gpui_mobile/src/android/platform.rs` | Stores `preferred_backend: AndroidBackend` and exposes setter/getter; defaults to Vulkan. | Wire preference into WGPU backend selection or replace misleading surface. |
| Android window | `crates/gpux/crates/gpui_mobile/src/android/window.rs` | `RawAndroidWindow` feeds `WgpuRenderer::new`; renderer uses `Mailbox`; `term_window` unconfigures the surface and keeps renderer/atlas state alive while `init_window` replaces the surface; effect capabilities delegate to WGPU renderer; `create_renderer` does not yet receive the preferred backend. | Preserve lifecycle, add backend-policy diagnostics, and validate Android runtime. Requirements: FR5, FR7, FR8, FR10, FR11. |
| iOS window | `crates/gpux/crates/gpui_mobile/src/ios/window.rs` | Uses `GPUIMetalView`/`CAMetalLayer`, creates `wgpu::Instance` with `Backends::METAL`, pre-populates `GpuContext`, then constructs `WgpuRenderer`; `PlatformWindow` delegates draw/atlas/GPU specs/effect capabilities to the active renderer and reports default unsupported capabilities only when no renderer exists. | Preserve explicit Metal WGPU path and platform-window effect capability delegation. Requirements: FR5, FR9, FR10, FR11. |
| iOS text | `crates/gpux/crates/gpui_mobile/src/ios/text_system.rs` and `crates/gpux/crates/gpui_mobile/src/ios/platform.rs` | `IosPlatform::new` constructs `IosTextSystem`; `ios/text_system.rs` uses CoreText and font-kit CoreText loaders and does not depend on `CosmicTextSystem` or the Windows WGPU text migration. | Preserve the independent iOS CoreText path while Windows uses its separate WGPU-compatible `CosmicTextSystem`. |
| Mobile docs | `crates/gpux/crates/gpui_mobile/README.md` | Describes Android/iOS as WGPU-backed. | Update only if backend-policy details or capability behavior changes. |
| WASM validation | `xtask/`, `tools-node/`, `apps/lab` | Repository flow supports headed Playwright WebGPU validation through `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`. | Use as baseline proof during each implementation sprint. |

## Reference Behaviors To Preserve

- Windows app/window startup through `gpui_platform::current_platform` remains source-compatible for app authors.
- Input, focus, IME/text input, cursor, window actions, and close behavior remain Windows platform responsibilities independent of renderer choice.
- Android WGPU renderer survives native window loss/recreation by unconfiguring/replacing the surface rather than discarding all renderer state.
- iOS WGPU initialization stays Metal-specific through `Backends::METAL`.
- Shader-effect capabilities remain renderer-proven and do not become static platform promises.
- Android and iOS must render 100% of the WGPU shader-effect suite advertised by `WgpuRenderer`, including element effects, screen effects, and retained snapshot/transition effects.

## Validation Inventory

| Target | Required Proof | Command Or Tool |
| --- | --- | --- |
| WASM baseline | Shared WGPU renderer path and shader-effect contracts still work. | `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu` |
| Windows native | WGPU renderer startup, first frame, resize, text, effect capabilities, lifecycle close. | Lab/host native run plus DevKit/runtime diagnostics. |
| Android | Backend policy, surface lifecycle, first frame, logcat diagnostics, and full WGPU shader-effect suite proof. | Repository `xtask` Android path plus `adb`/`logcat`. |
| iOS | Metal/WGPU configuration, capability reporting, and full WGPU shader-effect suite proof. | macOS/device/simulator/CI runtime proof; local Windows blockers do not complete iOS shader tasks. |
| Formatting/lints | Code hygiene. | `cargo fmt --all`; focused checks/tests; `cargo clippy --workspace --all-features -- -D warnings`. |
