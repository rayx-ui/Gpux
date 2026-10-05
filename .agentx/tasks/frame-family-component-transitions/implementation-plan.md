---
agentx:
  kind: knowledge
  scope: repo:tasks/frame-family-component-transitions
  status: current
  created: 2026-07-07T00:00:00+02:00
  updated: 2026-07-07T00:00:00+02:00
  topics:
    - frame-family
    - effect-frame
    - animated-frame
    - transition-frame
    - implementation-plan
  supersedes: []
  superseded_by: []
---

# Implementation Plan: Frame Family And Component Transitions

## Sprint 1: Effect Frame Rename

### Group 1: Source Surface Audit

Dependency: none

Parallelization: sequential

- [x] Inspect current `frame()`, `effects()`, `FrameStyle`, `shader_effects`, scene, WGPU renderer, navigation region view, and Lab shader effect story immediately before editing. Owned scope: `crates/gpux/crates/gpui/src`, `crates/gpux/crates/gpui_wgpu/src`, `crates/rayx/rayx_runtime/src/services/navigation`, `apps/lab/src/stories`. Validation: repository search documents every public `effects()` call site.
- [x] Identify all public exports and prelude paths for `effects()` and `Effects`. Owned scope: `crates/gpux/crates/gpui/src/gpui.rs`, `crates/gpux/crates/gpui/src/prelude.rs`, `crates/gpux/crates/gpui/src/elements/mod.rs`. Validation: targeted `rg "effects\\(|Effects"` before and after rename.

### Group 2: Rename Element API

Dependency: depends on Group 1

Parallelization: sequential

- [x] Rename `crates/gpux/crates/gpui/src/elements/effects.rs` to `effect_frame.rs`, rename `Effects` to `EffectFrame`, and rename the constructor to `effect_frame()`. Owned scope: GPUI element module. Validation: targeted gpui compile/test for element construction.
- [x] Update GPUI exports, prelude exports, examples, and tests to use `effect_frame()`. Owned scope: GPUI public exports and tests. Validation: `rg "effects\\(|Effects"` finds no public API uses except historical task docs or intentional comments.
- [x] Migrate RayX navigation region rendering and Lab shader effects story from `effects()` to `effect_frame()`. Owned scope: `crates/rayx/rayx_runtime/src/services/navigation`, `apps/lab/src/stories/shader_effects_story.rs`. Validation: navigation tests and Lab route compile checks.
- [x] Preserve effect rendering behavior, including ordered effects, screen effect regions, rounded clipping, and additive WGPU glow composition. Owned scope: `EffectFrame` paint path and existing renderer tests. Validation: targeted `gpui_wgpu` tests including glow blend assertion.

### Group 3: Remove Old Surface

Dependency: depends on Group 2

Parallelization: sequential

- [x] Remove any leftover `effects()` constructor or `Effects` type alias. Owned scope: GPUI exports and docs. Validation: compile-fail-by-absence through no symbol exports and repository search.
- [x] Update docs and examples that teach shader effects to use `effect_frame()`. Owned scope: `crates/gpux/crates/gpui/examples/README.md`, RayX docs if present, Lab strings. Validation: repository search no longer suggests `effects()` as current public API.

## Sprint 2: Animated Frame

### Group 1: Animation Model

Dependency: depends on Sprint 1

Parallelization: sequential

- [x] Design and add `FrameAnimation` or reuse/refactor existing `Animation` so frame-family animation supports duration, one-shot behavior, and easing without duplicating timing concepts. Owned scope: `crates/gpux/crates/gpui/src/elements/animation.rs` and/or new frame animation module. Validation: unit tests for duration clamping/easing progress.
- [x] Add interpolation helpers for compatible `FrameStyle` fields and explicit snap behavior for incompatible values. Owned scope: `frame_style.rs` or a new private helper module. Validation: unit tests for px-to-px interpolation, opacity, transform, corner radii, and auto/relative snap cases.

### Group 2: AnimatedFrame Element

Dependency: depends on Group 1

Parallelization: sequential

- [x] Add `crates/gpux/crates/gpui/src/elements/animated_frame.rs` with `animated_frame(id)` and `AnimatedFrame`. Owned scope: new GPUI element module and exports. Validation: constructor and basic layout tests.
- [x] Implement stable element state tracking for previous, target, current, target key, start time, duration, and completion. Owned scope: `AnimatedFrame` element lifecycle. Validation: tests using simulated time or test app frame advancement.
- [x] Implement frame-owned property interpolation during `request_layout` and paint. Owned scope: `AnimatedFrame` layout/prepaint/paint. Validation: tests assert intermediate style values and final target values.
- [x] Implement interrupted animation handling by starting new transitions from the current interpolated style. Owned scope: `AnimatedFrameState`. Validation: test toggling target before completion.
- [x] Implement `.child(...)` and one progress-aware child source such as `.child_with_progress(...)`, with debug/test rejection when both are set. Owned scope: `AnimatedFrame` child model. Validation: tests for static child, progress child, and double-child assertion.

