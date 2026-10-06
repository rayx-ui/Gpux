# Update From gpui_mobile

Use this reference when updating gpux's `gpui_mobile` crate (package `gpux-mobile`) from its upstream repository.

Run from the Gpux repository root. Upstream source paths are from `https://github.com/itsbalamurali/gpui-mobile.git`; destination paths are under `crates/gpui_mobile/`.

## Guardrails

- Start from a clean worktree or separate branch.
- Use `https://github.com/itsbalamurali/gpui-mobile.git` as the source of truth for upstream `gpui_mobile` imports.
- Fetch or shallow-clone upstream `gpui_mobile` into a temporary/cache location. A local checkout is optional cache only, not a required source.
- Import the upstream repository root into `crates/gpui_mobile/`; the crate intentionally keeps the same layout so imports stay mechanical.
- Apply upstream deletions inside `crates/gpui_mobile/` intentionally, but do not delete gpux-local files outside that crate.
- gpux distributes `gpui_mobile` under the Apache-2.0 option of upstream's `GPL-3.0-or-later OR AGPL-3.0-or-later OR Apache-2.0` license. Imports must not bring back `LICENSE-GPL`, `LICENSE-AGPL`, or the triple license field; the crate keeps `license.workspace = true` and its `LICENSE-APACHE`.
- Keep the `gpux-mobile` publishing shape from the closure reference's [Manifest And Packaging Shape](update-from-zed-gpui-closure.md#manifest-and-packaging-shape): package `gpux-mobile` with `[lib] name = "gpui_mobile"`, workspace version, edition, rust-version, and license, a `description`, no `publish = false`, and crates.io-only dependencies. Its `gpui = { package = "gpux", path = "../gpui", version = "<workspace version>" }` dependency needs the `version` so the crate publishes. (Unlike the closure crates, its `repository` and `homepage` currently point at upstream gpui-mobile.)
- Preserve or replay RayX/gpux mobile patches from gpux history after importing upstream state.
- Keep one import/replay step per commit so provenance stays reviewable.
- Record provenance against upstream `gpui_mobile` commits, not a local checkout.

## Current Path Mapping

Treat this mapping as the whole upstream `gpui_mobile` import surface:

- `<gpui-mobile repository root>` -> `crates/gpui_mobile/`

Re-check `crates/gpui_mobile/Cargo.toml`, the root `Cargo.toml`, and `crates/gpui_platform/Cargo.toml` before each update because dependency wiring or feature gates can change. Also inspect RayX app platform mirrors such as `../RayX/apps/lab/platform/android`, `../RayX/apps/examples_mobile/platform/android`, and iOS project files when an upstream import changes Java/JNI class names, package-helper names, native activity wiring, bundle metadata, or generated headers.

## Workflow

1. Read current provenance:
   - `.agentx/memory/knowledge/platform/mobile/state.md`, plus its `rules.md` and `knowledge.md` when they answer a question.
   - `.agentx/memory/decisions/INDEX.md`
   - Relevant `platform` decisions for mobile lifecycle, backend policy, Java/JNI package names, shader capabilities, and iOS proof limits.
   - `git log --format='%h%n%B' -- crates/gpui_mobile`
   - latest import commits with subjects like `chore(gpux): import gpui_mobile ...`

2. Prepare upstream source:
   - Fetch or shallow-clone `https://github.com/itsbalamurali/gpui-mobile.git` into a temporary/cache location.
   - Use an explicit upstream commit SHA for the import target.
   - If reusing an existing local checkout, verify its `origin` or selected remote points at `https://github.com/itsbalamurali/gpui-mobile.git` and fetch before selecting the commit.

3. Identify the source range:
   - Find the last upstream `gpui_mobile` commit already represented in gpux from `Source gpui_mobile commit:` or older import provenance.
   - Identify newer upstream commits that touch mobile lifecycle, Android/iOS windowing, frame scheduling, surfaces, input, package helpers, safe areas, backend selection, renderer capabilities, FFI/JNI, or crate metadata.
   - Use upstream `gpui_mobile` commits as import points.
   - Audit the touched upstream paths for the whole repository root because the crate mirrors that layout.
   - Pay special attention to `src/android_backend.rs`, `src/android/window.rs`, `src/android/platform.rs`, `src/android/jni.rs`, `src/android/embedded.rs`, `src/ios/window.rs`, `src/ios/ffi.rs`, `src/ios/platform.rs`, `src/ios/text_system.rs`, and `src/packages/**`.

