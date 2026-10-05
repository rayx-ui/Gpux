---
agentx:
  kind: knowledge
  scope: repo:tasks/shader-effects-and-screen-transitions
  status: current
  created: 2026-07-04T06:38:30+02:00
  updated: 2026-07-07T12:06:16+02:00
  topics:
    - shader-effects-and-screen-transitions
    - shader-effects
    - screen-transitions
    - implementation-plan
    - effects
  supersedes: []
  superseded_by: []
---

# Implementation Plan: Shader Effects And Screen Transitions

Final implementation note: early checklist items in this completed task refer to `div().effect(...)`, `div().effects(...)`, and broad `Styled` APIs. The accepted final architecture supersedes those checklist sketches with the frame-family API: use `effect_frame().effect(...)` / `effect_frame().effects(...)` for shader effects, `frame()` for lightweight geometry/clipping, `animated_frame(id)` for frame-property animation, and `transition_frame(id)` for snapshot-backed replacement transitions.

Do not spawn sub-agents unless the user explicitly asks for sub-agents, delegation, or parallel agent work. Where a group says it is parallelizable, use `multi_agent_v1.spawn_agent` with `agent_type: worker` only for the named owned scopes.

## Sprint 1: Contracts And Public API Skeleton

### Group 1: Effect Type Contracts

Dependency: none.

Parallelization: sequential.

- [x] Add `crates/gpux/crates/gpui/src/effects/` with `ElementEffect`, `BackdropBlurEffect`, `GlassEffect`, `GlowEffect`, `ScreenEffect`, `RippleEffect`, `ScreenEffectTarget`, and `ScreenEffectRegionId`. Owned scope: gpux effects module. Validation: focused unit tests for defaults, parameter clamping, and clean public constructors.
- [x] Export effect constructors such as `backdrop_blur`, `glass`, `glow`, and `screen::ripple` from gpux without temporary aliases. Owned scope: gpux crate exports/prelude. Validation: compile-only usage tests or doctests for the intended fluent API.
- [x] Add `IntoElementEffects` support for repeated `.effect(...)` and heterogeneous tuple `.effects((...))` usage. Owned scope: gpux effects module and styled API traits. Validation: unit tests for one, two, three, and array-style effect input where supported.

### Group 2: Styled Surface Integration

Dependency: Group 1.

Parallelization: sequential.

- [x] Add effect storage to `Style`/`StyleRefinement` using the existing style refinement pattern. Owned scope: `crates/gpux/crates/gpui/src/style.rs`. Validation: style accumulation and clear tests.
- [x] Add `Styled::effect`, `Styled::effects`, and `Styled::clear_effects` methods and wire `Div` through the existing `Styled for Div` implementation. Owned scope: gpux `Styled` trait and `elements/div.rs`. Validation: compile tests showing `div().effect(...).effect(...)`.
- [x] Add a visual-only `screen_effect_region(...)` style/element registration API if region registration belongs in the styled surface. Owned scope: gpux style/div paint metadata. Validation: unit test or paint test confirming region metadata does not create hitboxes or listeners.

### Group 3: Lab Story Scaffold

Dependency: Groups 1 and 2.

Parallelization: parallelizable with sub-agents because Lab story scaffolding is disjoint from renderer internals after public API contracts compile.

- [x] Add a Lab story route for shader effects with placeholder controls and static fallback content. Owned scope: `apps/lab/src` story catalog/shell files only. Validation: `cargo check --manifest-path apps/lab/Cargo.toml`.
- [x] Add minimal documentation comments on public effect constructors explaining WGPU-first support and capability reporting. Owned scope: gpux effects docs only. Validation: doc build/check if available, otherwise focused cargo check.

## Sprint 2: Scene And Frame Effect Plumbing

### Group 1: Scene Primitive Ordering

Dependency: Sprint 1.

Parallelization: sequential.

