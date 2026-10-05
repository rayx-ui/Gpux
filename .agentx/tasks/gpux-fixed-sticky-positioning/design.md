---
agentx:
  kind: knowledge
  scope: repo:tasks/gpux-fixed-sticky-positioning
  status: current
  created: 2026-07-04T06:38:30+02:00
  updated: 2026-07-06T02:30:00+02:00
  topics:
    - gpux-fixed-sticky-positioning
    - design
    - positioning
  supersedes: []
  superseded_by: []
---

# Design: GPUI Fixed And Sticky Positioning

## Current-State Summary

Current source inspection identifies these implementation surfaces:

- `crates/gpux/crates/gpui/src/style.rs` owns `Style`, default style values, `Position`, inset fields, paint behavior, and conversion to Taffy style.
- `crates/gpux/crates/gpui_macros/src/styles.rs` generates position style methods. It currently generates only `.relative()` and `.absolute()`.
- `crates/gpux/crates/gpui/src/styled.rs` exposes generated position style methods on the styled surface.
- `crates/gpux/crates/gpui/src/taffy.rs` converts gpux style into Taffy layout style.
- `crates/gpux/crates/gpui/src/elements/div.rs` owns `Div`, `Interactivity`, scroll tracking, child layout/prepaint/paint, hitbox insertion, overflow masks, and `ScrollHandle`.
- `crates/gpux/crates/gpui/src/window.rs` owns layout bounds, element offsets, content masks, deferred drawing, paint/prepaint state, and scene insertion.
- `crates/gpux/crates/gpui/src/elements/deferred.rs` already supports mask-preserving deferred drawing and explicit `.unclipped()` overlays.
- `crates/gpux/crates/gpui/src/elements/uniform_list.rs` owns fixed-size virtual list rendering and only instantiates visible ranges.
- `crates/gpux/crates/gpui/src/elements/list.rs` owns gpux variable-height virtual list rendering and only instantiates visible/focused ranges.
- `crates/rayx/rayx_components/rayx_components_core/src/virtual_list/mod.rs` owns the RayX explicit-size `VirtualList`, `VirtualListScrollHandle`, `VirtualListExternalScrollBehavior`, ParentViewport mode, and visible item prepaint/paint.
- `crates/rayx/rayx_components/rayx_components_core/src/list_view/state.rs` owns high-level keyed ListView state, visible range math, external scroll handling, and rendering through `virtual_list`.
- `crates/rayx/rayx_components/rayx_components_core/src/tree_view/state.rs` owns high-level keyed TreeView projection, expansion, visible range math, external scroll handling, and rendering through `virtual_list`.
- Lab stories under `apps/lab/src/stories` already validate high-level list/tree and parent-scroll virtualization behavior.

AgentX memory adds these constraints:

- gpux should be modified only for framework-level behavior unavailable through public APIs.
- `gpux::deferred(...)` must remain content-mask-preserving by default; `.unclipped()` is only for intentional top-level overlays.
- ParentViewport virtualized components must remain in normal flow and derive visibility from the parent content mask. Do not add fake spacer scrollers or sibling viewport overlays.
- ListView/TreeView external scrolling must remain state-owned through `use_external_scroll_handle(handle, VirtualListExternalScrollBehavior::ParentViewport)`.
- WASM/browser validation should use `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`.
- Runtime feature work must use RayX diagnostics or target-specific runtime proof, not compile checks alone.

## Proposed Architecture

Implement positioning in gpux as a framework-level feature, then extend virtualized collection renderers so sticky candidates continue to exist when needed.

The architecture has three cooperating pieces:

1. Position style contract: add `Fixed` and `Sticky` to gpux `Position` and fluent style APIs.
2. Frame-local positioning context: add internal `Window` scroll/position context stacks that let `Div` compute effective visual bounds during prepaint/paint.
3. Virtualized sticky candidate rendering: teach virtualizers to include explicit active sticky candidates in their rendered item set in addition to the ordinary visible range.

