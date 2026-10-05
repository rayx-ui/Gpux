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
    - requirements
  supersedes: []
  superseded_by: []
---

# Frame Family And Component Transitions Requirements

## Summary

Unify lightweight child-operating GPUI elements under a frame-family naming model:

- `frame()` is the lightweight single-child geometry container.
- `effect_frame()` is `frame` plus shader visual effects and screen effect regions.
- `animated_frame()` is `frame` plus explicit property/layout animation.
- `transition_frame()` is `frame` plus old/new snapshot or component replacement transitions.

The current source already implements `frame()` and `effects()`. This epic must rename the implemented `effects()` element to `effect_frame()` with no compatibility shim, then add `animated_frame()` and `transition_frame()` as first-class single-child frame elements. The Lab app must gain example pages for `animated_frame()` and `transition_frame()` after the implementation is complete.

## Current Source Findings

- `crates/gpux/crates/gpui/src/elements/frame.rs` exposes `frame()` and `Frame` as a lightweight single-child geometry/clip container backed by `FrameStyle`.
- `crates/gpux/crates/gpui/src/elements/effects.rs` exposes the already-implemented `effects()` and `Effects` element. It shares the lightweight frame geometry surface, owns ordered `ElementEffect` values, registers `screen_effect_region(...)`, paints visual effects, and paints one child.
- `crates/gpux/crates/gpui/src/elements/frame_style.rs` contains the shared geometry/compositing state used by `Frame` and `Effects`.
- `crates/gpux/crates/gpui/src/elements/animation.rs` already has a lower-level `AnimationElement` wrapper that calls an animator closure with progress, but it is not a frame-family API and does not retain previous/target frame style values.
- `Window::capture_screen_effect_snapshot(...)`, `Window::play_screen_effect(...)`, `Window::paint_screen_effect_region(...)`, and retained `ScreenEffectSnapshotId` values already support snapshot-backed screen transitions.
- Navigation page transitions in `crates/rayx/rayx_runtime/src/services/navigation/transition.rs` are route/region helpers. They prove the screen snapshot path, but `transition_frame()` must live in GPUI/gpux rather than in `NavigationService`.
- `apps/lab/src/app/story_routes.rs` and `apps/lab/src/stories/mod.rs` are the registration points for new Lab stories.

## In Scope

1. Rename public `effects()` to `effect_frame()`.
2. Rename public `Effects` type to `EffectFrame`.
3. Rename source files, docs, examples, tests, and imports that refer to `effects()` or `Effects` as the public element API.
4. Remove the old `effects()` constructor and `Effects` type instead of keeping aliases or forwarding shims.
5. Keep the internal shader model under `shader_effects`; do not rename shader data types just because the element constructor changes.
6. Preserve all current `effects()` behavior under `effect_frame()`, including ordered effects, screen effect region registration, rounded clipping, opacity, transform, and additive WGPU glow composition.
7. Add `animated_frame(id)` as a stable-id, single-child frame that animates frame-owned geometry/compositing properties when target values change.
8. Add `transition_frame(id)` as a stable-id, single-child frame that animates replacement between old and new child states using retained renderer snapshots.
9. Keep `frame`, `effect_frame`, `animated_frame`, and `transition_frame` single-child only.
10. Do not implement `ParentElement` for any frame-family element.
11. Do not add `.children(...)` to any frame-family element.
12. Create Lab example pages for `animated_frame()` and `transition_frame()`, including route registration, locale metadata, and runtime validation coverage.
13. Update public documentation and examples to teach the frame-family naming model.
14. Update AgentX memory and skill guidance only where implementation produces durable current behavior.

## Out Of Scope

