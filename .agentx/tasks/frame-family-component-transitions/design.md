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
    - design
  supersedes: []
  superseded_by: []
---

# Frame Family And Component Transitions Design

## Goals

The GPUI frame family should make the mental model obvious:

1. `frame()` is a lightweight single-child geometry boundary.
2. `effect_frame()` is a frame that applies shader/screen effects to its child boundary.
3. `animated_frame()` is a frame that interpolates frame-owned properties and optionally provides progress to one child.
4. `transition_frame()` is a frame that transitions between previous and target child visuals.

Each member is a single-child boundary that operates on the child; none of them should become a rich `div` replacement.

## Proposed Public API

### Frame

```rust
use gpui::{frame, px};

frame()
    .w(px(320.))
    .rounded(px(16.))
    .clip()
    .child(content)
```

`frame()` remains the baseline geometry primitive.

### Effect Frame

```rust
use gpui::{backdrop_blur, effect_frame, glass, glow, hsla, px};

effect_frame()
    .w(px(320.))
    .rounded(px(18.))
    .clip()
    .effect(backdrop_blur(px(18.)).high_quality())
    .effect(glass().refraction(0.08))
    .effect(glow(hsla(0.58, 0.8, 0.75, 0.36)).radius(px(28.)))
    .child(content)
```

`effect_frame()` is a breaking rename of current `effects()`. Keep the effect method names because they describe effect declarations, not child collection.

### Animated Frame

```rust
use gpui::{animated_frame, ease_out_quint, px};
use std::time::Duration;

animated_frame("profile-card")
    .target_key(if compact { "compact" } else { "expanded" })
    .duration(Duration::from_millis(220))
    .easing(ease_out_quint())
    .h(if compact { px(72.) } else { px(148.) })
    .rounded(if compact { px(12.) } else { px(22.) })
    .opacity(if compact { 0.86 } else { 1.0 })
    .child_with_progress(move |progress, _window, _cx| {
        profile_card(compact, progress)
    })
```

`animated_frame()` requires a stable id because it needs element state across renders. The child source is still singular: callers may use `.child(...)` for a normal child or `.child_with_progress(...)` for one progress-aware child source, but never both.

### Transition Frame

```rust
use gpui::{FrameTransition, transition_frame};
use std::time::Duration;

transition_frame("settings-panel")
    .target_key(active_panel.id())
    .transition(FrameTransition::blur_crossfade().duration(Duration::from_millis(260)))
    .rounded(px(18.))
    .clip()
    .child(active_panel.render())
```

`transition_frame()` targets complete replacement cases. It captures the previously rendered region as pixels, paints the new child, and plays a snapshot-backed transition into the new state. It is intentionally image-space first; automatic layout morphing across arbitrary child trees is not part of v1.

## Module Layout

Recommended GPUI layout:

```text
crates/gpux/crates/gpui/src/
  elements/
    frame.rs
    effect_frame.rs       # renamed from effects.rs
    animated_frame.rs
    transition_frame.rs
    frame_style.rs
  shader_effects/
    mod.rs
```

Constructor and type exports should live beside existing element exports:

```rust
pub fn frame() -> Frame;
pub fn effect_frame() -> EffectFrame;
pub fn animated_frame(id: impl Into<ElementId>) -> AnimatedFrame;
pub fn transition_frame(id: impl Into<ElementId>) -> TransitionFrame;
```

Do not keep `effects()` or `Effects` aliases. This branch already accepts clean API breaks.

## Effect Frame Rename Design

Rename the current `Effects` element mechanically but cleanly:

```rust
pub struct EffectFrame {
    style: FrameStyle,
    child: Option<AnyElement>,
    effects: Vec<ElementEffect>,
    screen_effect_region: Option<ScreenEffectRegionId>,
    source_location: Option<&'static core::panic::Location<'static>>,
}
```

The paint behavior should remain the current `Effects` behavior:

1. request layout for the optional child
2. establish frame style, transform, opacity, and rounded clip
3. paint visual effects using bounds, corner radii, content mask, and effect list
4. register screen effect region if configured
5. paint the single child inside the inherited content mask

The WGPU renderer split remains untouched: blur/glass source-replacement effects run in the normal visual-effect pass, and glow runs in the additive color pass that preserves destination alpha.

## Animated Frame Design

`AnimatedFrame` should build on `FrameStyle` and element state.

Suggested shape:

```rust
pub struct AnimatedFrame {
    id: ElementId,
    style: FrameStyle,
    child: AnimatedFrameChild,
    animation: FrameAnimation,
    target_key: Option<SharedString>,
    source_location: Option<&'static core::panic::Location<'static>>,
}

enum AnimatedFrameChild {
    None,
    Static(AnyElement),
    WithProgress(Box<dyn FnMut(f32, &mut Window, &mut App) -> AnyElement>),
}

struct AnimatedFrameState {
    from: FrameStyle,
    target: FrameStyle,
    current: FrameStyle,
    target_key: Option<SharedString>,
    started_at: Instant,
    duration: Duration,
    done: bool,
}
```

