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
    - requirements
    - effects
  supersedes: []
  superseded_by: []
---

# Shader Effects And Screen Transitions Requirements

Final implementation note: early sketches in this completed task refer to `div().effect(...)`, `div().effects(...)`, and broad `Styled` APIs. The accepted final architecture supersedes those sketches with the frame-family API: use `effect_frame().effect(...)` / `effect_frame().effects(...)` for shader effects, `frame()` for lightweight geometry/clipping, `animated_frame(id)` for frame-property animation, and `transition_frame(id)` for snapshot-backed replacement transitions.

## Scope

Implement first-class shader-backed visual effects for GPUI `div()` elements and screen-level post-processing effects for whole-window or region-scoped UI. The initial user-facing effects are CSS-like backdrop blur, Apple-style glass with fast distortion/refraction, glow, click-triggered animated ripple distortion, and navigation transition hooks that can use screen effects.

The epic spans embedded `gpux` framework rendering, RayX navigation integration, and Lab/example validation. It does not implement arbitrary user-authored shader languages.

## Problem Statement

RayX and gpux currently support styled `div()` surfaces, shadows, gradients, canvas demos, and mobile glass-like component styling, but there is no reusable renderer-level effect pipeline that samples already-painted content behind an element or post-processes a full screen/region. Existing demos draw decorative ripples through canvas-like painting, which is useful as reference material but cannot provide true backdrop blur, refraction, or transition distortion over arbitrary UI content.

The desired outcome is a clean Rust API that lets app authors write fluent UI code such as `div().effect(backdrop_blur(...)).effect(glow(...))` or `div().effects((backdrop_blur(...), glow(...)))`, plus window/region screen effects such as click ripples and navigation transitions.

## In Scope

- Add a public gpux effect model for element effects and screen effects.
- Add fluent `div()`/`Styled` APIs for one or more element effects.
- Implement CSS-like backdrop blur for a div by sampling content already painted behind that div.
- Implement Apple-style glass as a fast composited effect with tint, blur, highlight, distortion, and refraction controls.
- Implement glow as a reusable effect that can be stacked with blur and glass.
- Add screen effects that target the whole window, explicit bounds, or named screen-effect regions.
- Add an animated ripple distortion screen effect that can be triggered from mouse/touch clicks.
- Add RayX navigation integration through GPUI-facing router/region surfaces, not through retained route state inside `NavigationService`.
- Add a Lab story demonstrating div effects, screen ripples, region effects, and navigation transitions.
- Validate WGPU/WASM with headed Playwright and WebGPU enabled; validate native/mobile targets as each renderer backend becomes supported.

## Out Of Scope

- Arbitrary runtime WGSL/Metal/HLSL shader authoring by app authors.
- DOM/CSS `backdrop-filter` integration or browser DOM overlays as the implementation path.
- Fake native backend parity through static colors or non-shader approximations that claim to be glass/refraction.
- Retained route view caches inside `NavigationService`.
- Full old/new page snapshot transitions in the first runnable increment; those are planned after the screen-effect foundation.
- Compatibility aliases for misspelled or obsolete API names.

## Actors

- App authors using gpux directly.
- RayX app authors using `NavigationRegionView`, `NavigationRouter`, and Lab-style story shells.
- Renderer/backend maintainers implementing WGPU, Metal, DirectX, and mobile behavior.
- Test/diagnostic agents validating runtime output with DevKit, Playwright, and target-specific tooling.

## Functional Requirements

### Element Effect API

- App authors must be able to attach a single effect to any styled `div()` with a fluent method such as `.effect(backdrop_blur(px(18.)))`.
- App authors must be able to attach multiple effects in deterministic order with a Rust-idiomatic API such as `.effects((backdrop_blur(px(18.)), glass().refraction(0.04), glow(color).radius(px(24.))))`.
- The same effect API should live on the `Styled` surface where practical so wrappers that carry `Style` can opt in without special-case div-only code.
- Effect order must be observable and documented. Earlier effects paint before later effects at the same element effect slot.
- Effects must respect element bounds, border radii, transforms, opacity, and content masks consistently with existing `Style::paint` behavior.
- Existing background, child content, and border rendering must keep their current semantics. Backdrop/glass effects paint between shadows and the element background so app authors can still use `.bg(...)` as an extra tint layer.

### Element Effects

- `backdrop_blur(radius)` must blur content behind the element similarly to browser CSS backdrop blur, clipped to the element shape.
- `backdrop_blur` must provide controls for blur radius and optional saturation/brightness/tint where those controls are renderer-supported.
- `glass()` must combine backdrop blur, tint, highlight, optional noise/distortion, and refraction-style sampling offset. It must be fast enough for mobile WebGPU and should expose quality/performance presets.
- `glow(color)` must render a halo around the element or its rounded bounds with radius, spread, intensity, and blend controls.
- Effects must be serializable/debuggable enough for DevKit or renderer diagnostics to report which effect primitives were emitted.

### Screen Effect API

- App authors must be able to play an effect on the whole window through `Window` or a close GPUI rendering primitive, for example `window.play_screen_effect(screen::ripple(position))`.
- App authors must be able to restrict a screen effect to an explicit bounds rectangle.
- App authors must be able to name a screen-effect region from an element, for example `div().screen_effect_region("content")`, and later target it.
- Region-targeted effects must use the most recent painted bounds and content mask for that region.
- If a target region is not present in the current frame, the effect must fail transparently with diagnostics or no-op behavior that is explicitly reported as unsupported/missing, not silently retarget to unrelated content.
- Multiple screen effects may be active at once, and the system must define capping behavior for active effects so runaway click streams do not allocate without bound.

