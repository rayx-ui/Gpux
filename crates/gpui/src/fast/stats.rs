//! `FastStats`: the work a frame did and the work fast mode saved, counted per
//! frame and accumulated over a run, for tests and benchmarks.
//!
//! Adapted from gpui-fast's `fast/stats.rs` (`LayoutStats`). gpux counts every
//! mechanism in one struct, separates the work every frame does in every mode
//! from the decisions only fast mode makes, and compiles the whole module only
//! under `test-support`, `bench-support` or `diagnostics`, so production
//! builds carry neither the fields nor the increments. Increments go through
//! [`crate::fast::fast_stat!`], which compiles to nothing elsewhere.

use crate::Window;

/// Why a scroll container drew its content on the ordinary path although
/// scroll layers are on. Indexes [`FastStats::layer_bypasses`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum LayerBypassReason {
    /// Layers are off or not active this frame (refreshing, a drag, the
    /// inspector, accessibility).
    Inactive,
    /// The container has not scrolled on enough frames in a row yet.
    NotPromoted,
    /// The container was demoted because its content kept changing or its
    /// tiles did not fit the budget, and is waiting to be promoted again.
    Demoted,
    /// The content held something a layer cannot composite.
    Ineligible,
    /// The container is painted inside another layer.
    Nested,
    /// The container's content is not a plain scroll offset away from what
    /// the layer holds (a resize, a rescale, a moved viewport).
    Moved,
}

impl LayerBypassReason {
    /// Every reason, in index order.
    pub const ALL: [LayerBypassReason; LAYER_BYPASS_REASONS] = [
        LayerBypassReason::Inactive,
        LayerBypassReason::NotPromoted,
        LayerBypassReason::Demoted,
        LayerBypassReason::Ineligible,
        LayerBypassReason::Nested,
        LayerBypassReason::Moved,
    ];

    /// A short name for reports.
    pub fn name(self) -> &'static str {
        match self {
            LayerBypassReason::Inactive => "inactive",
            LayerBypassReason::NotPromoted => "not-promoted",
            LayerBypassReason::Demoted => "demoted",
            LayerBypassReason::Ineligible => "ineligible",
            LayerBypassReason::Nested => "nested",
            LayerBypassReason::Moved => "moved",
        }
    }
}

/// How many [`LayerBypassReason`]s there are.
pub const LAYER_BYPASS_REASONS: usize = 6;

/// Why a view with view retention on was not drawn from last frame although
/// nothing it depends on changed. Each is counted in its own
/// `views_bypassed_*` counter of [`FastStats`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewBypassReason {
    /// The window is refreshing: every view is built.
    Refreshing,
    /// A drag is under way, and its preview reads what views are dragged
    /// over.
    Drag,
    /// The inspector is picking an element, which every view's build reports
    /// to.
    Inspector,
    /// AccessKit is active, and its tree is built from every view's build.
    Accessibility,
    /// Another view in the frame has the same id, so neither can be told
    /// apart from its record.
    Duplicate,
}

impl ViewBypassReason {
    /// Every reason, in report order.
    pub const ALL: [ViewBypassReason; 5] = [
        ViewBypassReason::Refreshing,
        ViewBypassReason::Drag,
        ViewBypassReason::Inspector,
        ViewBypassReason::Accessibility,
        ViewBypassReason::Duplicate,
    ];

    /// The name of the counter the reason is counted in.
    pub fn counter(self) -> &'static str {
        match self {
            ViewBypassReason::Refreshing => "views_bypassed_refreshing",
            ViewBypassReason::Drag => "views_bypassed_drag",
            ViewBypassReason::Inspector => "views_bypassed_inspector",
            ViewBypassReason::Accessibility => "views_bypassed_accessibility",
            ViewBypassReason::Duplicate => "views_bypassed_duplicate",
        }
    }
}

