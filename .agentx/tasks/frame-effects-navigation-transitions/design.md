---
agentx:
  kind: knowledge
  scope: repo:tasks/frame-effects-navigation-transitions
  status: current
  created: 2026-07-06T00:00:00+02:00
  updated: 2026-07-06T00:00:00+02:00
  topics:
    - frame
    - effects
    - navigation-transitions
    - design
  supersedes: []
  superseded_by: []
---

# Frame, Effects, And Navigation Transition Defaults Design

## Goals

The design separates three concerns that are currently too entangled:

1. `div()` remains the rich, styled, interactive, multi-child GPUI workhorse.
2. `frame()` becomes the minimal geometry container for exactly one child.
3. `effects()` becomes the minimal geometry container plus shader/screen effect ownership for exactly one child.

Navigation transitions then use the same explicit `effects()` concept for screen effect regions and gain a default transition path that can be configured once at app startup.

## Proposed Public API

### Frame

```rust
use gpui::{frame, px};

frame()
    .size_full()
    .absolute()
    .inset_0()
    .rounded(px(16.))
    .clip()
    .opacity(0.9)
    .child(content)
```

`Frame` should expose inherent methods, not the full `Styled` trait. The exact method set should mirror the small geometry subset already used throughout GPUI where possible:

- size helpers
- position helpers
- inset helpers
- transform helpers
- opacity helpers
- radius helpers
- clipping helpers
- `.child(...)`

`Frame` must not implement `ParentElement`, so `.children(...)` is not available.

### Effects

```rust
use gpui::{effects, shader_effects::ElementEffect};

effects()
    .size_full()
    .rounded(px(18.))
    .clip()
    .effect(ElementEffect::ripple(...))
    .child(content)
```

`Effects` should share the same single-child geometry core as `Frame`, but additionally expose:

- `.effect(...)`
- `.clear_effects()`
- `.screen_effect_region(...)`

If a collection method is necessary for effect definitions, name it only for effects, such as `.effect(...)` repeated or `.with_effects(...)`; do not add `.children(...)`.

## Module Layout

Recommended GPUI layout:

```text
crates/gpux/crates/gpui/src/
  elements/
    frame.rs
    effects.rs
    frame_style.rs       # only if sharing the geometry subset is non-trivial
  shader_effects/
    mod.rs               # renamed from the current effects module
```

The current `gpui/src/effects` module name conflicts with the desired public `effects()` constructor. Rename the data/model module to `shader_effects` or `visual_effects`, then re-export the stable effect model from `gpui::shader_effects` or a similarly explicit namespace.

Preferred constructor exports from `gpui::lib.rs`:

```rust
pub fn frame() -> Frame { ... }
pub fn effects() -> Effects { ... }
```

## Frame Internals

`Frame` should be implemented as a normal GPUI element, using `DivElement` as the lifecycle reference but not copying the full div feature set.

Suggested fields:

```rust
pub struct Frame {
    style: FrameStyle,
    child: Option<AnyElement>,
    source_location: Option<&'static core::panic::Location<'static>>,
}
```

`FrameStyle` should hold only the lightweight properties:

- sizing
- absolute/relative positioning
- inset
- transform and transform origin
- opacity
- corner radius
- clip flag

Do not store flex/grid/text/background/border/shadow/interactivity state.

`.child(...)` should set the single child. If a child is already present, debug/test builds should panic so accidental multi-child composition is caught early.

## Effects Internals

`Effects` should reuse the same geometry representation as `Frame` and add:

```rust
pub struct Effects {
    frame: FrameCore,
    effects: SmallVec<[ElementEffect; 1]>,
    screen_effect_region: Option<ScreenEffectRegion>,
}
```

`Effects` owns shader effect painting. It should emit visual effect primitives around the same bounds and radii used by the lightweight frame style.

Paint order should preserve the existing visual intent from `Style::paint`:

1. establish the element bounds
2. apply clipping scope if enabled
3. emit owned visual effects
4. emit owned screen effect region metadata if configured
5. paint the single child

If the final visual result requires effects behind the child rather than over the child, preserve the current renderer behavior from `Style::paint` and document the order in tests. The important part is that effect ownership moves to `Effects`, not `Div`.

## Shader Composition Constraints

The current WGPU renderer has one important composition constraint that the `effects()` migration must preserve:

- the normal visual-effect pass handles source-replacement effects such as backdrop blur and Liquid Glass
- that normal pass ignores `glow`
- glow renders through a separate additive pass that outputs only glow energy
- glow color blending uses source one plus destination one
- glow alpha blending preserves destination alpha

This split prevents expanded glow geometry from sampling and returning stale source pixels over neighboring glass or blur content. `Effects` should move ownership of effect declarations, not collapse glow back into the regular visual-effect pass.

## Rounded Clipping Design

The current content mask model appears rectangular. `frame().rounded(...).clip().child(...)` needs a rounded clip that applies to the whole child subtree and to effect primitives.

Preferred design:

1. Introduce an explicit clip shape type, for example:

```rust
pub struct ClipMask<P> {
    pub bounds: Bounds<P>,
    pub corner_radii: Corners<P>,
}
```

