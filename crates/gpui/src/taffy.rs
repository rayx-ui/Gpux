use crate::{
    AbsoluteLength, App, Bounds, DefiniteLength, Edges, GridTemplate, Length, Pixels, Point, Size,
    Style, Window, size,
    util::{
        ceil_to_device_pixel, round_half_toward_zero, round_stroke_to_device_pixel,
        round_to_device_pixel,
    },
};
use collections::{FxHashMap, FxHashSet};
use std::{fmt::Debug, ops::Range};
use taffy::{
    TaffyTree, TraversePartialTree as _,
    geometry::{Point as TaffyPoint, Rect as TaffyRect, Size as TaffySize},
    prelude::{max_content, min_content},
    style::AvailableSpace as TaffyAvailableSpace,
    tree::NodeId,
};

#[cfg(feature = "stacker")]
type StackSafe<T> = stacksafe::StackSafe<T>;
#[cfg(not(feature = "stacker"))]
type StackSafe<T> = T;

type MeasureFn =
    dyn FnMut(Size<Option<Pixels>>, Size<AvailableSpace>, &mut Window, &mut App) -> Size<Pixels>;
type NodeMeasureFn = StackSafe<Box<MeasureFn>>;

struct NodeContext {
    measure: Option<NodeMeasureFn>,
    #[cfg(feature = "rayx_diagnostics")]
    kind: MeasuredLayoutKind,
    #[cfg(feature = "rayx_diagnostics")]
    previous_measurements: Vec<MeasuredLayoutSample>,
    #[cfg(feature = "rayx_diagnostics")]
    current_measurements: Vec<MeasuredLayoutSample>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MeasuredLayoutKind {
    Custom,
    #[cfg(feature = "rayx_diagnostics")]
    Text,
    #[cfg(feature = "rayx_diagnostics")]
    List,
    #[cfg(feature = "rayx_diagnostics")]
    UniformList,
}

impl MeasuredLayoutKind {
    #[cfg(feature = "rayx_diagnostics")]
    pub(crate) fn from_source_file(file: &str) -> Self {
        if file.ends_with("elements/text.rs") || file.ends_with("elements\\text.rs") {
            Self::Text
        } else if file.ends_with("elements/list.rs") || file.ends_with("elements\\list.rs") {
            Self::List
        } else if file.ends_with("elements/uniform_list.rs")
            || file.ends_with("elements\\uniform_list.rs")
        {
            Self::UniformList
        } else {
            Self::Custom
        }
    }

    #[cfg(feature = "rayx_diagnostics")]
    const fn diagnostic_index(self) -> usize {
        match self {
            Self::Custom => 25,
            Self::Text => 26,
            Self::List => 27,
            Self::UniformList => 28,
        }
    }
}

#[cfg(feature = "rayx_diagnostics")]
#[derive(Clone, Copy, Debug, PartialEq)]
struct MeasuredLayoutSample {
    known_dimensions: TaffySize<Option<f32>>,
    available_space: TaffySize<TaffyAvailableSpace>,
    output: TaffySize<f32>,
}

#[cfg(feature = "rayx_diagnostics")]
impl NodeContext {
    fn begin_measurement_frame(&mut self) {
        std::mem::swap(
            &mut self.previous_measurements,
            &mut self.current_measurements,
        );
        self.current_measurements.clear();
    }

    fn clear_measurement_history(&mut self) {
        self.previous_measurements.clear();
        self.current_measurements.clear();
    }

    fn record_measurement(&mut self, diagnostics: &mut [u64; 31], sample: MeasuredLayoutSample) {
        diagnostics[20] += 1;
        diagnostics[self.kind.diagnostic_index()] += 1;
        if let Some(previous) = self
            .previous_measurements
            .get(self.current_measurements.len())
        {
            diagnostics[21] += 1;
            if previous.known_dimensions == sample.known_dimensions
                && previous.available_space == sample.available_space
            {
                diagnostics[22] += 1;
                if previous.output == sample.output {
                    diagnostics[23] += 1;
                } else {
                    diagnostics[24] += 1;
                }
            }
        }
        self.current_measurements.push(sample);
    }
}

pub struct TaffyLayoutEngine {
    taffy: TaffyTree<NodeContext>,
    layout_pool: Vec<LayoutId>,
    active_layout_count: usize,
    active_child_layouts: FxHashSet<LayoutId>,
    measured_layouts: Vec<LayoutId>,
    absolute_layout_bounds: FxHashMap<LayoutId, Bounds<Pixels>>,
    /// Unrounded absolute border-box top-left per-node coordinate in device pixels.
    absolute_outer_origins: FxHashMap<LayoutId, Point<f32>>,
    computed_layouts: FxHashSet<LayoutId>,
    layout_bounds_scratch_space: Vec<LayoutId>,
    #[cfg(feature = "rayx_diagnostics")]
    diagnostics: [u64; 31],
    #[cfg(feature = "rayx_diagnostics")]
    request_diagnostics: [u64; 11],
}

const EXPECT_MESSAGE: &str = "we should avoid taffy layout errors by construction if possible";

impl TaffyLayoutEngine {
    pub fn new() -> Self {
        let mut taffy = TaffyTree::new();
        taffy.disable_rounding();
        TaffyLayoutEngine {
            taffy,
            layout_pool: Vec::new(),
            active_layout_count: 0,
            active_child_layouts: FxHashSet::default(),
            measured_layouts: Vec::new(),
            absolute_layout_bounds: FxHashMap::default(),
            absolute_outer_origins: FxHashMap::default(),
            computed_layouts: FxHashSet::default(),
            layout_bounds_scratch_space: Vec::new(),
            #[cfg(feature = "rayx_diagnostics")]
            diagnostics: [0; 31],
            #[cfg(feature = "rayx_diagnostics")]
            request_diagnostics: [0; 11],
        }
    }

    pub fn clear(&mut self) {
        for id in self.measured_layouts.drain(..) {
            let context = self.taffy.get_node_context_mut(id.0).expect(EXPECT_MESSAGE);
            context.measure = None;
            #[cfg(feature = "rayx_diagnostics")]
            context.begin_measurement_frame();
            self.taffy.mark_dirty(id.0).expect(EXPECT_MESSAGE);
        }
        self.active_layout_count = 0;
        self.active_child_layouts.clear();
        self.absolute_layout_bounds.clear();
        self.absolute_outer_origins.clear();
        self.computed_layouts.clear();
        #[cfg(feature = "rayx_diagnostics")]
        {
            self.diagnostics = [0; 31];
            self.request_diagnostics = [0; 11];
        }
    }

    #[cfg(feature = "rayx_diagnostics")]
    pub(crate) fn diagnostics(&self) -> [u64; 31] {
        self.diagnostics
    }

    #[cfg(feature = "rayx_diagnostics")]
    pub(crate) fn request_diagnostics(&self) -> [u64; 11] {
        self.request_diagnostics
    }

