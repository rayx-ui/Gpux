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
  supersedes: []
  superseded_by: []
---

# Requirements: GPUI WGPU Platform Renderers

## Scope

Move Windows native GPUI rendering onto the shared `gpui_wgpu::WgpuRenderer` path instead of the checked-in DirectX renderer, while preserving and tightening the existing WGPU-backed Android and iOS mobile paths in `gpui_mobile`.

This epic is limited to platform renderer plumbing, renderer capabilities, platform lifecycle behavior, validation, and documentation/memory for the gpux foundation. It does not redesign GPUI scene construction, shader effects, or RayX app startup except where those surfaces are needed as runtime proof.

## Problem

Windows native currently enters `gpui_windows::WindowsPlatform`, eagerly creates `DirectXDevices`, uses `DirectWriteTextSystem`, and gives each native window a `DirectXRenderer`. That keeps Windows on a renderer path that cannot share the WGPU shader-effect implementation already used by WASM, Linux, Android, and iOS.

`gpui_wgpu::WgpuRenderer` already supports native raw-window surfaces and is used by other targets, so Windows must use the same WGPU renderer family. Android and iOS are already WGPU-backed, but Android exposes a preferred backend enum that is not currently plumbed into WGPU instance creation, and iOS must advertise WGPU effect capabilities through its platform window when the renderer is active.

## Actors

- GPUI platform integrator: owns `gpui_platform::current_platform` and per-target platform crates.
- Windows native app user: expects host apps and Lab to open, resize, render, handle input, and recover from renderer lifecycle events.
- Mobile app user: expects Android and iOS renderers to stay WGPU-backed and keep lifecycle behavior correct.
- Renderer developer: needs WGPU shader effects and renderer capabilities to behave consistently across WGPU-backed targets.
- Diagnostics agent: uses RayX DevKit, headed Playwright, WebGPU browser validation, Android tooling, and temporary traces to prove behavior.

## In Scope

- Replace the active Windows native rendering path with `gpui_wgpu::WgpuRenderer`.
- Add or reuse a Win32 raw-window wrapper that satisfies `HasWindowHandle + HasDisplayHandle + Clone + Send + Sync + 'static` for WGPU surface creation.
- Replace Windows DirectX-backed text/atlas dependencies in the active WGPU path with a WGPU-compatible text system, expected to be `CosmicTextSystem` unless source inspection proves another clean end-state path.
- Make Windows WGPU backend selection explicit. The non-DirectX Windows path must default to `VULKAN | GL` with clear diagnostics unless a later decision intentionally enables DX12 through WGPU.
- Preserve Windows input, window lifecycle, size/DPI handling, transparency, present mode, redraw, device-loss recovery, and screenshots/render-to-image where those are currently supported.
- Ensure Windows reports WGPU renderer effect capabilities only when the WGPU renderer is active and actually renders the effects.
- Preserve Android WGPU renderer lifecycle across `term_window` and `init_window`, including surface unconfigure/replacement and atlas retention.
- Wire Android `AndroidBackend::{Vulkan,Gles}` preference into WGPU backend selection, or remove/replace the misleading preference surface with a documented clean API in the same change.
- Ensure iOS keeps its explicit Metal-backed WGPU instance and delegates effect capability reporting to `WgpuRenderer` when the renderer is active.
- Require 100% WGPU shader-effect support on Android and iOS for every capability advertised by `WgpuRenderer`, including element effects, screen effects, and retained snapshot/transition effects.
- Add or update runtime diagnostics needed to identify the active renderer backend, adapter/backend choice, surface lifecycle state, and effect capability state.
- Update gpux source inventory, traceability, docs, and AgentX memory with proven durable facts after implementation.

## Out Of Scope

- Implementing a native DirectX shader-effect backend.
- Keeping app-facing compatibility shims or alias APIs for old renderer choices.
- Replacing WGPU with another renderer on Android or iOS.
- Rewriting the GPUI scene graph, WGPU shader-effect model, or frame-family APIs.
- Rewriting RayX mobile app startup. Android/iOS app-wrapper work belongs to the existing mobile app builder epic unless renderer lifecycle changes require a narrow coordination fix.
- Treating screenshots or compile checks alone as runtime proof.

## Functional Requirements

### FR1: Windows Uses WGPU For Active Native Rendering

The default Windows native platform path must construct and use `gpui_wgpu::WgpuRenderer` for visible window rendering. `WindowsWindowState` must not own `DirectXRenderer` in the accepted end state.

### FR2: Windows Avoids DirectX Renderer Dependencies

Windows WGPU rendering must not require `DirectXDevices`, `DirectXRenderer`, or D3D-backed atlas state. If any DirectWrite or Windows text API remains necessary, it must not require DirectX GPU devices for the active WGPU route.

### FR3: Clean Platform API Boundary

The Windows platform must expose one clean renderer ownership model. Do not add long-lived proxy renderers, compatibility adapters, obsolete aliases, or hidden fallback paths that simulate a unified API while keeping DirectX as the real renderer.

### FR4: Explicit Windows WGPU Backend Policy

Windows backend selection must be visible in code and diagnostics. The initial non-DirectX policy is `VULKAN | GL` unless a decision record changes it. If WGPU DX12 is later enabled, it must be documented as WGPU-over-DX12, not the old checked-in DirectX renderer.

### FR5: Renderer Capabilities Stay Truthful

