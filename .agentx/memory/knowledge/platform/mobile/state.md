---
agentx:
  kind: state
  scope: knowledge/platform/mobile
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-10T15:00:00+02:00
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

`gpui_mobile` is Rust 2024 and distributed under its Apache-2.0 option. Android routes `AndroidBackend` preference into WGPU backend policy: `Vulkan` means `VULKAN | GL`, because emulator images can fail Vulkan surface creation while supporting GL/EGL; `Gles` is strict GL. The window tries one surface per backend, Vulkan then GL (`AndroidBackend::surface_attempts`): Android's Vulkan loader connects the native window when it creates a Vulkan surface and only destroying it releases the window, so a shared surface never fell back to GL where Vulkan exists but is unusable (the arm64 emulator's software SwiftShader Vulkan, which gpux skips). Diagnostics can therefore show `backend_policy=VULKAN|GL` with `backend=gl`. Apps using the native activity JNI helper enable the `android-native-activity` feature. Foreground task draining clones the dispatcher under the platform state mutex, releases it, then runs queued callbacks. Android and iOS deliver touches as raw `PlatformInput::Touch` events for Zed's touch recognizer, supply fling physics through `Platform::gestures` (`ScrollPhysics::android()`/`ios()`), and synthesize two-finger `Pinch` input, cancelling the forwarded touches when a pinch starts. Android stamps touches with their sample time: NativeActivity converts `AMotionEvent_getEventTime` (`System.nanoTime()`, `CLOCK_MONOTONIC` like `Instant`), and the embedded host stamps on arrival.

`AndroidWindow` implements `render_to_image` by taking the renderer out of the state lock while it renders, as `draw` does, and warns when sprites are drawn through the fallback atlas before a renderer exists. Touch input replays each motion event's history samples (`pointer.history()`) as moves with their own timestamps before the current sample.

Backgrounding unconfigures the primary window's surface (`term_window`) and keeps the renderer, so captures fail with the unconfigured-surface error until `InitWindow` replaces the surface; every GPUI window on Android wraps that one primary native window. Lane E's lifecycle check observes exactly that loss and recovery before probing the screen. Android takes the surface only once it stopped the activity, seconds after the home key on a slow emulator, and an activity relaunched sooner resumes with its surface intact: the host asks the testkit to go to the background and, after it saw the loss, to come back (`background`, `foreground`).

Lane E proves Android through the `android_host` conformance example: a NativeActivity cdylib packaged without Gradle (`aapt2` link, stored 16 KB aligned libraries, `apksigner` with a debug key) that runs a startup check, the lane-E scenes, and touch, swipe, and lifecycle checks the testkit performs through adb. The x86_64 emulator renders through SwiftShader on the GL fallback with an RGB565 EGL config, because wgpu-hal requests 8-bit alpha only where sRGB framebuffers exist. The arm64 emulator on Apple silicon (API 34, ANGLE on SwiftShader Vulkan) passed 29 with the fling skip, twice (2026-10-09). `adb logcat` can drop stream lines while adb transfers a screenshot, so the host repeats screenshot requests and the testkit tallies from the device's log buffer when it holds more results; screen checks capture until the system's window animation (launch fade, return scale) has ended. `adb shell input swipe` injects each event once the app handled the one before and sends only the moves its duration has time left for, and the app handles input between frames: on CI's SwiftShader (frames near 0.8 s) a 600 ms swipe was a press and a release with no move, which the recognizer resolves as a tap. The pan check therefore swipes for at least eight of the window's measured frames.

The NativeActivity loop polls the main looper through `poll_main_looper`: AChoreographer frame callbacks and the dispatcher's main-thread tasks run in gpux's own `ALooper_pollOnce`, and `AndroidApp::poll_events` runs only for android-activity's own idents. android-activity 0.6.1 logs every callback that runs inside its own poll as a spurious `ALOOPER_POLL_CALLBACK` error, which flooded the log once per frame. That loop blocks in the looper while nothing wakes it, and frames stop when the activity loses focus or goes to the background, so the dispatcher releases delayed tasks from its own timer thread (`gpui-timers`) instead of the loop's `tick()`: timers then fire in the background and during slow frames.

