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
    - requirements
  supersedes: []
  superseded_by: []
---

# Frame, Effects, And Navigation Transition Defaults Requirements

## Summary

Introduce a clean GPUI element split for lightweight containers and shader effects:

- `frame()` is the ultra-light single-child geometry container.
- `effects()` is the single-child shader/screen-effect container built on the same lightweight geometry model.
- shader effects move off broad `div()`/`Styled` APIs.
- navigation gains an app-wide default transition effect and a GPUI helper that automatically captures snapshots when the configured transition requires them.

The goal is a simpler end-state API with no compatibility shims and no hidden layout system expansion.

## Current Source Findings

- Current shader effect APIs are exposed through the broad `Styled` trait in `crates/gpux/crates/gpui/src/styled.rs`.
- Effect state currently lives on `Style` and `StyleRefinement` in `crates/gpux/crates/gpui/src/style.rs`, so any element using `Style::paint` can appear to support effects.
- `Div` in `crates/gpux/crates/gpui/src/elements/div.rs` is intentionally rich: it supports interactivity, text, child collections, prepaint listeners, image cache behavior, flex/grid style surface, and many style APIs.
- Renderer-side shader effect primitives are already separate scene primitives, primarily through `VisualEffectPrimitive` and `ScreenEffectRegionPrimitive`.
- Navigation transitions live in `crates/rayx/rayx_runtime/src/services/navigation/transition.rs`.
- Navigation options and service setup live in `crates/rayx/rayx_runtime/src/services/navigation/service.rs` and `app_ext.rs`.
- Navigation GPUI helpers and region views live in `router.rs` and `region_view.rs`.

## In Scope

1. Add `frame()` as a public GPUI constructor and `Frame` element.
2. Add `effects()` as a public GPUI constructor and `Effects` element.
3. Make both `frame` and `effects` single-child containers with `.child(...)` only.
4. Do not implement `ParentElement` for `frame` or `effects`.
5. Do not add `.children(...)` to `frame` or `effects`.
6. Support size, position, inset, transform, opacity, radius, and clipping on both `frame` and `effects`.
7. Implement rounded clipping so radius can clip the contained child subtree and shader effects.
8. Move shader effect APIs off `Styled` and off ordinary `div()` usage.
9. Preserve the renderer primitive model for visual effects and screen effect regions.
10. Preserve WGPU visual-effect composition semantics, including glow as a separate additive color pass that preserves destination alpha.
11. Rename or restructure the internal GPUI effects module if needed so `effects()` can be the clean public element constructor.
12. Add `NavigationOptions.default_transition`, defaulting to no transition.
13. Allow `RayXAppBuilder` navigation configuration to set the app-wide default navigation transition.
14. Make navigation region/router builders consume the configured default unless explicitly overridden.
15. Add a GPUI/window-aware navigation helper that automatically captures a screen-effect snapshot when the effective transition requires one.
16. Update tests, examples, lab stories, and documentation references that currently use div/styled shader effects.

## Out Of Scope

1. Do not add flex, grid, text, background, border, shadow, scrolling, interactivity, image cache, or event APIs to `frame`.
2. Do not add flex, grid, text, background, border, shadow, scrolling, interactivity, image cache, or event APIs to `effects`.
3. Do not keep obsolete aliases, compatibility shims, duplicate APIs, or adapter wrappers.
4. Do not preserve div shader effect APIs once `effects()` is available.
5. Do not implement route retention, route caching, or view lifecycle changes beyond what transitions need.
6. Do not introduce a multi-child lightweight container in this epic.
7. Do not redesign the shader effect language or renderer algorithms except where required by clipping and API ownership.

## Functional Requirements

### Frame Element

1. `gpui::frame()` creates a `Frame` element.
2. `Frame` accepts at most one child through `.child(...)`.
3. Calling `.child(...)` more than once must be treated as programmer error, preferably by panicking in debug/test builds.
4. `Frame` must not implement `ParentElement`.
5. `Frame` must not expose `.children(...)`.
6. `Frame` must expose only lightweight geometry and compositing APIs:
   - size
   - position
   - inset
   - transform
   - opacity
   - radius
   - clipping
7. `Frame` must not inherit the full `Styled` API.
8. `Frame` must not paint its own text, background, border, shadow, or shader effects.
9. `Frame` must be usable as a structural container around any GPUI element or component that implements `IntoElement`.
10. `Frame` layout must remain predictable and simple: one container node, one optional child node, no flex/grid child collection behavior.

### Effects Element

1. `gpui::effects()` creates an `Effects` element.
2. `Effects` follows the same single-child rules as `Frame`.
3. `Effects` exposes the same lightweight geometry and compositing APIs as `Frame`.
4. `Effects` additionally exposes shader visual effect APIs.
5. `Effects` additionally exposes screen effect region APIs.
6. `Effects` must be the normal public way to apply element shader effects to arbitrary child content.
7. `Effects` must be the normal public way to mark a child subtree as a screen effect region.
8. `Effects` must not implement `ParentElement`.
9. `Effects` must not expose `.children(...)`.
10. `Effects` must not expose broad div styling APIs.
11. Migrating effect ownership to `Effects` must not change existing ordered effect semantics for blur, glass, glow, or stacked effects.
12. WGPU glow must remain additive: the normal visual-effect source-replacement pass ignores glow, and a separate glow pass outputs only glow energy with additive color blending and destination alpha preserved.