    #[cfg(feature = "rayx_diagnostics")]
    fn record_node(&mut self, display: taffy::style::Display, child_count: usize, measured: bool) {
        let child_count = child_count as u64;
        self.diagnostics[0] += 1;
        self.diagnostics[1] += u64::from(measured);
        if child_count == 0 {
            self.diagnostics[2] += 1;
            return;
        }

        self.diagnostics[3] += 1;
        self.diagnostics[4] += child_count;
        self.diagnostics[5] = self.diagnostics[5].max(child_count);
        self.diagnostics[6] += u64::from(child_count <= 1);
        self.diagnostics[7] += u64::from(child_count <= 2);
        self.diagnostics[8] += u64::from(child_count <= 4);
        self.diagnostics[9] += u64::from(child_count <= 8);
        match display {
            taffy::style::Display::Flex => self.diagnostics[10] += 1,
            taffy::style::Display::Block | taffy::style::Display::FlowRoot => {
                self.diagnostics[11] += 1
            }
            taffy::style::Display::Grid => self.diagnostics[12] += 1,
            taffy::style::Display::None => {}
        }
    }

    fn request_taffy_node(
        &mut self,
        style: taffy::style::Style,
        children: &[LayoutId],
        context: Option<NodeContext>,
    ) -> LayoutId {
        let is_measured = context.is_some();
        let pool_index = self.active_layout_count;
        self.active_layout_count += 1;
        #[cfg(feature = "rayx_diagnostics")]
        {
            self.request_diagnostics[0] += 1;
        }

        let id = if let Some(id) = self.layout_pool.get(pool_index).copied() {
            #[cfg(feature = "rayx_diagnostics")]
            {
                self.request_diagnostics[1] += 1;
            }
            let style_matches = self.taffy.style(id.0).expect(EXPECT_MESSAGE) == &style;
            #[cfg(feature = "rayx_diagnostics")]
            {
                self.request_diagnostics[if style_matches { 3 } else { 4 }] += 1;
            }
            if !style_matches {
                self.taffy.set_style(id.0, style).expect(EXPECT_MESSAGE);
            }
            let taffy_children = LayoutId::to_taffy_slice(children);
            let topology_matches = self.taffy.child_count(id.0) == children.len()
                && taffy_children.iter().enumerate().all(|(index, child)| {
                    self.taffy
                        .child_at_index(id.0, index)
                        .is_ok_and(|retained_child| retained_child == *child)
                });
            #[cfg(feature = "rayx_diagnostics")]
            {
                self.request_diagnostics[if topology_matches { 5 } else { 6 }] += 1;
            }
            if !topology_matches {
                self.taffy
                    .set_children(id.0, taffy_children)
                    .expect(EXPECT_MESSAGE);
            }
            if let Some(context) = context {
                if let Some(retained_context) = self.taffy.get_node_context_mut(id.0) {
                    #[cfg(feature = "rayx_diagnostics")]
                    {
                        self.request_diagnostics[7] += 1;
                    }
                    retained_context.measure = context.measure;
                    #[cfg(feature = "rayx_diagnostics")]
                    {
                        retained_context.kind = context.kind;
                    }
                    if !self.taffy.dirty(id.0).expect(EXPECT_MESSAGE) {
                        #[cfg(feature = "rayx_diagnostics")]
                        {
                            self.request_diagnostics[9] += 1;
                        }
                        self.taffy.mark_dirty(id.0).expect(EXPECT_MESSAGE);
                    }
                } else {
                    #[cfg(feature = "rayx_diagnostics")]
                    {
                        self.request_diagnostics[8] += 1;
                    }
                    self.taffy
                        .set_node_context(id.0, Some(context))
                        .expect(EXPECT_MESSAGE);
                }
            } else {
                #[cfg(feature = "rayx_diagnostics")]
                if let Some(retained_context) = self.taffy.get_node_context_mut(id.0) {
                    retained_context.clear_measurement_history();
                }
            }
            id
        } else {
            #[cfg(feature = "rayx_diagnostics")]
            {
                self.request_diagnostics[2] += 1;
            }
            debug_assert_eq!(pool_index, self.layout_pool.len());
            let id = match context {
                Some(context) => {
                    debug_assert!(children.is_empty());
                    self.taffy
                        .new_leaf_with_context(style, context)
                        .expect(EXPECT_MESSAGE)
                }
                None if children.is_empty() => self.taffy.new_leaf(style).expect(EXPECT_MESSAGE),
                None => self
                    .taffy
                    .new_with_children(style, LayoutId::to_taffy_slice(children))
                    .expect(EXPECT_MESSAGE),
            }
            .into();
            self.layout_pool.push(id);
            id
        };

        if is_measured {
            self.measured_layouts.push(id);
        }
        for child in children {
            #[cfg(feature = "rayx_diagnostics")]
            {
                let inserted = self.active_child_layouts.insert(*child);
                self.request_diagnostics[10] += u64::from(inserted);
            }
            #[cfg(not(feature = "rayx_diagnostics"))]
            self.active_child_layouts.insert(*child);
        }
        id
    }

    pub fn request_layout(
        &mut self,
        style: Style,
        rem_size: Pixels,
        scale_factor: f32,
        children: &[LayoutId],
    ) -> LayoutId {
        self.request_layout_ref(&style, rem_size, scale_factor, children)
    }

    pub(crate) fn request_layout_ref(
        &mut self,
        style: &Style,
        rem_size: Pixels,
        scale_factor: f32,
        children: &[LayoutId],
    ) -> LayoutId {
        let taffy_style = style.to_taffy(rem_size, scale_factor);
        #[cfg(feature = "rayx_diagnostics")]
        self.record_node(taffy_style.display, children.len(), false);

        self.request_taffy_node(taffy_style, children, None)
    }

    pub fn request_measured_layout(
        &mut self,
        style: Style,
        rem_size: Pixels,
        scale_factor: f32,
        _kind: MeasuredLayoutKind,
        measure: impl FnMut(
            Size<Option<Pixels>>,
            Size<AvailableSpace>,
            &mut Window,
            &mut App,
        ) -> Size<Pixels>
        + 'static,
    ) -> LayoutId {
        self.request_measured_layout_ref(&style, rem_size, scale_factor, _kind, measure)
    }

    pub(crate) fn request_measured_layout_ref(
        &mut self,
        style: &Style,
        rem_size: Pixels,
        scale_factor: f32,
        _kind: MeasuredLayoutKind,
        measure: impl FnMut(
            Size<Option<Pixels>>,
            Size<AvailableSpace>,
            &mut Window,
            &mut App,
        ) -> Size<Pixels>
        + 'static,
    ) -> LayoutId {
        let taffy_style = style.to_taffy(rem_size, scale_factor);
        let measure = Box::new(measure) as Box<MeasureFn>;
        #[cfg(feature = "stacker")]
        let measure = StackSafe::new(measure);

        #[cfg(feature = "rayx_diagnostics")]
        self.record_node(taffy_style.display, 0, true);

        self.request_taffy_node(
            taffy_style,
            &[],
            Some(NodeContext {
                measure: Some(measure),
                #[cfg(feature = "rayx_diagnostics")]
                kind: _kind,
                #[cfg(feature = "rayx_diagnostics")]
                previous_measurements: Vec::new(),
                #[cfg(feature = "rayx_diagnostics")]
                current_measurements: Vec::new(),
            }),
        )
    }

