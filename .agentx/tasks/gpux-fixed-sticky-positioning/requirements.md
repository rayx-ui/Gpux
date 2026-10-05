---
agentx:
  kind: knowledge
  scope: repo:tasks/gpux-fixed-sticky-positioning
  status: current
  created: 2026-07-04T06:38:30+02:00
  updated: 2026-07-06T02:30:00+02:00
  topics:
    - gpux-fixed-sticky-positioning
    - requirements
    - positioning
  supersedes: []
  superseded_by: []
---

# GPUI Fixed And Sticky Positioning Requirements

## Scope

Implement first-class CSS-like `position: fixed` and `position: sticky` behavior for gpux `div()` elements.

The epic spans the embedded `gpux` GPUI foundation, normal div visual trees, scroll container tracking, low-level gpux virtual collections, RayX `VirtualList`, `ListView`, and `TreeView`, plus Lab/runtime validation. The public authoring experience must stay simple:

```rust
div().fixed().top_0().left_0()
div().sticky().top_0()
```

The implementation must behave like browser/CSS positioning where feasible, while being explicit about virtualization constraints where elements are not instantiated until the collection renders them.

## Problem Statement

GPUI currently supports only `Position::Relative` and `Position::Absolute`. App authors can build scroll containers and virtualized lists, but they cannot declare viewport-fixed chrome or sticky headers with CSS-like layout semantics on ordinary `div()` elements.

The desired outcome is a clean framework-level positioning model where:

- fixed divs are positioned relative to the active containing block/content mask and are not affected by ancestor scrolling;
- sticky divs keep their normal layout space but visually stick to the nearest scroll container edge once the scroll threshold is reached;
- rendered items inside lists and trees inherit the same behavior automatically;
- virtualized collections render active sticky candidates even when those candidate items would otherwise be outside the visible range.

## In Scope

- Add `Position::Fixed` and `Position::Sticky` to gpux style.
- Add fluent `.fixed()` and `.sticky()` style methods through the existing position style macro surface.
- Preserve current `.relative()` and `.absolute()` semantics.
- Reuse existing inset APIs such as `.top(...)`, `.left(...)`, `.right(...)`, and `.bottom(...)`.
- Implement fixed positioning for divs in normal visual trees.
- Implement sticky positioning for divs in normal visual trees.
- Support sticky positioning against the nearest scroll ancestor tracked during prepaint/paint.
- Support vertical and horizontal sticky axes where GPUI has enough layout information.
- Keep sticky elements in normal layout so their original box reserves space without creating manual placeholder elements.
- Ensure hitboxes, child content, backgrounds, borders, hover, focus, cursor behavior, and event listeners use the effective fixed/sticky visual bounds.
- Ensure fixed elements ignore ancestor scroll offsets while preserving the active containing-block content mask; inside a clipped parent they should anchor to and clip within that parent scrollport.
- Integrate scroll context tracking with scrollable gpux `Div`.
- Integrate scroll context tracking with gpux `uniform_list`, gpux `list`, RayX `VirtualList`, `ListView`, and `TreeView`.
- Extend virtualized collections with explicit sticky candidate metadata or equivalent state-owned knowledge so active sticky items remain rendered while stuck.
- Add tests for style API, layout semantics, hitbox behavior, sticky clamping, fixed containing-block clipping, and virtualized sticky rendering.
- Add Lab stories that prove fixed and sticky behavior in normal trees, nested scroll containers, gpux list/uniform list, RayX VirtualList, ListView, and TreeView.
- Validate affected runtime targets according to repository rules, with WASM/WebGPU as the preferred target for target-agnostic behavior.
- Use RayX diagnostics and temporary traces while diagnosing runtime behavior, then remove trace-only code before sprint completion.

## Out Of Scope

- Implementing every obscure CSS containing-block rule in the first version, such as transformed ancestors becoming fixed containing blocks.
- DOM/CSS overlays as the implementation path.
- Moving sticky elements into separate user-authored overlay trees or adding fake sibling spacer elements for normal div trees.
- Compatibility aliases, obsolete API names, or duplicate positioning surfaces.
- Making arbitrary offscreen virtualized row contents discoverable without collection cooperation. Virtualized sticky support must be explicit because offscreen elements do not exist.
- Replacing GPUI/Taffy layout with a custom layout engine.
- Rewriting ListView or TreeView source-collection architecture beyond the sticky candidate rendering required by this epic.
- Android-specific UI redesign. Android validation is required when mobile/runtime behavior is affected, but the feature itself is framework positioning.

## Actors

- gpux app authors using `div()` directly.
- RayX app authors building scrollable shells, sticky headers, toolbars, list group headers, and tree section headers.
- Component authors for `VirtualList`, `ListView`, `TreeView`, and higher-level controls built on them.
- Renderer/runtime maintainers validating hitboxes, content masks, scroll offsets, and WebGPU/WASM behavior.
- Future coding agents implementing this epic sprint by sprint from `.agentx/tasks/gpux-fixed-sticky-positioning/`.

## Functional Requirements

### Position Style API