1. Do not add flex, grid, text, background, border, shadow, scrolling, interactivity, image cache, or event APIs to frame-family elements.
2. Do not preserve `effects()` as a compatibility alias.
3. Do not add automatic arbitrary component-tree diffing in the first `animated_frame()` implementation.
4. Do not add shared-element transitions between unrelated parts of the tree in this epic.
5. Do not put component transition playback state into `NavigationService`.
6. Do not make non-WGPU renderers report shader/snapshot transition support unless they actually render it.
7. Do not create fake native transition approximations that silently diverge from WGPU behavior.
8. Do not add multi-child transition containers.

## Functional Requirements

### Frame Family Model

1. `frame()` remains the baseline lightweight geometry container.
2. `effect_frame()` owns shader effects and screen effect regions.
3. `animated_frame()` owns explicit frame property/layout interpolation.
4. `transition_frame()` owns old/new snapshot or component replacement transition behavior.
5. All frame-family elements accept exactly one child through a single child source.
6. Repeated child assignment must be treated as programmer error, preferably through debug/test assertions.
7. Frame-family elements must expose the same lightweight geometry/compositing surface where applicable: size, position, inset, transform, opacity, radius, and clipping.
8. Frame-family elements must not expose broad `Styled`/`div()` APIs.

### Effect Frame Rename

1. `gpui::effect_frame()` creates an `EffectFrame` element.
2. `EffectFrame` exposes the current `effects()` behavior:
   - `.child(...)`
   - `.effect(...)`
   - effect-list helper for ordered effects if still needed
   - `.clear_effects()`
   - `.screen_effect_region(...)`
   - lightweight frame geometry/compositing APIs
3. Existing call sites using `effects()` must migrate to `effect_frame()`.
4. The old `effects()` constructor must be removed.
5. The old `Effects` public type must be removed or renamed without a public alias.
6. Tests must prove `effect_frame().children(...)` is unavailable by construction.
7. Tests must prove shader effects and screen effect regions are still emitted after the rename.
8. WGPU glow must remain a separate additive color pass that preserves destination alpha.

### Animated Frame

1. `gpui::animated_frame(id)` creates an `AnimatedFrame` with a required stable `ElementId`.
2. `AnimatedFrame` tracks previous and target frame-owned properties using element state keyed by the stable id.
3. `AnimatedFrame` starts a new animation when a target style value changes or when an explicit target key changes.
4. `AnimatedFrame` interpolates supported numeric properties:
   - absolute width and height
   - absolute min/max width and height when both endpoints are compatible
   - absolute insets when both endpoints are compatible
   - opacity
   - translate
   - scale
   - rotate
   - skew
   - transform origin
   - corner radii
5. Unsupported or incompatible values, such as `auto` to `relative`, must snap to the target rather than producing invalid layout.
6. `AnimatedFrame` must request animation frames until the active animation completes.
7. `AnimatedFrame` must handle interrupted animations by using the current interpolated value as the next animation start.
8. `AnimatedFrame` must support a static `.child(...)`.
9. `AnimatedFrame` must support a single progress-aware child source, such as `.child_with_progress(...)`, so a component can animate child-owned values like font size while the frame animates geometry.
10. `.child(...)` and the progress-aware child source must be mutually exclusive single-child sources.
11. The first render must not animate from zero or uninitialized geometry.
12. `AnimatedFrame` must not retain arbitrary old child trees after animation completes.

### Transition Frame

1. `gpui::transition_frame(id)` creates a `TransitionFrame` with a required stable `ElementId`.
2. `TransitionFrame` has a target key that identifies the rendered child state or replacement component.
3. The first render must paint the current child without transition.
4. When the target key changes, `TransitionFrame` captures the previous rendered region as a retained screen-effect snapshot before painting the replacement transition.
5. Snapshot capture must use the existing `Window::capture_screen_effect_snapshot(...)` path and a stable screen-effect region derived from the frame id unless explicitly overridden.
6. `TransitionFrame` must register its current bounds as a screen effect region every frame.
7. `TransitionFrame` must play a configured snapshot transition targeted to that region, such as crossfade, blur crossfade, radial reveal, or ripple reveal.
8. `TransitionFrame` must release retained snapshots after transition playback completes.
9. Snapshot release must not require a GPUI view `Context`; element-owned state may release expired snapshots during later frame passes.
10. `TransitionFrame` must request animation frames while a transition is active.
11. If the renderer does not support snapshot transitions, the new child must still appear without crashing or faking support.
12. `TransitionFrame` must avoid recursive retained snapshots and must not leak snapshot ids after repeated toggles.
13. `TransitionFrame` must support arbitrary replacement children because it treats the old state as pixels.