    #[cfg(feature = "rayx_diagnostics")]
    pub(crate) fn set_text_measurement_path_diagnostics(&mut self, diagnostics: [u64; 2]) {
        self.diagnostics[29..].copy_from_slice(&diagnostics);
    }

    fn detach_stale_root(&mut self, id: LayoutId) {
        if !self.active_child_layouts.contains(&id)
            && let Some(parent) = self.taffy.parent(id.0)
        {
            self.taffy.remove_child(parent, id.0).expect(EXPECT_MESSAGE);
        }
    }

    /// Treats any `auto` dimension of the given node's style as filling `size`.
    ///
    /// This is applied to window roots before layout so they behave like the
    /// root element on the web, which stretches to fill the initial containing
    /// block (the viewport) unless given an explicit size. Explicitly styled
    /// dimensions are preserved.
    pub fn stretch_auto_size_to_fill(
        &mut self,
        id: LayoutId,
        size: Size<Pixels>,
        scale_factor: f32,
    ) {
        let style = self.taffy.style(id.0).expect(EXPECT_MESSAGE);
        let stretch_width = style.size.width.is_auto();
        let stretch_height = style.size.height.is_auto();
        if !stretch_width && !stretch_height {
            return;
        }
        let mut style = style.clone();
        if stretch_width {
            style.size.width =
                taffy::style::Dimension::length(round_to_device_pixel(size.width.0, scale_factor));
        }
        if stretch_height {
            style.size.height =
                taffy::style::Dimension::length(round_to_device_pixel(size.height.0, scale_factor));
        }
        self.taffy.set_style(id.0, style).expect(EXPECT_MESSAGE);
    }

    // Used to understand performance
    #[allow(dead_code)]
    fn count_all_children(&self, parent: LayoutId) -> anyhow::Result<u32> {
        let mut count = 0;

        for child in self.taffy.children(parent.0)? {
            // Count this child.
            count += 1;

            // Count all of this child's children.
            count += self.count_all_children(LayoutId(child))?
        }

        Ok(count)
    }

    // Used to understand performance
    #[allow(dead_code)]
    fn max_depth(&self, depth: u32, parent: LayoutId) -> anyhow::Result<u32> {
        println!(
            "{parent:?} at depth {depth} has {} children",
            self.taffy.child_count(parent.0)
        );

        let mut max_child_depth = 0;

        for child in self.taffy.children(parent.0)? {
            max_child_depth = std::cmp::max(max_child_depth, self.max_depth(0, LayoutId(child))?);
        }

        Ok(depth + 1 + max_child_depth)
    }

    // Used to understand performance
    #[allow(dead_code)]
    fn get_edges(&self, parent: LayoutId) -> anyhow::Result<Vec<(LayoutId, LayoutId)>> {
        let mut edges = Vec::new();

        for child in self.taffy.children(parent.0)? {
            edges.push((parent, LayoutId(child)));

            edges.extend(self.get_edges(LayoutId(child))?);
        }

        Ok(edges)
    }

    #[cfg_attr(feature = "stacker", stacksafe::stacksafe)]
    pub fn compute_layout(
        &mut self,
        id: LayoutId,
        available_space: Size<AvailableSpace>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.detach_stale_root(id);

        // Leaving this here until we have a better instrumentation approach.
        // println!("Laying out {} children", self.count_all_children(id)?);
        // println!("Max layout depth: {}", self.max_depth(0, id)?);

        // Output the edges (branches) of the tree in Mermaid format for visualization.
        // println!("Edges:");
        // for (a, b) in self.get_edges(id)? {
        //     println!("N{} --> N{}", u64::from(a), u64::from(b));
        // }
        //

        #[cfg(feature = "rayx_diagnostics")]
        {
            self.diagnostics[13] += 1;
        }
        let already_computed = !self.computed_layouts.insert(id);
        if already_computed {
            #[cfg(feature = "rayx_diagnostics")]
            {
                self.diagnostics[14] += 1;
            }
            let stack = &mut self.layout_bounds_scratch_space;
            stack.push(id);
            while let Some(id) = stack.pop() {
                #[cfg(feature = "rayx_diagnostics")]
                {
                    self.diagnostics[15] += 1;
                }
                self.absolute_layout_bounds.remove(&id);
                self.absolute_outer_origins.remove(&id);
                stack.extend(
                    self.taffy
                        .children(id.into())
                        .expect(EXPECT_MESSAGE)
                        .into_iter()
                        .map(LayoutId::from),
                );
            }
        }

        let scale_factor = window.scale_factor();

        let transform = |v: AvailableSpace| match v {
            AvailableSpace::Definite(pixels) => {
                AvailableSpace::Definite(Pixels(pixels.0 * scale_factor))
            }
            AvailableSpace::MinContent => AvailableSpace::MinContent,
            AvailableSpace::MaxContent => AvailableSpace::MaxContent,
        };
        let available_space = size(
            transform(available_space.width),
            transform(available_space.height),
        );

        let taffy = &mut self.taffy;
        #[cfg(feature = "rayx_diagnostics")]
        let diagnostics = &mut self.diagnostics;
        taffy
            .compute_layout_with_measure(
                id.into(),
                available_space.into(),
                |inputs, _id, node_context, style| {
                    taffy::compute::compute_leaf_layout(
                        inputs,
                        style,
                        |_, _| 0.0,
                        |known_dimensions, available_space| {
                            let Some(node_context) = node_context else {
                                return taffy::geometry::Size::default();
                            };
                            let Some(measure) = node_context.measure.as_mut() else {
                                return taffy::geometry::Size::default();
                            };

                            #[cfg(feature = "rayx_diagnostics")]
                            let measurement_inputs = (known_dimensions, available_space);

                            let known_dimensions = Size {
                                width: known_dimensions.width.map(|e| Pixels(e / scale_factor)),
                                height: known_dimensions.height.map(|e| Pixels(e / scale_factor)),
                            };

                            let available_space: Size<AvailableSpace> = available_space.into();
                            let untransform = |ev: AvailableSpace| match ev {
                                AvailableSpace::Definite(pixels) => {
                                    AvailableSpace::Definite(Pixels(pixels.0 / scale_factor))
                                }
                                AvailableSpace::MinContent => AvailableSpace::MinContent,
                                AvailableSpace::MaxContent => AvailableSpace::MaxContent,
                            };
                            let available_space = size(
                                untransform(available_space.width),
                                untransform(available_space.height),
                            );

                            let measured_size: Size<Pixels> =
                                (measure)(known_dimensions, available_space, window, cx);
                            let measured_size: TaffySize<f32> =
                                snap_measured_size_to_device_pixels(measured_size, scale_factor)
                                    .into();
                            #[cfg(feature = "rayx_diagnostics")]
                            node_context.record_measurement(
                                diagnostics,
                                MeasuredLayoutSample {
                                    known_dimensions: measurement_inputs.0,
                                    available_space: measurement_inputs.1,
                                    output: measured_size,
                                },
                            );
                            measured_size
                        },
                    )
                },
            )
            .expect(EXPECT_MESSAGE);
    }

