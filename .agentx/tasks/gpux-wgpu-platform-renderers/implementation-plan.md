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
    - implementation-plan
  supersedes: []
  superseded_by: []
---

# Implementation Plan: GPUI WGPU Platform Renderers

Do not spawn sub-agents unless the user explicitly asks for sub-agents, delegation, or parallel agent work. Where a group says it is parallelizable, use `multi_agent_v1.spawn_agent` with `agent_type: worker` only for the named owned scopes.

## Global Implementation Rules

- Use the repo-local gpux skill before modifying `crates/gpux`.
- During development, use the WASM target for baseline testing with headed Playwright and WebGPU enabled: `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`.
- When an issue appears, add temporary targeted traces to inspect exact runtime state instead of guessing from screenshots or symptoms. Remove trace-only code before completing the task.
- For every affected runtime target, run target-specific proof before marking work complete: Windows native for Windows renderer changes, Android for Android renderer changes, and iOS proof through macOS/device/simulator/CI tooling for iOS renderer changes.
- Android and iOS must have 100% WGPU shader-effect support for every capability advertised by `WgpuRenderer`; a missing or unproven mobile shader effect blocks completion.
- Keep renderer capabilities truthful. Do not make non-WGPU or inactive renderer states report WGPU shader-effect support.
- Do not keep compatibility shims, aliases, proxy renderers, or hidden fallback paths for the old Windows DirectX renderer.

## Sprint 1: Windows WGPU First Frame

### Group 1: Baseline Evidence And Source Map

Dependency: none.

Parallelization: sequential.

- [x] Re-read the touched renderer surfaces and update `source-inventory.md` if current source has drifted. Owned scope: AgentX task artifacts only. Validation: source inventory links every touched renderer/platform file to at least one requirement.
- [x] Run the baseline WASM headed WebGPU validation before code changes: `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`. Owned scope: validation only. Validation: command succeeds or exact pre-existing blocker is recorded before implementation begins.
- [x] Run a Windows native Lab/host startup baseline and record the current active renderer diagnostics or source-confirmed DirectX state. Owned scope: validation only. Validation: baseline evidence distinguishes current DirectX path from the target WGPU path.

### Group 2: Windows WGPU Construction

Dependency: Group 1.

Parallelization: sequential.

- [x] Add `gpui_wgpu` as a Windows platform dependency without changing app-facing platform APIs. Owned scope: `crates/gpux/crates/gpui_windows/Cargo.toml`. Validation: focused `cargo check` for `gpui_windows`.
- [x] Add a focused Win32 raw-window wrapper for WGPU surface creation using the existing HWND/display handle data and satisfying `Clone + Send + Sync + 'static`. Owned scope: `crates/gpux/crates/gpui_windows/src/window.rs` or a small adjacent module. Validation: compile-time trait bounds prove the wrapper can be passed to `WgpuRenderer::new`.
- [x] Construct `WgpuRenderer` for Windows windows with explicit WGPU surface configuration and backend diagnostics. Owned scope: `gpui_windows` platform/window creation. Validation: Windows native Lab/host opens a nonblank first frame through WGPU.

### Group 3: First-Frame Diagnostics

Dependency: Group 2.

Parallelization: sequential.

- [x] Add temporary traces for renderer kind, backend set, selected adapter, surface size, and present mode while bringing up first frame. Owned scope: temporary diagnostics around Windows WGPU construction. Validation: traces identify WGPU renderer state during first-frame validation.
- [x] Promote only useful stable fields into existing diagnostics if they belong there; remove all trace-only code. Owned scope: diagnostics surfaces only. Validation: final diff has no auto-enabled temporary traces.
- [x] Re-run `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu` after the first Windows WGPU construction change. Owned scope: validation only. Validation: WASM shared WGPU path remains green.

## Sprint 2: Windows Text, Atlas, And Capabilities

### Group 1: WGPU-Compatible Text Path

Dependency: Sprint 1.

Parallelization: sequential.

- [x] Replace the active Windows DirectX-backed text system with a WGPU-compatible text path, expected to be `CosmicTextSystem` unless current source proves a cleaner alternative. Owned scope: `gpui_windows` platform text-system initialization and imports. Validation: Windows native Lab renders text on WGPU without creating `DirectXDevices`.
- [x] Verify font fallback, emoji fallback where supported, high-DPI text scale, and text input surfaces on Windows WGPU. Owned scope: Windows runtime validation and any narrow text fixes. Validation: diagnostics or runtime assertions prove text renders at expected scale.
- [x] Remove DirectX GPU device requirements from the active Windows WGPU route. Owned scope: `gpui_windows` platform state and creation info. Validation: source search shows the active WGPU route does not instantiate `DirectXDevices`.

### Group 2: Capability Reporting

Dependency: Sprint 1.

Parallelization: sequential.