Normal div trees should not need manual placeholders or app-authored overlay containers. Sticky keeps its Taffy layout slot and applies an effective visual offset while painting/prepainting. Fixed ignores ancestor scroll offsets but resolves against and preserves the active content-mask containing block.

## Public API

### Position

Extend the existing position enum:

```rust
pub enum Position {
    Relative,
    Absolute,
    Fixed,
    Sticky,
}
```

Style method usage:

```rust
div()
    .fixed()
    .top_0()
    .left_0()
    .w_full()
    .child(toolbar)
```

```rust
div()
    .sticky()
    .top_0()
    .child(section_header)
```

Taffy mapping:

```rust
impl From<Position> for taffy::style::Position {
    fn from(position: Position) -> Self {
        match position {
            Position::Relative => Self::Relative,
            Position::Absolute => Self::Absolute,
            Position::Fixed => Self::Absolute,
            Position::Sticky => Self::Relative,
        }
    }
}
```

Rationale:

- Fixed is out-of-flow, so Taffy absolute layout is the closest layout-level representation.
- Sticky is in-flow, so Taffy relative layout naturally reserves the placeholder slot.
- Runtime prepaint/paint handles the visual movement that Taffy does not know about.

### Virtualized Sticky Candidate APIs

Core sticky works for any rendered subtree. Virtualized collections need explicit candidate knowledge because invisible rows are not instantiated.

Recommended API shape:

```rust
VirtualList::new(...)
    .sticky_items([0, 10, 25]);
```

For high-level state:

```rust
list_state.set_sticky_keys([group_key]);
tree_state.set_sticky_keys([node_key]);
```

Builder-style variants can be added if that matches existing state construction style:

```rust
let state = ListViewState::new(items, |item| item.id)
    .item_height(|item, _| item.row_height)
    .sticky_keys(group_keys);
```

Candidate APIs should be additive and optional. Ordinary virtualized lists without sticky candidates keep their current behavior.

## Internal Data Model

### Positioning Contexts

Add internal frame-local context types under gpux, likely near `window.rs` or a focused `positioning.rs` module:

```rust
pub(crate) struct ScrollContainerContext {
    pub scrollport_bounds: Bounds<Pixels>,
    pub content_bounds: Bounds<Pixels>,
    pub scroll_offset: Point<Pixels>,
    pub overflow: Overflow,
}

pub(crate) struct PositionedBounds {
    pub layout_bounds: Bounds<Pixels>,
    pub visual_bounds: Bounds<Pixels>,
    pub delta: Point<Pixels>,
    pub content_mask_override: Option<ContentMaskMode>,
}
```

The exact type names can change, but the responsibilities should remain:

- scroll contexts are pushed by scroll containers during prepaint/paint;
- sticky descendants query the nearest matching scroll context;
- fixed descendants resolve against the active content-mask containing block, which defaults to the viewport;
- the effective visual delta is applied consistently to interactivity and children.

### Content Mask Behavior

Fixed positioning resolves against `Window::content_mask().bounds` and preserves the same mask for prepaint, paint, hitboxes, and children. The mask is the viewport when no clipped ancestor is active and the clipped scrollport when the fixed element is authored inside an overflow-clipped parent. Do not add a fixed-only viewport mask override; use top-layer APIs or `Deferred::unclipped()` for intentional global overlays.

### Prepaint State

`Div::prepaint` must store enough effective positioning state for `Div::paint` to use the same visual bounds.

The current `Div` flow computes:

1. layout id;
2. `bounds = window.layout_bounds(layout_id)`;
3. child bounds/content size;
4. scroll offset;
5. `Interactivity::prepaint(...)`;
6. child prepaint inside `window.with_element_offset(scroll_offset, ...)`;
7. `paint` later reuses interactivity state.