/// Counters of the work frames did and the work fast mode saved.
///
/// The *work* counters ([`FastStats::WORK`]) count what a frame does in every
/// mode, fast mode off included, so modes can be compared by them. The
/// *mechanism* counters ([`FastStats::MECHANISM`]) count decisions only a
/// fast-mode mechanism makes, and stay zero on frames drawn with every switch
/// off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FastStats {
    /// Frames drawn.
    pub frames: u64,

    // Work, counted in every mode.
    /// Views rendered: `Render::render` called for a view or cached view.
    pub views_built: u64,
    /// Cached views (`AnyView::cached`) drawn again from their last frame.
    /// A gpux feature that predates fast mode, so counted in every mode.
    pub cached_views_replayed: u64,
    /// Styles written to a Taffy node.
    pub style_writes: u64,
    /// Child lists written to a Taffy node.
    pub children_writes: u64,
    /// Measured leaves Taffy measured.
    pub leaves_measured: u64,
    /// Lines of text handed to the platform to be shaped.
    pub lines_shaped: u64,
    /// Primitives painted into the scene by elements.
    pub primitives_painted: u64,
    /// Primitives copied into the scene from the last frame.
    pub primitives_copied: u64,
    /// Draw orders computed by searching the bounds tree.
    pub draw_orders_computed: u64,
    /// Draw orders replayed from the last frame's fill of the bounds tree
    /// instead of searched for. The grid bounds tree does this in every mode.
    pub draw_orders_replayed: u64,

    // Layout retention.
    /// Taffy nodes claimed from the last frame by element identity.
    pub nodes_claimed: u64,
    /// Kept Taffy nodes released because their element was not drawn.
    pub nodes_released: u64,
    /// Kept Taffy nodes of list rows left unclaimed this frame, kept for up
    /// to 240 frames before release.
    pub nodes_lingering: u64,
    /// Measured leaves that carried last frame's measurement unmeasured.
    pub leaves_carried: u64,
    /// Measured leaves that measured other text to the same sizes, replayed.
    pub leaves_replayed: u64,
    /// Measured leaves measured again because their measurement changed.
    pub leaves_remeasured: u64,

    // View retention and splice.
    /// Views rendered whose layout was kept from the last frame.
    pub views_built_at_retained_layout: u64,
    /// Views drawn again from their retained record without rendering.
    pub views_reused: u64,
    /// Views built while view retention was on, though nothing they depend
    /// on changed, by [`ViewBypassReason`].
    pub views_bypassed_refreshing: u64,
    /// See [`ViewBypassReason::Drag`].
    pub views_bypassed_drag: u64,
    /// See [`ViewBypassReason::Inspector`].
    pub views_bypassed_inspector: u64,
    /// See [`ViewBypassReason::Accessibility`].
    pub views_bypassed_accessibility: u64,
    /// See [`ViewBypassReason::Duplicate`].
    pub views_bypassed_duplicate: u64,
    /// Views copied from the last frame around a rebuilt nested view.
    pub views_spliced: u64,
    /// Views built ahead of the frame that draws them.
    pub views_prebuilt: u64,

    // Scroll layers.
    /// Scroll-container frames drawn by compositing cached tiles.
    pub layers_composited: u64,
    /// Scroll-container frames whose content was painted into its layer.
    pub layers_repainted: u64,
    /// Scroll-container frames drawn on the ordinary path, by reason.
    pub layer_bypasses: [u64; LAYER_BYPASS_REASONS],
    /// Tiles the renderer was asked to rasterize.
    pub tiles_rasterized: u64,
    /// Tiles composited into frames.
    pub tiles_composited: u64,
    /// List rows rendered into a list's layer.
    pub list_rows_rendered: u64,
    /// List rows kept in a list's layer without rendering.
    pub list_rows_kept: u64,
}

impl FastStats {
    /// The names of the work counters, counted in every mode.
    pub const WORK: [&'static str; 10] = [
        "views_built",
        "cached_views_replayed",
        "style_writes",
        "children_writes",
        "leaves_measured",
        "lines_shaped",
        "primitives_painted",
        "primitives_copied",
        "draw_orders_computed",
        "draw_orders_replayed",
    ];

    /// The names of the mechanism counters, zero with fast mode off. Layer
    /// bypasses are reported per reason as `layer_bypasses.<reason>`.
    pub const MECHANISM: [&'static str; 22] = [
        "nodes_claimed",
        "nodes_released",
        "nodes_lingering",
        "leaves_carried",
        "leaves_replayed",
        "leaves_remeasured",
        "views_built_at_retained_layout",
        "views_reused",
        "views_bypassed_refreshing",
        "views_bypassed_drag",
        "views_bypassed_inspector",
        "views_bypassed_accessibility",
        "views_bypassed_duplicate",
        "views_spliced",
        "views_prebuilt",
        "layers_composited",
        "layers_repainted",
        "tiles_rasterized",
        "tiles_composited",
        "list_rows_rendered",
        "list_rows_kept",
        "layer_bypasses",
    ];

