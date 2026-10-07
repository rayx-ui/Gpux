---
agentx:
  kind: rules
  scope: knowledge/platform/mobile
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-07T19:30:00+02:00
  topics:
    - android
    - ios
  supersedes: []
  superseded_by: []
---
# Mobile Rules

- Mark Android native extern blocks `unsafe extern "C"` (Rust 2024).
- Release `AndroidPlatformState` before draining foreground callbacks.
- Treat `AndroidBackend::Vulkan` as Vulkan-preferred with GL fallback; use `AndroidBackend::Gles` for strict GL.
- Enable `android-native-activity` in Android app crates that use the native activity JNI helper.
- Keep iOS WGPU initialization Metal-specific through `GPUIMetalView`/`CAMetalLayer` and `Backends::METAL`, not the desktop backend policy.
- Keep iOS text on `IosTextSystem`/CoreText; do not reuse the Cosmic Text system for iOS without a separate decision and runtime proof.
- Keep iOS startup retaining the GPUI `Application` after `Application::run()` returns.
- Do not mark iOS renderer runtime work complete from Windows checks; runtime proof needs macOS with Xcode, XcodeGen, and xcrun (lane F). `cargo check -p gpux-mobile --target aarch64-apple-ios` type-checks iOS code from any host.
- Free an `IosWindow` only after it leaves the FFI window list and its UIKit objects stop pointing at it: lifecycle calls, touches, and text input reach windows through raw pointers. Drop the renderer before releasing the view whose layer it draws into.
- Prove Android changes in lane E (`gpux-testkit lane e`). On emulators, read pixels from OS screenshots: their GL drivers read rendered textures back as zeros, and their 16-bit surfaces round colors.
- Replay a motion event's batched history samples as moves at their own times before its current sample; fast gestures otherwise reach the recognizer as a press and a release.