### Group 3: AnimatedFrame API Cleanup

Dependency: depends on Group 2

Parallelization: sequential

- [x] Export `animated_frame()` and `AnimatedFrame` through the same GPUI surfaces as `frame()`. Owned scope: `gpui.rs`, prelude, element module. Validation: downstream compile check from Lab or a focused example.
- [x] Add documentation examples showing expanded-to-compact state animation with geometry and child progress. Owned scope: GPUI examples/readme. Validation: docs compile or repository search confirms current names.

## Sprint 3: Transition Frame

### Group 1: Transition Model

Dependency: depends on Sprint 1

Parallelization: sequential

- [x] Add a GPUI-owned transition configuration type such as `FrameTransition` plus `FrameTransitionKind`, independent of `rayx_runtime::NavigationTransition`. Owned scope: `crates/gpux/crates/gpui/src/shader_effects` or a new element-local module. Validation: unit tests for defaults, duration clamping, and snapshot-kind mapping.
- [x] Map `FrameTransition` variants to existing `screen::crossfade`, `screen::blur_crossfade`, `screen::radial_reveal`, and `screen::ripple_reveal` builders. Owned scope: GPUI shader/screen effect model. Validation: tests assert resolved `ScreenEffect` kind and target.

### Group 2: TransitionFrame Element

Dependency: depends on Group 1

Parallelization: sequential

- [x] Add `crates/gpux/crates/gpui/src/elements/transition_frame.rs` with `transition_frame(id)` and `TransitionFrame`. Owned scope: new GPUI element module and exports. Validation: constructor and single-child tests.
- [x] Implement target key tracking so first render skips transition and later key changes trigger replacement transition. Owned scope: `TransitionFrameState`. Validation: tests for first render, unchanged key, and changed key.
- [x] Register a stable screen effect region derived from the frame id and allow explicit region override only if necessary. Owned scope: `TransitionFrame` paint path. Validation: scene tests assert one region with expected id/bounds/radius.
- [x] Capture retained snapshots with `Window::capture_screen_effect_snapshot(ScreenEffectTarget::region(...))` when the target key changes. Owned scope: `TransitionFrame` lifecycle. Validation: capability-gated implementation with fallback test coverage.
- [x] Play the configured snapshot transition targeted to the frame region. Owned scope: `TransitionFrame` lifecycle and screen effect resolution. Validation: `FrameTransition` tests assert resolved screen effect kind and target.
- [x] Release retained snapshots after transition duration plus a safety margin using element state and animation-frame ticks, without requiring a GPUI view `Context`. Owned scope: `TransitionFrameState`. Validation: release is tied to retained state and animation-frame ticks.
- [x] Degrade cleanly when snapshot transition capabilities are absent by painting the target child and not claiming unsupported renderer behavior. Owned scope: capability checks or existing renderer handling. Validation: capability/unit test and native no-crash check where feasible.

### Group 3: TransitionFrame Exports And Docs

Dependency: depends on Group 2

Parallelization: sequential

- [x] Export `transition_frame()`, `TransitionFrame`, and transition configuration types through GPUI public surfaces. Owned scope: `gpui.rs`, prelude, docs. Validation: downstream compile check.
- [x] Document that `transition_frame()` is image-space replacement transition, not automatic shared-element/tree-diff morphing. Owned scope: GPUI examples/readme. Validation: docs review.

## Sprint 4: Lab Example Pages

### Group 1: AnimatedFrame Story

Dependency: depends on Sprint 2

Parallelization: sequential

- [x] Add `apps/lab/src/stories/animated_frame_story.rs` with a compact/expanded card example using `animated_frame()`. Owned scope: new Lab story. Validation: story compiles and renders in native/WASM Lab.
- [x] Demonstrate frame-owned animation for height, radius, opacity, and transform, plus child-owned font or label sizing through the progress-aware child source. Owned scope: story UI. Validation: visual/manual route check and Playwright screenshot nonblank check.
- [x] Register `AnimatedFrameStory` in `apps/lab/src/stories/mod.rs` and `apps/lab/src/app/story_routes.rs` at `/stories/animated-frame`. Owned scope: Lab routing. Validation: route registration unit test.
- [x] Add English and Croatian locale metadata for `AnimatedFrameStory`, updating generated asset manifests if required. Owned scope: `apps/lab/assets/locales`, asset manifests. Validation: locale metadata test.

### Group 2: TransitionFrame Story

Dependency: depends on Sprint 3

Parallelization: sequential

- [x] Add `apps/lab/src/stories/transition_frame_story.rs` with two visually distinct child components toggled by a control through `transition_frame()`. Owned scope: new Lab story. Validation: story compiles and renders in native/WASM Lab.
- [x] Demonstrate at least crossfade and one reveal/blur transition variant when the API supports variants. Owned scope: story UI. Validation: visual/manual route check and Playwright screenshot nonblank check.
- [x] Register `TransitionFrameStory` in `apps/lab/src/stories/mod.rs` and `apps/lab/src/app/story_routes.rs` at `/stories/transition-frame`. Owned scope: Lab routing. Validation: route registration unit test.
- [x] Add English and Croatian locale metadata for `TransitionFrameStory`, updating generated asset manifests if required. Owned scope: `apps/lab/assets/locales`, asset manifests. Validation: locale metadata test.