4. Import upstream state:
   - Replace `crates/gpui_mobile/` with the chosen upstream repository state, including deletions.
   - Reconcile workspace metadata manually when needed: manifests, workspace dependency references, feature gates, local docs, and imports must still build from the root `Cargo.toml`.
   - Restore the `gpux-mobile` publishing shape and Apache-2.0-only licensing from the guardrails above.
   - Preserve the local dependency relationship where `gpui_platform` selects `gpui_mobile` for Android and iOS targets.
   - Commit with a subject like `chore(gpux): import gpui_mobile <short-sha>`.
   - Include commit body lines:

```text
Source repository: https://github.com/itsbalamurali/gpui-mobile.git
Source gpui_mobile commit: <gpui-mobile-upstream-sha>
Original subject: <original source subject>
```

5. Preserve or replay RayX/gpux mobile patches:
   - Use gpux history and the AgentX mobile state to identify local patches that must survive the import.
   - Preserve patches already present after reconciliation or replay local gpux commits when upstream replacement overwrote them.
   - Keep each local patch as a separate gpux commit when practical.
   - Include commit body lines:

```text
Preserved gpux patch commit: <gpux-local-sha>
Original subject: <original local subject>
```

6. Validate:
   - Run `cargo metadata --no-deps`.
   - Run `cargo fmt --check`.
   - Run `cargo ndk -t arm64-v8a check -p gpux-mobile` with `ANDROID_NDK_HOME` set.
   - Run `cargo ndk -t arm64-v8a check -p gpux-platform` when Android tooling is available because platform selection depends on `gpui_mobile`.
   - Run `cargo package --workspace` when the import changed the `gpux-mobile` manifest or dependencies.
   - From `../RayX`, run `cargo run -p xtask -- app apps/test_suite build android --debug` when Android app packaging, JNI class names, package helpers, native activity wiring, or manifest metadata changes.
   - From `../RayX`, run Android device/emulator validation with app-scoped `adb logcat` and DevKit/runtime diagnostics when lifecycle, surface, backend selection, frame scheduling, input, safe-area, or shader/effect capability behavior changed.
   - On macOS with iOS tooling, run the equivalent iOS-target check for `gpux-mobile`.
   - On macOS with Xcode/XcodeGen/xcrun, run RayX's iOS path from `../RayX` when iOS lifecycle, surface, Metal/WGPU, text, safe-area, package helper, or shader/effect capability behavior changed.
   - From Windows, iOS source/configuration checks are useful but are not runtime proof; do not close iOS renderer or shader-output work from Windows checks alone.

## RayX Patch Surface To Preserve

The RayX memories identify these `gpui_mobile` behaviors as easy to lose during imports:

### Workspace And Crate Wiring

- Preserve the crate layout at `crates/gpui_mobile/`, the `gpux-mobile` package name with Rust name `gpui_mobile`, and the dependency relationship where `gpui_platform` selects `gpui_mobile` on Android and iOS.
- Preserve the Apache-2.0-only distribution: no `LICENSE-GPL`, `LICENSE-AGPL`, or triple license field.
- Preserve Rust 2024 FFI compatibility, especially `unsafe extern "C"` blocks in Android/iOS dispatchers and package helpers.
- Preserve the `android-native-activity` feature and app-side feature enablement for Android crates that use the native activity JNI helper.
- Preserve package helper class lookups and JNI exports together with the app Java mirrors. If helper packages or class names move, update Rust lookup strings, JNI export names, Java source paths, Android manifests, ProGuard rules, Gradle config, and app metadata in the same change.

### Android Lifecycle And Java/JNI

- Preserve Android embedded `SurfaceView` lifecycle handoff instead of requiring GPUI `NativeActivity` for managed RayX hosts.
- Preserve `RayXActivity` as the generic framework activity under `dev.rayx.mobile`, including native activity integration, AndroidX splash-screen gating, deep-link forwarding, and the JNI exports `Java_dev_rayx_mobile_RayXActivity_nativeIsInitialized` and `Java_dev_rayx_mobile_RayXActivity_nativeOnDeepLink`.
- Preserve `NATIVE_INITIALIZED`/splash initialization state and the content-rect safe-area update path from Android JNI into `AndroidWindow`.
- Preserve Android lifecycle/surface resume handling, dirty state, size updates, and frame callback scheduling around native window availability.

### Android WGPU Backend And Effects

