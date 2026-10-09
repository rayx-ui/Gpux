//! Taffy layout nodes kept across frames by element identity.
//!
//! Adapted from the claim, lifecycle and write-elision parts of gpui-fast's
//! `fast/layout.rs`. With layout retention on, [`TaffyLayoutEngine`] finds an
//! element's node by the key [`crate::fast::layout_key`] derives for it,
//! instead of taking the next node of its ordinal pool. A node kept this way
//! stays where it is in the tree, so inserting, removing or reordering keyed
//! siblings leaves every other node unwritten, and Taffy's cache keeps it
//! clean.
//!
//! A kept node is written only where the request changed. Its style is
//! compared through what the Taffy conversion reads: a fingerprint of those
//! fields tells a changed request apart cheaply, and equal fingerprints are
//! confirmed field by field, so a request is converted only when it changed
//! (the layout rules allow no inference from position or identity). Its
//! children are written only when the ordered child ids differ.
//!
//! Measured leaves are given their new closure, and dirtied, each frame as in
//! the pool, and their closures are still taken when the frame is cleared:
//! nothing a frame captured outlives it.

use std::{
    hash::{Hash as _, Hasher as _},
    mem,
};

use collections::{FxHashMap, FxHasher};

use crate::{
    AbsoluteLength, AlignContent, AlignItems, AlignSelf, DefiniteLength, Display, Edges,
    FlexDirection, FlexWrap, GridLocation, GridTemplate, JustifyContent, LayoutDirection, LayoutId,
    Length, Overflow, Pixels, Point, Position, Size, Style, TaffyLayoutEngine, Window,
    taffy::{NodeContext, ToTaffy as _},
    util::round_to_device_pixel,
};

/// What a Taffy call that cannot fail by construction panics with.
const EXPECT_MESSAGE: &str = "we should avoid taffy layout errors by construction if possible";

/// How many frames a node of a list row is kept unclaimed before it is
/// released. A list lays out only the rows it shows, so a row scrolled out
/// and back in would otherwise be laid out from scratch.
pub(crate) const LIST_ROW_NODE_LINGER_FRAMES: u64 = 240;

/// The kept half of a [`TaffyLayoutEngine`]: the nodes it keeps from one
/// frame to the next by key.
#[derive(Default)]
pub(crate) struct LayoutRetention {
    /// Whether this frame keys its layout; fixed when it begins.
    pub(crate) active: bool,
    /// Nodes kept from earlier frames, by the key of their element.
    retained: FxHashMap<u64, RetainedNode>,
    /// Nodes made this frame without a key, or for a key already claimed:
    /// released when the frame is cleared, as every node of a frame was
    /// before nodes were kept.
    transient: Vec<LayoutId>,
    /// Incremented once per frame; stamped onto nodes as they are claimed.
    frame: u64,
    /// How many kept nodes were claimed or made this frame. When it equals
    /// the number kept, no node went unclaimed and nothing is swept.
    claimed_this_frame: usize,
    /// How many list rows are being laid out, nested.
    list_rows: usize,
    /// The style each node [`stretch_auto_size_to_fill`] stretched asked
    /// for: a stretched size looks like an explicit one, and the next
    /// frame's request is compared with what was asked.
    unstretched_styles: FxHashMap<LayoutId, taffy::style::Style>,
    /// How many requests were converted to Taffy styles.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) conversions: u64,
}

/// A node kept from one frame to the next.
struct RetainedNode {
    id: LayoutId,
    /// The frame it was last claimed in. A node a whole frame goes without is
    /// released, unless it lingers.
    claimed_in_frame: u64,
    /// Whether it was last claimed laying out a list row, and is kept for
    /// [`LIST_ROW_NODE_LINGER_FRAMES`] unclaimed.
    lingers: bool,
    /// [`layout_fingerprint`] of the request the node was last given.
    fingerprint: u64,
    /// What of that request the Taffy conversion reads.
    fields: LayoutFields,
}

/// What [`LayoutRetention::claim`] found for a key.
enum Claim {
    /// A node from an earlier frame, now claimed for this one.
    Reused(u64, LayoutId),
    /// No node yet: one is made and kept under this key.
    Vacant(u64),
    /// No node may be kept: the element has no key, or another element
    /// claimed it this frame.
    Unkeyed,
}