### Group 3: Browser Validation Coverage

Dependency: depends on Groups 1 and 2

Parallelization: sequential

- [x] Extend the Lab WASM Playwright coverage to visit `/stories/animated-frame` and `/stories/transition-frame`, click their controls, and assert the WebGPU canvas remains nonblank. Owned scope: `tools-node/tests/rayx-wasm.spec.ts` or existing WASM validation route list. Validation: headed WebGPU test command.
- [x] Add lightweight diagnostics hooks only if needed for reliable validation, and remove temporary trace-only hooks before completion. Owned scope: Lab stories/tests. Validation: clean diff without temporary diagnostics.

## Sprint 5: Validation, Documentation, And Memory

### Group 1: Focused Validation

Dependency: depends on Sprint 4

Parallelization: sequential

- [x] Run `cargo fmt --all`. Owned scope: repository formatting. Validation: command passes.
- [x] Run targeted gpui tests for `frame`, `effect_frame`, `animated_frame`, `transition_frame`, content masks, screen effect regions, and snapshot release. Owned scope: gpux workspace tests. Validation: targeted test command passes.
- [x] Run targeted `gpui_wgpu` tests for shader composition and snapshot transition packing. Owned scope: WGPU renderer tests. Validation: targeted test command passes.
- [x] Run targeted `rayx_runtime` navigation tests to ensure the `effect_frame()` rename did not regress router/region transition behavior. Owned scope: RayX navigation tests. Validation: targeted test command passes.
- [x] Run `cargo check --manifest-path apps/lab/Cargo.toml`. Owned scope: Lab app compile. Validation: command passes or exact unrelated blocker recorded.
- [x] Run `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`. Owned scope: browser/WASM runtime validation. Validation: command passes or exact blocker recorded.

### Group 2: Documentation

Dependency: depends on Group 1

Parallelization: sequential

- [x] Update public docs/examples to teach the frame-family API: `frame`, `effect_frame`, `animated_frame`, and `transition_frame`. Owned scope: GPUI examples and RayX docs. Validation: repository search finds no current docs teaching `effects()`.
- [x] Update migration notes or task docs to call out the breaking `effects()` to `effect_frame()` rename. Owned scope: docs/task notes. Validation: docs review.

### Group 3: AgentX And Skill Knowledge

Dependency: depends on Group 2

Parallelization: sequential

- [x] Update AgentX memory after implementation with current behavior only: `effects()` must not be described as current once removed. Owned scope: `.agentx/memory/knowledge/runtime/gpux-foundation`. Validation: AgentX memory audit.
- [x] Add or update a gpux/RayX skill reference if implementation yields reusable DO/DON'T guidance for frame-family APIs or transition snapshots. Owned scope: `.codex/skills/gpux` and/or `.codex/skills/rayx`. Validation: examples in skill guidance match current source APIs.
- [x] Record validation notes and mark this epic complete only after all acceptance criteria and runtime checks are satisfied. Owned scope: this task state and plan. Validation: `state.json.totalSprints` remains correct and task checkboxes reflect actual completion.

## Validation Notes

- `cargo fmt --all`
- `cargo test --manifest-path crates/gpux/Cargo.toml -p gpux frame_`
- `cargo test --manifest-path crates/gpux/Cargo.toml -p gpux content_mask`
- `cargo test --manifest-path crates/gpux/Cargo.toml -p gpux capture_screen_effect_snapshot_retains_target_until_released`
- `cargo test --manifest-path crates/gpux/Cargo.toml -p gpux screen_effects_expire_and_rendered_scene_snapshot_clears`
- `cargo test --manifest-path crates/gpux/Cargo.toml -p gpui_wgpu effect`
- `cargo test --manifest-path crates/gpux/Cargo.toml -p gpui_wgpu snapshot_transition_packing_requires_captured_snapshot`
- `cargo test --manifest-path crates/gpux/Cargo.toml -p gpui_wgpu glow_blend_state_adds_color_and_preserves_alpha`
- `cargo test -p rayx_runtime navigation_region_view_paints_configured_screen_effect_region`
- `cargo check --manifest-path apps/lab/Cargo.toml`
- `cargo test --manifest-path apps/lab/Cargo.toml frame_family_story_routes_are_registered_with_locale_metadata`
- `cargo run -p xtask -- app apps/lab assets validate`
- `pnpm exec tsc --noEmit` from `tools-node/`
- `python agentx\scripts\audit_agentx_memory.py`
- `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`

The first headed WASM/WebGPU run exposed a missed in-canvas click target in the new frame-family Playwright route coverage; retained Playwright screenshots showed the controls were lower than the clicked y-coordinate. The test click points were corrected to the actual button centers, and the rerun passed all 20 headed WebGPU tests across normal DPR and DPR2 projects.