- [x] Add `VisualEffectPrimitive` and any needed `ScreenEffectRegionPrimitive` to `Scene`, `Primitive`, `PrimitiveKind`, `PrimitiveBatch`, `insert_primitive`, `finish`, `replay`, and batch iteration. Owned scope: `crates/gpux/crates/gpui/src/scene.rs`. Validation: scene ordering tests interleaving quads, shadows, effects, paths, and replay.
- [x] Update `Style::paint` to emit element effect primitives after shadows and before background, preserving background, children, and border order. Owned scope: `crates/gpux/crates/gpui/src/style.rs`. Validation: focused test or scene snapshot proving effect order relative to quads and borders.
- [x] Register screen-effect regions during paint with transformed bounds, content mask, corner radii where relevant, and stable ids. Owned scope: gpux style/window/frame metadata. Validation: tests for latest region bounds and missing-region behavior.

### Group 2: Window Active Screen Effects

Dependency: Group 1.

Parallelization: sequential.

- [x] Add `Window::play_screen_effect`, active effect storage, queue caps, and deterministic eviction. Owned scope: `crates/gpux/crates/gpui/src/window.rs` and effects module. Validation: unit tests for enqueue, cap, expiry, and no active animation after expiry.
- [x] Add browser-safe timing for active effects using existing gpux time/scheduler primitives rather than unsupported `std::time` clocks on WASM. Owned scope: gpux window/effects timing. Validation: wasm-target cargo check and timing unit tests where feasible.
- [x] Ensure active screen effects request animation frames only while needed. Owned scope: `Window` frame loop. Validation: tests or runtime diagnostics showing frame requests stop after effects expire.

### Group 3: Capability Diagnostics Contract

Dependency: Group 1.

Parallelization: sequential.

- [x] Add `EffectCapabilities` to gpux and expose renderer capability defaults. Owned scope: gpux effects capabilities and renderer trait boundary. Validation: unit tests for default unsupported capabilities.
- [x] Add DevKit/runtime-visible diagnostics fields for active effect counts, missing regions, and backend support if an existing diagnostics surface is available. Owned scope: gpux diagnostics integration only. Validation: diagnostic snapshot test or manual Lab diagnostics check.

## Sprint 3: WGPU Post-Process Foundation

### Group 1: Offscreen Scene Target

Dependency: Sprint 2.

Parallelization: sequential.

- [x] Add WGPU effect resource manager for scene color, ping-pong color, and resize/device-loss invalidation. Owned scope: `crates/gpux/crates/gpui_wgpu/src/wgpu_renderer.rs`. Validation: cargo check and resize-focused renderer tests if available.
- [x] Change WGPU scene encoding to render through offscreen color and copy/composite to the surface, preserving existing output with no effects. Owned scope: WGPU renderer render path. Validation: Lab WASM smoke with visual screenshot comparison/no blank frame.
- [x] Add a pass-through post-process pipeline and WGSL entry points before implementing blur/refraction. Owned scope: `wgpu_renderer.rs` and `shaders.wgsl`. Validation: layout assertions and pass-through Lab render proof.

### Group 2: Effect Batch Sequencing

Dependency: Group 1.

Parallelization: sequential.

- [x] Teach `encode_scene_to_view` to break render passes at visual effect batches, sample the previously rendered scene color, composite the effect, and continue later batches. Owned scope: WGPU renderer batch loop. Validation: test scene with colored content behind a no-op effect primitive.
- [x] Add packed WGPU structs for visual effects and assert Rust sizes/offsets against WGSL expectations. Owned scope: WGPU renderer packing module or adjacent code. Validation: unit tests or const assertions for every packed struct.
- [x] Add WGPU capability reporting for pass-through element and screen effects. Owned scope: WGPU renderer capabilities. Validation: diagnostics report WGPU support while other backends remain unsupported.

### Group 3: WASM Runtime Proof

Dependency: Groups 1 and 2.

Parallelization: sequential.

- [x] Run `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu` against the pass-through foundation. Owned scope: validation only. Validation: Playwright trace/screenshots show nonblank app and no WebGPU/page errors.
- [x] Record any shader validation failures and fix root causes before adding real effects. Owned scope: WGPU structs/shaders. Validation: clean WebGPU command submission and no Dawn layout errors.