impl LayoutRetention {
    /// How many nodes are kept by key.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn kept_count(&self) -> usize {
        self.retained.len()
    }

    /// Takes the node kept under `key` for this frame.
    fn claim(&mut self, key: Option<u64>) -> Claim {
        let Some(key) = key else {
            return Claim::Unkeyed;
        };
        let frame = self.frame;
        let Some(node) = self.retained.get_mut(&key) else {
            return Claim::Vacant(key);
        };
        if node.claimed_in_frame == frame {
            // Two elements resolved to one key. Sharing the node would
            // corrupt the tree, and keeping the second under the key would
            // strand the first, so the second goes unkeyed.
            return Claim::Unkeyed;
        }
        node.claimed_in_frame = frame;
        node.lingers = self.list_rows > 0;
        self.claimed_this_frame += 1;
        Claim::Reused(key, node.id)
    }

    /// Records a node made this frame under `key`, or as transient.
    fn keep(&mut self, key: Option<u64>, id: LayoutId, fingerprint: u64, fields: LayoutFields) {
        let Some(key) = key else {
            self.transient.push(id);
            return;
        };
        self.retained.insert(
            key,
            RetainedNode {
                id,
                claimed_in_frame: self.frame,
                lingers: self.list_rows > 0,
                fingerprint,
                fields,
            },
        );
        self.claimed_this_frame += 1;
    }

    /// Converts `style` to a Taffy style.
    fn convert(
        &mut self,
        style: &Style,
        rem_size: Pixels,
        scale_factor: f32,
    ) -> taffy::style::Style {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.conversions += 1;
        }
        style.to_taffy(rem_size, scale_factor)
    }
}

/// Adds a node to the layout tree for an element keyed `key`, reusing the
/// node kept under it when there is one. A kept node's style is converted
/// and written only when the request changed, and its children only when
/// they differ; a measured node is given its new closure and dirtied, as the
/// pool does. See [`TaffyLayoutEngine::request_layout`].
pub(crate) fn request_node(
    engine: &mut TaffyLayoutEngine,
    key: Option<u64>,
    style: &Style,
    rem_size: Pixels,
    scale_factor: f32,
    children: &[LayoutId],
    context: Option<NodeContext>,
) -> LayoutId {
    let fingerprint = layout_fingerprint(style, rem_size, scale_factor);
    let key = match engine.retention.claim(key) {
        Claim::Reused(key, id) => {
            crate::fast::fast_stat!(engine.fast_stats, nodes_claimed += 1);
            apply_requested_style(engine, key, id, fingerprint, style, rem_size, scale_factor);
            engine.update_node_topology(id, children, context);
            return id;
        }
        Claim::Vacant(key) => Some(key),
        Claim::Unkeyed => None,
    };
    let measured = context.is_some();
    let taffy_style = engine.retention.convert(style, rem_size, scale_factor);
    crate::fast::fast_stat!(engine.fast_stats, style_writes += 1);
    crate::fast::fast_stat!(engine.fast_stats, children_writes += !children.is_empty());
    let id: LayoutId = match context {
        Some(context) => engine.taffy.new_leaf_with_context(taffy_style, context),
        None => engine.taffy.new_leaf(taffy_style),
    }
    .expect(EXPECT_MESSAGE)
    .into();
    if !children.is_empty() {
        // A kept child can arrive still listed under the parent it had last
        // frame; `set_children` detaches it from there first, where
        // `new_with_children` would leave it listed twice.
        debug_assert!(!measured);
        engine
            .taffy
            .set_children(id.into(), LayoutId::to_taffy_slice(children))
            .expect(EXPECT_MESSAGE);
    }
    let fields = LayoutFields::of(style, rem_size, scale_factor);
    engine.retention.keep(key, id, fingerprint, fields);
    engine.note_requested_node(id, children, measured);
    id
}

/// Brings a kept node's style up to date with the request: unchanged when
/// the request reads as it did last frame, otherwise converted, and written
/// only if the conversion differs from what the node was asked for.
fn apply_requested_style(
    engine: &mut TaffyLayoutEngine,
    key: u64,
    id: LayoutId,
    fingerprint: u64,
    style: &Style,
    rem_size: Pixels,
    scale_factor: f32,
) {
    let retention = &mut engine.retention;
    let node = retention
        .retained
        .get_mut(&key)
        .expect("a claimed key is kept");
    if node.fingerprint == fingerprint && node.fields.matches(style, rem_size, scale_factor) {
        return;
    }
    node.fingerprint = fingerprint;
    node.fields = LayoutFields::of(style, rem_size, scale_factor);
    let taffy_style = retention.convert(style, rem_size, scale_factor);
    let asked = retention
        .unstretched_styles
        .get(&id)
        .unwrap_or_else(|| engine.taffy.style(id.into()).expect(EXPECT_MESSAGE));
    if asked == &taffy_style {
        return;
    }
    // Taffy now holds exactly what was asked.
    retention.unstretched_styles.remove(&id);
    crate::fast::fast_stat!(engine.fast_stats, style_writes += 1);
    engine
        .taffy
        .set_style(id.into(), taffy_style)
        .expect(EXPECT_MESSAGE);
}