- `Position` must include `Fixed` and `Sticky`.
- The existing default remains `Position::Relative`.
- `fixed` and `sticky` must be available as fluent style methods anywhere the existing generated position style methods are available.
- Fixed and sticky must reuse the existing inset model; no separate fixed-only or sticky-only inset APIs should be introduced.
- Public names must be final and simple: `.fixed()` and `.sticky()`.
- No compatibility shim should preserve a temporary or experimental name.

### Layout Semantics

- `Position::Fixed` must be removed from normal flow like CSS fixed/absolute positioning.
- `Position::Sticky` must remain in normal flow like CSS sticky positioning.
- Sticky layout must reserve the element's original normal-flow space without adding an explicit placeholder div.
- Fixed positioning must resolve insets against the active content-mask containing block, which is the viewport when no clipped ancestor is active.
- Sticky positioning must resolve insets against the nearest scroll container on the relevant axis.
- Sticky positioning must clamp at the containing block boundary so the sticky element does not escape the scrollable content section that contains its normal layout box.
- When no relevant inset is specified, sticky behaves as relative on that axis.
- When no scroll ancestor exists, sticky behaves as relative.
- Existing absolute and relative layout behavior must not regress.

### Scroll Container Semantics

- Scrollable divs must push an internal scroll context during prepaint/paint.
- The scroll context must include viewport/scrollport bounds, content bounds or content size, current scroll offset, and active axes.
- Descendant sticky divs must find the nearest relevant scroll context without requiring app authors to pass a `ScrollHandle` to each sticky child.
- Nested scroll containers must resolve sticky descendants to the nearest ancestor scroll context on the relevant axis.
- Parent-viewport virtualized collections must use the parent scroll context rather than applying a second scroll offset.
- Self-scrolling virtualized collections must expose their own scroll context for sticky descendants.

### Fixed Positioning Behavior

- A fixed div must render at its containing-block-relative position regardless of ancestor scroll offset.
- A fixed div inside a clipped or scrollable ancestor must anchor to and clip within that ancestor's active scrollport.
- A fixed div outside clipped ancestors must remain constrained by the window viewport unless a later feature intentionally supports broader overlay surfaces.
- Fixed div hitboxes and event listeners must align with the fixed visual bounds.
- Fixed children must inherit the fixed element's effective visual position.
- Fixed behavior must not globally change deferred drawing semantics; `gpux::deferred(...)` remains mask-preserving by default.

### Sticky Positioning Behavior

- A sticky div must initially render at its normal layout position.
- When scrolled past its sticky threshold, a sticky div must visually hold at the specified inset from the scrollport edge.
- Sticky behavior must preserve the normal layout slot so sibling layout does not collapse or jump.
- Sticky background, border, child content, shadows, cursor, hitboxes, hover, focus, scroll listeners, and child event routing must move together.
- Sticky elements must remain clipped by the relevant scroll container/content mask where CSS-like behavior requires it.
- Sticky elements must release when the containing block end is reached.
- Sticky behavior must work for vertical top/bottom and horizontal left/right axes when those insets are specified.
- If both opposing insets are specified on an axis, the implementation must define and test the resolution behavior. Prefer CSS-compatible behavior where practical, and document any GPUI-specific limitation.

### Virtualized Collection Behavior

- Rendered items in gpux `uniform_list`, gpux `list`, RayX `VirtualList`, `ListView`, and `TreeView` must inherit core fixed/sticky `div()` behavior.
- Virtualized collections must not assume that arbitrary offscreen item content can be inspected without rendering.
- To support sticky items outside the current visible range, virtualized collections must maintain explicit sticky candidate metadata or an equivalent state-owned candidate mechanism.
- Active sticky candidates must be rendered in addition to the visible range while they can affect the viewport.
- Candidate rendering must not create fake spacer scrollers, sibling viewport overlays, or separate app-authored overlay trees.
- Candidate rendering must preserve scroll anchors, visible range events, source collection mutations, and parent-viewport scroll behavior.
- `ListViewState` and `TreeViewState` should expose high-level sticky candidate APIs that are key-aware and stable across insert/remove/move operations.
- Low-level `VirtualList` should expose index/range-based candidate support suitable for explicit-size virtual content.
- gpux `uniform_list` should expose an index-based candidate support path compatible with its fixed item size.
- gpux `list` should support sticky candidates through its existing state/layout mechanisms where feasible.

### Diagnostics And Observability

- Runtime diagnostics should make it possible to inspect sticky/fixed effective bounds during implementation.
- Temporary trace hooks may be added around scroll context push/pop, sticky threshold calculation, fixed containing-block/mask behavior, and virtualized candidate inclusion.
- Temporary traces and auto-enabled debug flags must be removed before each sprint is marked complete unless they become intentional diagnostics.
- Lab stories must provide visible proof of sticky/fixed behavior without relying on in-app instructional prose.

## Non-Functional Requirements

### Performance