    /// Every counter as `(name, value)`, `frames` first, then the work
    /// counters, then the mechanism counters, each layer bypass reason as
    /// `layer_bypasses.<reason>`. The order is stable; reports and the
    /// `gpux-fast-stats:` bench lines use it.
    pub fn counters(&self) -> Vec<(String, u64)> {
        let mut counters: Vec<(String, u64)> = vec![
            ("frames".into(), self.frames),
            ("views_built".into(), self.views_built),
            ("cached_views_replayed".into(), self.cached_views_replayed),
            ("style_writes".into(), self.style_writes),
            ("children_writes".into(), self.children_writes),
            ("leaves_measured".into(), self.leaves_measured),
            ("lines_shaped".into(), self.lines_shaped),
            ("primitives_painted".into(), self.primitives_painted),
            ("primitives_copied".into(), self.primitives_copied),
            ("draw_orders_computed".into(), self.draw_orders_computed),
            ("draw_orders_replayed".into(), self.draw_orders_replayed),
            ("nodes_claimed".into(), self.nodes_claimed),
            ("nodes_released".into(), self.nodes_released),
            ("nodes_lingering".into(), self.nodes_lingering),
            ("leaves_carried".into(), self.leaves_carried),
            ("leaves_replayed".into(), self.leaves_replayed),
            ("leaves_remeasured".into(), self.leaves_remeasured),
            (
                "views_built_at_retained_layout".into(),
                self.views_built_at_retained_layout,
            ),
            ("views_reused".into(), self.views_reused),
            (
                "views_bypassed_refreshing".into(),
                self.views_bypassed_refreshing,
            ),
            ("views_bypassed_drag".into(), self.views_bypassed_drag),
            (
                "views_bypassed_inspector".into(),
                self.views_bypassed_inspector,
            ),
            (
                "views_bypassed_accessibility".into(),
                self.views_bypassed_accessibility,
            ),
            (
                "views_bypassed_duplicate".into(),
                self.views_bypassed_duplicate,
            ),
            ("views_spliced".into(), self.views_spliced),
            ("views_prebuilt".into(), self.views_prebuilt),
            ("layers_composited".into(), self.layers_composited),
            ("layers_repainted".into(), self.layers_repainted),
            ("tiles_rasterized".into(), self.tiles_rasterized),
            ("tiles_composited".into(), self.tiles_composited),
            ("list_rows_rendered".into(), self.list_rows_rendered),
            ("list_rows_kept".into(), self.list_rows_kept),
        ];
        counters.extend(LayerBypassReason::ALL.iter().map(|reason| {
            (
                format!("layer_bypasses.{}", reason.name()),
                self.layer_bypasses[*reason as usize],
            )
        }));
        counters
    }

    /// The value of the counter `name` names in [`Self::counters`].
    pub fn get(&self, name: &str) -> Option<u64> {
        self.counters()
            .into_iter()
            .find_map(|(counter, value)| (counter == name).then_some(value))
    }

    /// The sum of layout retention's counters: zero on frames drawn with it
    /// off.
    pub fn layout_retention_total(&self) -> u64 {
        self.nodes_claimed
            + self.nodes_released
            + self.nodes_lingering
            + self.leaves_carried
            + self.leaves_replayed
            + self.leaves_remeasured
    }

    /// The sum of view retention's counters: zero on frames drawn with it
    /// off.
    pub fn view_retention_total(&self) -> u64 {
        self.views_built_at_retained_layout + self.views_reused + self.views_bypassed()
    }

    /// The sum of splice's counters: zero on frames drawn with it off.
    pub fn splice_total(&self) -> u64 {
        self.views_spliced + self.views_prebuilt
    }

    /// The views built for any [`ViewBypassReason`].
    pub fn views_bypassed(&self) -> u64 {
        self.views_bypassed_refreshing
            + self.views_bypassed_drag
            + self.views_bypassed_inspector
            + self.views_bypassed_accessibility
            + self.views_bypassed_duplicate
    }