    // Pixel snapping
    //
    // Painting primitives at non-integer pixel coordinates produces blurry
    // output. Pixel snapping converts layout coordinates into integer
    // device-pixel coordinates so painted edges land exactly on physical
    // pixel boundaries.
    //
    // Non-integer coordinates can arise for several reasons, including:
    //   - flex distribution, percentages, centering, and text measurement
    //     can produce fractional element sizes and positions;
    //   - at fractional scale factors (for example 125% or 150%), integer
    //     logical-pixel values can map to non-integer device-pixel values.
    //
    // We pixel-snap by rounding in device-pixel space, after multiplying
    // by `scale_factor`, so that snapping targets physical pixels. Bounds
    // are divided by `scale_factor` before being returned to GPUI.
    //
    // Midpoints are rounded toward zero. This is a stylistic choice: a
    // 1-logical-pixel line at 150% scale should render as 1 dp rather than
    // 2 dp.
    //
    // Pixel snapping is done in two phases:
    //
    //  1. Pre-layout metric snapping. Before Taffy computes layout, all
    //     authored absolute lengths are rounded in `to_taffy`. This
    //     includes borders, padding, gaps, and explicit sizes.
    //     Custom-measured leaf nodes have their measured sizes rounded up
    //     to integer device-pixel lengths.
    //
    //  2. Post-layout edge snapping. After Taffy resolves the tree, layout
    //     relationships such as flex shares, grid tracks, percentages, and
    //     centering can produce new fractional edge positions. Boxes now
    //     have edges in absolute coordinates, and snapping must decide
    //     where those edges land on the device-pixel grid.
    //
    // Ideally, post-layout snapping would satisfy:
    //
    //  - Edge closure. Two raw layout edges at the same absolute position
    //    should snap to the same pixel column.
    //  - Translation stability. A component's internal geometry should not
    //    change when it moves to a new absolute position.
    //
    // These goals are in tension because rounding is not associative.
    // The simple local schemes make different tradeoffs:
    //
    //  - Absolute edge rounding gives each window coordinate one answer,
    //    so coincident edges always close globally. But a span's snapped
    //    length is `round(far) - round(near)`, which may change by 1 dp
    //    as its absolute origin moves.
    //
    //  - Parent-relative edge rounding rounds each child inside its
    //    parent's coordinate space. This guarantees translation stability,
    //    but a shared edge reached through different parents can
    //    accumulate different rounding, causing non-closure between
    //    cousins.
    //
    //  - Length rounding rounds each width, height, and thickness
    //    independently and then places boxes from those rounded lengths.
    //    Sizes stay stable under translation, but neighboring boxes derive
    //    their shared boundary from different sources, so closure is not
    //    guaranteed.
    //
    // We apply absolute edge rounding for each element's outer box in
    // post-layout rounding to preserve closure. Border and padding widths
    // are not touched by post-layout rounding; they keep their pre-layout
    // rounded value so that they remain stable under translation.
    //
    // This gives both closure and translation stability in the case that
    // all local metrics are integer device-pixel lengths. Pre-layout
    // rounding covers that in most cases. The exception is metrics
    // resolved by layout relationships, such as percentages. Outer box
    // edges will still close globally, and painted border widths are still
    // snapped independently, but the raw content-box origin can carry a
    // 1dp residual into descendants.

    pub fn layout_bounds(&mut self, id: LayoutId, scale_factor: f32) -> Bounds<Pixels> {
        self.layout_bounds_for(id, scale_factor, false)
    }

    fn layout_bounds_for(
        &mut self,
        id: LayoutId,
        scale_factor: f32,
        _parent_lookup: bool,
    ) -> Bounds<Pixels> {
        #[cfg(feature = "rayx_diagnostics")]
        {
            self.diagnostics[16] += 1;
        }
        if let Some(layout) = self.absolute_layout_bounds.get(&id).cloned() {
            #[cfg(feature = "rayx_diagnostics")]
            {
                self.diagnostics[17] += 1;
                if _parent_lookup {
                    self.diagnostics[18] += 1;
                }
            }
            return layout;
        }

        #[cfg(feature = "rayx_diagnostics")]
        if _parent_lookup {
            self.diagnostics[19] += 1;
        }

        let layout = self.taffy.layout(id.into()).expect(EXPECT_MESSAGE);
        let layout_location = layout.location;
        let layout_size = layout.size;
        let parent = self.taffy.parent(id.0);

        let absolute_outer_origin = match parent {
            Some(parent_id) => {
                let parent_id = LayoutId::from(parent_id);
                self.layout_bounds_for(parent_id, scale_factor, true);
                let parent_origin = *self
                    .absolute_outer_origins
                    .get(&parent_id)
                    .expect("parent absolute outer origin should be cached");
                parent_origin + Point::from(layout_location)
            }
            None => Point::from(layout_location),
        };
        self.absolute_outer_origins
            .insert(id, absolute_outer_origin);

        let absolute_far = absolute_outer_origin + Point::from(Size::from(layout_size));
        let snapped_bounds = Bounds::from_corners(
            absolute_outer_origin.map(round_half_toward_zero),
            absolute_far.map(round_half_toward_zero),
        );

        let bounds = (snapped_bounds / scale_factor).map(Pixels);
        self.absolute_layout_bounds.insert(id, bounds);
        bounds
    }
}

/// A unique identifier for a layout node, generated when requesting a layout from Taffy
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(transparent)]
pub struct LayoutId(NodeId);

impl LayoutId {
    fn to_taffy_slice(node_ids: &[Self]) -> &[taffy::NodeId] {
        // SAFETY: LayoutId is repr(transparent) to taffy::tree::NodeId.
        unsafe { std::mem::transmute::<&[LayoutId], &[taffy::NodeId]>(node_ids) }
    }
}

impl std::hash::Hash for LayoutId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        u64::from(self.0).hash(state);
    }
}

impl From<NodeId> for LayoutId {
    fn from(node_id: NodeId) -> Self {
        Self(node_id)
    }
}

impl From<LayoutId> for NodeId {
    fn from(layout_id: LayoutId) -> NodeId {
        layout_id.0
    }
}

fn snap_measured_size_to_device_pixels(size: Size<Pixels>, scale_factor: f32) -> Size<f32> {
    size.map(|d| ceil_to_device_pixel(d.0.max(0.0), scale_factor))
}

fn border_widths_to_taffy(
    widths: &Edges<AbsoluteLength>,
    rem_size: Pixels,
    scale_factor: f32,
) -> TaffyRect<taffy::style::LengthPercentage> {
    let snap = |w: &AbsoluteLength| {
        taffy::style::LengthPercentage::length(round_stroke_to_device_pixel(
            w.to_pixels(rem_size).0,
            scale_factor,
        ))
    };
    TaffyRect {
        top: snap(&widths.top),
        right: snap(&widths.right),
        bottom: snap(&widths.bottom),
        left: snap(&widths.left),
    }
}

trait ToTaffy<Output> {
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> Output;
}

