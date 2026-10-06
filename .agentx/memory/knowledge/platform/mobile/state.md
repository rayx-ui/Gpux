---
agentx:
  kind: state
  scope: knowledge/platform/mobile
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-06T03:20:00+02:00
  topics:
    - gpui-mobile
    - android
    - ios
    - backend-policy
    - metal
    - coretext
  supersedes: []
  superseded_by: []
---
# Mobile State

## Purpose And Usage

Read before changing `gpui_mobile` or importing upstream `gpui-mobile` (use the gpux skill's [gpui-mobile update guide](../../../../../.claude/skills/gpux/references/update-from-gpui-mobile.md)).

## Current Technical Shape

`gpui_mobile` is Rust 2024 and distributed under its Apache-2.0 option. Android routes `AndroidBackend` preference into WGPU backend policy: `Vulkan` means `VULKAN | GL`, because emulator images can fail Vulkan surface creation while supporting GL/EGL; `Gles` is strict GL. Diagnostics can therefore show `backend_policy=VULKAN|GL` with `backend=gl`. Apps using the native activity JNI helper enable the `android-native-activity` feature. Foreground task draining clones the dispatcher under the platform state mutex, releases it, then runs queued callbacks. Android and iOS deliver touches as raw `PlatformInput::Touch` events for Zed's touch recognizer, supply fling physics through `Platform::gestures` (`ScrollPhysics::android()`/`ios()`), and synthesize two-finger `Pinch` input, cancelling the forwarded touches when a pinch starts. Android stamps touches with their sample time: NativeActivity converts `AMotionEvent_getEventTime` (`System.nanoTime()`, `CLOCK_MONOTONIC` like `Instant`), and the embedded host stamps on arrival.

The NativeActivity loop polls the main looper through `poll_main_looper`: AChoreographer frame callbacks and the dispatcher's main-thread tasks run in gpux's own `ALooper_pollOnce`, and `AndroidApp::poll_events` runs only for android-activity's own idents. android-activity 0.6.1 logs every callback that runs inside its own poll as a spurious `ALOOPER_POLL_CALLBACK` error, which flooded the log once per frame.

iOS keeps a Metal-specific WGPU path: `GPUIMetalView` exposes `CAMetalLayer`, the window creates a `wgpu::Instance` with `Backends::METAL`, pre-populates `WgpuContext`, and builds `WgpuRenderer` from it. Effect capabilities delegate to the active renderer. Text uses the independent `IosTextSystem` on CoreText and font-kit CoreText loaders. `gpui_mobile` retains a cloned `gpux::Application` in FFI-owned state after `Application::run()` returns, because UIKit owns the event loop and later callbacks need GPUI state.

## Backlinks

- [Android platform](../../../../../crates/gpui_mobile/src/android/platform.rs)
- [Android window](../../../../../crates/gpui_mobile/src/android/window.rs)
- [iOS FFI](../../../../../crates/gpui_mobile/src/ios/ffi.rs)
- [iOS window](../../../../../crates/gpui_mobile/src/ios/window.rs)
- [iOS text system](../../../../../crates/gpui_mobile/src/ios/text_system.rs)