### Ripple Effect

- The first screen effect must be an animated ripple distortion.
- A ripple must have origin, start time, duration, radius, amplitude, wavelength, damping, and optional chromatic/tint controls.
- Ripples must animate without app authors manually requesting every frame after enqueueing the effect.
- Click and touch input handlers must be able to pass window-relative pointer coordinates directly to the effect.
- Multiple active ripples should compose deterministically and be capped to a configurable maximum.

### Navigation Transitions

- RayX navigation transitions must integrate through `NavigationRouter`, `NavigationRegionView`, or an adjacent GPUI-facing helper.
- `NavigationService` must remain route state, matching, journaling, synchronization, and subscription infrastructure. It must not retain route views or region content.
- The first navigation integration may enqueue a post-navigation screen effect over the active region or window.
- Later snapshot-based transitions must support old-to-new page effects without storing retained route entities inside `NavigationService`.
- Navigation transition APIs must support route changes triggered by normal RayX navigation calls and explicit "navigate with transition" helpers where a pre-navigation snapshot is needed.

## Non-Functional Requirements

### Performance

- WGPU must be the first backend because it covers WASM and gpui_mobile paths.
- Backdrop blur should use a bounded offscreen/post-process path such as downsampled separable blur or dual-Kawase blur rather than full-resolution large-kernel sampling.
- The renderer must avoid sampling from the same texture it is currently writing to; use offscreen/ping-pong targets or pass boundaries as needed.
- Effect resources must be cached by viewport size, scale factor, format, and quality tier, and recreated on resize or device loss.
- Blur and screen-effect work should use scissor/bounds-limited passes where feasible after correctness is proven.

### Reliability

- Renderer capabilities must be explicit. Unsupported backends should report unsupported effect capabilities rather than drawing misleading approximations.
- WebGPU/WGSL storage-buffer and uniform layouts must match Rust `Pod` structs exactly, with size/offset assertions.
- Browser-active effect timing must avoid unsupported `std::time` clocks and use gpux/browser-safe elapsed time.
- Active animations must stop requesting frames after all effects expire.

### Maintainability

- gpux changes must stay limited to framework-level rendering behavior that RayX cannot implement through public APIs.
- RayX integration must remain a vertical slice in the navigation module and Lab app, not a central app-builder or service-registry rewrite.
- Public API names should be final and clean in the first implementation; do not introduce temporary aliases.
- Each effect should have focused builder/default tests close to the public API and renderer layout tests close to the backend implementation.

### Observability And Diagnostics

- Lab must expose a story where each effect can be toggled and inspected.
- WGPU diagnostics should expose effect support, active effect counts, region registration, and shader/pipeline failures.
- Playwright validation should capture screenshots, console output, and JSON diagnostics for WASM/WebGPU.
- Temporary trace hooks added during implementation must be removed before sprint completion unless they become intentional diagnostics.

### Accessibility And Interaction

- Element effects must not add hitboxes, focus behavior, pointer interception, or hidden proxy elements.
- Screen effects must be visual-only unless explicitly tied to an input handler by app code.
- Reduced-motion or animation-disable hooks should be considered for RayX app-level policy before enabling navigation transitions by default.

## Acceptance Criteria

- A gpux app can compile code that attaches `backdrop_blur`, `glass`, and `glow` to a `div()`.
- Multiple div effects render in deterministic order and preserve existing child/background/border behavior.
- In WGPU/WASM, backdrop blur visibly samples content behind the element rather than only blurring the element's own background.
- In WGPU/WASM, glass distortion/refraction visibly changes sampled background content while remaining clipped to the element shape.
- In WGPU/WASM, `window.play_screen_effect(screen::ripple(...))` creates a visible animated ripple at the click/touch point.
- Region-targeted ripples affect only the named region bounds.
- NavigationRegionView or NavigationRouter can trigger a ripple/transition effect on route change without modifying `NavigationService` to retain views.
- Lab includes an interactive story for div effects and screen effects.
- `cargo fmt --all`, focused Rust tests, relevant cargo checks, and headed `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu` are part of the implementation validation.
- Unsupported backends expose capability diagnostics and do not present shader effects as fully supported until their renderer path is implemented.

## Assumptions

- The initial public API will use Rust-friendly `.effect(...)` and `.effects((...))` shapes rather than variadic `.effects(a, b, c)`, because Rust does not support variadic methods.
- `backdrop_blur` is the canonical public name for CSS-like background/backdrop blur.
- WGPU first is acceptable because it gives the earliest proof for browser and gpui_mobile targets.
- Native Metal and DirectX parity can land after the WGPU behavior is correct and tested.
- Navigation transition defaults should be opt-in until reduced-motion and performance behavior are proven.

## Risks And Follow-Ups

- True backdrop blur requires careful render-pass sequencing and offscreen textures; naive same-target sampling is invalid and may fail WebGPU validation.
- Snapshot-based page transitions may need renderer-managed previous-frame or region texture capture before navigation state mutates.
- High blur radii and many active ripples can be expensive on mobile GPUs; quality presets and caps are required.
- Metal and DirectX parity may expose shader layout or coordinate-system differences after the WGPU design is proven.
- Existing gpui_mobile glass components are styling references, not proof of renderer-level glass effects.