## Sprint 4: Element Effects WGPU MVP

### Group 1: Glow Effect

Dependency: Sprint 3.

Parallelization: parallelizable with sub-agents because glow shader and pipeline work is disjoint from backdrop blur resources after the pass-through foundation is green.

- [x] Implement WGPU glow rendering around rounded element bounds with radius, spread, intensity, and color. Owned scope: WGPU effect shader/pipeline and `GlowEffect` packing. Validation: Lab story visual proof and pixel/screenshot check that glow extends beyond the base bounds without affecting hit testing.
- [x] Add tests for glow parameter packing, transparent color behavior, and clipping/content-mask behavior. Owned scope: gpux effects tests and WGPU layout tests. Validation: focused cargo tests.

### Group 2: Backdrop Blur

Dependency: Sprint 3.

Parallelization: parallelizable with sub-agents because backdrop blur resources and shaders are disjoint from glow work after the pass-through foundation is green.

- [x] Implement downsampled blur resources and shader passes for bounded backdrop blur. Owned scope: WGPU blur pipeline/resources/WGSL. Validation: Rust/WGSL layout assertions and Lab blur proof.
- [x] Composite blurred backdrop into element bounds with border-radius clipping and optional saturation/brightness/tint controls. Owned scope: WGPU effect composite shader and `BackdropBlurEffect` packing. Validation: Playwright screenshot/pixel check using patterned background behind blur.
- [x] Add quality presets or radius caps that keep mobile/WebGPU cost bounded. Owned scope: effect model and WGPU resource sizing. Validation: unit tests for cap behavior and runtime diagnostics for selected quality tier.

### Group 3: Glass Effect

Dependency: Group 2.

Parallelization: sequential.

- [x] Implement glass tint, highlight, refraction offset, and distortion/noise controls on top of backdrop blur. Owned scope: `GlassEffect`, WGPU packing, and WGSL composite. Validation: Lab story with moving patterned background proving refraction samples behind content.
- [x] Add fast/mobile quality defaults and high-quality opt-in values. Owned scope: effect defaults and docs. Validation: unit tests for defaults and WASM runtime proof for default path.
- [x] Ensure stacked effects render deterministically for blur, glass, and glow on one div. Owned scope: scene order and WGPU sequencing. Validation: Lab stack case and focused scene order tests.

## Sprint 5: Screen Effects And Click Ripple

### Group 1: Ripple Shader

Dependency: Sprint 3.

Parallelization: sequential.

- [x] Implement WGPU screen ripple distortion shader using origin, elapsed progress, duration, amplitude, radius, wavelength, damping, and optional chromatic/tint controls. Owned scope: WGPU screen effect pipeline/WGSL and `RippleEffect` packing. Validation: layout assertions and Lab visual proof.
- [x] Support multiple active ripples with deterministic composition and a maximum active count. Owned scope: window active effects and WGPU packing. Validation: unit tests for caps and Playwright click burst test.
- [x] Apply ripple to whole-window, explicit-bounds, and region targets. Owned scope: target resolution and WGPU scissor/mask logic. Validation: Lab region story proving only the target area distorts.

### Group 2: Input Integration Demo

Dependency: Group 1.

Parallelization: sequential.

- [x] Add click/touch handlers in the Lab story that call `window.play_screen_effect(screen::ripple(event.position))`. Owned scope: Lab shader effects story only. Validation: headed WASM Playwright click produces active ripple diagnostics and visible screenshot delta.
- [x] Add controls for ripple duration, amplitude, radius, and target mode without visible instructional text inside the app surface beyond normal labels. Owned scope: Lab story UI. Validation: manual and Playwright interaction checks.

### Group 3: Cross-Target Runtime Proof

Dependency: Groups 1 and 2.

Parallelization: sequential.

- [x] Run `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu` and capture ripple proof. Owned scope: validation only. Validation: WebGPU available, no page errors, visible ripple, and diagnostics show effect expiry.
- [x] Run Android gpui_mobile validation for the Lab app if the app target supports the new story on mobile; otherwise document the exact blocker and run the narrowest available mobile/WGPU check. Owned scope: validation only. Validation: `cargo run -p xtask -- app apps/lab run android` or documented blocker. Blocker recorded 2026-07-04: `adb` is not available on PATH, so device install/logcat runtime validation cannot be exercised from this environment.