2. Replace or extend current `ContentMask` usage so scene primitives can carry rounded clip data.
3. Keep the zero-radius case equivalent to existing rectangular clipping.
4. Apply clip masks through a window/scene clip stack so descendants inherit the active rounded clip.
5. Update renderer packing and shaders to test both primitive bounds and the active rounded clip shape.

This must be a real rendering primitive concern, not a local frame-only special case. Child content can contain arbitrary quads, images, text, canvases, components, and effects, so the active clip must flow through scene building.

Validation should include:

- child content visibly clipped by a rounded `frame`
- `effects()` visual effect clipped by the same radius
- nested clips using intersection rules
- zero-radius clip matching the old rectangular path

## Styled And Div Cleanup

After `effects()` exists:

1. remove effect fields from `Style` and `StyleRefinement`, or make any remaining storage private to the new `Effects` element only
2. remove `.effect(...)`, `.effects(...)`, `.clear_effects()`, and `.screen_effect_region(...)` from `Styled`
3. update tests from div-based effects to `effects()` wrappers
4. update demos/stories/navigation wrappers to use `effects()`
5. remove any now-dead div effect paint path

This keeps the end-state API honest: if a caller wants shader effects, they opt into the `effects()` element.

## Navigation Default Transition Design

Add default transition configuration to navigation options:

```rust
pub struct NavigationOptions {
    ...
    pub default_transition: NavigationTransition,
}
```

Default:

```rust
NavigationTransition::none()
```

If `NavigationTransition` cannot cleanly derive the same serialization/equality traits as `NavigationOptions`, introduce a serializable `NavigationTransitionOptions` and convert it into runtime `NavigationTransition` during navigation service construction.

Builder behavior:

- `RayXAppBuilder::with_navigation(...)` configures the default transition with the rest of navigation.
- `NavigationRouterBuilder` initializes from service default if no explicit transition was set.
- `NavigationRegionViewBuilder` initializes from service default if no explicit transition was set.
- explicit `.transition(...)` on a router/region builder overrides the app-wide default.

`NavigationRegionView` should use `effects()` to register screen effect regions when the effective transition targets a region. It should not use `div().screen_effect_region(...)` after the effect API migration.

## Snapshot-Aware Navigation Helper

Pure navigation service methods cannot automatically capture snapshots because snapshot capture requires GPUI `Window`. Keep pure navigation pure and add GPUI-aware helper methods.

Recommended API shape:

```rust
pub trait NavigationTransitionGpuiWindowExt {
    fn navigate_to_with_default_transition(
        &mut self,
        window: &mut Window,
        uri: impl Into<String>,
        origin: Point<Pixels>,
    ) -> Result<NavigationResult>;

    fn navigate_in_region_with_default_transition(
        &mut self,
        window: &mut Window,
        region_name: impl Into<SharedString>,
        uri: impl Into<String>,
        origin: Point<Pixels>,
    ) -> Result<NavigationResult>;
}
```

The exact receiver can be `App`, `NavigationService`, or a GPUI extension trait, but the helper must receive `&mut Window`.

Helper algorithm:

1. resolve the navigation service
2. resolve the effective transition for the global route or named region
3. if disabled, perform normal navigation
4. if enabled and origin-based, store the transition origin
5. if `requires_snapshot()` is true, resolve the capture target from the transition
6. capture the snapshot before route mutation
7. store the snapshot in `NavigationTransitionService`
8. perform navigation
9. if navigation fails, clear the stored origin/snapshot
10. return the navigation result

This is the helper previously described as "a GPUI navigation helper later that captures automatically when the configured default requires it." The helper makes snapshot transitions ergonomic while keeping `NavigationService` independent of rendering/window concerns.

## Testing Strategy

### Unit And Compile Checks

- `frame()` constructor exists and builds a one-child element.
- `effects()` constructor exists and builds a one-child element.
- repeated `.child(...)` is rejected in debug/test builds.
- `Frame` and `Effects` do not implement `ParentElement`.
- migrated effect tests assert effects are emitted only by `effects()`.
- `Styled` no longer exposes effect methods.
- WGPU additive glow blending remains covered by tests or renderer assertions.
- navigation options default to no transition.
- explicit router/region transitions override default transition.
- default transition is inherited by router/region builders.
- helper clears transition state when navigation fails.

### Rendering And Runtime Checks

- WASM/WebGPU test page with rounded `frame` clipping visible child content.
- WASM/WebGPU test page with rounded `effects` clipping shader effects.
- navigation transition story using default ripple or reveal transition.
- navigation transition story using snapshot-required default transition through the helper.
- demo app launches without runtime crash.

## Migration Strategy

1. Add `frame()` first without touching shader effects.
2. Add rounded clip infrastructure before making clipping part of acceptance.
3. Add `effects()` on top of the frame geometry core.
4. Migrate shader effect call sites to `effects()`.
5. Remove old `Styled`/`div` effect APIs.
6. Add navigation default transition configuration.
7. Add snapshot-aware GPUI helper.
8. Update tests, stories, docs, and AgentX memory.

## Compatibility Position

This branch is still implementing the feature, so use the clean final API. Do not keep old names or forwarding shims.