/// Treats any `auto` dimension of a kept node's style as filling `size`, as
/// [`TaffyLayoutEngine::stretch_auto_size_to_fill`] does, writing the node
/// only when the stretched style differs from the one it has. The style
/// asked for is kept aside, so the next frame compares like with like
/// instead of rewriting, and dirtying, the root every frame.
pub(crate) fn stretch_auto_size_to_fill(
    engine: &mut TaffyLayoutEngine,
    id: LayoutId,
    size: Size<Pixels>,
    scale_factor: f32,
) {
    let retention = &mut engine.retention;
    let asked = match retention.unstretched_styles.get(&id) {
        Some(asked) => asked,
        None => engine.taffy.style(id.into()).expect(EXPECT_MESSAGE),
    };
    let stretch_width = asked.size.width.is_auto();
    let stretch_height = asked.size.height.is_auto();
    if !stretch_width && !stretch_height {
        return;
    }
    let asked = asked.clone();
    let mut stretched = asked.clone();
    if stretch_width {
        stretched.size.width =
            taffy::style::Dimension::length(round_to_device_pixel(size.width.0, scale_factor));
    }
    if stretch_height {
        stretched.size.height =
            taffy::style::Dimension::length(round_to_device_pixel(size.height.0, scale_factor));
    }
    if engine.taffy.style(id.into()).expect(EXPECT_MESSAGE) != &stretched {
        crate::fast::fast_stat!(engine.fast_stats, style_writes += 1);
        engine
            .taffy
            .set_style(id.into(), stretched)
            .expect(EXPECT_MESSAGE);
    }
    retention.unstretched_styles.insert(id, asked);
}

/// Ends the frame for the kept nodes, once its measurement closures were
/// taken: releases the transient nodes and the kept nodes the frame did not
/// claim, except list rows still lingering. Nodes it claimed stay, with
/// their Taffy layout caches. See [`TaffyLayoutEngine::clear`].
pub(crate) fn release_unclaimed_nodes(engine: &mut TaffyLayoutEngine) {
    let retention = &mut engine.retention;
    let mut released = 0u64;
    let mut lingering = 0u64;
    for id in retention.transient.drain(..) {
        retention.unstretched_styles.remove(&id);
        engine.taffy.remove(id.into()).expect(EXPECT_MESSAGE);
        released += 1;
    }
    // In a steady frame every kept node was claimed, and nothing is swept.
    if retention.retained.len() != retention.claimed_this_frame {
        let frame = retention.frame;
        let taffy = &mut engine.taffy;
        let unstretched_styles = &mut retention.unstretched_styles;
        retention.retained.retain(|_, node| {
            if node.claimed_in_frame == frame {
                return true;
            }
            if node.lingers && frame - node.claimed_in_frame < LIST_ROW_NODE_LINGER_FRAMES {
                lingering += 1;
                return true;
            }
            unstretched_styles.remove(&node.id);
            taffy.remove(node.id.into()).expect(EXPECT_MESSAGE);
            released += 1;
            false
        });
    }
    retention.claimed_this_frame = 0;
    retention.frame += 1;
    let _ = (released, lingering);
    crate::fast::fast_stat!(engine.fast_stats, nodes_released += released);
    crate::fast::fast_stat!(engine.fast_stats, nodes_lingering += lingering);
}

/// Begins a frame for `window`'s layout: it keys its layout while layout
/// retention is on. A frame that switches retention on or off starts from a
/// fresh layout engine, so kept and pooled nodes never meet.
pub(crate) fn begin_frame(window: &mut Window) {
    let active = window.fast_mode.layout_retention();
    window.fast_layout.begin_frame(active);
    let Some(engine) = window.layout_engine.as_mut() else {
        return;
    };
    if engine.retention.active != active {
        *engine = TaffyLayoutEngine::new();
        engine.retention.active = active;
    }
}

