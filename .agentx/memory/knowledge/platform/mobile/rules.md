---
agentx:
  kind: rules
  scope: knowledge/platform/mobile
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-10T15:00:00+02:00
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
- Give each Android backend its own surface (Vulkan, then GL): a Vulkan surface keeps the native window connected until it is destroyed, so GL cannot fall back on a surface Vulkan shares.
- Release Android delayed tasks from the dispatcher's timer thread, never from the main loop alone: the loop blocks in the looper once frames stop, and timers must still fire in the background.
- Size lane E gestures sent through `adb shell input swipe` in frames of the device, not milliseconds: each event is injected after the app handled the one before, so a swipe shorter than a frame carries no moves.
- Prove Android changes in lane E (`gpux-testkit lane e`). On emulators, read pixels from OS screenshots: their GL drivers read rendered textures back as zeros, and their 16-bit surfaces round colors.
- Build lane E with the pinned NDK only: an `ANDROID_NDK_HOME` override must be the pinned directory or declare its base revision (the pinned 28.0.12674087 is r28-beta2, `Pkg.Revision = 28.0.12674087-beta2`); CI runner images point the variable at other NDKs.
- Replay a motion event's batched history samples as moves at their own times before its current sample; fast gestures otherwise reach the recognizer as a press and a release.
- Never keep per-activity Android state in a set-once static: a restarted `android_main` in the same process must replace it.
- Keep Android composition panel windows `FLAG_NOT_FOCUSABLE` except while a text-taking view in one was touched; announce that focus move to Rust first (`nativePanelFocus`), since the activity's `LostFocus` otherwise pauses GPUI.
- Build `gpux-mobile` for Android with and without `android-native-activity` after touching `android/`: the embedded host is the default feature set and no lane builds it.
- Prove that a native window or view is gone by listing windows or the view tree, not by a tap or a pixel: a hidden panel passes both.
- Never present a GPUI overlay through a GLES surface over native views: wgpu's GLES surfaces are opaque; use cut-out mode there and the overlay window only under Vulkan.
- Place a platform view through `platform_view_element` (a composition native surface), never by adding it to the activity's or view controller's hierarchy; the element clips it to its content mask.
- Do not route NativeActivity touches by platform-view bounds: a view's panel window takes its own touches, and GPUI gets the ones on its content.
- Ship Android Java that gpux calls through JNI in `crates/gpui_mobile/java` under `dev.gpux.mobile`; consumers compile those sources and keep the package from R8.