impl ToTaffy<taffy::style::Style> for Style {
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> taffy::style::Style {
        use taffy::style_helpers::{fr, length, minmax, repeat};

        fn to_grid_line(
            placement: &Range<crate::GridPlacement>,
        ) -> taffy::Line<taffy::GridPlacement> {
            taffy::Line {
                start: placement.start.into(),
                end: placement.end.into(),
            }
        }

        fn to_grid_repeat<T: taffy::style::CheapCloneStr>(
            unit: &Option<GridTemplate>,
        ) -> Vec<taffy::GridTemplateComponent<T>> {
            unit.map(|template| {
                match template.min_size {
                    // grid-template-*: repeat(<number>, minmax(0, 1fr));
                    crate::GridTemplateMinSize::Zero => {
                        vec![repeat(
                            template.repeat,
                            vec![minmax(length(0.0_f32), fr(1.0_f32))],
                        )]
                    }
                    // grid-template-*: repeat(<number>, minmax(min-content, 1fr));
                    crate::GridTemplateMinSize::MinContent => {
                        vec![repeat(
                            template.repeat,
                            vec![minmax(min_content(), fr(1.0_f32))],
                        )]
                    }
                    // grid-template-*: repeat(<number>, minmax(0, max-content))
                    crate::GridTemplateMinSize::MaxContent => {
                        vec![repeat(
                            template.repeat,
                            vec![minmax(length(0.0_f32), max_content())],
                        )]
                    }
                }
            })
            .unwrap_or_default()
        }

        taffy::style::Style {
            display: self.display.into(),
            direction: self.direction.into(),
            overflow: self.overflow.into(),
            scrollbar_width: self.scrollbar_width.to_taffy(rem_size, scale_factor),
            position: self.position.into(),
            inset: self.inset.to_taffy(rem_size, scale_factor),
            size: self.size.to_taffy(rem_size, scale_factor),
            min_size: self.min_size.to_taffy(rem_size, scale_factor),
            max_size: self.max_size.to_taffy(rem_size, scale_factor),
            aspect_ratio: self.aspect_ratio,
            margin: self.margin.to_taffy(rem_size, scale_factor),
            padding: self.padding.to_taffy(rem_size, scale_factor),
            border: border_widths_to_taffy(&self.border_widths, rem_size, scale_factor),
            align_items: self.align_items.map(|x| x.into()),
            align_self: self.align_self.map(|x| x.into()),
            align_content: self.align_content.map(|x| x.into()),
            justify_content: self.justify_content.map(|x| x.into()),
            gap: self.gap.to_taffy(rem_size, scale_factor),
            flex_direction: self.flex_direction.into(),
            flex_wrap: self.flex_wrap.into(),
            flex_basis: self.flex_basis.to_taffy(rem_size, scale_factor),
            flex_grow: self.flex_grow,
            flex_shrink: self.flex_shrink,
            grid_template_rows: to_grid_repeat(&self.grid_rows),
            grid_template_columns: to_grid_repeat(&self.grid_cols),
            grid_row: self
                .grid_location
                .as_ref()
                .map(|location| to_grid_line(&location.row))
                .unwrap_or_default(),
            grid_column: self
                .grid_location
                .as_ref()
                .map(|location| to_grid_line(&location.column))
                .unwrap_or_default(),
            ..Default::default()
        }
    }
}

impl ToTaffy<f32> for AbsoluteLength {
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> f32 {
        round_to_device_pixel(self.to_pixels(rem_size).0, scale_factor)
    }
}

impl ToTaffy<taffy::style::LengthPercentageAuto> for Length {
    fn to_taffy(
        &self,
        rem_size: Pixels,
        scale_factor: f32,
    ) -> taffy::prelude::LengthPercentageAuto {
        match self {
            Length::Definite(length) => length.to_taffy(rem_size, scale_factor),
            Length::Auto => taffy::prelude::LengthPercentageAuto::auto(),
        }
    }
}

impl ToTaffy<taffy::style::Dimension> for Length {
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> taffy::prelude::Dimension {
        match self {
            Length::Definite(length) => length.to_taffy(rem_size, scale_factor),
            Length::Auto => taffy::prelude::Dimension::auto(),
        }
    }
}

impl ToTaffy<taffy::style::LengthPercentage> for DefiniteLength {
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> taffy::style::LengthPercentage {
        match self {
            DefiniteLength::Absolute(length) => length.to_taffy(rem_size, scale_factor),
            DefiniteLength::Fraction(fraction) => {
                taffy::style::LengthPercentage::percent(*fraction)
            }
        }
    }
}

impl ToTaffy<taffy::style::LengthPercentageAuto> for DefiniteLength {
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> taffy::style::LengthPercentageAuto {
        match self {
            DefiniteLength::Absolute(length) => length.to_taffy(rem_size, scale_factor),
            DefiniteLength::Fraction(fraction) => {
                taffy::style::LengthPercentageAuto::percent(*fraction)
            }
        }
    }
}

impl ToTaffy<taffy::style::Dimension> for DefiniteLength {
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> taffy::style::Dimension {
        match self {
            DefiniteLength::Absolute(length) => length.to_taffy(rem_size, scale_factor),
            DefiniteLength::Fraction(fraction) => taffy::style::Dimension::percent(*fraction),
        }
    }
}

impl ToTaffy<taffy::style::LengthPercentage> for AbsoluteLength {
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> taffy::style::LengthPercentage {
        taffy::style::LengthPercentage::length(self.to_taffy(rem_size, scale_factor))
    }
}

impl ToTaffy<taffy::style::LengthPercentageAuto> for AbsoluteLength {
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> taffy::style::LengthPercentageAuto {
        taffy::style::LengthPercentageAuto::length(self.to_taffy(rem_size, scale_factor))
    }
}

impl ToTaffy<taffy::style::Dimension> for AbsoluteLength {
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> taffy::style::Dimension {
        taffy::style::Dimension::length(self.to_taffy(rem_size, scale_factor))
    }
}

impl<T, T2> From<TaffyPoint<T>> for Point<T2>
where
    T: Into<T2>,
    T2: Clone + Debug + Default + PartialEq,
{
    fn from(point: TaffyPoint<T>) -> Point<T2> {
        Point {
            x: point.x.into(),
            y: point.y.into(),
        }
    }
}

impl<T, T2> From<Point<T>> for TaffyPoint<T2>
where
    T: Into<T2> + Clone + Debug + Default + PartialEq,
{
    fn from(val: Point<T>) -> Self {
        TaffyPoint {
            x: val.x.into(),
            y: val.y.into(),
        }
    }
}

impl<T, U> ToTaffy<TaffySize<U>> for Size<T>
where
    T: ToTaffy<U> + Clone + Debug + Default + PartialEq,
{
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> TaffySize<U> {
        TaffySize {
            width: self.width.to_taffy(rem_size, scale_factor),
            height: self.height.to_taffy(rem_size, scale_factor),
        }
    }
}

impl<T, U> ToTaffy<TaffyRect<U>> for Edges<T>
where
    T: ToTaffy<U> + Clone + Debug + Default + PartialEq,
{
    fn to_taffy(&self, rem_size: Pixels, scale_factor: f32) -> TaffyRect<U> {
        TaffyRect {
            top: self.top.to_taffy(rem_size, scale_factor),
            right: self.right.to_taffy(rem_size, scale_factor),
            bottom: self.bottom.to_taffy(rem_size, scale_factor),
            left: self.left.to_taffy(rem_size, scale_factor),
        }
    }
}