/// Runs `f`, laying out a list row: the nodes it claims linger.
pub(crate) fn laying_out_list_row<R>(window: &mut Window, f: impl FnOnce(&mut Window) -> R) -> R {
    let rows = |window: &mut Window, delta: isize| {
        if let Some(engine) = window.layout_engine.as_mut() {
            engine.retention.list_rows = engine.retention.list_rows.saturating_add_signed(delta);
        }
    };
    rows(window, 1);
    let result = f(window);
    rows(window, -1);
    result
}

/// Every field of a [`Style`] its Taffy conversion reads, and the rem size
/// and scale factor it resolves lengths against: two requests whose fields
/// are equal convert to equal Taffy styles.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutFields {
    rem_size: Pixels,
    scale_factor: f32,
    display: Display,
    direction: LayoutDirection,
    overflow: Point<Overflow>,
    scrollbar_width: AbsoluteLength,
    position: Position,
    inset: Edges<Length>,
    size: Size<Length>,
    min_size: Size<Length>,
    max_size: Size<Length>,
    aspect_ratio: Option<f32>,
    margin: Edges<Length>,
    padding: Edges<DefiniteLength>,
    border_widths: Edges<AbsoluteLength>,
    align_items: Option<AlignItems>,
    align_self: Option<AlignSelf>,
    align_content: Option<AlignContent>,
    justify_content: Option<JustifyContent>,
    gap: Size<DefiniteLength>,
    flex_direction: FlexDirection,
    flex_wrap: FlexWrap,
    flex_basis: Length,
    flex_grow: f32,
    flex_shrink: f32,
    grid_rows: Option<GridTemplate>,
    grid_cols: Option<GridTemplate>,
    grid_location: Option<GridLocation>,
}

impl LayoutFields {
    /// What of `style` its conversion at `rem_size` and `scale_factor` reads.
    pub(crate) fn of(style: &Style, rem_size: Pixels, scale_factor: f32) -> Self {
        Self {
            rem_size,
            scale_factor,
            display: style.display,
            direction: style.direction,
            overflow: style.overflow,
            scrollbar_width: style.scrollbar_width,
            position: style.position,
            inset: style.inset,
            size: style.size,
            min_size: style.min_size,
            max_size: style.max_size,
            aspect_ratio: style.aspect_ratio,
            margin: style.margin,
            padding: style.padding,
            border_widths: style.border_widths,
            align_items: style.align_items,
            align_self: style.align_self,
            align_content: style.align_content,
            justify_content: style.justify_content,
            gap: style.gap,
            flex_direction: style.flex_direction,
            flex_wrap: style.flex_wrap,
            flex_basis: style.flex_basis,
            flex_grow: style.flex_grow,
            flex_shrink: style.flex_shrink,
            grid_rows: style.grid_rows,
            grid_cols: style.grid_cols,
            grid_location: style.grid_location.clone(),
        }
    }

    /// Whether `style` at `rem_size` and `scale_factor` reads as these
    /// fields, compared exactly and without copying.
    pub(crate) fn matches(&self, style: &Style, rem_size: Pixels, scale_factor: f32) -> bool {
        self.rem_size == rem_size
            && self.scale_factor == scale_factor
            && self.display == style.display
            && self.direction == style.direction
            && self.overflow == style.overflow
            && self.scrollbar_width == style.scrollbar_width
            && self.position == style.position
            && self.inset == style.inset
            && self.size == style.size
            && self.min_size == style.min_size
            && self.max_size == style.max_size
            && self.aspect_ratio == style.aspect_ratio
            && self.margin == style.margin
            && self.padding == style.padding
            && self.border_widths == style.border_widths
            && self.align_items == style.align_items
            && self.align_self == style.align_self
            && self.align_content == style.align_content
            && self.justify_content == style.justify_content
            && self.gap == style.gap
            && self.flex_direction == style.flex_direction
            && self.flex_wrap == style.flex_wrap
            && self.flex_basis == style.flex_basis
            && self.flex_grow == style.flex_grow
            && self.flex_shrink == style.flex_shrink
            && self.grid_rows == style.grid_rows
            && self.grid_cols == style.grid_cols
            && self.grid_location == style.grid_location
    }
}