Windows, Android, and iOS must report effect capabilities from the active renderer. Non-WGPU or missing-renderer states must not report shader-effect support.

### FR6: Windows Lifecycle Parity

Windows WGPU windows must render the Lab app, resize, redraw, update DPI/scale, handle transparency/background settings, handle minimized or zero-sized surfaces, recover or fail clearly on device loss, and close without leaking renderer resources.

### FR7: Android WGPU Backend Preference Is Real

Android backend preference must either control the WGPU backend set used to create the renderer, or the preference API must be removed/replaced so callers are not promised behavior that is ignored.

### FR8: Android Surface Lifecycle Stays Stable

Android must continue to preserve renderer/atlas state across native window loss and recreation where the current design does so through unconfigure/replacement.

### FR9: iOS WGPU Capabilities Are Advertised Correctly

iOS must keep using a Metal `wgpu::Instance` for the `GPUIMetalView`/`CAMetalLayer` surface and report WGPU effect capabilities from its active renderer.

### FR10: Android And iOS Fully Support WGPU Shaders

Android and iOS must render every WGPU shader-effect capability exposed by `WgpuRenderer::effect_capabilities()`. This includes element effects, screen effects, and retained snapshot/transition effects. A missing Android or iOS shader path, disabled mobile capability, shader compilation failure, backend-specific bind-layout failure, blank output, or unproven advertised capability is a blocker, not a follow-up.

### FR11: Runtime Diagnostics Are Part Of The Work

The implementation must expose enough diagnostic state to prove active renderer kind, adapter/backend choice, surface configured/unconfigured state, present mode, and effect capability state on affected targets.

## Non-Functional Requirements

- Keep changes scoped to `crates/gpux` and the narrow validation/docs surfaces needed to prove behavior.
- Prefer existing GPUI/WGPU patterns from Linux, Web, Android, and iOS over new abstractions.
- Use the simplest clean end-state APIs; do not add abstraction layers unless they remove real cross-target duplication.
- Do not preserve obsolete DirectX renderer APIs as app-facing compatibility shims.
- Use the repo-local gpux skill before modifying gpux implementation code.
- Use the repo-local rayx diagnostics guidance before final validation of any RayX runtime proof.
- Use WASM target testing with headed Playwright and WebGPU enabled as the baseline validation route for development work: `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`.
- When runtime behavior is unclear, place temporary targeted traces in the code to inspect exact state. Remove trace-only code before completing the task instead of guessing from screenshots or symptoms.

## Acceptance Criteria

- `gpui_platform::current_platform(false)` on Windows opens the active Windows platform with WGPU-backed rendering.
- The accepted Windows window state owns `WgpuRenderer`, not `DirectXRenderer`.
- Windows text rendering works on the WGPU path without DirectX GPU device creation.
- Lab renders a nonblank frame on Windows native through the WGPU path and reports WGPU effect capabilities when the renderer supports them.
- Headed WASM/WebGPU validation passes for the Lab app during the implementation sprints.
- Windows native runtime validation proves startup, first frame, resize, redraw, transparency/background behavior where supported, and close lifecycle.
- Android renderer creation honors the selected backend policy or the misleading preference surface is removed/replaced.
- Android surface recreation still renders after a native window loss/recreation cycle when an emulator/device is available.
- iOS continues to construct WGPU through Metal and reports WGPU effect capabilities when the renderer is present.
- Android renders the full WGPU shader-effect suite it advertises, with logcat/runtime diagnostics proving no shader compilation, pipeline, bind-layout, surface, or blank-frame failure.
- iOS renders the full WGPU shader-effect suite it advertises over Metal, with runtime diagnostics proving no shader compilation, pipeline, bind-layout, surface, or blank-frame failure.
- `cargo fmt --all` passes.
- Focused checks/tests for changed gpux crates pass.
- `cargo clippy --workspace --all-features -- -D warnings` passes when feasible, or the exact blocker is documented.
- Temporary traces and auto-enabled debug hooks are absent from the final diff.
- Source inventory and traceability are updated to reflect implemented behavior.

## Assumptions

- Windows development and validation can run on this repository's Windows host.
- Android runtime validation requires `adb` and an emulator/device. If unavailable, implementation must document the exact blocker and still run compile/configuration checks, but Android shader-support tasks remain incomplete until runtime proof exists.
- iOS runtime validation requires macOS hardware, device/simulator access, or CI. From Windows, implementation must preserve source correctness and compile/configuration expectations, but iOS shader-support tasks remain incomplete until runtime proof exists.
- The WGPU native Windows backend can start with `VULKAN | GL`. Enabling WGPU DX12 is a separate policy decision because the user goal is to avoid the existing DirectX renderer.

## Risks And Edge Cases

- Windows DirectWrite code is currently tied to `DirectXDevices`; replacing it with a WGPU-compatible text system may expose font fallback, glyph rasterization, or emoji differences.
- Windows DirectComposition/transparency behavior in `DirectXRenderer` may not map one-to-one to WGPU surface configuration.
- `wgpu::Backends::VULKAN | GL` on Windows may have driver availability constraints on some machines; diagnostics must make adapter/backend failure obvious.
- Device-loss recovery differs between DirectX and WGPU and must be validated with explicit state traces when failures occur.
- Android backend preference may require a target-specific WGPU context factory rather than a global `WgpuContext::instance` change.
- iOS capability reporting may be missing only at the `PlatformWindow` boundary, not in the renderer itself; fix the narrow boundary rather than duplicating capability state.