- [x] Make Windows `PlatformWindow::effect_capabilities()` delegate to `WgpuRenderer::effect_capabilities()` when the WGPU renderer is active. Owned scope: `gpui_windows` window implementation. Validation: DevKit/runtime diagnostics report WGPU effect support on Windows WGPU.
- [x] Ensure headless or missing-renderer Windows states report unsupported capabilities. Owned scope: `gpui_windows` platform/window capability paths. Validation: focused unit/compile test or source-level assertion for headless behavior.
- [x] Confirm DirectX and other non-WGPU states do not report fake shader-effect support. Owned scope: capability defaults and tests. Validation: existing capability tests plus focused Windows checks.

### Group 3: Text And Effect Runtime Proof

Dependency: Groups 1 and 2.

Parallelization: sequential.

- [x] Add or use a Lab story that visibly exercises text plus a WGPU shader/effect capability probe on Windows. Owned scope: Lab validation surface only if existing stories are insufficient. Validation: Windows native runtime proof shows text and effect-capability diagnostics.
- [x] Run headed WASM/WebGPU validation after text/capability changes. Owned scope: validation only. Validation: WebGPU run passes without shader/device errors.
- [x] Record exact blockers if Windows WGPU text behavior differs from DirectWrite and requires a follow-up decision. Owned scope: task evidence and decision prep only. Validation: blocker includes source file, runtime symptom, and proposed narrow decision.

## Sprint 3: Windows Lifecycle Parity And DirectX Removal

### Group 1: Window Lifecycle Parity

Dependency: Sprint 2.

Parallelization: sequential.

- [x] Wire resize, scale factor, redraw, minimized or zero-sized surface handling, and close/drop behavior through `WgpuRenderer` using existing Linux/mobile patterns as required references where they match Windows source constraints. Owned scope: `gpui_windows` window lifecycle. Validation: Windows native runtime proof covers resize, minimize/restore, and clean close, or records an exact OS/tooling blocker for any lifecycle action that cannot be triggered locally.
- [x] Wire transparency/background updates through the WGPU renderer path or explicitly document unsupported Windows transparency behavior with a narrow follow-up decision. Owned scope: `gpui_windows` renderer/window settings. Validation: runtime proof or documented target-specific blocker.
- [x] Wire device-loss detection/recovery through WGPU recovery APIs and add temporary traces around recovery state while validating. Owned scope: `gpui_windows` WGPU device lifecycle. Validation: targeted diagnostics prove recovery path is reachable or failures are clear and non-silent.

### Group 2: Renderer Surface Completeness

Dependency: Group 1.

Parallelization: sequential.

- [x] Preserve screenshot/render-to-image behavior if current Windows APIs expose it; otherwise document the precise unsupported surface. Owned scope: `gpui_windows` render-to-image/screenshot methods. Validation: focused check or runtime capture proof.
- [x] Preserve input, cursor, IME/text input, mouse, keyboard, and window actions while replacing the renderer. Owned scope: `gpui_windows` event/window code adjacent to renderer ownership. Validation: Windows native interaction smoke plus existing input tests/checks.
- [x] Keep renderer diagnostics truthful across startup, resize, device-loss, and close. Owned scope: diagnostics surfaces. Validation: DevKit/runtime diagnostics match actual WGPU lifecycle state.

### Group 3: Remove Active DirectX Renderer Path

Dependency: Groups 1 and 2.

Parallelization: sequential.

- [x] Remove `DirectXRenderer` ownership from `WindowsWindowState` and update all active call sites to the WGPU renderer path. Owned scope: `crates/gpux/crates/gpui_windows/src/window.rs` and adjacent platform state. Validation: source search shows no active `DirectXRenderer` window ownership.
- [x] Delete or decommission DirectX renderer/device/atlas code that has no remaining active callers, keeping only clearly named Windows OS integration that is still required. Owned scope: `gpui_windows` DirectX modules/imports/Cargo dependencies. Validation: `rg` confirms obsolete renderer modules are unused or removed.
- [x] Run `cargo fmt --all`, focused gpux checks/tests, Windows native runtime proof, and headed WASM/WebGPU validation. Owned scope: validation only. Validation: commands succeed or exact blockers are recorded with root-cause notes.

## Sprint 4: Android And iOS WGPU Alignment

### Group 1: Android Backend Policy

Dependency: Sprint 1.

Parallelization: parallelizable with sub-agents because Android backend policy is isolated from iOS capability reporting after Windows WGPU construction exists.

- [x] Route `AndroidBackend::{Vulkan,Gles}` preference into WGPU instance/backend selection, or replace the preference API with a clean truthful surface. Owned scope: `crates/gpux/crates/gpui_mobile/src/android/platform.rs`, `android/window.rs`, and any narrow WGPU context factory needed. Validation: Android diagnostics report the selected backend policy and source proves it is used.
- [x] Preserve Android `Mailbox` present mode and surface unconfigure/replacement behavior across `term_window` and `init_window`. Owned scope: Android window lifecycle. Validation: Android runtime proof or exact `adb`/device blocker plus compile/configuration checks.
- [x] Add targeted temporary traces around Android native window lifecycle only when diagnosing failures, then remove them. Owned scope: temporary Android diagnostics. Validation: final diff has no trace-only hooks.
- [x] Prove Android renders every WGPU shader-effect capability advertised by `WgpuRenderer`, including element effects, screen effects, and retained snapshot/transition effects. Owned scope: Android WGPU renderer validation and any Android-specific WGPU fixes. Validation: Android runtime proof shows nonblank effect output and no shader compilation, pipeline, bind-layout, surface, or logcat renderer errors.