### Lab Examples

1. Add an `AnimatedFrameStory` under `apps/lab/src/stories/`.
2. The animated frame story must demonstrate a compact/expanded component where height, radius, opacity, transform, and at least one child-owned visual value such as font size animate together.
3. Add a `TransitionFrameStory` under `apps/lab/src/stories/`.
4. The transition frame story must demonstrate replacing one child component with a different child component on click.
5. The transition frame story must include at least two transition variants if the API supports variants.
6. Register both stories in `apps/lab/src/stories/mod.rs`.
7. Register both story routes in `apps/lab/src/app/story_routes.rs`.
8. Add English and Croatian story locale metadata.
9. Update asset manifests if the Lab asset pipeline requires generated manifest changes for locale assets.
10. Add route registration tests and browser/WASM validation coverage for the stories.

## Non-Functional Requirements

1. Keep implementation minimal and vertical-slice oriented.
2. Prefer the existing `Frame`, `EffectFrame`, `FrameStyle`, `AnimationElement`, and screen-effect paths over new renderer architecture.
3. Use clean final APIs with no compatibility shims.
4. Preserve WGPU shader composition correctness, especially additive glow.
5. Preserve rounded clipping behavior for all frame-family elements.
6. Validate with targeted Rust tests and actual runtime validation.
7. Run `cargo fmt --all` after code changes.
8. Run targeted gpui/gpui_wgpu tests for frame-family behavior.
9. Run Lab route/story tests after adding examples.
10. Run headed WASM/WebGPU validation for animated and transition frame stories.
11. Run the Lab app enough to confirm no runtime crash.

## Acceptance Criteria

1. `effect_frame()` exists and replaces `effects()` in public examples and call sites.
2. `effects()` and `Effects` are no longer public APIs.
3. Existing shader effect demos still render through `effect_frame()`.
4. `animated_frame()` animates supported frame-owned properties across state changes.
5. `animated_frame()` supports a progress-aware single child for child-owned animation values.
6. `transition_frame()` transitions between replacement child states using retained snapshots.
7. `transition_frame()` releases retained snapshots after playback.
8. All frame-family elements remain single-child and do not expose `.children(...)`.
9. Lab includes visible `AnimatedFrameStory` and `TransitionFrameStory` routes.
10. Route metadata tests cover both new stories.
11. Runtime validation confirms both stories render and animate without crashes.
12. Unsupported renderer paths degrade by showing the target child without claiming unsupported shader/snapshot capabilities.

## Assumptions

1. `animated_frame()` should live in gpux/GPUI because it is a generic element primitive, not a RayX service feature.
2. `transition_frame()` should live in gpux/GPUI because it uses GPUI screen-effect snapshots and should be usable outside navigation.
3. Navigation default transitions remain separate; `transition_frame()` may reuse shader/screen-effect model types but should not depend on `rayx_runtime`.
4. The new Lab pages are implementation deliverables in this epic, not planning-only mockups.

## Risks And Follow-Ups

1. Animating layout properties can cause parent layout churn; tests should cover interrupted and repeated state toggles.
2. Automatic interpolation is only safe for compatible value kinds; future work may add measured auto-size animation.
3. Snapshot transitions depend on a previous rendered frame for the target region; first render and missing previous region must skip transition cleanly.
4. Shared-element transitions across different tree positions remain a future feature.
5. Existing docs or examples may still mention `effects()`; migration should use repository-wide search.
