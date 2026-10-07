# Sticky Positioning

Use current source in `crates/gpui/src/style.rs`, `window.rs`, and
`elements/{div,list,uniform_list}.rs` as the authority.

## Ownership

Keep ordinary sticky layout, scroll-context discovery, clipping, visual bounds, child offsets,
paint order, hit testing, and callbacks in GPUI. Keep offscreen candidate retention in the
virtualizer that owns those candidates. Do not use app overlays, cloned headers, proxy elements,
or a global registration service.

## Canonical Relation

Use `StickyStacking` everywhere:

- `Level(u16)` expresses explicit hierarchy and same-level replacement.
- `StackId(ElementId)` expresses one contiguous keyed run.

Equal stack ids accumulate predecessors. A different id replaces the complete prior run. A
later A after B starts a new run and does not resurrect the earlier A. Levels retain lower-level
ancestors and replace the suffix beginning at the incoming level. The relation index keeps linear
records and resolves an active stack with candidate search plus active-depth traversal.

Every incoming replacement pushes its outgoing suffix toward the start edge once it reaches that
suffix's combined primary-axis extent. Preserve outgoing candidates until this motion scrolls them
fully out; apply the same rule to vertical heights and horizontal widths. For ordinary unkeyed
sticky siblings, equal resolved start insets form a flat replacement lane and increasing insets
form hierarchy lanes. Use `.sticky_stack_id(...)` only when contiguous siblings must accumulate
without separate insets. GPUI discovers the complete candidate sequence for the nearest scroll
context before committing sticky visual bounds.

Paint and prepaint active hierarchy candidates from deepest child to shallowest parent. This keeps
the retained parent above a pushed child in both scene and positioned-hitbox order without changing
the global positioned-layer lane. Keep an incoming same-level candidate after the active candidates
so it still paints and hit-tests above the outgoing replacement.

## Prepaint Style Resolution

Retain the style computed before sticky bounds resolution through ordinary prepaint. Recompute it
after publishing sticky state only when an applicable `when_stuck(...)` refinement can change the
result or an `on_sticky_state_changed(...)` listener may synchronously change other style inputs.
Do not rebuild the complete style for ordinary non-sticky elements or inactive sticky refinements;
the pre-resolution result is already identical in those cases.

## Frame-Bounded Style Handoff

For structurally static interactivity, move the final prepaint `Style` through the window-owned
draw-local style store and consume it during paint. Keep only an optional store index on
`Interactivity`; do not enlarge every element with an inline `Style` or allocate one box per
element. Clear owned styles at every `Window::draw` boundary while retaining vector capacity, so
storage stays bounded by that window's maximum eligible draw.

Do not retain a style when focus, in-focus, focus-visible, hover, group-hover, active, or
group-active refinement exists. Even for a retained style, recompute during an active drag when
the element has a hitbox, because drag-over builders, drop predicates, and drag cursor state are
paint-time behavior. Cached view and deferred ranges must continue replaying prepaint and paint
together; they do not participate in the draw-local handoff.

Preserve empty content-mask bounds before snapping logical coordinates to device pixels. Cover
rounding may floor a near edge and ceil a far edge only for a non-empty region; otherwise a
zero-width or zero-height border mask at a fractional device scale can become a one-device-pixel
strip outside the scrollport.

For low-level `list` and `uniform_list`, pass `StickyItem` values to the existing
`sticky_items` configuration:

```rust
use gpux::{StickyItem, StickyStacking};

let candidates = [
    StickyItem::new(0, StickyStacking::StackId("a".into())),
    StickyItem::new(1, StickyStacking::StackId("a".into())),
    StickyItem::new(8, StickyStacking::StackId("b".into())),
];
```

Plain indices convert to `Level(0)`. Do not add separate key-only or level-only APIs. Keep
first-wins duplicate-index handling, measured predecessor extents, root hitboxes, wheel
propagation, and ordinary row layout unchanged. Low-level List and UniformList must apply the same
deepest-first active-candidate traversal before appending incoming candidates.

## Validation

Run the focused host checks, then the `layout-fixed-sticky` conformance scene, which runs in every
lane, from the Gpux repository root:

```powershell
cargo test -p gpux sticky -- --nocapture
cargo run -p gpux-testkit -- lane a --scene layout-fixed-sticky
cargo run -p gpux-testkit -- lane b --scene layout-fixed-sticky
cargo run -p gpux-testkit -- lane c --scene layout-fixed-sticky
```

Extend that scene, or add one in `crates/gpux_conformance/src/scenes/` that names its lanes, for
new sticky behaviour. At scales 1 and 2 it must exercise numeric bounds, contiguous membership,
replacement, A-B-A behavior, clipping, callbacks, clicks, and duplicate-row absence. Lane B runs
it in WASM/WebGPU with trusted input (`--scene` runs the DPR 1 spec;
`cargo run -p gpux-testkit -- web e2e --grep layout-fixed-sticky` adds DPR 2). Exercise border
clipping at a fractional device scale as well, because integer scales cannot expose empty-mask
expansion from cover rounding.