## Sprint 6: RayX Navigation Transition Integration

### Group 1: Navigation Transition Model

Dependency: Sprint 5.

Parallelization: sequential.

- [x] Add `NavigationTransition`, transition target policy, and origin policy in the navigation module without adding retained route views to `NavigationService`. Owned scope: `crates/rayx/rayx_runtime/src/services/navigation/transition.rs` and module exports. Validation: unit tests for policy defaults and no service route-view storage.
- [x] Add transition configuration to `NavigationRouter`/`NavigationRegionView`; use a builder if needed to avoid transition-specific constructor proliferation. Owned scope: navigation router/region view files. Validation: tests that route subscriptions still update current routing data and enqueue effects only at the GPUI layer.
- [x] Paint configured navigation regions as gpux screen-effect regions so route transitions can target named region bounds. Owned scope: `NavigationRegionView` render path. Validation: region registration tests or Lab diagnostics.

### Group 2: Post-Navigation Ripple Transition

Dependency: Group 1.

Parallelization: sequential.

- [x] Enqueue a region-targeted ripple/reveal effect when `NavigationRouter` receives a route change for its region. Owned scope: navigation router and gpux screen effect call sites. Validation: navigation unit test with fake transition recorder plus Lab route-change runtime proof.
- [x] Add an explicit helper for `navigate_to` with a transition origin when app code has the pointer position. Owned scope: navigation GPUI extension/helper module. Validation: compile test and Lab sidebar click proof.
- [x] Keep reduced-motion or transition-disabled policy opt-in and testable. Owned scope: navigation transition model. Validation: unit tests proving no effect is enqueued when disabled.

### Group 3: Lab Navigation Story

Dependency: Group 2.

Parallelization: sequential.

- [x] Wire the Lab story shell or a dedicated demo region to use navigation transitions. Owned scope: `apps/lab/src/app/story_shell.rs` and shader effects story routes. Validation: `cargo check --manifest-path apps/lab/Cargo.toml`.
- [x] Add Playwright validation that route changes trigger a ripple transition without crashing or blanking the canvas. Owned scope: `tools-node` WASM tests if the existing test suite is the right surface. Validation: headed WebGPU test run.

## Sprint 7: Snapshot-Based Page Transitions

### Group 1: Snapshot Capture Foundation

Dependency: Sprint 6.

Parallelization: sequential.

- [x] Design and implement renderer-managed capture of a window or screen-effect region texture before navigation mutation, without requiring `NavigationService` to retain views. Owned scope: gpux window/renderer snapshot APIs. Validation: unit tests for snapshot lifecycle and WGPU proof of captured old content.
- [x] Add snapshot resource lifetime, resize invalidation, and capability reporting. Owned scope: gpux renderer resources and capabilities. Validation: tests for stale snapshot cleanup and diagnostics.
- [x] Add API support for explicit pre-navigation snapshot transitions through a GPUI helper around `NavigationService`. Owned scope: RayX navigation transition helper. Validation: compile test and route transition unit test.

### Group 2: Initial Snapshot Effects

Dependency: Group 1.

Parallelization: parallelizable with sub-agents because each effect shader owns a distinct transition variant after the snapshot foundation is available.

- [x] Implement `crossfade` and `blur_crossfade` transitions between old snapshot and new scene. Owned scope: WGPU snapshot transition shader variants. Validation: Lab route demo and Playwright screenshot sequence.
- [x] Implement `radial_reveal` and `ripple_reveal` transitions using pointer or center origin. Owned scope: WGPU transition shader variants. Validation: route click test and visual proof.
- [x] Add tests for fallback behavior when a snapshot is unavailable, ensuring diagnostics report why the transition downgraded. Owned scope: navigation transition model and gpux capabilities. Validation: focused Rust tests.