impl<T, U> From<TaffySize<T>> for Size<U>
where
    T: Into<U>,
    U: Clone + Debug + Default + PartialEq,
{
    fn from(taffy_size: TaffySize<T>) -> Self {
        Size {
            width: taffy_size.width.into(),
            height: taffy_size.height.into(),
        }
    }
}

impl<T, U> From<Size<T>> for TaffySize<U>
where
    T: Into<U> + Clone + Debug + Default + PartialEq,
{
    fn from(size: Size<T>) -> Self {
        TaffySize {
            width: size.width.into(),
            height: size.height.into(),
        }
    }
}

/// The space available for an element to be laid out in
#[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
pub enum AvailableSpace {
    /// The amount of space available is the specified number of pixels
    Definite(Pixels),
    /// The amount of space available is indefinite and the node should be laid out under a min-content constraint
    #[default]
    MinContent,
    /// The amount of space available is indefinite and the node should be laid out under a max-content constraint
    MaxContent,
}

impl AvailableSpace {
    /// Returns a `Size` with both width and height set to `AvailableSpace::MinContent`.
    ///
    /// This function is useful when you want to create a `Size` with the minimum content constraints
    /// for both dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use gpux::AvailableSpace;
    /// let min_content_size = AvailableSpace::min_size();
    /// assert_eq!(min_content_size.width, AvailableSpace::MinContent);
    /// assert_eq!(min_content_size.height, AvailableSpace::MinContent);
    /// ```
    pub const fn min_size() -> Size<Self> {
        Size {
            width: Self::MinContent,
            height: Self::MinContent,
        }
    }
}

impl From<AvailableSpace> for TaffyAvailableSpace {
    fn from(space: AvailableSpace) -> TaffyAvailableSpace {
        match space {
            AvailableSpace::Definite(Pixels(value)) => TaffyAvailableSpace::Definite(value),
            AvailableSpace::MinContent => TaffyAvailableSpace::MinContent,
            AvailableSpace::MaxContent => TaffyAvailableSpace::MaxContent,
        }
    }
}

impl From<TaffyAvailableSpace> for AvailableSpace {
    fn from(space: TaffyAvailableSpace) -> AvailableSpace {
        match space {
            TaffyAvailableSpace::Definite(value) => AvailableSpace::Definite(Pixels(value)),
            TaffyAvailableSpace::MinContent => AvailableSpace::MinContent,
            TaffyAvailableSpace::MaxContent => AvailableSpace::MaxContent,
        }
    }
}

impl From<Pixels> for AvailableSpace {
    fn from(pixels: Pixels) -> Self {
        AvailableSpace::Definite(pixels)
    }
}