The fixed/sticky extension should compute an effective `position_delta` before interactivity prepaint:

```rust
let layout_bounds = window.layout_bounds(layout_id);
let positioning = window.resolve_positioned_bounds(&style, layout_bounds);
let visual_bounds = positioning.visual_bounds;
let child_offset = scroll_offset + positioning.delta;
```

Then:

- pass `visual_bounds` to `Interactivity::prepaint`;
- prepaint children with `window.with_element_offset(child_offset, ...)`;
- store `positioning` in prepaint state;
- paint with the same visual bounds and mask mode.

This avoids creating proxy elements and keeps hitboxes, painting, and child coordinates aligned.

## Fixed Positioning Design

### Layout

`Position::Fixed` maps to Taffy absolute. Existing inset fields drive the fixed layout size and preferred position where possible.

### Visual Position

During prepaint:

1. Read the element's layout bounds.
2. Resolve fixed insets against the active content-mask containing block.
3. Compute `delta = fixed_origin - layout_bounds.origin`.
4. Preserve the active content mask for hitboxes, paint, and children.
5. Apply the delta to the element's interactivity and children.

Fixed elements are content-mask-containing-block-relative in the current implementation. A future design can add CSS transformed ancestor containing blocks if current source introduces transform contexts that require parity.

### Paint And Hit Testing

The same visual bounds must drive:

- background/border/shadow paint;
- child prepaint/paint;
- hitbox insertion;
- cursor/hover/focus listeners;
- scroll listeners attached to the fixed element;
- DevKit bounds if DevKit snapshots are affected.

Fixed divs must not add hidden forwarding elements.

## Sticky Positioning Design

### Layout

`Position::Sticky` maps to Taffy relative, so the natural layout box remains in flow.

### Nearest Scroll Context

During scrollable `Div` prepaint/paint, push a scroll context around children:

```rust
window.with_scroll_container_context(context, |window| {
    window.with_element_offset(scroll_offset, |window| {
        child.prepaint(window, cx);
    });
});
```

The context should represent the scrollport bounds before applying child scroll offset and the content bounds/content size after measuring children.

For virtualized collections, push an equivalent context around rendered items. In ParentViewport mode, reuse or derive from the parent viewport metrics rather than applying a second scroll offset.

### Sticky Offset Calculation

For each axis:

1. If the style has no inset for that axis, return no sticky delta.
2. Find the nearest scroll context that scrolls or clips on that axis.
3. Compute the desired sticky edge from scrollport edge plus inset.
4. Compare the current layout/visual edge against the desired sticky edge.
5. Clamp the desired origin so the element cannot escape its containing block.
6. Return the axis delta.

The effective visual position is:

```rust
visual_origin = layout_origin + sticky_delta;
```

Because GPUI's `window.layout_bounds(...)` already includes current element offset, the sticky resolver should work in the same coordinate space as existing prepaint bounds.

### Containing Block Clamp

Sticky must release at the end of its containing block. The first implementation can use the nearest parent scroll content bounds as the containing boundary for ordinary scroll containers. If a tighter CSS containing block is needed later, add an explicit ancestor/position context rather than guessing from arbitrary layout parents.

For grouped list/tree headers, the virtualized candidate metadata should supply enough item extent information to clamp within the intended group when group-level sticky headers are required.

## Virtualized Collection Design

### Why Explicit Candidates Are Required

Virtualizers currently render only visible ranges. An offscreen row closure has no element tree and therefore cannot be searched for `div().sticky()`.

Therefore, browser-like sticky behavior inside virtualized collections requires the collection to keep active sticky candidates rendered. The collection must know candidate indices/keys from state or author metadata.

This is a real architectural constraint, not an implementation shortcut.

### Candidate Model

Use a small internal model:

```rust
struct StickyCandidate<I> {
    index_or_key: I,
    axis: Axis,
    inset: Option<Pixels>,
    extent: Option<Range<Pixels>>,
}
```