Implementation notes:

- `AnimatedFrame::id()` returns the stable element id.
- `request_layout` calls `window.with_element_state(...)` using the global element id.
- On first render, initialize state from the target style and render without animation.
- On target style or target key change, start a new animation from the currently interpolated style to the new target style.
- During animation, interpolate compatible `FrameStyle` fields before requesting child layout.
- Request animation frames until progress reaches 1.
- For incompatible value kinds, use the target value and record this behavior in tests.
- If `.child_with_progress(...)` is used, build the child with eased progress before layout so child-owned values such as text size can participate in the same visual transition.

The existing `Animation` and easing functions in `elements/animation.rs` can be reused or factored into a shared `FrameAnimation` type. Avoid introducing an unrelated animation framework in this epic.

## Transition Frame Design

`TransitionFrame` should use the existing screen effect snapshot model.

Suggested shape:

```rust
pub struct TransitionFrame {
    id: ElementId,
    style: FrameStyle,
    child: Option<AnyElement>,
    target_key: SharedString,
    transition: FrameTransition,
    screen_effect_region: Option<ScreenEffectRegionId>,
    source_location: Option<&'static core::panic::Location<'static>>,
}

pub struct FrameTransition {
    enabled: bool,
    kind: FrameTransitionKind,
    duration: Duration,
    origin: FrameTransitionOrigin,
    radius: Pixels,
    amplitude: Pixels,
    wavelength: Pixels,
    damping: f32,
    tint: Option<Hsla>,
}
```

`FrameTransitionKind` should map to existing GPUI screen effects:

- `Crossfade`
- `BlurCrossfade`
- `RadialReveal`
- `RippleReveal`

Optional `Ripple` can be supported if useful, but replacement transitions should prioritize snapshot-backed kinds.

State:

```rust
struct TransitionFrameState {
    target_key: Option<SharedString>,
    active_snapshot: Option<ScreenEffectSnapshotId>,
    active_started_at: Option<Instant>,
    active_duration: Duration,
}
```

Algorithm:

1. Derive a stable region id from the frame id unless `.screen_effect_region(...)` overrides it.
2. On first render, remember the target key and paint normally.
3. On later renders, detect target key changes in element state.
4. When the key changes, call `window.capture_screen_effect_snapshot(ScreenEffectTarget::region(region_id.clone()))`.
5. Build a `ScreenEffect::SnapshotTransition(...)` from the captured snapshot id and configured transition.
6. Target the effect at the same region id.
7. Call `window.play_screen_effect(...)`.
8. Store the active snapshot id and duration in element state.
9. Register the current frame bounds as a screen effect region during paint.
10. Paint the new child.
11. Request animation frames while the transition is active.
12. During later frame passes, release the snapshot once elapsed time is at least duration plus a small safety margin.

This mirrors the navigation transition timing without making `TransitionFrame` depend on a view `Context` or `rayx_runtime`.

## Lab Story Design

Add two component stories:

```text
apps/lab/src/stories/
  animated_frame_story.rs
  transition_frame_story.rs
```

Routes:

- `/stories/animated-frame`
- `/stories/transition-frame`

Story registration:

- import/export in `apps/lab/src/stories/mod.rs`
- route entries in `apps/lab/src/app/story_routes.rs`
- locale entries in `apps/lab/assets/locales/en/story.json`
- locale entries in `apps/lab/assets/locales/hr/story.json`
- manifest updates if generated assets change

The animated story should show an expanded/compact card with a real control. The transition story should show two visually different panels/components toggled by a control. Both stories should be designed as usable examples, not prose pages.

## Testing Strategy

### Unit And Compile Checks

- `effect_frame()` constructor exists and `effects()` no longer resolves.
- `EffectFrame` emits visual effects and screen effect regions.
- `animated_frame()` requires/stores a stable element id.
- `AnimatedFrame` first render does not animate from uninitialized values.
- `AnimatedFrame` interpolates compatible values and snaps incompatible values.
- `AnimatedFrame` handles interrupted transitions.
- `TransitionFrame` skips first-render transitions.
- `TransitionFrame` captures a snapshot when target key changes.
- `TransitionFrame` releases snapshots after duration.
- All frame-family elements remain non-`ParentElement` single-child surfaces.

### Runtime Checks

- Lab animated-frame story visibly animates geometry and child font scale.
- Lab transition-frame story visibly crossfades or reveals replacement content.
- Headed WASM/WebGPU validation loads both routes and checks that the canvas is nonblank.
- DevKit/RayX diagnostics are used where available for live runtime proof.

## Migration Strategy

1. Rename `effects()` to `effect_frame()` first and update all call sites.
2. Add `animated_frame()` using existing frame geometry and animation timing.
3. Add `transition_frame()` using existing screen region and snapshot primitives.
4. Add Lab example pages and route/locale coverage.
5. Update docs, skill guidance, and AgentX memory after implementation confirms final behavior.

## Compatibility Position

No compatibility shim should preserve `effects()`. The clean end-state API is the frame-family API.
