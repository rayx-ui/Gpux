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
    - design
    - effects
  supersedes: []
  superseded_by: []
---

# Design: Shader Effects And Screen Transitions

Final implementation note: early sketches in this completed task refer to `div().effect(...)`, `div().effects(...)`, and broad `Styled` APIs. The accepted final architecture supersedes those sketches with the frame-family API: use `effect_frame().effect(...)` / `effect_frame().effects(...)` for shader effects, `frame()` for lightweight geometry/clipping, `animated_frame(id)` for frame-property animation, and `transition_frame(id)` for snapshot-backed replacement transitions.

## Current-State Summary

Current source inspection identifies these implementation surfaces:

- `crates/gpux/crates/gpui/src/elements/div.rs` owns `div()`, `Div`, `Styled for Div`, and div paint behavior.
- `crates/gpux/crates/gpui/src/style.rs` owns `Style` and `Style::paint`. It currently paints shadows, then background, then children through the continuation, then border.
- `crates/gpux/crates/gpui/src/scene.rs` owns `Scene`, primitive insertion, draw order, `PrimitiveKind`, `PrimitiveBatch`, replay, and batch iteration. It already stores separate vectors for shadows, quads, HTML islands/elements, paths, sprites, and surfaces, then interleaves batches by draw order.
- `crates/gpux/crates/gpui/src/window.rs` owns `Window`, frame drawing, input dispatch, `request_animation_frame`, `on_next_frame`, and the frame lifecycle where screen effects can be stored and expired.
- `crates/gpux/crates/gpui_wgpu/src/wgpu_renderer.rs` owns WGPU pipelines, render-pass sequencing, intermediate path textures, and `encode_scene_to_view`.
- `crates/gpux/crates/gpui_wgpu/src/shaders.wgsl` contains existing quad, shadow, sprite, surface, and path shader entry points but no backdrop/post-process effect shader yet.
- `crates/gpux/crates/gpui_mobile` has Apple-style glass components, but those are styled GPUI elements, not reusable renderer-level shader effects.
- `apps/examples_mobile/src/demos/shader_showcase.rs` has an animated ripple/orb demo using canvas-style painting and `window.request_animation_frame()`. It is a useful interaction reference but not the target architecture for post-processing arbitrary UI.
- `crates/rayx/rayx_runtime/src/services/navigation/service.rs` owns route state and subscriptions.
- `crates/rayx/rayx_runtime/src/services/navigation/router.rs` owns GPUI route rendering and subscribes to navigation changes.
- `crates/rayx/rayx_runtime/src/services/navigation/region_view.rs` wraps `NavigationRouter` in a region component.
- `apps/lab/src/app/story_shell.rs` uses `NavigationRegionView::root(...)` to render active Lab story routes.

AgentX memory and decisions add these constraints:

- gpux should be modified only for framework-level behavior unavailable through public APIs.
- Navigation must not add retained route views to `NavigationService`; retained visual state belongs in GPUI router/app layers.
- WASM/browser validation must go through `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`.
- Browser-active code must avoid unsupported `std::time` clocks.
- WGPU POD/WGSL layout must be asserted exactly.

## Proposed Architecture

### Layering

The epic should be split across two layers:

- `gpux` owns effect primitives, effect APIs, renderer capabilities, shader resources, and WGPU/Metal/DirectX implementation.
- `rayx_runtime` owns navigation integration only at the GPUI-facing router/region layer, using gpux screen-effect APIs.

This keeps visual effects available to all gpux users while letting RayX compose navigation transitions without contaminating service-state infrastructure.

### Public API Shape

Element effects should live under a gpux module such as `gpux::effects` and be re-exported from the crate prelude when stable.

Proposed usage:

```rust
use gpux::effects::{backdrop_blur, glass, glow};

div()
    .rounded_lg()
    .effect(backdrop_blur(px(18.)).saturation(1.15))
    .effect(glow(hsla(0.57, 0.88, 0.62, 0.45)).radius(px(28.)))
    .child("Frosted content");
```

For multiple effects:

```rust
div()
    .effects((
        backdrop_blur(px(20.)).saturation(1.2),
        glass()
            .blur(px(16.))
            .refraction(0.045)
            .distortion(0.025)
            .tint(hsla(0.58, 0.35, 0.92, 0.18)),
        glow(hsla(0.6, 0.8, 0.7, 0.35)).radius(px(24.)),
    ));
```

Rust does not support a true variadic `.effects(a, b, c)` method. The implementation should support repeated `.effect(...)` and tuple/array-style `.effects(...)` through a small trait such as `IntoElementEffects`.

Screen effect usage:

```rust
use gpux::effects::{screen, ScreenEffectTarget};

div()
    .screen_effect_region("story-content")
    .size_full()
    .child(story);

window.play_screen_effect(
    screen::ripple(event.position)
        .target(ScreenEffectTarget::Window)
        .duration_ms(650)
        .radius(px(420.))
        .amplitude(px(12.)),
);
```