### Group 2: iOS WGPU Capability Reporting

Dependency: Sprint 1.

Parallelization: parallelizable with sub-agents because iOS capability reporting is isolated from Android backend policy.

- [x] Make iOS platform-window capability reporting delegate to `WgpuRenderer::effect_capabilities()` when the renderer exists. Owned scope: `crates/gpux/crates/gpui_mobile/src/ios/window.rs`. Validation: compile/source proof plus runtime proof through macOS/device/simulator/CI tooling.
- [x] Preserve explicit `wgpu::Backends::METAL` instance creation for `GPUIMetalView`/`CAMetalLayer`. Owned scope: iOS WGPU initialization. Validation: source assertion and compile/configuration check.
- [x] Confirm iOS CoreText text system remains independent from the Windows text-system migration. Owned scope: source audit only unless breakage is found. Validation: source inventory/traceability updated.
- [ ] Prove iOS renders every WGPU shader-effect capability advertised by `WgpuRenderer`, including element effects, screen effects, and retained snapshot/transition effects. Owned scope: iOS WGPU renderer validation and any Metal/WGPU fixes. Validation: iOS runtime proof shows nonblank effect output and no shader compilation, pipeline, bind-layout, surface, or renderer errors.

### Group 3: Mobile Runtime Proof

Dependency: Groups 1 and 2.

Parallelization: sequential.

- [ ] Run Android validation through the repository `xtask` path plus `adb`/`logcat`. Owned scope: validation only. Validation: app launches, first frame renders, backend diagnostics appear, the full WGPU shader-effect suite renders, and no renderer crash occurs.
- [ ] Run iOS validation through macOS/device/simulator/CI tooling. Owned scope: validation only. Validation: app launches, first frame renders, Metal/WGPU diagnostics appear, the full WGPU shader-effect suite renders, and no renderer crash occurs.
- [ ] Run headed WASM/WebGPU validation after mobile changes to protect the shared WGPU renderer path. Owned scope: validation only. Validation: WebGPU run remains green.

## Sprint 5: Final Validation, Docs, And Durable Memory

### Group 1: Broad Validation Matrix

Dependency: Sprints 1 through 4.

Parallelization: sequential.

- [ ] Run `cargo fmt --all`. Owned scope: validation only. Validation: command succeeds.
- [ ] Run focused checks/tests for changed gpux crates and affected app manifests. Owned scope: validation only. Validation: commands succeed or exact blockers are recorded.
- [ ] Run `cargo clippy --workspace --all-features -- -D warnings`. Owned scope: validation only. Validation: command succeeds or an exact pre-existing blocker is documented.
- [ ] Run `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`. Owned scope: validation only. Validation: headed Playwright/WebGPU passes with no page/WebGPU errors.
- [ ] Run Windows native Lab/host runtime proof and inspect diagnostics for WGPU renderer state. Owned scope: validation only. Validation: first frame, resize, text, effect capabilities, and clean close are proven.

### Group 2: Documentation And Traceability

Dependency: Sprints 1 through 4.

Parallelization: sequential.

- [ ] Update `source-inventory.md` and `traceability.md` with implemented source links, validation commands, and any accepted blockers. Owned scope: AgentX task artifacts. Validation: every requirement maps to implementation and proof.
- [ ] Update any gpux/RayX docs that still claim Windows native rendering is DirectX-only after the WGPU migration lands. Owned scope: docs only. Validation: `rg` over docs/tasks finds no stale Windows renderer claims.
- [ ] Record a decision if backend policy changes beyond the planned Windows `VULKAN | GL` default or Android preference wiring. Owned scope: `.agentx/memory/decisions/runtime/gpux-foundation/`. Validation: decision follows AgentX format and is linked from traceability.

### Group 3: Cleanup And Memory

Dependency: Groups 1 and 2.

Parallelization: sequential.

- [ ] Remove temporary traces, debug flags, local validation artifacts, and unused renderer imports/modules. Owned scope: cleanup only. Validation: `rg` confirms no trace-only markers remain and git status contains no generated artifacts.
- [ ] Update `.agentx/memory/knowledge/runtime/gpux-foundation/state.md`, `knowledge.md`, `rules.md`, or `hypotheses.md` only with proven durable behavior from the completed implementation. Owned scope: AgentX memory. Validation: memory statements cite source/validation facts and do not overclaim untested targets.
- [ ] Run the AgentX task audit and repair any contract/state/traceability issues before closeout. Owned scope: AgentX task artifacts. Validation: audit exits successfully.
