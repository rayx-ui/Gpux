---
agentx:
  kind: knowledge
  scope: repo:tasks/frame-effects-navigation-transitions
  status: current
  created: 2026-07-06T00:00:00+02:00
  updated: 2026-07-06T15:30:00+02:00
  topics:
    - frame
    - effects
    - navigation-transitions
    - implementation-plan
  supersedes: []
  superseded_by: []
---

# Frame, Effects, And Navigation Transition Defaults Implementation Plan

## Sprint 1: Lightweight Frame Element

### Group 1.1: Source Surface Preparation

Dependency: none

Parallelization: sequential

Tasks:

- [x] Inspect the current `DivElement`, `Style`, `Styled`, scene, and renderer clip paths immediately before editing to catch branch drift.
- [x] Rename the current GPUI shader effect model module from `effects` to an explicit name such as `shader_effects` or `visual_effects` if needed to free the public `effects()` constructor.
- [x] Update internal imports and exports after the module rename.
- [x] Keep public effect model type names stable only where they are still part of the new `effects()` API.

### Group 1.2: Frame Geometry Core

Dependency: Group 1.1

Parallelization: sequential

Tasks:

- [x] Add a minimal `FrameStyle` or equivalent private geometry struct.
- [x] Implement size helpers used by the repository's existing GPUI style conventions.
- [x] Implement position helpers.
- [x] Implement inset helpers.
- [x] Implement transform helpers.
- [x] Implement opacity helpers.
- [x] Implement radius helpers.
- [x] Implement clipping helpers.
- [x] Exclude flex, grid, text, background, border, shadow, scrolling, interactivity, and event APIs.

### Group 1.3: Frame Element

Dependency: Group 1.2

Parallelization: sequential

Tasks:

- [x] Add `crates/gpux/crates/gpui/src/elements/frame.rs`.
- [x] Implement `Frame` as a normal GPUI element using `DivElement` only as a lifecycle/layout reference.
- [x] Add `gpui::frame()` constructor.
- [x] Implement `.child(...)` with exactly one child slot.
- [x] Reject repeated `.child(...)` calls in debug/test builds.
- [x] Do not implement `ParentElement`.
- [x] Do not implement broad `Styled`.
- [x] Add focused tests for construction, one-child behavior, and basic layout participation.

## Sprint 2: Rounded Clip Infrastructure

### Group 2.1: Clip Model

Dependency: Sprint 1

Parallelization: sequential

Tasks:

- [x] Audit current `ContentMask` usage in scene building, window painting, and renderers.
- [x] Introduce a rounded clip representation or extend the existing content mask with corner radii.
- [x] Preserve existing rectangular behavior for zero-radius clips.
- [x] Add clip intersection rules for nested clips.
- [x] Thread active clip data through scene primitives that can be produced by child subtrees.

### Group 2.2: Renderer Support

Dependency: Group 2.1

Parallelization: sequential

Tasks:

- [x] Update WGPU primitive packing to include rounded clip metadata where needed.
- [x] Update WGPU shader logic to discard pixels outside the active rounded clip.
- [x] Verify visual effects respect the active rounded clip.
- [x] Ensure rounded clipping applies consistently to both the normal visual-effect pass and the separate additive glow pass.
- [x] Audit native renderer paths for equivalent clip support.
- [x] Implement native rounded clip parity where required by the current renderer architecture.
- [x] Add tests or renderer assertions for zero-radius rectangular compatibility.

### Group 2.3: Frame Clipping

Dependency: Group 2.2

Parallelization: sequential

Tasks:

- [x] Connect `Frame` clipping helpers to the new rounded clip stack.
- [x] Ensure child subtree painting inherits the frame clip.
- [x] Ensure opacity and transform compose correctly with clipping.
- [x] Add a runtime test/story that makes rounded child clipping visually obvious.

## Sprint 3: Effects Element And Styled Cleanup