/// A hash of every field of `style` its Taffy conversion reads, and of the
/// rem size and scale factor it resolves lengths against. Equal requests
/// hash equally; a different hash means a changed request, and an equal one
/// is confirmed by [`LayoutFields::matches`].
pub(crate) fn layout_fingerprint(style: &Style, rem_size: Pixels, scale_factor: f32) -> u64 {
    fn absolute(hasher: &mut FxHasher, length: &AbsoluteLength) {
        match length {
            AbsoluteLength::Pixels(pixels) => (0u8, pixels.0.to_bits()).hash(hasher),
            AbsoluteLength::Rems(rems) => (1u8, rems.0.to_bits()).hash(hasher),
        }
    }
    fn definite(hasher: &mut FxHasher, length: &DefiniteLength) {
        match length {
            DefiniteLength::Absolute(length) => {
                0u8.hash(hasher);
                absolute(hasher, length);
            }
            DefiniteLength::Fraction(fraction) => (1u8, fraction.to_bits()).hash(hasher),
        }
    }
    fn length(hasher: &mut FxHasher, length: &Length) {
        match length {
            Length::Definite(length) => {
                0u8.hash(hasher);
                definite(hasher, length);
            }
            Length::Auto => 1u8.hash(hasher),
        }
    }
    fn edges<T: Clone + std::fmt::Debug + Default + PartialEq>(
        hasher: &mut FxHasher,
        edges: &Edges<T>,
        each: fn(&mut FxHasher, &T),
    ) {
        each(hasher, &edges.top);
        each(hasher, &edges.right);
        each(hasher, &edges.bottom);
        each(hasher, &edges.left);
    }
    fn sizes<T: Clone + std::fmt::Debug + Default + PartialEq>(
        hasher: &mut FxHasher,
        size: &Size<T>,
        each: fn(&mut FxHasher, &T),
    ) {
        each(hasher, &size.width);
        each(hasher, &size.height);
    }
    fn placement(hasher: &mut FxHasher, placement: &crate::GridPlacement) {
        match placement {
            crate::GridPlacement::Line(line) => (0u8, *line).hash(hasher),
            crate::GridPlacement::Span(span) => (1u8, *span).hash(hasher),
            crate::GridPlacement::Auto => 2u8.hash(hasher),
        }
    }
    fn template(hasher: &mut FxHasher, template: &Option<GridTemplate>) {
        match template {
            Some(template) => {
                (1u8, template.repeat).hash(hasher);
                mem::discriminant(&template.min_size).hash(hasher);
            }
            None => 0u8.hash(hasher),
        }
    }

    let mut hasher = FxHasher::default();
    let hasher = &mut hasher;
    rem_size.0.to_bits().hash(hasher);
    scale_factor.to_bits().hash(hasher);

    mem::discriminant(&style.display).hash(hasher);
    mem::discriminant(&style.direction).hash(hasher);
    mem::discriminant(&style.overflow.x).hash(hasher);
    mem::discriminant(&style.overflow.y).hash(hasher);
    absolute(hasher, &style.scrollbar_width);
    mem::discriminant(&style.position).hash(hasher);
    edges(hasher, &style.inset, length);
    sizes(hasher, &style.size, length);
    sizes(hasher, &style.min_size, length);
    sizes(hasher, &style.max_size, length);
    style.aspect_ratio.map(f32::to_bits).hash(hasher);
    edges(hasher, &style.margin, length);
    edges(hasher, &style.padding, definite);
    edges(hasher, &style.border_widths, absolute);
    style
        .align_items
        .map(|x| mem::discriminant(&x))
        .hash(hasher);
    style.align_self.map(|x| mem::discriminant(&x)).hash(hasher);
    style
        .align_content
        .map(|x| mem::discriminant(&x))
        .hash(hasher);
    style
        .justify_content
        .map(|x| mem::discriminant(&x))
        .hash(hasher);
    sizes(hasher, &style.gap, definite);
    mem::discriminant(&style.flex_direction).hash(hasher);
    mem::discriminant(&style.flex_wrap).hash(hasher);
    length(hasher, &style.flex_basis);
    style.flex_grow.to_bits().hash(hasher);
    style.flex_shrink.to_bits().hash(hasher);
    template(hasher, &style.grid_rows);
    template(hasher, &style.grid_cols);
    match &style.grid_location {
        Some(location) => {
            1u8.hash(hasher);
            for line in [&location.row, &location.column] {
                placement(hasher, &line.start);
                placement(hasher, &line.end);
            }
        }
        None => 0u8.hash(hasher),
    }
    hasher.finish()
}