- Normal-tree sticky/fixed computation must be lightweight and frame-local.
- Sticky calculations should avoid tree-wide scans and use the current prepaint/paint traversal context.
- Virtualized sticky candidate lookup must be state-owned and proportional to nearby/candidate items, not to rendering every item.
- Sticky candidate rendering must avoid rendering all rows merely to discover sticky descendants.
- WASM/WebGPU validation must remain responsive for large list/tree demos.

### Reliability

- Effective visual bounds used by painting, hit testing, hover, focus, and listener registration must match.
- Scroll context stacks must be correctly balanced even when child prepaint/paint code returns early.
- Missing or invalid sticky candidate metadata must degrade to ordinary virtualized behavior without panics.
- Fixed/sticky behavior must not corrupt `ScrollHandleState.child_bounds`, max offsets, or programmatic scroll-to-item calculations.
- ParentViewport mode must not apply scroll offset twice.

### Maintainability

- Modify gpux only for framework-level behavior unavailable through public APIs.
- Keep the implementation near existing `Style`, `Div`, `Window`, and virtualizer responsibilities.
- Do not add broad abstraction layers unless the same positioning context is reused by multiple affected elements.
- Keep virtualized collection APIs state-owned and aligned with existing `ListViewState`/`TreeViewState` conventions.
- Keep code organized as vertical slices and avoid multiple public types in one file unless they are private or tightly local.

### Compatibility And Migration

- Existing code using `.relative()` and `.absolute()` must continue to compile and behave the same.
- Existing scroll containers and scrollbars must keep their current behavior when no fixed/sticky elements are present.
- Existing ListView/TreeView external scroll handle APIs must remain the single configuration path for parent viewport mode.
- No obsolete aliases or fallback positioning modes should be introduced.

### Accessibility And Interaction

- Fixed/sticky behavior must not create hidden proxy elements, extra hitboxes, or event-forwarding wrappers.
- Keyboard focus and pointer interactions must follow the effective visual position.
- Sticky/fixed positioning must not add in-app text or visual affordances beyond the author's styled element.

### Validation

- Run `cargo fmt --all` after implementation changes.
- Run focused gpux tests for style, div prepaint/paint, window positioning context, and virtualizer behavior.
- Run focused RayX component tests for VirtualList, ListView, and TreeView candidate behavior.
- Run `cargo check --manifest-path apps/lab/Cargo.toml`.
- Run `cargo check --manifest-path apps/lab/platform/web/Cargo.toml`.
- Run `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu` for runtime proof.
- Run `cargo run -p xtask -- app apps/lab run host` for native runtime proof when implementation changes desktop behavior.
- Run Android validation if the change affects gpui_mobile/mobile runtime behavior, or document the exact blocker and run the narrowest available diagnostic route.

## Acceptance Criteria

- A gpux app can compile `div().fixed().top_0().left_0()` and `div().sticky().top_0()`.
- `Position::Fixed` maps to a layout mode that removes the element from normal flow.
- `Position::Sticky` maps to a layout mode that keeps the element in normal flow.
- A fixed child inside a scrollable/clipped parent remains visually fixed to that parent's scrollport while the parent scrolls.
- A sticky child inside a scrollable parent remains in normal flow, sticks at the configured inset, and releases at the containing block boundary.
- Hit testing and click handlers fire at the fixed/sticky visual location, not at the original unstuck location.
- Nested scroll containers use the nearest relevant scroll ancestor.
- Fixed/sticky behavior works for rendered children inside ordinary div trees, gpux `uniform_list`, gpux `list`, RayX `VirtualList`, `ListView`, and `TreeView`.
- Virtualized sticky candidates remain rendered while active even if they are outside the normal visible range.
- ParentViewport ListView/TreeView demos preserve page scroll behavior and do not double-apply scroll offsets.
- Lab includes fixed/sticky demos for normal scroll containers and every target collection family.
- Required validation commands pass or have explicit environment blockers recorded.

## Assumptions

- The implementation treats the active GPUI content mask as the fixed containing block. CSS transformed-containing-block behavior can be added later if required.
- Sticky uses normal-flow layout as the placeholder instead of creating an empty div.
- Virtualized sticky support requires explicit collection candidate metadata because an offscreen unrendered closure cannot reveal arbitrary `div().sticky()` descendants without rendering all items.
- WASM/WebGPU is the preferred runtime proof target for target-agnostic positioning behavior.
- Existing content-mask semantics remain the default; fixed positioning preserves the active mask, while explicit top-layer APIs or `Deferred::unclipped()` handle intentional global overlay escape.

## Risks And Follow-Ups

- Fixed containing-block mask handling must stay aligned between bounds, hitboxes, paint, and children so clipped-parent fixed descendants do not accidentally unclip unrelated content.
- Sticky clamping at containing block boundaries can expose coordinate-space mistakes in nested scroll containers.
- Virtualized collection candidate APIs may need naming iteration to stay simple while covering list, tree, and low-level virtual content.
- gpux `list` has more complex variable-height state than `uniform_list`; its sticky candidate support may require a separate internal candidate cache.
- Browser/CSS parity for mixed `top`/`bottom` or `left`/`right` sticky constraints should be tested before claiming exact parity.
- Android runtime validation may be constrained by local device/emulator availability.