The public API does not need to expose this exact struct. It can expose simple keys/indices first and derive axis/inset from rendered sticky divs once the item is rendered.

Recommended staged behavior:

1. Candidate metadata identifies which items may contain sticky divs.
2. The virtualizer includes active candidate items in the render set.
3. The normal gpux sticky div implementation computes visual sticking.
4. The virtualizer keeps candidate items alive while their sticky range intersects the viewport.

### RayX VirtualList

`VirtualList` already owns explicit item sizes, visible range calculation, external scroll behavior, and item prepaint bounds.

Add:

- optional sticky candidate indices/ranges;
- a render range union step that combines visible range plus active sticky candidates;
- prepaint/paint bookkeeping that distinguishes visible range events from extra sticky candidate rendering;
- tests for self-scroll and ParentViewport modes.

### ListView

`ListViewState` owns stable keys, source mutations, item sizes, visible range math, and external scroll handles.

Add:

- key-based sticky candidate storage;
- APIs to set/add/remove sticky keys;
- mapping from keys to current indices after insert/remove/move/source changes;
- integration into the `virtual_list` render range/candidate path;
- tests preserving scroll anchors and visible range semantics.

### TreeView

`TreeViewState` owns tree projection, visible indices, expansion, and stable keys.

Add:

- key-based sticky candidate storage for visible projected nodes;
- behavior when a sticky key is collapsed/hidden;
- mapping through expansion/collapse and source mutation;
- integration into the `virtual_list` candidate path;
- tests proving expanded/collapsed tree behavior and sticky candidates do not resurrect hidden nodes.

### gpux UniformList

`uniform_list` has fixed item size and simpler visible-range math.

Add:

- optional sticky candidate indices;
- union visible range plus active candidate indices;
- render/prepaint those candidates with correct item origins;
- keep existing scroll strategy behavior intact.

### gpux List

`list` has variable-height internal state, cached layouts, focus support, and visible item prepaint.

Add:

- optional sticky candidate identity/index support compatible with existing list state;
- candidate measurement reuse where possible;
- active candidate inclusion similar to focused offscreen item logic;
- tests focused on variable-height sticky headers and scroll-to behavior.

## Affected Files And Modules

Expected gpux files:

- `crates/gpux/crates/gpui/src/style.rs`
- `crates/gpux/crates/gpui/src/styled.rs`
- `crates/gpux/crates/gpui/src/taffy.rs`
- `crates/gpux/crates/gpui_macros/src/styles.rs`
- `crates/gpux/crates/gpui/src/elements/div.rs`
- `crates/gpux/crates/gpui/src/window.rs`
- `crates/gpux/crates/gpui/src/elements/uniform_list.rs`
- `crates/gpux/crates/gpui/src/elements/list.rs`
- gpux tests adjacent to these files or existing test modules.

Expected RayX files:

- `crates/rayx/rayx_components/rayx_components_core/src/virtual_list/mod.rs`
- `crates/rayx/rayx_components/rayx_components_core/src/list_view/state.rs`
- `crates/rayx/rayx_components/rayx_components_core/src/list_view/element.rs`
- `crates/rayx/rayx_components/rayx_components_core/src/tree_view/state.rs`
- `crates/rayx/rayx_components/rayx_components_core/src/tree_view/element.rs`
- Lab story files under `apps/lab/src/stories`
- Playwright validation files under `tools-node/` only if the existing WASM suite needs new probes.

## Testing Strategy

### Unit And Integration Tests

Add focused tests for:

- `Position::Fixed` and `Position::Sticky` style API compile and set the expected style variant.
- Taffy mapping maps fixed to absolute and sticky to relative.
- Sticky without a scroll context behaves like relative.
- Sticky with `top` sticks at the scrollport top inset.
- Sticky releases at the containing block end.
- Fixed ignores ancestor scroll offset.
- Fixed ignores ancestor content masks but respects viewport bounds.
- Hitboxes follow fixed/sticky visual bounds.
- Nested scroll containers select the nearest relevant scroll context.
- Horizontal sticky with `left` works in horizontal scroll.
- Existing absolute/relative tests still pass.