    /// Counts a view built for `reason`.
    pub(crate) fn bypass_view(&mut self, reason: ViewBypassReason) {
        match reason {
            ViewBypassReason::Refreshing => self.views_bypassed_refreshing += 1,
            ViewBypassReason::Drag => self.views_bypassed_drag += 1,
            ViewBypassReason::Inspector => self.views_bypassed_inspector += 1,
            ViewBypassReason::Accessibility => self.views_bypassed_accessibility += 1,
            ViewBypassReason::Duplicate => self.views_bypassed_duplicate += 1,
        }
    }

    /// The sum of every mechanism counter: zero on frames drawn with fast
    /// mode off.
    pub fn mechanism_total(&self) -> u64 {
        let bypasses: u64 = self.layer_bypasses.iter().sum();
        self.nodes_claimed
            + self.nodes_released
            + self.nodes_lingering
            + self.leaves_carried
            + self.leaves_replayed
            + self.leaves_remeasured
            + self.views_built_at_retained_layout
            + self.views_reused
            + self.views_bypassed()
            + self.views_spliced
            + self.views_prebuilt
            + self.layers_composited
            + self.layers_repainted
            + self.tiles_rasterized
            + self.tiles_composited
            + self.list_rows_rendered
            + self.list_rows_kept
            + bypasses
    }

    /// Counts a scroll-container frame drawn on the ordinary path.
    #[allow(
        dead_code,
        reason = "counted by the scroll-layer policy of a later sprint"
    )]
    pub(crate) fn bypass(&mut self, reason: LayerBypassReason) {
        self.layer_bypasses[reason as usize] += 1;
    }

    /// Adds `other`'s counts to these.
    pub fn accumulate(&mut self, other: &FastStats) {
        self.frames += other.frames;
        self.views_built += other.views_built;
        self.cached_views_replayed += other.cached_views_replayed;
        self.style_writes += other.style_writes;
        self.children_writes += other.children_writes;
        self.leaves_measured += other.leaves_measured;
        self.lines_shaped += other.lines_shaped;
        self.primitives_painted += other.primitives_painted;
        self.primitives_copied += other.primitives_copied;
        self.draw_orders_computed += other.draw_orders_computed;
        self.nodes_claimed += other.nodes_claimed;
        self.nodes_released += other.nodes_released;
        self.nodes_lingering += other.nodes_lingering;
        self.leaves_carried += other.leaves_carried;
        self.leaves_replayed += other.leaves_replayed;
        self.leaves_remeasured += other.leaves_remeasured;
        self.draw_orders_replayed += other.draw_orders_replayed;
        self.views_built_at_retained_layout += other.views_built_at_retained_layout;
        self.views_reused += other.views_reused;
        self.views_bypassed_refreshing += other.views_bypassed_refreshing;
        self.views_bypassed_drag += other.views_bypassed_drag;
        self.views_bypassed_inspector += other.views_bypassed_inspector;
        self.views_bypassed_accessibility += other.views_bypassed_accessibility;
        self.views_bypassed_duplicate += other.views_bypassed_duplicate;
        self.views_spliced += other.views_spliced;
        self.views_prebuilt += other.views_prebuilt;
        self.layers_composited += other.layers_composited;
        self.layers_repainted += other.layers_repainted;
        for (total, count) in self.layer_bypasses.iter_mut().zip(other.layer_bypasses) {
            *total += count;
        }
        self.tiles_rasterized += other.tiles_rasterized;
        self.tiles_composited += other.tiles_composited;
        self.list_rows_rendered += other.list_rows_rendered;
        self.list_rows_kept += other.list_rows_kept;
    }

    /// One `gpux-fast-stats:` line for the bench `id`, as the testkit report
    /// parses it: `gpux-fast-stats: <id> <counter>=<n> ...`.
    pub fn bench_line(&self, id: &str) -> String {
        let mut line = format!("gpux-fast-stats: {id}");
        for (name, value) in self.counters() {
            line.push_str(&format!(" {name}={value}"));
        }
        line
    }
}