### Rounded Clipping

1. `radius(...)` plus clipping must clip contained child content.
2. `radius(...)` plus clipping must clip shader effects owned by `effects()`.
3. Clipping must work for arbitrary child primitives, not only for the immediate child element.
4. Clipping must preserve the existing rectangular clipping behavior where radius is zero.
5. Renderer scene data must carry enough clip information to apply rounded clipping correctly.
6. WGPU/WASM rendering must support rounded clipping for the affected primitives.
7. Native renderer behavior must be audited and brought to parity or explicitly tracked if a backend requires follow-up work.

### Styled And Div API Cleanup

1. Remove public shader effect methods from `Styled`.
2. Remove public screen effect region methods from `Styled`.
3. Remove effect state from `Style`/`StyleRefinement` unless a narrow private renderer bridge remains necessary.
4. Remove div-specific shader effect tests or migrate them to `effects()`.
5. Update call sites to wrap content in `effects().child(...)` instead of attaching effects to `div()`.
6. Avoid compatibility aliases such as old div effect methods forwarding to `effects()`.

### Navigation Default Transition

1. `NavigationOptions` has a default transition configuration.
2. The default value is no transition.
3. Navigation setup through `RayXAppBuilder` can configure the default transition app-wide.
4. Navigation router and region view builders use the configured default unless an explicit local transition is provided.
5. Explicit builder transition configuration continues to override the app-wide default.
6. Existing no-transition navigation behavior remains the default.
7. The default transition model must be serializable or use a serializable options representation compatible with existing navigation configuration.

### Snapshot-Aware Navigation Helper

1. Add a GPUI/window-aware helper because automatic snapshot capture needs `Window`.
2. The helper resolves the effective transition before navigation starts.
3. If the effective transition does not require a snapshot, the helper records the transition origin when needed and performs navigation.
4. If the effective transition requires a snapshot, the helper captures the configured target before navigation mutates the route.
5. The helper stores the snapshot in the same transition service path used by existing snapshot transitions.
6. The helper clears any captured snapshot and origin if navigation fails.
7. The helper supports global navigation and named-region navigation.
8. The helper has clear API names that distinguish snapshot-aware behavior from pure service navigation.

## Non-Functional Requirements

1. Keep implementation minimal and vertical-slice oriented.
2. Prefer existing GPUI element patterns, especially `DivElement`, for lifecycle and layout integration.
3. Do not introduce broad abstraction layers just to share small amounts of code.
4. Keep public APIs clean and final; do not add temporary compatibility surfaces.
5. Preserve existing renderer primitive architecture where practical.
6. Validate with source-level tests and actual runtime diagnostics for affected targets.
7. Run `cargo fmt --all` after code changes.
8. Run targeted tests for GPUI elements, shader effects, and navigation transitions.
9. Run a WASM/WebGPU runtime validation path for shader/effect behavior.
10. Run a demo application to confirm no runtime crash.

## Acceptance Criteria

1. `frame()` exists and can wrap one child.
2. `frame().children(...)` is not available.
3. `effects()` exists and can wrap one child.
4. `effects().children(...)` is not available.
5. `div()` no longer exposes shader effect APIs.
6. Existing shader effect demos and tests use `effects()`.
7. Rounded clipping clips both child content and effects.
8. Navigation can be configured with an app-wide default transition.
9. The default navigation transition is none unless configured otherwise.
10. Router/region builders honor the configured default and still allow explicit overrides.
11. Snapshot-requiring default transitions work through the GPUI helper without callers manually capturing snapshots.
12. Tests cover one-child behavior, effect migration, clipping metadata, navigation defaults, and helper cleanup on failure.
13. Runtime validation confirms the demo app does not crash.
14. WGPU tests or renderer assertions cover additive glow blending so expanded glow geometry cannot overwrite neighboring glass or blur content.

## Risks

1. Rounded clipping may require renderer changes deeper than the current effect API migration.
2. Removing `Styled` effect methods may reveal non-div elements that were unintentionally using effects.
3. `NavigationOptions` currently derives `Eq`; transition configuration contains float-like fields and may require changing equality semantics or introducing a serializable options model.
4. The internal GPUI `effects` module name may collide with the desired public `effects()` constructor.
5. Snapshot-aware navigation helpers must avoid capturing after route mutation; ordering mistakes would make transitions visually incorrect.

## Open Implementation Decisions

1. Whether the internal GPUI effect data module should be renamed to `shader_effects` or `visual_effects`.
2. Whether rounded clipping is represented by extending `ContentMask` or by adding a new clip mask/scope type.
3. Whether `NavigationTransition::disabled()` is replaced by `NavigationTransition::none()` as part of the no-shim cleanup.
4. Whether app-wide default transition is stored directly as `NavigationTransition` or as a separate serializable `NavigationTransitionOptions`.