### Virtualization Tests

Add focused tests for:

- `VirtualList` renders visible range plus active sticky candidate.
- `VirtualListExternalScrollBehavior::ParentViewport` does not double-apply offsets.
- `ListViewState` sticky keys survive insert/remove/move and source updates.
- `TreeViewState` sticky keys follow projection changes and hidden/collapsed nodes do not render as sticky candidates.
- gpux `uniform_list` active candidate inclusion.
- gpux `list` variable-height candidate inclusion.
- Visible range events continue to report logical visible ranges, not extra sticky candidates, unless a deliberate diagnostics field is added.

### Runtime Tests

Add Lab stories for:

- normal scroll container with sticky section header;
- nested scroll containers with inner and outer sticky headers;
- viewport fixed toolbar inside a scrolled/clipped parent;
- horizontal sticky column/row case if component styling permits;
- gpux uniform list sticky row;
- gpux list sticky variable-height row;
- RayX VirtualList sticky item;
- ListView group header sticky key;
- TreeView section/header sticky key;
- ParentViewport list/tree sticky behavior inside the story page scroll container.

WASM validation should include:

- WebGPU enabled;
- no page errors;
- nonblank canvas;
- scroll actions that move ordinary content while sticky/fixed items remain in expected visual positions;
- browser-readable diagnostics or pixel/bounds checks when available.

### Validation Commands

Implementation sprints should run the narrowest relevant checks first, then broaden:

```powershell
cargo fmt --all
cargo test -p gpux
cargo test -p rayx_components_core
cargo check --manifest-path apps/lab/Cargo.toml
cargo check --manifest-path apps/lab/platform/web/Cargo.toml
cargo run -p xtask -- app apps/lab test wasm --headed --webgpu
cargo run -p xtask -- app apps/lab run host
```

Use Android validation when implementation affects mobile runtime behavior:

```powershell
cargo run -p xtask -- app apps/lab run android
```

If the exact package names or test commands differ during implementation, use the current project files as the source of truth.

## Rollout And Migration

This is an additive public API with no compatibility shims.

Rollout order:

1. Add public enum variants and style methods.
2. Land normal-tree fixed/sticky behavior behind tests.
3. Add virtualizer candidate support.
4. Add Lab proof and runtime diagnostics.
5. Update AgentX memory and skill references only after implementation produces durable guidance.

No existing code should need migration unless it has custom assumptions that `Position` has exactly two variants. Such matches must be updated in the same implementation slice.

## Alternatives Considered

### Physically Move Sticky Elements Out Of Their Parent

Rejected. It fights GPUI ownership, layout, prepaint state, hitbox registration, and event routing. Keeping the element in normal layout and applying an effective visual offset gives the desired placeholder behavior without proxy elements.

### Use `deferred().unclipped()` For All Fixed/Sticky Rendering

Rejected. Deferred drawing is mask-preserving by default by design, and `.unclipped()` is for intentional overlays. Sticky should usually remain clipped by its scroll container. Fixed needs a targeted viewport-positioned escape path, not a global deferred behavior change.

### Implement Only In RayX Components

Rejected. The requested authoring surface is `div()` positioning, and correct fixed/sticky behavior affects GPUI style, layout, hitboxes, masks, and prepaint/paint traversal.

### Automatically Discover Sticky Descendants In Virtualized Offscreen Rows

Rejected as impossible without rendering the offscreen rows. Virtualized collection support must use explicit candidate metadata or an equivalent state-owned hint.

### Render All Virtualized Items To Find Sticky Elements

Rejected. It destroys virtualization performance and violates the reason these collections exist.