/// A window's counters: the frame being drawn and the run so far.
#[derive(Default)]
pub(crate) struct WindowFastStats {
    /// The frame being drawn, or the last one drawn between frames.
    pub(crate) frame: FastStats,
    /// Every frame drawn since the window opened or the last reset.
    pub(crate) run: FastStats,
}

impl WindowFastStats {
    /// Starts counting a frame.
    pub(crate) fn begin_frame(&mut self) {
        self.frame = FastStats {
            frames: 1,
            ..FastStats::default()
        };
    }

    /// Adds the frame just drawn to the run.
    pub(crate) fn finish_frame(&mut self) {
        let frame = self.frame;
        self.run.accumulate(&frame);
    }
}

impl Window {
    /// The fast-mode counters accumulated over every frame drawn since the
    /// window opened or [`Window::reset_fast_stats`] was called.
    pub fn fast_stats(&self) -> FastStats {
        self.fast_stats.run
    }

    /// The fast-mode counters of the last frame drawn.
    pub fn fast_frame_stats(&self) -> FastStats {
        self.fast_stats.frame
    }

    /// Zeroes the counters [`Window::fast_stats`] accumulates.
    pub fn reset_fast_stats(&mut self) {
        self.fast_stats.run = FastStats::default();
    }
}

/// Starts counting the frame `window` is about to draw.
pub(crate) fn begin_frame(window: &mut Window) {
    window.fast_stats.begin_frame();
    if let Some(engine) = window.layout_engine.as_mut() {
        engine.fast_stats = FastStats::default();
    }
    window
        .text_system()
        .line_layout_cache
        .fast_lines_shaped
        .store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Gathers what the layout engine, the text system and the scene counted
/// into the frame `window` just drew, and adds the frame to the run. Called
/// once the layout engine is cleared, which releases the nodes the frame did
/// not claim, and before the frames are swapped.
pub(crate) fn finish_frame(window: &mut Window) {
    let engine = window
        .layout_engine
        .as_mut()
        .map(|engine| std::mem::take(&mut engine.fast_stats))
        .unwrap_or_default();
    let lines_shaped = window
        .text_system()
        .line_layout_cache
        .fast_lines_shaped
        .swap(0, std::sync::atomic::Ordering::Relaxed);
    let (primitives, draw_orders) = scene_work(&window.next_frame.scene);
    let scene = &window.next_frame.scene;
    let replayed = u64::from(scene.primitive_bounds.replayed)
        + u64::from(scene.positioned_primitive_bounds.replayed)
        + u64::from(scene.top_layer_primitive_bounds.replayed);
    let frame = &mut window.fast_stats.frame;
    frame.draw_orders_replayed += replayed;
    frame.accumulate(&FastStats {
        frames: 0,
        ..engine
    });
    frame.lines_shaped += lines_shaped;
    frame.primitives_painted += primitives.saturating_sub(frame.primitives_copied);
    frame.draw_orders_computed += draw_orders.saturating_sub(frame.draw_orders_replayed);
    window.fast_stats.finish_frame();
}

/// How many primitives `scene` holds and how many draw orders inserting its
/// operations took from the bounds trees, searched or replayed.
fn scene_work(scene: &crate::Scene) -> (u64, u64) {
    use crate::scene::PaintOperation;
    // Whether each open layer fixes the draw order of what it holds.
    let mut fixed: Vec<bool> = Vec::new();
    let (mut primitives, mut orders) = (0, 0);
    for operation in &scene.paint_operations {
        match operation {
            PaintOperation::Primitive(_) => {
                primitives += 1;
                orders += u64::from(!fixed.last().copied().unwrap_or(false));
            }
            PaintOperation::StartLayer(_) => {
                orders += 1;
                fixed.push(true);
            }
            PaintOperation::StartPositionedLayer(_) | PaintOperation::StartTopLayer(_) => {
                fixed.push(false)
            }
            PaintOperation::EndLayer => {
                fixed.pop();
            }
        }
    }
    (primitives, orders)
}

impl crate::Scene {
    /// How many primitives the paint operations in `range` insert.
    pub(crate) fn primitive_count_in(&self, range: std::ops::Range<usize>) -> usize {
        self.paint_operations[range]
            .iter()
            .filter(|operation| matches!(operation, crate::scene::PaintOperation::Primitive(_)))
            .count()
    }
}