### Group 3: Transition Audit

Dependency: Group 2.

Parallelization: sequential.

- [x] Audit that snapshot transitions do not retain GPUI route entities in `NavigationService`. Owned scope: code review and navigation tests. Validation: service tests and source inspection.
- [x] Validate transition behavior on WASM with headed WebGPU and on every implemented native backend. Owned scope: validation only. Validation: `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu` plus target-specific run commands.

## Sprint 8: Backend Parity, Documentation, And Final Audit

### Group 1: Metal And DirectX Parity

Dependency: Sprints 4 through 7.

Parallelization: parallelizable with sub-agents because Metal and DirectX own different renderer crates, but coordinate through shared packed structs and capabilities.

- [x] Keep Metal renderer shader-effect capabilities unsupported until a native Metal shader path exists; do not claim fake parity. Owned scope: `crates/gpux/crates/gpui_macos/src/metal_renderer.rs` capability behavior. Validation: capability diagnostics remain false; macOS runtime proof is blocked on this Windows environment.
- [x] Keep DirectX renderer shader-effect capabilities unsupported until a native DirectX shader path exists; do not claim fake parity. Owned scope: `crates/gpux/crates/gpui_windows/src/directx_renderer.rs` capability behavior. Validation: Windows host Lab runtime starts without crash while unsupported effects remain capability-false.
- [x] Keep unsupported capabilities false for any backend/effect pair not implemented. Owned scope: renderer capability reporting. Validation: capability tests and diagnostics review.

### Group 2: Documentation And Examples

Dependency: Sprints 4 through 7.

Parallelization: parallelizable with sub-agents because documentation and example files are disjoint from backend renderer code; coordinate backend support wording before final merge.

- [x] Add gpux docs/examples for `div().effect(...)`, stacked `.effects((...))`, screen ripples, and region targeting. Owned scope: gpux docs/examples. Validation: doc examples compile where possible.
- [x] Add RayX navigation transition docs showing router/region integration and manual navigate-with-transition usage. Owned scope: RayX runtime navigation docs and Lab story docs. Validation: cargo check and doc review.
- [x] Update the narrowest relevant AgentX memory or RayX/gpux skill references only if implementation produces reusable guidance beyond this plan. Owned scope: `.agentx/memory` or `.codex/skills` only when justified. Validation: memory audit script if memory changes.

### Group 3: Final Validation Matrix

Dependency: Groups 1 and 2.

Parallelization: sequential.

- [x] Run `cargo fmt --all`. Owned scope: validation only. Validation: command succeeds.
- [x] Run focused gpux and RayX navigation tests added by the epic. Owned scope: validation only. Validation: command succeeds.
- [x] Run `cargo check --manifest-path apps/lab/Cargo.toml` and the available WASM check path. Owned scope: validation only. Validation: Lab cargo check succeeds; direct `--target wasm32-unknown-unknown` check is blocked on stable by `wasm_thread` requiring `stdarch_wasm_atomic_wait`, so the repository xtask WASM flow is the authoritative browser check.
- [x] Run `cargo clippy --workspace --all-features -- -D warnings` when feasible, or document blockers. Owned scope: validation only. Validation: command succeeds or blocker is explicit.
- [x] Run `cargo run -p xtask -- app apps/lab run host` for native runtime proof on implemented desktop backend(s). Owned scope: validation only. Validation: no runtime crash and effect diagnostics match capabilities.
- [x] Run `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`. Owned scope: validation only. Validation: Playwright WebGPU proof, screenshots/traces, no page errors, visible effects.
- [x] Run Android validation through `cargo run -p xtask -- app apps/lab run android` when mobile effect behavior is enabled, or document the exact environment blocker. Owned scope: validation only. Validation: blocked because `adb` is not available on PATH; `cargo-ndk` and Android Rust targets are installed.
- [x] Audit every requirement in `requirements.md` against implemented behavior and update task state only after all accepted requirements are proven. Owned scope: task closeout. Validation: WGPU/WASM requirements are proven; native non-WGPU backends remain capability-false rather than claiming unsupported shader behavior.