### Group 3.1: Effects Element

Dependency: Sprint 2

Parallelization: sequential

Tasks:

- [x] Add `crates/gpux/crates/gpui/src/elements/effects.rs`.
- [x] Implement `Effects` on top of the lightweight frame geometry core.
- [x] Add `gpui::effects()` constructor.
- [x] Implement `.child(...)` with the same one-child behavior as `Frame`.
- [x] Do not implement `ParentElement`.
- [x] Do not implement broad `Styled`.
- [x] Add `.effect(...)`, optional effect-list helpers, `.clear_effects()`, and `.screen_effect_region(...)`.
- [x] Emit visual effect primitives using the element bounds and corner radii.
- [x] Emit screen effect region primitives using the element bounds and corner radii.
- [x] Ensure active rounded clip applies to effects and child content.
- [x] Preserve the current WGPU composition split: backdrop/glass source-replacement effects stay in the normal visual-effect pass, while glow remains a separate additive color pass that preserves destination alpha.

### Group 3.2: Migrate Call Sites

Dependency: Group 3.1

Parallelization: sequential

Tasks:

- [x] Replace div/styled shader effect usages with `effects().child(...)`.
- [x] Update GPUI tests that currently check div effect behavior.
- [x] Update shader effect lab/demo code.
- [x] Update navigation region rendering to use `effects()` for screen effect regions.
- [x] Update imports after removing old styled effect methods.

### Group 3.3: Remove Old API Surface

Dependency: Group 3.2

Parallelization: sequential

Tasks:

- [x] Remove `.effect(...)`, `.effects(...)`, `.clear_effects()`, and `.screen_effect_region(...)` from `Styled`.
- [x] Remove effect fields from `Style` and `StyleRefinement`, unless a narrow private bridge remains necessary.
- [x] Remove div effect paint code.
- [x] Remove obsolete tests that only validate the old API.
- [x] Run `cargo fmt --all`.
- [x] Run targeted GPUI tests for frame/effects/style cleanup.

## Sprint 4: Navigation Default Transition Configuration

### Group 4.1: Transition Options Shape

Dependency: Sprint 3

Parallelization: sequential

Tasks:

- [x] Add a no-transition constructor/name, preferably `NavigationTransition::none()`.
- [x] Decide whether to remove `NavigationTransition::disabled()` under the no-shim rule.
- [x] Make transition configuration compatible with `NavigationOptions` serialization.
- [x] If direct serialization is unsuitable, add a separate serializable transition options struct and conversion path.
- [x] Adjust `NavigationOptions` equality derives if transition fields make `Eq` inappropriate.

### Group 4.2: Service And Builder Defaults

Dependency: Group 4.1

Parallelization: sequential

Tasks:

- [x] Add `NavigationOptions.default_transition`, defaulting to no transition.
- [x] Expose builder/configuration helpers so `RayXAppBuilder` navigation setup can set the default transition.
- [x] Update `NavigationService` construction to retain the configured default.
- [x] Update router builder logic to use the service default unless explicitly overridden.
- [x] Update region view builder logic to use the service default unless explicitly overridden.
- [x] Preserve explicit local transition override behavior.

### Group 4.3: Navigation Tests

Dependency: Group 4.2

Parallelization: sequential

Tasks:

- [x] Add tests for default no-transition behavior.
- [x] Add tests for configured app-wide default transition.
- [x] Add tests for explicit router transition override.
- [x] Add tests for explicit region transition override.
- [x] Add tests that region screen-effect registration uses the effective transition.

## Sprint 5: Snapshot-Aware GPUI Navigation Helper

### Group 5.1: Helper API

Dependency: Sprint 4

Parallelization: sequential

Tasks:

- [x] Add a GPUI/window-aware navigation extension trait in the navigation GPUI layer.
- [x] Include helpers for global navigation and named-region navigation.
- [x] Require `&mut Window` for helpers that can auto-capture snapshots.
- [x] Keep pure `NavigationService` methods window-independent.
- [x] Name helpers clearly so callers can distinguish automatic transition capture from plain navigation.