Region target:

```rust
window.play_screen_effect(
    screen::ripple(event.position)
        .target(ScreenEffectTarget::region("story-content"))
        .duration_ms(700),
);
```

Navigation integration should avoid transition-specific constructor sprawl. If the current constructor set becomes unwieldy, add a single builder and update call sites in the same sprint:

```rust
NavigationRegionView::builder(service, render_route)
    .region(service.root_region_name())
    .screen_effect_region("story-content")
    .transition(NavigationTransition::ripple_reveal().origin(TransitionOrigin::PointerOrCenter))
    .build(cx);
```

Manual pre-navigation transition helper:

```rust
cx.navigation()
    .transition(window, NavigationTransition::ripple_reveal_at(event.position))
    .navigate_to("/settings")?;
```

The helper may wrap `NavigationService` for one call, but it must not store retained route views inside the service.

### Core Data Types

Add a new gpux module, likely `crates/gpux/crates/gpui/src/effects/`, with focused files:

- `mod.rs` for exports.
- `element.rs` for `ElementEffect`, `BackdropBlurEffect`, `GlassEffect`, `GlowEffect`, and `IntoElementEffects`.
- `screen.rs` for `ScreenEffect`, `RippleEffect`, `ScreenEffectTarget`, `ScreenEffectRegionId`, animation timing, and active-effect caps.
- `capabilities.rs` for renderer support reporting.

Proposed core types:

```rust
pub enum ElementEffect {
    BackdropBlur(BackdropBlurEffect),
    Glass(GlassEffect),
    Glow(GlowEffect),
}

pub enum ScreenEffect {
    Ripple(RippleEffect),
    SnapshotTransition(SnapshotTransitionEffect),
}

pub enum ScreenEffectTarget {
    Window,
    Bounds(Bounds<Pixels>),
    Region(ScreenEffectRegionId),
}
```

Builder functions should return final clean public types:

```rust
pub fn backdrop_blur(radius: Pixels) -> ElementEffect;
pub fn glass() -> ElementEffect;
pub fn glow(color: Hsla) -> ElementEffect;

pub mod screen {
    pub fn ripple(origin: Point<Pixels>) -> ScreenEffect;
}
```

If builder methods are variant-specific, use lightweight variant builder structs that convert into `ElementEffect`, then make `.effect(...)` accept `impl Into<ElementEffect>`. For tuple-based `.effects(...)`, implement tuple support for common arities so heterogeneous builders work.

### Style And Div Integration

Add `effects: Vec<ElementEffect>` or equivalent to `Style`/`StyleRefinement`, depending on the current style refinement model.

Add methods on `Styled`:

- `effect(effect: impl Into<ElementEffect>) -> Self`
- `effects(effects: impl IntoElementEffects) -> Self`
- `clear_effects() -> Self`
- `screen_effect_region(id: impl Into<SharedString>) -> Self` or a small wrapper if region registration should remain separate from style refinement.

Paint order:

1. Apply element transform.
2. Paint debug outline if enabled.
3. Compute corner radii.
4. Paint shadows.
5. Paint element effects clipped to bounds/corner radii/content mask.
6. Paint existing background.
7. Paint continuation/children.
8. Paint existing border.

This gives backdrop/glass access to already-painted scene content while preserving existing background and child semantics.

### Scene Model

Add new scene primitive vectors and batches:

- `visual_effects: Vec<VisualEffectPrimitive>`
- `screen_effects: Vec<ScreenEffectPrimitive>` or a post-process list stored after scene completion
- `screen_effect_regions: Vec<ScreenEffectRegionPrimitive>` if regions should be captured from paint rather than style metadata

`VisualEffectPrimitive` should include:

- draw order
- bounds in scaled pixels
- content mask
- corner radii
- opacity
- effect enum or packed effect parameters
- element transform metadata already represented in the scene path, if needed

`ScreenEffectPrimitive` should include:

- effect parameters
- resolved target bounds/content mask
- elapsed time/progress
- draw order if region-limited effects can be interleaved, otherwise a post-scene stage marker

Scene insertion must participate in `PaintOperation` and `Scene::replay` so retained/replayed painting preserves effects. `Scene::finish` must sort effects by draw order like other primitives. `PrimitiveBatch` must include effect batches at the correct position.

### Window And Frame State

Add active screen-effect state to `Window` or the next-frame/rendered-frame data that already tracks input, hit tests, listeners, and paint state.

Responsibilities:

- `Window::play_screen_effect(effect: impl Into<ScreenEffect>)`
- `Window::active_screen_effect_count()`
- Expire effects based on browser-safe elapsed time.
- Request animation frames while active effects remain.
- Resolve `ScreenEffectTarget::Window` from current window bounds.
- Resolve `ScreenEffectTarget::Region` from screen-effect regions painted in the current or most recent rendered frame.
- Keep a bounded queue per window and deterministic eviction policy.