Lane F proves iOS through the `ios_host` conformance example: a static library behind the minimal UIKit shell of `tooling/ios` (XcodeGen spec, Objective-C app delegate that registers the host, starts gpui_mobile, and forwards the lifecycle), built unsigned for the Simulator and run with `simctl launch --console-pty`. Its checks cover startup, wgpu on Metal with readback, CoreText metrics of the bundled IBM Plex Sans, in-app touch, and background/foreground through another app and the host's `gpux-conformance://` URL scheme. `IosWindow` renders to images through the shared wgpu renderer and reports a missing renderer (no Metal device) as an error that pixel steps turn into skips. Closing an `IosWindow` unregisters it from the FFI window list, clears the UIKit back-pointers, drops the renderer, hides the UIWindow, and autoreleases the UIKit objects it created. The shell force-loads the Rust library, which pulls in io-surface's macOS-only CGL calls (through gpux's core-video), so it defines them as aborting stubs (`tooling/ios/Sources/macos_gl_stubs.c`): `-Wl,-U` links but dyld fails at launch, and dead-code stripping keeps force-loaded code. The Simulator on Apple silicon offers Metal: lane F passed 30/30 (five platform checks, 25 scenes) on an iPhone 16 Simulator with iOS 26.5 on an M5 (2026-10-09).

iOS keeps a Metal-specific WGPU path: `GPUIMetalView` exposes `CAMetalLayer`, the window creates a `wgpu::Instance` with `Backends::METAL`, pre-populates `WgpuContext`, and builds `WgpuRenderer` from it. Effect capabilities delegate to the active renderer. Text uses the independent `IosTextSystem` on CoreText and font-kit CoreText loaders. `gpui_mobile` retains a cloned `gpux::Application` in FFI-owned state after `Application::run()` returns, because UIKit owns the event loop and later callbacks need GPUI state.

Android may run `android_main` again in the same process after destroying the native activity (CI's lane E x86_64 emulator does at startup). `jni::init_platform` replaces the process-wide `AndroidApp` and `AndroidPlatform` each time (`Current<T>`, leaking replaced values); a kept platform's dispatcher would belong to the previous run's looper and App construction would panic ("must construct App on main thread").

Window composition on mobile is gpux's own (`gpui_mobile/src/{ios,android}/fast/composition.rs`). iOS: native `UIView` containers and a `GPUIOverlayView` (transparent `CAMetalLayer`) are subviews of the Metal view; the overlay's `hitTest:withEvent:` takes a touch only within its input region; its renderer comes from `WgpuRenderer::new_sharing_target` on a UiKit raw handle, is configured `PostMultiplied` and is released before the window's renderer. Android: the Java helper `dev.gpux.mobile.Composition` (`crates/gpui_mobile/java`, compiled by `gpux-testkit android build` with `javac` and `d8`, natives registered by gpux) puts each native surface's container in a `TYPE_APPLICATION_PANEL` window on the activity's window token, which serves the NativeActivity and embedded hosts alike (an embedding host whose `SurfaceView` does not fill the window names it with `Composition.setHostView`; only the NativeActivity host has a lane). Panels are not focusable, so the activity keeps the window focus; one whose views take text (`WebView`, `EditText`) becomes focusable when touched and gives the focus back on a touch outside or the back key, and the helper's `nativePanelFocus` keeps the NativeActivity loop from treating that `LostFocus` as a pause. A changed order re-mounts only the panels from the first changed position on. Touches GPUI takes over native views are forwarded with a move's batched history, each sample at its own time. Under Vulkan the overlay is a sub-panel window with a `SurfaceView`, touchable only while it has content. wgpu presents GLES surfaces only opaque (`R8G8B8X8`), so under GL the whole scene is drawn into GPUI's window and each container clips the overlay content above it out of itself (`setHoles`) and forwards touches there to GPUI: cut-out mode, where nothing blends over a native view.

Platform views sit on composition: `platform_view_element` makes a native surface per window and view, attaches the view (`PlatformView::attach`), places the surface at the element's bounds intersected with its content mask and gives the view its full frame inside (`PlatformView::set_frame`); a disposed view's surface is removed by a task waiting on the disposal, whether or not a platform view is laid out again. `platform_view_element` and `ManagedPlatformView::render` return a `Div` the caller sizes. Android views come from `dev.gpux.mobile.PlatformViews` (`container`, `webview`, app-registered factories); RayX's apps add gpux's Java sources to Gradle, keep `dev.gpux.mobile.**` from R8 and register their own types. Lanes E and F prove it with `composition-{android,ios}-*`, `platform-view-{android,ios}` and `platform-view-android-text`; lane E reads the process's panel windows through `WindowInspector.getGlobalWindowViews`.

## Backlinks

- [Android conformance host](../../../../../crates/gpux_conformance/examples/android_host)
- [Android lane tooling](../../../../../tooling/testkit/src/android.rs)
- [Android composition](../../../../../crates/gpui_mobile/src/android/fast/composition.rs)
- [iOS composition](../../../../../crates/gpui_mobile/src/ios/fast/composition.rs)
- [Android Java helpers](../../../../../crates/gpui_mobile/java/dev/gpux/mobile)
- [Platform view element](../../../../../crates/gpui_mobile/src/components/platform_view_element.rs)
- [Android platform](../../../../../crates/gpui_mobile/src/android/platform.rs)
- [Android window](../../../../../crates/gpui_mobile/src/android/window.rs)
- [iOS conformance host](../../../../../crates/gpux_conformance/examples/ios_host)
- [iOS app shell](../../../../../tooling/ios)
- [iOS lane tooling](../../../../../tooling/testkit/src/ios.rs)
- [iOS FFI](../../../../../crates/gpui_mobile/src/ios/ffi.rs)
- [iOS window](../../../../../crates/gpui_mobile/src/ios/window.rs)
- [iOS text system](../../../../../crates/gpui_mobile/src/ios/text_system.rs)
- 2026-10-07 current [Android lane E](../../../decisions/platform/mobile/2026-10-07-platform-mobile-android-lane-e.md)
- 2026-10-07 current [iOS lane F](../../../decisions/platform/mobile/2026-10-07-platform-mobile-ios-lane-f.md)