### Group 5.2: Helper Behavior

Dependency: Group 5.1

Parallelization: sequential

Tasks:

- [x] Resolve the effective transition before route mutation.
- [x] Store transition origin when the transition uses an origin.
- [x] Resolve capture target from transition target policy.
- [x] Capture snapshot before navigation when `requires_snapshot()` is true.
- [x] Store snapshot through the existing navigation transition service state.
- [x] Perform navigation after transition state is prepared.
- [x] Clear prepared transition state if navigation fails.
- [x] Return the original navigation result.

### Group 5.3: Helper Tests And Stories

Dependency: Group 5.2

Parallelization: sequential

Tasks:

- [x] Add unit tests for non-snapshot default transition helpers.
- [x] Add unit tests for snapshot-required default transition helpers.
- [x] Add failure cleanup tests.
- [x] Update lab/demo navigation transition code to use the helper for snapshot-required defaults.
- [x] Add a manual runtime story that demonstrates app-wide default transition behavior.

## Sprint 6: Validation, Documentation, And Memory

### Group 6.1: Validation

Dependency: Sprint 5

Parallelization: sequential

Tasks:

- [x] Run `cargo fmt --all`.
- [x] Run targeted GPUI tests for `frame`, `effects`, clipping, and effect migration.
- [x] Run targeted navigation service/router/region tests.
- [x] Run `cargo check` or the narrowest relevant workspace build if full workspace cost is too high.
- [x] Run the affected demo application to confirm no runtime crash.
- [x] Run WASM/WebGPU validation for rounded clipping and shader effects.
- [x] Use RayX diagnostics or DevKit/MCP inspection for the affected runtime target.
- [x] Include additive-glow validation, such as the `gpui_wgpu` glow blend-state assertion and a shader-effects visual check that adjacent glass/blur content is not overwritten by expanded glow geometry.
- [x] Record any target that cannot be exercised with the exact blocker.

### Group 6.2: Documentation And Examples

Dependency: Group 6.1

Parallelization: sequential

Tasks:

- [x] Update public examples to show `frame()` for lightweight structure.
- [x] Update public examples to show `effects()` for shader effects and screen effect regions.
- [x] Update navigation examples to show default transition configuration.
- [x] Update navigation examples to show snapshot-aware helper usage.
- [x] Remove references that suggest attaching shader effects to `div()`.

### Group 6.3: AgentX And Skill Knowledge

Dependency: Group 6.2

Parallelization: sequential

Tasks:

- [x] Update the narrowest relevant AgentX knowledge files with durable facts learned during implementation.
- [x] Log any architecture decision that affects future work beyond this epic.
- [x] Update the narrowest relevant RayX or GPUI skill reference if implementation produces reusable guidance.
- [x] If no durable skill guidance changed, state that explicitly in the final implementation report.
- [x] Update this task state as sprints move from backlog to active to complete.

## Completion Validation Notes

- `cargo fmt --all` passed.
- Targeted GPUI tests passed for `frame`, `effects`, `content_mask`, and `gpui_wgpu`.
- `cargo test -p rayx_runtime navigation` passed.
- `cargo check --manifest-path apps/lab/Cargo.toml` and narrow clippy checks for `rayx_runtime`, `gpux`, and `gpui_wgpu` passed.
- `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu` passed after the lab app moved default transition construction into the app library surface.
- RayX MCP diagnostics could not inspect a live app because the WASM validation process exits and the daemon reported no active app for `RayX-story`; the headed WebGPU Playwright run is the target-specific runtime proof for this epic.
- `cargo check --workspace` and `cargo clippy --manifest-path apps/lab/Cargo.toml -- -D warnings` remain blocked by unrelated existing workspace/lab warnings and component collection type issues.
