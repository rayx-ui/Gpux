---
agentx:
  kind: decision
  scope: decisions/platform/mobile
  status: current
  created: 2026-10-09T17:00:00+02:00
  updated: 2026-10-09T17:00:00+02:00
  topics:
    - gpux
    - android
    - lane-e
    - emulator
    - adb
  supersedes: []
  superseded_by: []
---

## Decision: Lane E runs a Gradle-free NativeActivity conformance host over adb

### Context: Android proof needs a host that runs the scene catalog on a device or emulator, buildable on Linux, Windows and macOS and in CI without an Android Studio project.

### Alternatives considered: A Gradle project with a Java activity; instrumentation tests; physical devices only.

### Reasoning: `android_host` is a NativeActivity cdylib (`android-native-activity`) that runs the catalog through `AppHost`. `gpux-testkit android build` compiles it with cargo-ndk for the device's ABI and packages it without Gradle: `aapt2` links `tools/android/AndroidManifest.xml`, the libraries go in stored and 16 KB aligned, and `apksigner` signs with a local debug key (JDK 21, pinned SDK and NDK). `android run` installs and launches the app, follows logcat, and performs the taps, swipes, background and resume, and screenshots the host requests through adb. The Android window renders to images through the shared wgpu renderer. CI runs an API 34 x86_64 emulator under KVM, where the GL fallback on SwiftShader is expected.

### Trade-offs accepted: The Vulkan-preferred-with-GL-fallback policy is observed, not changed. Emulators read rendered textures back empty, so pixel steps read `adb screencap`. Software frames are slow: a fast fling arrives as a tap and is skipped, and a local emulator needs a small screen (`wm size 720x1280`, `wm density 320`) for the pan check to see a drag.

### Supersedes: None.