impl From<Size<Pixels>> for Size<AvailableSpace> {
    fn from(size: Size<Pixels>) -> Self {
        Size {
            width: AvailableSpace::Definite(size.width),
            height: AvailableSpace::Definite(size.height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::px;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use taffy::prelude::TaffyMaxContent;

    struct DropCounter(Arc<AtomicUsize>);

    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn clear_drops_measured_node_contexts() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut engine = TaffyLayoutEngine::new();

        for _ in 0..3 {
            let counter = DropCounter(dropped.clone());
            engine.request_measured_layout(
                Style::default(),
                Pixels(16.0),
                1.0,
                MeasuredLayoutKind::Custom,
                move |_, _, _, _| {
                    let _counter = &counter;
                    Size::default()
                },
            );
        }

        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        engine.clear();
        assert_eq!(dropped.load(Ordering::Relaxed), 3);
        assert!(engine.layout_pool[..3].iter().all(|id| {
            engine
                .taffy
                .get_node_context(id.0)
                .is_some_and(|context| context.measure.is_none())
        }));

        for _ in 0..2 {
            let counter = DropCounter(dropped.clone());
            engine.request_measured_layout(
                Style::default(),
                Pixels(16.0),
                1.0,
                MeasuredLayoutKind::Custom,
                move |_, _, _, _| {
                    let _counter = &counter;
                    Size::default()
                },
            );
        }
        engine.clear();
        assert_eq!(dropped.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn measured_context_slot_handles_unmeasured_and_measured_reuse() {
        fn compute(engine: &mut TaffyLayoutEngine, id: LayoutId) {
            engine
                .taffy
                .compute_layout_with_measure(
                    id.0,
                    taffy::geometry::Size::MAX_CONTENT,
                    |inputs, _, context, style| {
                        taffy::compute::compute_leaf_layout(
                            inputs,
                            style,
                            |_, _| 0.0,
                            |_, _| match context {
                                Some(context) if context.measure.is_some() => {
                                    taffy::geometry::Size {
                                        width: 20.0,
                                        height: 10.0,
                                    }
                                }
                                _ => taffy::geometry::Size::default(),
                            },
                        )
                    },
                )
                .expect(EXPECT_MESSAGE);
        }

        let mut engine = TaffyLayoutEngine::new();
        let measured = engine.request_measured_layout(
            Style::default(),
            px(16.0),
            1.0,
            MeasuredLayoutKind::Custom,
            |_, _, _, _| Size::default(),
        );
        compute(&mut engine, measured);
        assert_eq!(
            engine
                .taffy
                .layout(measured.0)
                .expect(EXPECT_MESSAGE)
                .size
                .width,
            20.0
        );

        engine.clear();
        let unmeasured = engine.request_layout(Style::default(), px(16.0), 1.0, &[]);
        assert_eq!(unmeasured, measured);
        compute(&mut engine, unmeasured);
        assert_eq!(
            engine
                .taffy
                .layout(unmeasured.0)
                .expect(EXPECT_MESSAGE)
                .size
                .width,
            0.0
        );

        engine.clear();
        assert!(!engine.taffy.dirty(unmeasured.0).expect(EXPECT_MESSAGE));
        let measured_again = engine.request_measured_layout(
            Style::default(),
            px(16.0),
            1.0,
            MeasuredLayoutKind::Custom,
            |_, _, _, _| Size::default(),
        );
        assert_eq!(measured_again, measured);
        assert!(engine.taffy.dirty(measured_again.0).expect(EXPECT_MESSAGE));
        compute(&mut engine, measured_again);
        assert_eq!(
            engine
                .taffy
                .layout(measured_again.0)
                .expect(EXPECT_MESSAGE)
                .size
                .width,
            20.0
        );
    }

    #[cfg(feature = "rayx_diagnostics")]
    #[test]
    fn records_taffy_tree_shape() {
        let mut engine = TaffyLayoutEngine::new();
        let leaf = engine.request_layout(Style::default(), Pixels(16.0), 1.0, &[]);
        let measured = engine.request_measured_layout(
            Style::default(),
            Pixels(16.0),
            1.0,
            MeasuredLayoutKind::Custom,
            |_, _, _, _| Size::default(),
        );

        let mut flex_style = Style::default();
        flex_style.display = crate::Display::Flex;
        let flex = engine.request_layout(flex_style, Pixels(16.0), 1.0, &[leaf, measured]);

        let block = engine.request_layout(Style::default(), Pixels(16.0), 1.0, &[flex]);

        let mut grid_style = Style::default();
        grid_style.display = crate::Display::Grid;
        engine.request_layout(grid_style, Pixels(16.0), 1.0, &[leaf, measured, block]);

        assert_eq!(
            engine.diagnostics(),
            [
                5, 1, 2, 3, 6, 3, 1, 2, 3, 3, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0,
            ]
        );
    }

    #[cfg(feature = "rayx_diagnostics")]
    #[test]
    fn measured_layout_samples_compare_matching_call_ordinals() {
        let input = MeasuredLayoutSample {
            known_dimensions: TaffySize {
                width: Some(10.0),
                height: None,
            },
            available_space: TaffySize::MAX_CONTENT,
            output: TaffySize {
                width: 10.0,
                height: 20.0,
            },
        };
        let mut context = NodeContext {
            measure: None,
            kind: MeasuredLayoutKind::Text,
            previous_measurements: Vec::new(),
            current_measurements: Vec::new(),
        };
        let mut diagnostics = [0; 31];

        context.record_measurement(&mut diagnostics, input);
        context.begin_measurement_frame();
        context.record_measurement(&mut diagnostics, input);
        let changed_output = MeasuredLayoutSample {
            output: TaffySize {
                width: 11.0,
                height: 20.0,
            },
            ..input
        };
        context.record_measurement(&mut diagnostics, changed_output);
        context.begin_measurement_frame();
        context.record_measurement(&mut diagnostics, input);
        context.record_measurement(&mut diagnostics, input);

        assert_eq!(&diagnostics[20..], &[5, 3, 3, 2, 1, 0, 5, 0, 0, 0, 0]);
    }

    #[cfg(feature = "rayx_diagnostics")]
    #[test]
    fn measured_layout_kind_recognizes_internal_owners() {
        assert_eq!(
            MeasuredLayoutKind::from_source_file("src/elements/text.rs"),
            MeasuredLayoutKind::Text
        );
        assert_eq!(
            MeasuredLayoutKind::from_source_file("src\\elements\\list.rs"),
            MeasuredLayoutKind::List
        );
        assert_eq!(
            MeasuredLayoutKind::from_source_file("src/elements/uniform_list.rs"),
            MeasuredLayoutKind::UniformList
        );
        assert_eq!(
            MeasuredLayoutKind::from_source_file("external/custom.rs"),
            MeasuredLayoutKind::Custom
        );
    }

    #[test]
    fn border_widths_to_taffy_use_stroke_snapping() {
        let border_widths = Edges {
            top: Pixels(0.0).into(),
            right: Pixels(0.4).into(),
            bottom: Pixels(0.5).into(),
            left: Pixels(1.6).into(),
        };
        let taffy_border = border_widths_to_taffy(&border_widths, Pixels(16.0), 1.0);

        assert_eq!(
            taffy_border.top,
            taffy::style::LengthPercentage::length(0.0)
        );
        assert_eq!(
            taffy_border.right,
            taffy::style::LengthPercentage::length(1.0)
        );
        assert_eq!(
            taffy_border.bottom,
            taffy::style::LengthPercentage::length(1.0)
        );
        assert_eq!(
            taffy_border.left,
            taffy::style::LengthPercentage::length(2.0)
        );
    }

    #[test]
    fn style_direction_maps_to_taffy_direction() {
        let mut style = Style::default();
        assert_eq!(
            taffy::style::Direction::Ltr,
            style.to_taffy(Pixels(16.0), 1.0).direction
        );

        style.direction = crate::LayoutDirection::Rtl;

        assert_eq!(
            taffy::style::Direction::Rtl,
            style.to_taffy(Pixels(16.0), 1.0).direction
        );
    }

    #[test]
    fn position_variants_map_to_taffy_layout_modes() {
        fn mapped_position(position: crate::Position) -> taffy::style::Position {
            Style {
                position,
                ..Default::default()
            }
            .to_taffy(Pixels(16.0), 1.0)
            .position
        }

        assert_eq!(
            taffy::style::Position::Relative,
            mapped_position(crate::Position::Relative)
        );
        assert_eq!(
            taffy::style::Position::Absolute,
            mapped_position(crate::Position::Absolute)
        );
        assert_eq!(
            taffy::style::Position::Absolute,
            mapped_position(crate::Position::Fixed)
        );
        assert_eq!(
            taffy::style::Position::Relative,
            mapped_position(crate::Position::Sticky)
        );
    }

    fn sized_style(width: f32, height: f32) -> Style {
        Style {
            size: size(px(width).into(), px(height).into()),
            ..Default::default()
        }
    }

    #[test]
    fn clear_reuses_nodes_and_replaces_topology() {
        let mut engine = TaffyLayoutEngine::new();
        let first_leaf = engine.request_layout(sized_style(10.0, 10.0), px(16.0), 1.0, &[]);
        let second_leaf = engine.request_layout(sized_style(20.0, 10.0), px(16.0), 1.0, &[]);
        let first_root = engine.request_layout(
            sized_style(100.0, 20.0),
            px(16.0),
            1.0,
            &[first_leaf, second_leaf],
        );

        engine.clear();

        let reused_leaf = engine.request_layout(sized_style(40.0, 10.0), px(16.0), 1.0, &[]);
        let mut root_style = sized_style(100.0, 20.0);
        root_style.display = crate::Display::Flex;
        let reused_root = engine.request_layout(root_style, px(16.0), 1.0, &[reused_leaf]);

        assert_eq!(reused_leaf, first_leaf);
        assert_eq!(reused_root, second_leaf);
        assert_eq!(engine.layout_pool.len(), 3);
        assert_eq!(engine.taffy.child_count(reused_root.0), 1);
        assert_eq!(engine.taffy.parent(reused_leaf.0), Some(reused_root.0));

        engine.detach_stale_root(reused_root);
        engine
            .taffy
            .compute_layout(reused_root.0, taffy::geometry::Size::MAX_CONTENT)
            .expect(EXPECT_MESSAGE);
        assert_eq!(
            engine
                .taffy
                .layout(reused_leaf.0)
                .expect(EXPECT_MESSAGE)
                .size,
            taffy::geometry::Size {
                width: 40.0,
                height: 10.0,
            }
        );
        assert_eq!(engine.taffy.parent(reused_root.0), None);
        assert_eq!(engine.taffy.child_count(first_root.0), 0);
    }

    #[test]
    fn reused_node_preserves_cache_until_its_topology_changes() {
        let mut engine = TaffyLayoutEngine::new();
        let first_leaf = engine.request_layout(sized_style(10.0, 10.0), px(16.0), 1.0, &[]);
        let second_leaf = engine.request_layout(sized_style(20.0, 10.0), px(16.0), 1.0, &[]);
        let root_style = sized_style(100.0, 20.0);
        let root = engine.request_layout(
            root_style.clone(),
            px(16.0),
            1.0,
            &[first_leaf, second_leaf],
        );
        engine
            .taffy
            .compute_layout(root.0, taffy::geometry::Size::MAX_CONTENT)
            .expect(EXPECT_MESSAGE);
        assert!(!engine.taffy.dirty(root.0).expect(EXPECT_MESSAGE));

        engine.clear();
        let reused_first = engine.request_layout(sized_style(10.0, 10.0), px(16.0), 1.0, &[]);
        let reused_second = engine.request_layout(sized_style(20.0, 10.0), px(16.0), 1.0, &[]);
        let unchanged_root = engine.request_layout(
            root_style.clone(),
            px(16.0),
            1.0,
            &[reused_first, reused_second],
        );
        assert_eq!(unchanged_root, root);
        assert!(!engine.taffy.dirty(unchanged_root.0).expect(EXPECT_MESSAGE));

        engine.clear();
        let reused_first = engine.request_layout(sized_style(10.0, 10.0), px(16.0), 1.0, &[]);
        let reused_second = engine.request_layout(sized_style(20.0, 10.0), px(16.0), 1.0, &[]);
        let changed_root =
            engine.request_layout(root_style, px(16.0), 1.0, &[reused_second, reused_first]);
        assert_eq!(changed_root, root);
        assert!(engine.taffy.dirty(changed_root.0).expect(EXPECT_MESSAGE));
        assert_eq!(
            engine.taffy.children(changed_root.0).expect(EXPECT_MESSAGE),
            [reused_second.0, reused_first.0]
        );
    }

    #[test]
    fn reused_node_preserves_cache_until_its_style_changes() {
        let mut engine = TaffyLayoutEngine::new();
        let style = sized_style(10.0, 10.0);
        let first_leaf = engine.request_layout(style.clone(), px(16.0), 1.0, &[]);
        engine
            .taffy
            .compute_layout(first_leaf.0, taffy::geometry::Size::MAX_CONTENT)
            .expect(EXPECT_MESSAGE);
        assert!(!engine.taffy.dirty(first_leaf.0).expect(EXPECT_MESSAGE));

        engine.clear();
        let unchanged_leaf = engine.request_layout(style, px(16.0), 1.0, &[]);
        assert_eq!(unchanged_leaf, first_leaf);
        assert!(!engine.taffy.dirty(unchanged_leaf.0).expect(EXPECT_MESSAGE));

        engine.clear();
        let changed_leaf = engine.request_layout(sized_style(20.0, 10.0), px(16.0), 1.0, &[]);
        assert_eq!(changed_leaf, first_leaf);
        assert!(engine.taffy.dirty(changed_leaf.0).expect(EXPECT_MESSAGE));
        engine
            .taffy
            .compute_layout(changed_leaf.0, taffy::geometry::Size::MAX_CONTENT)
            .expect(EXPECT_MESSAGE);
        assert_eq!(
            engine
                .taffy
                .layout(changed_leaf.0)
                .expect(EXPECT_MESSAGE)
                .size
                .width,
            20.0
        );
    }

    fn compute_child_locations(
        parent_style: Style,
        child_styles: &[Style],
    ) -> Vec<taffy::geometry::Point<f32>> {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let children = child_styles
            .iter()
            .map(|style| {
                taffy
                    .new_leaf(style.to_taffy(Pixels(16.0), 1.0))
                    .expect(EXPECT_MESSAGE)
            })
            .collect::<Vec<_>>();
        let root = taffy
            .new_with_children(parent_style.to_taffy(Pixels(16.0), 1.0), &children)
            .expect(EXPECT_MESSAGE);

        taffy
            .compute_layout(root, taffy::geometry::Size::MAX_CONTENT)
            .expect(EXPECT_MESSAGE);

        children
            .into_iter()
            .map(|child| taffy.layout(child).expect(EXPECT_MESSAGE).location)
            .collect()
    }

    #[test]
    fn fixed_position_is_out_of_flow_for_taffy_layout() {
        let mut parent = sized_style(100.0, 20.0);
        parent.display = crate::Display::Flex;

        let mut fixed_child = sized_style(20.0, 20.0);
        fixed_child.position = crate::Position::Fixed;

        let normal_child = sized_style(30.0, 20.0);

        let locations = compute_child_locations(parent, &[fixed_child, normal_child]);

        assert_eq!(0.0, locations[0].x);
        assert_eq!(0.0, locations[1].x);
    }

    #[test]
    fn absolute_position_remains_out_of_flow_for_taffy_layout() {
        let mut parent = sized_style(100.0, 20.0);
        parent.display = crate::Display::Flex;

        let mut absolute_child = sized_style(20.0, 20.0);
        absolute_child.position = crate::Position::Absolute;

        let normal_child = sized_style(30.0, 20.0);

        let locations = compute_child_locations(parent, &[absolute_child, normal_child]);

        assert_eq!(0.0, locations[0].x);
        assert_eq!(0.0, locations[1].x);
    }

    #[test]
    fn sticky_position_remains_in_flow_for_taffy_layout() {
        let mut parent = sized_style(100.0, 20.0);
        parent.display = crate::Display::Flex;

        let mut sticky_child = sized_style(20.0, 20.0);
        sticky_child.position = crate::Position::Sticky;

        let normal_child = sized_style(30.0, 20.0);

        let locations = compute_child_locations(parent, &[sticky_child, normal_child]);

        assert_eq!(0.0, locations[0].x);
        assert_eq!(20.0, locations[1].x);
    }

    #[test]
    fn relative_position_remains_in_flow_for_taffy_layout() {
        let mut parent = sized_style(100.0, 20.0);
        parent.display = crate::Display::Flex;

        let mut relative_child = sized_style(20.0, 20.0);
        relative_child.position = crate::Position::Relative;

        let normal_child = sized_style(30.0, 20.0);

        let locations = compute_child_locations(parent, &[relative_child, normal_child]);

        assert_eq!(0.0, locations[0].x);
        assert_eq!(20.0, locations[1].x);
    }

    #[test]
    fn ltr_row_layout_matches_default_row_layout() {
        let mut default_parent = sized_style(100.0, 20.0);
        default_parent.display = crate::Display::Flex;

        let mut ltr_parent = default_parent.clone();
        ltr_parent.direction = crate::LayoutDirection::Ltr;

        let children = [sized_style(20.0, 20.0), sized_style(30.0, 20.0)];

        let default_locations = compute_child_locations(default_parent, &children);
        let ltr_locations = compute_child_locations(ltr_parent, &children);

        assert_eq!(default_locations, ltr_locations);
        assert_eq!(0.0, ltr_locations[0].x);
        assert_eq!(20.0, ltr_locations[1].x);
    }

    #[test]
    fn rtl_row_layout_flows_from_inline_end() {
        let mut parent = sized_style(100.0, 20.0);
        parent.display = crate::Display::Flex;
        parent.direction = crate::LayoutDirection::Rtl;

        let locations =
            compute_child_locations(parent, &[sized_style(20.0, 20.0), sized_style(30.0, 20.0)]);

        assert_eq!(80.0, locations[0].x);
        assert_eq!(50.0, locations[1].x);
    }

    #[test]
    fn rtl_grid_layout_flows_from_inline_end() {
        let mut parent = sized_style(100.0, 20.0);
        parent.display = crate::Display::Grid;
        parent.direction = crate::LayoutDirection::Rtl;
        parent.grid_cols = Some(crate::GridTemplate {
            repeat: 2,
            min_size: crate::GridTemplateMinSize::Zero,
        });

        let locations =
            compute_child_locations(parent, &[sized_style(10.0, 10.0), sized_style(10.0, 10.0)]);

        assert_eq!(90.0, locations[0].x);
        assert_eq!(40.0, locations[1].x);
    }
}