Region registration should be visual-only and must not create hitboxes, focus handles, or pointer interception.

### WGPU Renderer Design

WGPU should render the scene into an offscreen scene color target first, then present to the surface. This avoids depending on surface `COPY_SRC` and avoids invalid read/write sampling of the current target.

Resource model:

- Scene color texture matching viewport format/size.
- Ping-pong color texture for effect composition.
- Downsample/blur pyramid textures for backdrop blur and glass.
- Effect uniform/storage buffers with exact Rust/WGSL layout assertions.
- Pipelines for pass-through, backdrop blur, glass/refraction, glow, and ripple.

Element backdrop path:

1. Draw ordinary batches into `scene_color`.
2. When an element effect batch is reached, end the current render pass.
3. Use `scene_color` as the source snapshot for content already behind the effect.
4. Build or reuse blur pyramid for the effect bounds/quality tier.
5. Composite the effect quad into the destination color target, writing into ping-pong if sampling from the previous target.
6. Continue later scene batches from the updated target.

Screen effect path:

1. Finish drawing the full scene to `scene_color`.
2. Apply active full-window or region-limited screen effects as post-process passes.
3. Write the final result to the surface view.

This design should begin with correctness and simple pass-through shaders, then optimize with bounds-limited passes, scissor rectangles, and lower-resolution blur targets.

### Native Renderer Parity

WGPU is first. Metal and DirectX must not claim effect support until their shaders and render-pass sequencing are implemented.

Add explicit capability reporting:

```rust
pub struct EffectCapabilities {
    pub element_backdrop_blur: bool,
    pub element_glass: bool,
    pub element_glow: bool,
    pub screen_ripple: bool,
    pub snapshot_transitions: bool,
}
```

Capabilities should feed diagnostics and optional app-level decisions, but app code should remain portable.

### RayX Navigation Integration

Add transition support in `crates/rayx/rayx_runtime/src/services/navigation/` around `NavigationRouter` and `NavigationRegionView`.

Do not add retained route views to `NavigationService`.

Recommended structure:

- `transition.rs` for `NavigationTransition`, `TransitionOrigin`, and route-change policy.
- `router.rs` updates to enqueue post-navigation screen effects when its subscribed region changes.
- `region_view.rs` updates to paint a screen-effect region around the router child when configured.
- Optional `NavigationTransitionExt` helper for explicit pre-navigation effects and future snapshot capture.

The first transition implementation should play a region-targeted ripple or reveal after route changes. Snapshot transitions should come later once gpux can capture a region texture before route mutation.

### Lab And Examples

Add a Lab story under the existing Lab story system:

- Div effects section with patterned/scrolling background and cards using blur, glass, and glow.
- Screen ripple section where clicks/touches produce ripple distortion at the pointer.
- Region section with a named effect region so the same ripple is constrained to part of the UI.
- Navigation section showing route transitions through `NavigationRegionView` or the transition helper.

The existing mobile shader showcase can inform interaction and visual tuning but should not be copied as the renderer architecture.

### Testing Strategy

Unit tests:

- Effect builder defaults and parameter validation.
- `IntoElementEffects` tuple/array behavior.
- Style refinement accumulation and clear behavior.
- Scene primitive insertion, sorting, replay, and batch ordering.
- Screen-effect expiration, queue caps, and target resolution.
- Navigation router transition policy without retained route views.

Renderer tests:

- Rust/WGSL layout size and offset assertions for packed effect structs.
- WGPU pass-through effect proof before blur/refraction.
- Focused tests for resource resize and capability flags where feasible.

Runtime/visual tests:

- `cargo fmt --all`
- Focused cargo tests for gpux and rayx navigation modules.
- `cargo check --manifest-path apps/lab/Cargo.toml`
- `cargo check --manifest-path apps/lab/platform/web/Cargo.toml`
- `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`
- Desktop native runtime proof through `cargo run -p xtask -- app apps/lab run host` once the affected native backend is implemented.
- Android runtime proof through `cargo run -p xtask -- app apps/lab run android` when gpui_mobile/WGPU effect behavior is enabled.

Playwright should assert WebGPU availability, visible canvas bounds, no page errors, effect story visibility, pointer-triggered ripple diagnostics, and screenshots/pixel stats that distinguish true backdrop sampling from static tint.

### Alternatives Considered

- CSS or DOM backdrop filters: rejected because the gpux renderer must work over canvas/WebGPU content and native targets.
- Canvas-only demos: rejected because they cannot post-process arbitrary GPUI content behind elements.
- Implementing effects only in RayX components: rejected because backdrop and screen effects need renderer-level scene access.
- Putting transition state into `NavigationService`: rejected by existing navigation decision and ownership model.
- WGPU plus fake native approximations: rejected because capabilities must be honest and backend parity should be explicit.