- Preserve `AndroidBackend` and its policy labels. `AndroidBackend::Vulkan` means Vulkan-preferred with GL fallback (`VULKAN | GL`), and `AndroidBackend::Gles` is strict GL.
- Preserve `AndroidPlatform::preferred_backend()`, `set_preferred_backend(...)`, and passing the selected backend into `AndroidWindow` and the WGPU renderer context.
- Preserve Android WGPU renderer effect capabilities delegating to the active renderer, not hard-coded unsupported values when a renderer exists.
- Preserve Android shader/effect runtime diagnostics: element backdrop blur, glass, glow, screen ripple, and snapshot transitions should report capability support only when the WGPU renderer path is active.

### Android Frame Scheduling, Input, And Insets

- Preserve Android frame callback policy that requires presentation for scroll-input frames but uses one-shot force-render for text dirtiness and lifecycle/surface resume dirtiness.
- Deliver Android touches as raw `PlatformInput::Touch` events; GPUI's touch recognizer owns taps, pans, fling momentum (`ScrollPhysics::android()` from `Platform::gestures`), long presses, and touch drags. Keep two-finger pinch synthesis, cancelling the forwarded touches when a pinch starts.
- Preserve touch sample times: `TouchPoint::timestamp` carries `AMotionEvent_getEventTime` converted to `Instant` (`input_event_instant` in `jni.rs`; `System.nanoTime()` and `Instant` both read `CLOCK_MONOTONIC`), and the embedded host stamps touches on arrival.
- Preserve `poll_main_looper` in `jni.rs`: the NativeActivity loop blocks in gpux's own `ALooper_pollOnce`, where AChoreographer frame callbacks and the dispatcher's main-thread tasks run, and calls `AndroidApp::poll_events` only for android-activity's own idents. android-activity 0.6.1 logs every callback that runs inside its poll as a spurious `ALOOPER_POLL_CALLBACK` error, once per frame.
- Preserve deleted RayX-active mobile momentum helpers; do not reintroduce a `gpui_mobile` momentum pump into active RayX input paths.
- Preserve Android safe-area APIs and content-rect derived physical/logical inset conversion.
- Preserve Android keyboard/input, permissions, picker, media, and platform-view package helpers when importing upstream package changes.

### iOS Lifecycle, Metal, And Text

- Preserve iOS finish-launching handoff used by the managed single-view host.
- Preserve the retained `gpux::Application` after `Application::run()` returns, because UIKit callbacks outlive the one-shot launch closure.
- Preserve iOS Metal-specific WGPU initialization: `GPUIMetalView`, `CAMetalLayer`, `wgpu::Backends::METAL`, pre-populated `WgpuContext`, and `WgpuRenderer` construction from that context.
- Preserve iOS platform-window effect capabilities delegating to the active `WgpuRenderer`, with unsupported fallback only when no renderer exists.
- Preserve iOS `IosTextSystem`/CoreText/font-kit text loading. Do not replace it with Windows `CosmicTextSystem` without a separate iOS text-system decision and runtime proof.
- Preserve iOS safe-area insets. Deliver iOS touches as raw `PlatformInput::Touch` events with `ScrollPhysics::ios()` from `Platform::gestures`, and keep two-finger pinch synthesis like Android.

GOOD:

```text
Import the upstream gpui-mobile repository root into crates/gpui_mobile/, then replay the Android embedded SurfaceView patch as its own gpux commit with source provenance.
```

BAD:

```text
Copy only src/android.rs from upstream and leave deleted files or manifest changes behind because the conflict looked small.
```

Check:

- `git diff -- crates/gpui_mobile`
- `cargo metadata --no-deps`
- Targeted Android/iOS `gpux-mobile` and `gpux-platform` checks when toolchains are available.

## Durable Memory

After a completed import:

- Update `.agentx/memory/knowledge/platform/mobile/state.md` with any changed current mobile behavior and `.agentx/memory/knowledge/repository/upstream-sync/state.md` with the latest represented upstream `gpui_mobile` commit.
- Update the mobile `knowledge.md` only for durable facts that are not obvious from state, and `rules.md` only if the import/replay rule changed.
- Add a decision in gpux memory if the source repository, path mapping, provenance policy, or mobile patch ownership changes.
- Run the AgentX audit script from `../RayX` after memory changes.

## Review Checklist

- The import source was `https://github.com/itsbalamurali/gpui-mobile.git`.
- The upstream repository root was mapped to `crates/gpui_mobile/`.
- Upstream deletions inside `gpui_mobile` were applied intentionally.
- No GPL/AGPL license file or triple license field came back, and the `gpux-mobile` manifest still publishes (`cargo package --workspace`).
- Android/iOS RayX patch surfaces were preserved or deliberately reconciled.
- Commit bodies preserve upstream `gpui_mobile` commit provenance.
- Validation results are recorded in the final response or PR notes.
