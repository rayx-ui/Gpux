//! Rendering only the rows of a `uniform_list` or `list` its layer lacks.
//!
//! Adapted from gpui-fast's `fast/layers/lists.rs`. What gpux does its own
//! way:
//!
//! - A `list` places its rows by its item tree, each at its top less the
//!   scroll offset. The layer places the rows around those the list places
//!   by the same sum where the list knows every height between
//!   ([`anchor_rows`], [`list_row_top`]), so a row lands on the device pixels
//!   the list draws it on.
//! - Rows are prepainted in the rows' own space, as they are painted
//!   ([`begin_row`], [`row_clip`]): a row's hitboxes keep what the row clips
//!   them to, and the viewport clips them where the layer shows them.
//! - A row keeps the view holding its list among what it read: gpux counts
//!   an entity updated without a notification as changed. A row renderer
//!   leasing that view is part of drawing it ([`render_items`]).
//! - A row whose reads changed is rendered where the list shows it, and
//!   before it shows otherwise ([`RowSort`], [`Row::suspect`]).
//! - The rows the layer holds that the list does not show give the frame
//!   their pixels and nothing it answers by: no mouse listener
//!   ([`listen_while_shown`]), tab stop or debug bounds ([`hide_records`]),
//!   no focus ([`hide_focus`]), no DevKit element ([`NodeRow`]); the rows
//!   shown keep their order in the tab order ([`order_tab_stops`]) and
//!   among the DevKit elements ([`shown_nodes`]). A row drawing outside its
//!   own room is held only while shown ([`RowSort::spilling`]), and rows
//!   that can be hit outside theirs keep the list on today's path
//!   ([`rows_overflow`]).
//! - A `list` whose state changed from outside is not only scrolled
//!   (`LayerRows::state_changed`).
//! - A list with sticky items, a flipped uniform list and rows holding what
//!   no tile can keep today's path ([`layered`]).
//!
//! A virtual list renders only the rows it shows. Its layer holds more: the
//! rows its viewport shows and two viewports' worth of rows on each side
//! (the overscan), each painted into the layer at its place in content
//! space and kept there by row. On a frame that only scrolled the list the
//! rows the layer holds are neither rendered, laid out, prepainted nor
//! painted: only the rows the scroll brought into the overscan are, and
//! those whose hover changed, and are added to the layer, whose tiles over
//! them become dirty (spec §8). Any other frame paints the rows afresh, as
//! a div's layer is.
//!
//! Each row the layer holds keeps what it handed the frame besides what it
//! drew: where its prepaint and paint records (hitboxes, listeners, element
//! states, dispatch nodes) lie in the frame last drawn, its hitboxes as
//! painted and the hovers it was painted by. A frame that keeps the row
//! carries those records into itself, its hitboxes moved by the scroll
//! since the row was painted, as [`crate::fast::layers::reuse`] carries a
//! div's content; a row whose hovers changed is rendered again, alone.
//!
//! The layer's content is one part per row (see
//! [`crate::fast::layers::scene::LayerContent`]), in the order of the rows,
//! and each row's tiles are hashed once, when it is painted: a frame adding
//! or changing rows hands the renderer the other rows as they were and
//! combines the hashes of the tiles they reach. Rows that left the overscan
//! are dropped a batch at a time (see [`retained_rows`]).
//!
//! The list elements call in here from a few hooks:
//!
//! - `uniform_list`: [`measure_item`], [`snap_item_offset`],
//!   [`begin_uniform_list`], [`render_rows`], [`row_indices`],
//!   [`prepaint_row`] and [`end_rows`] where it prepaints its rows;
//!   [`begin_paint_rows`], [`paint_row`] and [`end_paint_rows`] where it
//!   paints them.
//! - `list`: [`begin_list`], [`keeps_row`], [`snap_scroll_top`],
//!   [`anchor_rows`], [`place_list_item`], [`row_clip`] and [`end_list`]
//!   where it lays out and prepaints its rows; [`begin_paint_list`],
//!   [`paint_row`] and [`end_paint_list`] where it paints them. A `list` has no element id: its layer is known by
//!   the id of the elements around it and its state.
//!
//! Between a list's prepaint and its paint, what the frame does with its
//! rows lives in its layer's [`LayerRows`]. While its rows prepaint and
//! paint, `WindowLayers::painting` is set, as for a div's layer, so that
//! nested views and scroll containers are painted into the layer.

use crate::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, ElementId, EntityId, GlobalElementId,
    Hitbox, HitboxId, PaintColor, PaintIndex, Pixels, Point, PrepaintStateIndex, ScaledPixels,
    Scene, Size, TextStyle, TileCoord, Window,
    fast::{
        dependencies::{DependencyRecording, RenderDependencies, StateVersion},
        layers::{
            COMPILED, active, input, invalidate,
            paint::{self, Painting},
            policy::{self, Decision},
            record::LayerRecord,
            reuse,
            scene::{LayerContent, LayerPart, move_primitive},
            tiles::{combine_tile_hashes, dirty_tiles, part_tile_hashes},
        },
    },
    point, px,
    scene::{PaintOperation, Primitive},
    size,
};
use collections::FxHashMap;
use smallvec::SmallVec;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    mem,
    ops::Range,
    rc::Rc,
};

/// Whether virtual lists get scroll layers.
pub(crate) const LIST_LAYERS: bool = true;

/// The rows of a list's layer, and what the frame being drawn does with
/// them.
#[derive(Default)]
pub(crate) struct LayerRows {
    /// Whether the layer is a list's, which a scroll extends by the rows it
    /// uncovers instead of painting it again.
    pub(crate) list: bool,
    /// Whether the layer is a `list`'s, painted afresh for a change of its
    /// content only over the rows the list shows (see [`overscan_reach`]),
    /// rather than over the whole overscan, as a `uniform_list`'s is.
    pub(crate) repaints_shown_rows: bool,
    /// How many rows frames that kept the rows the layer held added to it
    /// since its rows were last painted afresh. What the rows read is kept
    /// for the whole layer, not by row, so it only grows on those frames:
    /// past [`REPAINT_AFTER_ADDED`] times the rows the layer is to hold, the
    /// rows are painted afresh.
    added_since_repaint: usize,
    /// The rows the layer holds, by index.
    rows: BTreeMap<usize, Row>,
    /// A uniform list's measured item, as last measured.
    measured: Option<Measured>,
    /// The layer's translation in the last frame that drew its rows.
    last_translation: Option<Point<ScaledPixels>>,
    /// How many viewports' height of rows a `list`'s layer holds on each side
    /// of the rows it shows, up to [`paint::OVERSCAN_VIEWPORTS`]. See
    /// [`overscan_reach`].
    reach: f32,
    /// The rows the list showed in the last frame that drew it, and that
    /// frame: the rows whose element states it kept alive, as a list
    /// without a layer does. See [`sort_rows`].
    visible: Option<(u64, Range<usize>)>,
    /// What the frame being drawn does with the list's rows, from its
    /// prepaint to its paint.
    frame: Option<RowsFrame>,
    /// Whether the frame being drawn composites a `list`'s layer though the
    /// view holding the list renders again, handing it a row renderer that
    /// may render any row otherwise: the list renders the rows it shows
    /// again, and the rows the layer holds besides are rendered again before
    /// they show (see [`Row::suspect`]).
    pub(crate) rerendered: bool,
    /// How many items a `list` had when it was last laid out, and whether
    /// it has as many in the frame being drawn.
    item_count: Option<usize>,
    pub(crate) items_changed: bool,
    /// The rows with records in the frame last drawn, and that frame. See
    /// [`ToldRow`].
    told: Vec<ToldRow>,
    told_frame: Option<u64>,
    /// The frame that copied the paint records of `told` from the one
    /// before, once it has.
    told_painted: Option<u64>,
    /// The rows found to draw above or below the room their list gives
    /// them, and how tall each was, in device pixels: the layer holds such a
    /// row only while the list shows it (see [`RowSort::spilling`]).
    spilling: BTreeMap<usize, ScaledPixels>,
    /// The version of a `list`'s state once it was last painted, and whether
    /// the state changed since other than by a wheel: it was spliced,
    /// remeasured, reset or scrolled from outside, which the version counts
    /// alike. Its rows are then not as the layer holds them, whether or not
    /// the view holding the list was notified.
    state_version: Option<u64>,
    pub(crate) state_changed: bool,
}

impl LayerRows {
    /// Ends the frame being drawn.
    pub(crate) fn finish_frame(&mut self) {
        self.frame = None;
    }

    fn clear(&mut self) {
        self.rows.clear();
        self.added_since_repaint = 0;
        self.last_translation = None;
    }

    /// Whether the rows the layer holds, `needed` of them from now on, are
    /// to be painted afresh for having added too many rows since they last
    /// were (see [`LayerRows::added_since_repaint`]).
    fn due_for_repaint(&self, needed: &Range<usize>) -> bool {
        self.added_since_repaint > needed.len().max(1) * REPAINT_AFTER_ADDED
    }

    /// The rows the layer holds.
    #[cfg(test)]
    pub(crate) fn held(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.keys().copied()
    }

    /// The rows the list showed in the last frame that drew it.
    #[cfg(test)]
    pub(crate) fn shown(&self) -> Option<Range<usize>> {
        self.visible.as_ref().map(|(_, visible)| visible.clone())
    }

    /// The layout keys of each row the layer holds, those it keeps the
    /// nodes of while it holds the row.
    #[cfg(test)]
    pub(crate) fn held_layout_keys(&self) -> impl Iterator<Item = (usize, &[u64])> + '_ {
        self.rows
            .iter()
            .map(|(row, held)| (*row, held.layout_keys.as_slice()))
    }

    /// Moves the prepaint ranges of the rows the layer holds with `shift`,
    /// as the frame being drawn copies their records from the last one.
    pub(crate) fn follow_prepaint(
        &mut self,
        shift: &dyn Fn(&Range<PrepaintStateIndex>) -> Range<PrepaintStateIndex>,
    ) {
        for row in self.rows.values_mut() {
            row.prepaint = shift(&row.prepaint);
        }
    }

    /// Moves, with `shift`, where the rows the frame last drawn held records
    /// of lie (see [`ToldRow`]), if the frame being drawn, `frame`, copies
    /// them all (`copied`) from that one: they are this frame's then.
    pub(crate) fn follow_told_prepaint(
        &mut self,
        frame: u64,
        next: &mut crate::window::WindowFrame,
        hidden_hitboxes: &mut Option<Range<usize>>,
        copied: &dyn Fn(&Range<PrepaintStateIndex>) -> bool,
        shift: &dyn Fn(&Range<PrepaintStateIndex>) -> Range<PrepaintStateIndex>,
    ) {
        if self.told_frame.is_none_or(|told| told + 1 != frame)
            || self.told.is_empty()
            || !self.told.iter().all(|row| copied(&row.prepaint))
        {
            return;
        }
        for row in &mut self.told {
            row.prepaint = shift(&row.prepaint);
            row.layout = row.layout.as_ref().map(shift);
            // The copy put the focus back where a row off screen held it.
            if !row.shown {
                hide_focus(next, &row.prepaint);
                note_hidden_hitboxes(hidden_hitboxes, &row.prepaint);
            }
        }
        self.told_frame = Some(frame);
    }

    /// Moves the paint records [`LayerRows::follow_told_prepaint`] moved the
    /// prepaint records of, and hides, in `next`, which copied them, those
    /// of the rows the list does not show (see [`hide_records`]): a copy
    /// puts every tab stop back.
    pub(crate) fn follow_told_paint(
        &mut self,
        frame: u64,
        next: &mut crate::window::WindowFrame,
        copied: &dyn Fn(&Range<PaintIndex>) -> bool,
        shift: &dyn Fn(&Range<PaintIndex>) -> Range<PaintIndex>,
    ) {
        if self.told_frame != Some(frame)
            || self.told_painted == Some(frame)
            || !self
                .told
                .iter()
                .filter_map(|row| row.paint.as_ref())
                .all(copied)
        {
            return;
        }
        self.told_painted = Some(frame);
        for row in &mut self.told {
            if let Some(paint) = row.paint.as_mut() {
                *paint = shift(paint);
                if !row.shown {
                    hide_records(next, paint);
                }
            }
        }
    }

    /// Moves the paint ranges of the rows the layer holds with `shift`, as
    /// [`LayerRows::follow_prepaint`] moves their prepaint ranges.
    pub(crate) fn follow_paint(&mut self, shift: &dyn Fn(&Range<PaintIndex>) -> Range<PaintIndex>) {
        for row in self.rows.values_mut() {
            row.paint = shift(&row.paint);
        }
    }

    /// How far the rows the layer holds have moved since the last frame
    /// that drew them, with the layer's content moved by `translation` now,
    /// in logical pixels: the records a frame copies from the last one lie
    /// where that one showed them.
    fn step(&self, translation: Point<ScaledPixels>, scale_factor: f32) -> Point<Pixels> {
        let last = self.last_translation.unwrap_or(translation);
        point(
            px((translation.x.0 - last.x.0) / scale_factor),
            px((translation.y.0 - last.y.0) / scale_factor),
        )
    }

    /// Whether a row the layer holds inserted the hitbox `id`.
    pub(crate) fn holds_hitbox(&self, id: HitboxId) -> bool {
        self.rows
            .values()
            .any(|row| row.hitboxes.iter().any(|hitbox| hitbox.id == id))
    }
}

/// How many times the rows a list's layer is to hold its frames that keep
/// rows may add before its rows are painted afresh.
const REPAINT_AFTER_ADDED: usize = 4;

/// How many viewports' height of rows a frame that keeps a `list`'s rows may
/// add to the overscan on each side, growing it back after a repaint.
const REACH_STEP_VIEWPORTS: f32 = 0.5;

/// How many viewports' height of rows on each side of those it shows a
/// `list`'s layer is to hold after a frame in `mode`, having held `reach`
/// before, `had_record` whether it held any.
///
/// The first frame painting the layer paints the whole overscan, which the
/// frames compositing it pay back. A frame painting it afresh after that,
/// for a change of its content, paints only the rows the list shows, as the
/// list does without a layer: a view holding the list that is notified now
/// and then, as a chat transcript is when it reaches its end, would
/// otherwise rebuild five viewports of rows each time, and its layer be
/// demoted for those spikes (see [`crate::fast::layers::work`]). The frames
/// keeping the rows after it grow the overscan back a step at a time.
fn overscan_reach(mode: Mode, reach: f32, had_record: bool) -> f32 {
    match mode {
        Mode::Extend => (reach + REACH_STEP_VIEWPORTS).min(paint::OVERSCAN_VIEWPORTS),
        Mode::Repaint if had_record => 0.,
        Mode::Repaint => paint::OVERSCAN_VIEWPORTS,
    }
}

#[cfg(test)]
thread_local! {
    static EXTENDED_FRAMES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static RENDERED_FOR_HOVER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many frames on this thread composited a list's layer keeping the
/// rows it held.
#[cfg(test)]
pub(crate) fn extended_frames() -> usize {
    EXTENDED_FRAMES.with(|frames| frames.get())
}

/// How many rows a list's layer held that frames on this thread rendered
/// again because their hovers changed.
#[cfg(test)]
pub(crate) fn rows_rendered_for_hover() -> usize {
    RENDERED_FOR_HOVER.with(|rows| rows.get())
}

/// Counts a frame that composited a list's layer keeping the rows it held,
/// rendering `for_hover` of them again because their hovers changed.
fn count_extended_frame(for_hover: usize) {
    #[cfg(test)]
    {
        EXTENDED_FRAMES.with(|frames| frames.set(frames.get() + 1));
        RENDERED_FOR_HOVER.with(|rows| rows.set(rows.get() + for_hover));
    }
    let _ = for_hover;
}

/// A row as the layer holds it.
struct Row {
    /// The row's slot, as wide as the viewport, in content space.
    slot: Bounds<ScaledPixels>,
    /// What painting the row drew, in content space.
    part: LayerPart,
    /// The hash of what the row draws over each tile it reaches.
    tile_hashes: Vec<(TileCoord, u64)>,
    /// Whether the row painted a path, which tiles cannot hold.
    has_paths: bool,
    /// The layer's translation when the row was painted: its hitboxes, and
    /// the positions its closures and element states hold, are window
    /// positions at it.
    translation: Point<ScaledPixels>,
    /// The hitboxes the row inserted, as inserted, their masks clipped by
    /// the row's own clips but not by the viewport.
    hitboxes: Vec<Hitbox>,
    /// The hovers the row was painted by.
    hovers: Vec<(HitboxId, bool)>,
    /// Whether the row read the hover of a hitbox it did not insert, such
    /// as a group around the list: nothing the row holds says where that
    /// hover can change, and the row is rendered whenever it shows.
    foreign_hovers: bool,
    /// What prepainting the row added to the frame last drawn.
    prepaint: Range<PrepaintStateIndex>,
    /// What painting the row added to the frame last drawn, the scene aside.
    paint: Range<PaintIndex>,
    /// Whether the row keeps element states.
    has_states: bool,
    /// The keys of the layout nodes laying the row out claimed: kept while
    /// the row is, so that rendering it again reuses them.
    layout_keys: Vec<u64>,
    /// The entities laying the row out, prepainting and painting it read,
    /// as of when it was rendered: when one of them changes, the row is
    /// rendered again, alone (see [`changed_rows`]). What the list read
    /// besides is the layer's record's.
    dependencies: RenderDependencies,
    /// The views drawn in the row, which it is rendered again for when one
    /// is notified.
    views: Rc<[EntityId]>,
    /// Whether the row was held, but not rendered, through a frame its
    /// list's view rendered again in (see [`LayerRows::rerendered`]) or one
    /// that changed what it read while the list did not show it: it is
    /// rendered again before it shows.
    suspect: bool,
    /// Whether the row's element states were dropped, as a list without a
    /// layer drops them when the row leaves its viewport: the row is
    /// rendered afresh before it shows again. See [`sort_rows`].
    dropped: bool,
    /// Whether the list shows the row in the frame last drawn. A row it does
    /// not show gives that frame its pixels and nothing else, as a list
    /// without a layer does not draw it at all: see [`hide_records`].
    shown: Rc<Cell<bool>>,
}

impl Row {
    /// How far the row's records lag behind where it shows with the layer's
    /// content moved by `translation`, in logical pixels.
    fn delta(&self, translation: Point<ScaledPixels>, scale_factor: f32) -> Point<Pixels> {
        point(
            px((translation.x.0 - self.translation.x.0) / scale_factor),
            px((translation.y.0 - self.translation.y.0) / scale_factor),
        )
    }
}

/// A uniform list's measured item, and what it was measured with besides
/// the item itself.
struct Measured {
    size: Size<Pixels>,
    rem_size: Pixels,
    text_style: TextStyle,
}

/// The rows a list renders into its layer this frame.
pub(crate) struct RowPlan {
    /// The rows to render, prepaint and paint into the layer, in order.
    pub(crate) render: Vec<Range<usize>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// The rows are painted afresh.
    Repaint,
    /// The rows the layer holds stay; those it lacks are added.
    Extend,
}

/// What the frame being drawn does with a list's rows.
struct RowsFrame {
    mode: Mode,
    /// For a `list`, its state, to tell its hooks from a nested list's.
    list: Option<usize>,
    /// Whether the list leaves out the rows the layer holds when it lays
    /// out the rows it shows.
    skip_held: bool,
    /// The rows the layer is to hold after the frame, at least.
    needed: Range<usize>,
    /// The rows the layer may hold after the frame; those it holds outside
    /// are dropped.
    retain: Range<usize>,
    /// The rows the layer holds that are rendered again: their hovers
    /// changed, or what they read did, or they show again after their
    /// element states were dropped.
    stale: BTreeSet<usize>,
    /// The rows the layer holds that read something that changed since
    /// they were rendered. See [`changed_rows`].
    changed: BTreeSet<usize>,
    /// The rows the list shows this frame.
    visible: Range<usize>,
    /// The rows the list showed last frame and no longer does, whose
    /// element states the frame drops. See [`sort_rows`].
    leaving: BTreeSet<usize>,
    /// The rows the layer holds whose records the frame carries, in order.
    carried: Vec<usize>,
    /// Where each row painted into the layer this frame lies, in window
    /// space.
    slots: BTreeMap<usize, Bounds<Pixels>>,
    /// The list's clip rect, in window space.
    viewport: Bounds<Pixels>,
    /// The part of the content the frame paints, in window space: the
    /// viewport and the rows around it.
    painted_region: Bounds<Pixels>,
    /// The offset the rows are painted at, snapped.
    scroll_offset: Point<Pixels>,
    /// How far the layer's content is moved into window space this frame.
    translation: Point<ScaledPixels>,
    prepaint_start: PrepaintStateIndex,
    prepaint_range: Range<PrepaintStateIndex>,
    /// Where a `list` placed each row it prepainted itself: its top. The
    /// rows around them are placed from these, adding up heights as the
    /// list does, so that a row the list shows lies in the layer exactly
    /// where the list draws it without one.
    placed: BTreeMap<usize, Pixels>,
    /// Whether the row being prepainted is one the list does not show.
    open_row_hidden: bool,
    /// Whether the row being prepainted is prepainted in the rows' own
    /// space (see [`rows_region`]), as all are but the one a `list` draws
    /// off screen, which stays in the list's clip.
    open_row_masked: bool,
    /// The rows a `list` shows without laying them out, left to its layer
    /// (see [`keeps_row`]). With those it places itself (`placed`) they are
    /// the rows a list without a layer renders, whatever snapping the scroll
    /// offset does to where they lie.
    left_to_layer: BTreeSet<usize>,
    /// The rows a `list` places outside the run of rows it shows: the row
    /// holding the focus, drawn off screen after the others for the
    /// keyboard to reach. The list draws it every frame, into the frame, and
    /// the layer does not hold it.
    loose: BTreeSet<usize>,
    /// Where a `list` places its rows from, and how far it is scrolled. See
    /// [`anchor_rows`].
    placement: Option<(Pixels, Pixels)>,
    /// Where the write generation stood when the frame's mode was decided:
    /// nothing the layer's record read was written up to it, when the layer
    /// is extended.
    vetted_at: u64,
    /// The recording of what rendering and prepainting the rows reads.
    recording: Option<DependencyRecording>,
    dependencies: RenderDependencies,
    /// What prepainting each row painted into the layer added to the frame.
    row_prepaints: Vec<RowPrepaint>,
    /// The row being prepainted, where it began, how many hitbox masks
    /// the layer had noted then and where the log of the entities read was.
    open_row: Option<(usize, PrepaintStateIndex, usize, usize)>,
    /// Where the log of the entities read stood when the recording of what
    /// the rows read began, and where in it the reads of the rows the layer
    /// keeps were told again. See [`finish_prepaint`].
    reads_start: usize,
    replayed: Range<usize>,
    /// Where the recording of the layout keys the rows claim began, and
    /// where the last row prepainted ended in it.
    layout_keys: Option<(usize, usize)>,
    /// The rows laid out outside any row's prepaint. A `list` lays out the
    /// rows it shows before it prepaints the first of them, and the layer
    /// lays out a row before it prepaints it. See [`note_row_layout`].
    row_layouts: Vec<RowLayout>,
    /// A uniform list's rows in the order it paints them, and how many it
    /// painted.
    order: Vec<usize>,
    next: usize,
    /// A `list`'s first row shown and where it lies, as it prepaints its rows.
    anchor: Option<(usize, Point<Pixels>)>,
    /// The rows a `list` prepainted itself.
    prepainted: Vec<usize>,
    /// The rows rendered and prepainted here for a `list`, painted after its
    /// own.
    extra: Vec<(usize, AnyElement)>,
    paint: Option<PaintState>,
}

/// A row laid out outside any row's prepaint: where in the recording of the
/// layout keys the rows claim the keys laying it out claimed lie, and where
/// in the log of the entities read what it read does.
struct RowLayout {
    row: usize,
    keys: Range<usize>,
    reads: Range<usize>,
    /// What laying the row out added to the frame: the element states it
    /// reached.
    records: Range<PrepaintStateIndex>,
}

/// What prepainting a row added to the frame, and the masks of its hitboxes
/// before they were clipped to the viewport.
struct RowPrepaint {
    row: usize,
    range: Range<PrepaintStateIndex>,
    masks: Vec<ContentMask<Pixels>>,
    /// The layout keys claimed since the row before it was prepainted: those
    /// laying it out claimed.
    layout_keys: Vec<u64>,
    /// Where in the log of the entities read what laying the row out and
    /// prepainting it read lies, and, once the prepaint ends, those
    /// entities.
    reads: Vec<Range<usize>>,
    entities: Rc<[EntityId]>,
}

/// A list's rows being painted.
struct PaintState {
    /// The opaque colour under the viewport, if there is one; the rows are
    /// painted into the frame otherwise.
    background: Option<PaintColor>,
    /// Whether rows are painted into the layer's scene, which the frame's is
    /// then swapped out for.
    swapped: bool,
    /// The frame's scene, swapped out for the layer's.
    scene: Scene,
    /// Where the rows' paint records begin in the frame, those carried
    /// included.
    records_start: PaintIndex,
    recording: Option<DependencyRecording>,
    /// Where the log of the entities read stood when `recording` began.
    reads_start: usize,
    /// The row being painted.
    current: Option<usize>,
    /// What each row painted into the layer's scene.
    spans: Vec<RowPaint>,
    /// What the rows inherit from where they are painted.
    painted_with: paint::PaintedWith,
    /// Where the tab stops of the list's rows begin in the frame's, those of
    /// the rows carried included.
    tab_stops_start: usize,
    /// What the rows painted into the frame, not into the layer, added to
    /// its records, by row. See [`order_tab_stops`].
    in_frame: Vec<(usize, Range<PaintIndex>)>,
}

/// What painting a row into the layer drew and added to the frame.
struct RowPaint {
    row: usize,
    /// The row's operations in the layer's scene.
    operations: Range<usize>,
    paint: Range<PaintIndex>,
    hovers: Vec<(HitboxId, bool)>,
    /// Where in the log of the entities read what painting the row read
    /// lies.
    reads: Range<usize>,
    /// The row's [`Row::shown`], which its mouse listeners go by.
    shown: Rc<Cell<bool>>,
    /// Whether the row, which the list does not show, is not to be held:
    /// it asked to handle text input.
    unheld: bool,
}

/// What a `uniform_list` does with its rows this frame: nothing new, or
/// render the plan's rows into its layer.
pub(crate) struct Rows(Option<RowPlan>);

/// The rows `visible` and `overscan` rows on each side, of `item_count`.
fn needed_rows(visible: &Range<usize>, overscan: usize, item_count: usize) -> Range<usize> {
    visible.start.saturating_sub(overscan).min(item_count)..(visible.end + overscan).min(item_count)
}

/// The rows of `needed` that `held` lacks, in runs.
fn plan(held: &BTreeSet<usize>, needed: Range<usize>) -> RowPlan {
    let mut render: Vec<Range<usize>> = Vec::new();
    for row in needed {
        if !held.contains(&row) {
            match render.last_mut() {
                Some(run) if run.end == row => run.end = row + 1,
                _ => render.push(row..row + 1),
            }
        }
    }
    RowPlan { render }
}

/// The rows a list's layer may hold after a frame that needs `needed`,
/// holding `held`: the rows it holds past `needed` are dropped a batch at a
/// time, once they are more than a quarter of `needed`, and kept until
/// then. Dropping a row changes every tile it reached, which is rasterized
/// again; dropping rows one a frame would rasterize a tile a frame for
/// nothing.
fn retained_rows(held: &BTreeSet<usize>, needed: &Range<usize>) -> Range<usize> {
    let (Some(first), Some(last)) = (held.first(), held.last()) else {
        return needed.clone();
    };
    let outside = held.iter().filter(|row| !needed.contains(row)).count();
    if outside > (needed.len() / 4).max(1) {
        needed.clone()
    } else {
        (*first).min(needed.start)..(*last + 1).max(needed.end)
    }
}

/// What a frame showing the rows `visible` of a list does with the rows its
/// layer `layer` holds, keeping them (`mode` is [`Mode::Extend`]) or not.
///
/// A list without a layer renders only the rows it shows, and the element
/// states of a row it no longer shows (a nested scroll offset, a hover
/// state) are dropped with the frame. Rows a layer holds are rendered before
/// they show and kept after, so that their element states would outlive
/// that; instead, a row leaving the viewport (`leaving`) is neither rendered
/// nor keeps its element states that frame, and is rendered afresh before
/// it shows again (see [`Row::dropped`]).
struct RowSort {
    /// The rows rendered again (see [`RowsFrame::stale`]).
    stale: BTreeSet<usize>,
    /// The rows not rendered: those the layer keeps, or, painted afresh,
    /// those leaving the viewport.
    unrendered: BTreeSet<usize>,
    leaving: BTreeSet<usize>,
    /// The rows the list does not show that draw above or below their own
    /// room, as a shadow under a row or glyphs taller than it do, or that
    /// lie across the viewport all the same (see [`hidden_in_view`]), and
    /// the row a `list` draws off screen itself: neither kept nor rendered. A list without a layer draws the rows it shows
    /// and nothing of the others, so what such a row draws over its
    /// neighbours must not show from the layer once the row itself is off
    /// screen; it is rendered when the list shows it, and held while it
    /// does.
    spilling: BTreeSet<usize>,
}

fn sort_rows(
    window: &Window,
    layer: &crate::fast::layers::Layer,
    mode: Mode,
    translation: Point<ScaledPixels>,
    viewport: Bounds<Pixels>,
    visible: &Range<usize>,
    changed: &BTreeSet<usize>,
    unheld: BTreeSet<usize>,
) -> RowSort {
    let rows = &layer.rows;
    // A row the layer holds without element states has none to drop.
    let leaving: BTreeSet<usize> = match &rows.visible {
        Some((_, before)) => before
            .clone()
            .filter(|ix| !visible.contains(ix))
            .filter(|ix| rows.rows.get(ix).is_none_or(|row| row.has_states))
            .collect(),
        None => BTreeSet::new(),
    };
    let mut spilling: BTreeSet<usize> = rows
        .spilling
        .keys()
        .copied()
        .filter(|row| !visible.contains(row))
        .collect();
    spilling.extend(unheld);
    match mode {
        Mode::Repaint => RowSort {
            stale: BTreeSet::new(),
            unrendered: leaving.clone(),
            leaving,
            spilling,
        },
        Mode::Extend => {
            // Only a row the list shows shows its hover; one it does not is
            // checked again before it shows.
            let mut stale = stale_rows(window, layer, translation, viewport);
            stale.retain(|row| visible.contains(row) && !leaving.contains(row));
            stale.extend(
                rows.rows
                    .range(visible.clone())
                    .filter(|(_, row)| row.dropped || row.suspect)
                    .map(|(ix, _)| *ix),
            );
            // A row whose reads changed is rendered again where the list
            // shows it, as a list without a layer renders the rows it shows
            // and no other. One it does not show is rendered before it shows
            // (see [`Row::suspect`]): in gpux every row reads the view
            // holding its list, and a change of what they all read would
            // otherwise rebuild the whole overscan, several times what the
            // list shows, on each frame of it.
            stale.extend(
                changed
                    .iter()
                    .filter(|row| visible.contains(row) && !leaving.contains(row)),
            );
            let unrendered = rows
                .rows
                .keys()
                .copied()
                .filter(|row| !stale.contains(row))
                .chain(leaving.iter().copied())
                .filter(|row| !spilling.contains(row))
                .collect();
            RowSort {
                stale,
                unrendered,
                leaving,
                spilling,
            }
        }
    }
}

/// The rows the layer `layer` holds whose hovers may have changed when the
/// frame being drawn paints, with the layer's content moved by
/// `translation` and clipped to `viewport`: they are rendered again.
///
/// A frame hit tests the pointer against its own hitboxes between its
/// prepaint and its paint, and elements paint their hover styles by that.
/// Which rows to render again is decided before, as the rows prepaint, when
/// the frame's other hitboxes are not known: a sheet another view lays over
/// the list this frame takes the pointer from the row under it, and one it
/// takes away gives it back. So a row is kept only while no hitbox of the
/// frame can change what it read, whatever the frame holds besides: every
/// hover it read was false and of a hitbox of its own, the pointer is over
/// none of its hitboxes where the row shows now, and none of them holds the
/// pointer captured. The row under the pointer is rendered on every frame,
/// as a view the pointer is over is (`dependencies::may_replay_hovers`).
fn stale_rows(
    window: &Window,
    layer: &crate::fast::layers::Layer,
    translation: Point<ScaledPixels>,
    viewport: Bounds<Pixels>,
) -> BTreeSet<usize> {
    let position = window.mouse_position;
    let scale_factor = window.scale_factor();
    let stale = |row: &Row| {
        if row.hovers.is_empty() {
            return false;
        }
        if row.foreign_hovers
            || row
                .hovers
                .iter()
                .any(|(id, hovered)| *hovered || window.captured_hitbox == Some(*id))
        {
            return true;
        }
        let delta = row.delta(translation, scale_factor);
        input::LayerInput::hitboxes_at(&row.hitboxes, delta, viewport).any(|hitbox| {
            let hit = hitbox.bounds.intersect(&hitbox.content_mask.bounds);
            hit.contains(&position)
        })
    };
    let rows = &layer.rows.rows;
    rows.iter()
        .filter(|(_, row)| stale(row))
        .map(|(ix, _)| *ix)
        .collect()
}

/// The size of a uniform list's measured item: `measure`d, unless the
/// frame only scrolls the list's layer, whose content, the measured item
/// included, is then as it was (spec §8).
pub(crate) fn measure_item(
    window: &mut Window,
    cx: &mut App,
    id: Option<&GlobalElementId>,
    measure: impl FnOnce(&mut Window, &mut App) -> Size<Pixels>,
) -> Size<Pixels> {
    if !LIST_LAYERS {
        return measure(window, cx);
    }
    if !COMPILED || window.fast_layers.layers.is_empty() {
        return measure(window, cx);
    }
    let Some(id) = id else {
        return measure(window, cx);
    };
    if let Some(size) = kept_item_size(window, cx, id) {
        return size;
    }
    let size = measure(window, cx);
    let rem_size = window.rem_size();
    let text_style = window.text_style();
    if let Some(layer) = window.fast_layers.layers.get_mut(id) {
        layer.rows.measured = Some(Measured {
            size,
            rem_size,
            text_style,
        });
    }
    size
}

/// The measured item's size as the layer of the list `id` keeps it, if the
/// frame only scrolls the layer.
fn kept_item_size(window: &Window, cx: &App, id: &GlobalElementId) -> Option<Size<Pixels>> {
    if paint::inside_layer(window) || !active(window, cx) {
        return None;
    }
    let layer = window.fast_layers.layers.get(id)?;
    let record = layer.record.as_ref()?;
    let measured = layer.rows.measured.as_ref()?;
    if !layer.rows.list
        || measured.rem_size != window.rem_size()
        || measured.text_style != window.text_style()
    {
        return None;
    }
    #[cfg(any(test, feature = "test-support"))]
    if let Some(decision) = window.fast_layers.forced_decision {
        return (decision == Decision::Composite).then_some(measured.size);
    }
    // A list's record keeps no hovers: its rows keep theirs.
    invalidate::scroll_only(window, cx, id, record, record.scroll_offset).then_some(measured.size)
}

/// `scroll_offset`, a uniform list's offset about to place its rows, moved
/// to whole device pixels where layers are compiled, so that rows painted
/// into a layer and rows drawn without one land on the same pixels. See
/// [`paint::snap_scroll_offset`].
pub(crate) fn snap_item_offset(window: &Window, scroll_offset: Point<Pixels>) -> Point<Pixels> {
    // With the switch off nothing is snapped: output is today's.
    if !LIST_LAYERS || !window.fast_mode.snapping() {
        return scroll_offset;
    }
    paint::snap_scroll_offset(window, scroll_offset)
}

/// Whether a list about to prepaint its rows may have a layer this frame,
/// counting why not when it may not, as a scrolling `div` is counted (see
/// `paint::begin_children`). `carried` is whether the list holds nothing a
/// layer cannot carry: no sticky items, and not flipped.
fn layered(window: &mut Window, cx: &App, carried: bool) -> bool {
    if !COMPILED || !LIST_LAYERS || !window.fast_mode.scroll_layers() {
        return false;
    }
    if paint::inside_layer(window) {
        policy::count_bypass(window, policy::Bypass::Nested);
        return false;
    }
    if !active(window, cx) {
        policy::count_bypass(window, policy::Bypass::Inactive);
        return false;
    }
    // A layer's tiles are composited clipped to a rectangle on device
    // pixels, and only under the unit transform.
    let mask = window.content_mask();
    if !carried
        || mask.corner_radii != crate::Corners::default()
        || window.current_element_transform() != crate::TransformationMatrix::unit()
        || !paint::on_device_pixels(mask.bounds, window.scale_factor())
    {
        policy::count_bypass(window, policy::Bypass::Ineligible);
        return false;
    }
    true
}

/// Decides what the uniform list `id` does with its rows this frame, and
/// sets up rendering and prepainting them. The list's rows are
/// `item_height` tall, `item_count` of them from the top of
/// `padded_bounds`, scrolled by `scroll_offset`; it shows the rows
/// `visible`. A list flipped vertically keeps today's path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn begin_uniform_list(
    window: &mut Window,
    cx: &mut App,
    id: Option<&GlobalElementId>,
    padded_bounds: Bounds<Pixels>,
    scroll_offset: Point<Pixels>,
    item_height: Pixels,
    item_count: usize,
    visible: &Range<usize>,
    carried: bool,
) -> Rows {
    let Some(id) = id else {
        return Rows(None);
    };
    if item_height <= Pixels::ZERO || !layered(window, cx, carried) {
        return Rows(None);
    }
    let viewport = window.content_mask().bounds;
    let content_size = size(padded_bounds.size.width, item_height * item_count);
    let decision = policy::decide(window, cx, id, padded_bounds, content_size, scroll_offset);
    let decision = input::decide(window, id, decision, scroll_offset);
    if decision == Decision::Bypass {
        let frame = window.fast_layers.frame;
        if let Some(layer) = window.fast_layers.layers.get_mut(id) {
            layer.rows.visible = Some((frame, visible.clone()));
        }
        return Rows(None);
    }
    let overscan = (viewport.size.height * paint::OVERSCAN_VIEWPORTS / item_height)
        .ceil()
        .max(1.) as usize;
    let needed = needed_rows(visible, overscan, item_count);
    let layer = paint::layer_mut(window, id);
    let extends = decision == Decision::Composite
        && layer.rows.list
        && !layer.rows.due_for_repaint(&needed)
        && layer
            .record
            .as_ref()
            .is_some_and(|record| record.scroll_offset.x == scroll_offset.x);
    let mode = if extends { Mode::Extend } else { Mode::Repaint };
    let changed = match mode {
        Mode::Extend => changed_rows(window, cx, id),
        Mode::Repaint => BTreeSet::new(),
    };

    let translation = paint::translation(window, scroll_offset);
    let slot = |row: usize| Bounds {
        origin: point(
            viewport.origin.x,
            padded_bounds.origin.y + scroll_offset.y + item_height * row,
        ),
        size: size(viewport.size.width, item_height),
    };
    let unheld = hidden_in_view(
        needed
            .clone()
            .map(|row| (row, slot(row).top(), item_height)),
        visible,
        viewport,
    );
    let RowSort {
        stale,
        unrendered,
        leaving,
        spilling,
    } = sort_rows(
        window,
        &window.fast_layers.layers[id],
        mode,
        translation,
        viewport,
        visible,
        &changed,
        unheld,
    );
    let (retain, carried) = match mode {
        Mode::Extend => {
            let retain = retained_rows(&unrendered, &needed);
            let carried = unrendered.range(retain.clone()).copied().collect();
            (retain, carried)
        }
        Mode::Repaint => (needed.clone(), Vec::new()),
    };
    let plan = plan(&(&unrendered | &spilling), needed.clone());
    let slots: BTreeMap<usize, Bounds<Pixels>> = plan
        .render
        .iter()
        .flat_map(|run| run.clone())
        .map(|row| (row, slot(row)))
        .collect();
    let mut painted_region = viewport;
    if !needed.is_empty() {
        painted_region = painted_region
            .union(&slot(needed.start))
            .union(&slot(needed.end - 1));
    }
    let order = slots.keys().copied().collect();
    let frame = RowsFrame {
        mode,
        list: None,
        skip_held: false,
        needed,
        retain,
        stale,
        changed,
        visible: visible.clone(),
        leaving,
        carried,
        slots,
        viewport,
        painted_region,
        scroll_offset,
        translation,
        prepaint_start: window.prepaint_index(),
        prepaint_range: window.prepaint_index()..window.prepaint_index(),
        placed: BTreeMap::new(),
        open_row_hidden: false,
        open_row_masked: false,
        left_to_layer: BTreeSet::new(),
        loose: BTreeSet::new(),
        placement: None,
        vetted_at: cx.entities.fast_access_log.write_generation(),
        recording: Some(cx.begin_recording_dependencies()),
        dependencies: RenderDependencies::default(),
        row_prepaints: Vec::new(),
        open_row: None,
        reads_start: reads_len(cx),
        replayed: 0..0,
        layout_keys: Some(record_layout_keys(window)),
        row_layouts: Vec::new(),
        order,
        next: 0,
        anchor: None,
        prepainted: Vec::new(),
        extra: Vec::new(),
        paint: None,
    };
    window.fast_layers.painting = Some(marker(id, &frame));
    paint::layer_mut(window, id).rows.frame = Some(frame);
    if mode == Mode::Extend {
        keep_reads(window, cx, id);
        carry_prepaint_rows(window, id);
    }
    Rows(Some(plan))
}

/// What the rows of a list that `frame` draws are prepainted and painted
/// in, in place of the list's own clip: the list's width, and as far above
/// and below as a row could ever be held.
///
/// A row's masks (its hitboxes', its primitives') are kept with it, moved
/// with the scroll, for as long as the layer holds it. The rows are painted
/// in their own space so that no edge of what they were painted in comes to
/// clip them once they have moved; the viewport clips them where they are
/// composited, as the list's own clip does without a layer. Only the rows
/// the layer is to hold are rendered, so nothing more is drawn for it.
fn rows_region(frame: &RowsFrame) -> Bounds<Pixels> {
    const REACH: Pixels = px(1_000_000.);
    let region = frame.painted_region.union(&frame.viewport);
    Bounds::from_corners(
        point(region.left(), region.top() - REACH),
        point(region.right(), region.bottom() + REACH),
    )
}

/// Tells the window that what the rows held by the layer of the list `id`
/// read is read again this frame, which renders them not: so that the
/// window stays told of their changes, a row view's notifications included,
/// as it is on today's path, where the list renders the rows it shows every
/// frame.
fn keep_reads(window: &mut Window, cx: &mut App, id: &GlobalElementId) {
    let Some(record) = window
        .fast_layers
        .layers
        .get(id)
        .and_then(|layer| layer.record.as_ref())
    else {
        return;
    };
    cx.replay_dependencies(&record.dependencies);
    // What the rows read, and the views drawn in them, told apart from what
    // the list does: the rows keep it themselves.
    let start = reads_len(cx);
    cx.extend_accessed(record.views.iter());
    for row in window.fast_layers.layers[id].rows.rows.values() {
        cx.replay_dependencies(&row.dependencies);
    }
    let end = reads_len(cx);
    if let Some(frame) = frame_mut(window, id) {
        frame.replayed = start..end;
    }
}

thread_local! {
    /// The row a `list` rendered last, and where in the log of the entities
    /// read what rendering it read starts. See [`spanned_render_item`].
    static RENDERED_ROW: std::cell::Cell<Option<(usize, usize)>> =
        const { std::cell::Cell::new(None) };
}

/// Runs `render`, which renders rows of a list or the item a uniform list
/// measures, as part of drawing the view that holds the list (see
/// [`crate::fast::dependencies::rendering_rows_of`]). With scroll layers
/// off it is run as it is today.
pub(crate) fn render_items<R>(
    window: &mut Window,
    cx: &mut App,
    render: impl FnOnce(&mut Window, &mut App) -> R,
) -> R {
    if !COMPILED || !LIST_LAYERS || !window.fast_mode.scroll_layers() {
        return render(window, cx);
    }
    let view = window.rendered_entity_stack.last().copied();
    let outer = crate::fast::dependencies::rendering_rows_of(cx, view);
    let rendered = render(window, cx);
    crate::fast::dependencies::rendering_rows_of(cx, outer);
    rendered
}

/// A `list`'s row renderer `render_item`, noting where what rendering each
/// row reads starts: a list lays a row out right after rendering it, and
/// what rendering it read is the row's, as what laying it out read is (see
/// [`note_row_layout`]).
pub(crate) fn spanned_render_item(
    mut render_item: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static,
) -> Box<crate::RenderItemFn> {
    Box::new(move |ix, window, cx| {
        let start = reads_len(&*cx);
        let element = render_items(window, cx, |window, cx| render_item(ix, window, cx));
        RENDERED_ROW.set(Some((ix, start)));
        element
    })
}

/// How long the log of the entities read while a recording is open is. What
/// is read next is logged even if it repeats what was read last, so that it
/// is found in the stretch of the log that follows.
fn reads_len(cx: &App) -> usize {
    crate::fast::dependencies::read_log_mark(cx)
}

/// Starts recording the keys of the layout nodes claimed from here on,
/// returning where in the log of them the recording starts.
fn record_claimed_layout_keys(window: &mut Window) -> usize {
    window
        .layout_engine
        .as_mut()
        .unwrap()
        .retention
        .record_claimed_keys()
}

/// Ends the recording [`record_claimed_layout_keys`] started at `start`.
fn finish_recording_claimed_layout_keys(window: &mut Window, start: usize) -> Vec<u64> {
    window
        .layout_engine
        .as_mut()
        .unwrap()
        .retention
        .finish_recording_claimed_keys(start)
}

/// The keys of the layout nodes claimed since the outermost recording of
/// them began.
fn claimed_layout_keys(window: &Window) -> &[u64] {
    window
        .layout_engine
        .as_ref()
        .unwrap()
        .retention
        .claimed_key_log()
}

/// Keeps the layout nodes under `keys` for another frame without
/// requesting them, as a subtree drawn from last frame does.
fn keep_retained_layout(window: &mut Window, keys: &[u64]) {
    let engine = window.layout_engine.as_mut().unwrap();
    crate::fast::layout::keep_retained_nodes(engine, keys, false);
}

/// The rows the layer of the list `id` holds that read something that
/// changed since they were rendered, as the layer's record would be found
/// changed for it (see [`invalidate::scroll_only`]), or in which a view
/// drawn was notified. They are rendered again, alone, on a frame that
/// otherwise keeps the rows the layer holds.
fn changed_rows(window: &Window, cx: &App, id: &GlobalElementId) -> BTreeSet<usize> {
    let Some(layer) = window.fast_layers.layers.get(id) else {
        return BTreeSet::new();
    };
    let source = window.fast_layers.scrolls.source(id);
    layer
        .rows
        .rows
        .iter()
        .filter(|(_, row)| {
            invalidate::changed(window, cx, &row.dependencies, source.as_ref())
                || invalidate::views_notified(window, &row.views)
        })
        .map(|(ix, _)| *ix)
        .collect()
}

/// Whether the view `view` is drawn in a row the layer of the list `id`
/// holds: a notification of it changes that row, not the rest of what the
/// layer holds.
pub(crate) fn held_row_view(window: &Window, id: &GlobalElementId, view: EntityId) -> bool {
    window.fast_layers.layers.get(id).is_some_and(|layer| {
        layer
            .rows
            .rows
            .values()
            .any(|row| row.views.contains(&view))
    })
}

/// Carries into the frame being drawn the prepaint records of the rows the
/// layer of the list `id` keeps this frame, where prepainting them would add
/// them, their hitboxes moved by the scroll since each was painted.
fn carry_prepaint_rows(window: &mut Window, id: &GlobalElementId) {
    let scale_factor = window.scale_factor();
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    let Some(frame) = layer.rows.frame.as_ref() else {
        return;
    };
    let carried = frame.carried.clone();
    let leaving = frame.leaving.clone();
    let visible = frame.visible.clone();
    let (translation, viewport) = (frame.translation, frame.viewport);
    // A tooltip shows where its element was when it was requested; a scroll
    // hides tooltips (spec §7, rule 5).
    let tooltips = layer.rows.last_translation == Some(translation);
    let step = layer.rows.step(translation, scale_factor);
    let mut rows = mem::take(&mut layer.rows.rows);
    for ix in &carried {
        let Some(row) = rows.get_mut(ix) else {
            continue;
        };
        let delta = row.delta(translation, scale_factor);
        let element_states = !row.dropped && !leaving.contains(ix);
        keep_retained_layout(window, &row.layout_keys);
        row.prepaint = reuse::carry_prepaint_records(
            window,
            &row.prepaint,
            &row.hitboxes,
            delta,
            step,
            viewport,
            tooltips,
            element_states,
        );
        let nodes = row.prepaint.start.devtools_nodes_index..row.prepaint.end.devtools_nodes_index;
        for node in &mut window.next_frame.devtools_nodes[nodes] {
            node.fast_row.hidden = !visible.contains(ix);
        }
        if !visible.contains(ix) {
            hide_focus(&mut window.next_frame, &row.prepaint);
            note_hidden_hitboxes(&mut window.fast_layers.hidden_hitboxes, &row.prepaint);
        }
    }
    // Keeping the rows' layout nodes logged their keys: they are not the
    // next row's.
    let log_end = record_claimed_layout_keys(window);
    finish_recording_claimed_layout_keys(window, log_end);
    if let Some(layer) = window.fast_layers.layers.get_mut(id) {
        layer.rows.rows = rows;
        if let Some((_, mark)) = layer
            .rows
            .frame
            .as_mut()
            .and_then(|frame| frame.layout_keys.as_mut())
        {
            *mark = log_end;
        }
    }
}

/// Carries into the frame being drawn the paint records of the rows whose
/// prepaint records [`carry_prepaint_rows`] carried.
fn carry_paint_rows(window: &mut Window, id: &GlobalElementId) {
    let scale_factor = window.scale_factor();
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    let Some(frame) = layer.rows.frame.as_ref() else {
        return;
    };
    if frame.carried.is_empty() {
        return;
    }
    let carried = frame.carried.clone();
    let leaving = frame.leaving.clone();
    let visible = frame.visible.clone();
    let (translation, viewport) = (frame.translation, frame.viewport);
    let step = layer.rows.step(translation, scale_factor);
    let mut rows = mem::take(&mut layer.rows.rows);
    for ix in &carried {
        let Some(row) = rows.get_mut(ix) else {
            continue;
        };
        let element_states = !row.dropped && !leaving.contains(ix);
        row.paint = reuse::carry_paint_records(window, &row.paint, step, viewport, element_states);
        if visible.contains(ix) {
            // What the row was painted by, the view holding the list is
            // shown by: a view drawn again from its last frame is checked
            // against the hovers its rows carried, as against those of the
            // rows it painted (see `paint_row`).
            crate::fast::dependencies::note_replayed_hovers(window, &row.hovers);
        } else {
            hide_records(&mut window.next_frame, &row.paint);
        }
    }
    if let Some(layer) = window.fast_layers.layers.get_mut(id) {
        layer.rows.rows = rows;
    }
}

/// What marks the rows of the list `id` as painting into its layer, for
/// nested views and scroll containers to tell.
fn marker(id: &GlobalElementId, frame: &RowsFrame) -> Painting {
    Painting {
        id: id.clone(),
        viewport: frame.viewport,
        painted_region: frame.painted_region,
        scene: Scene::default(),
        scroll_offset: frame.scroll_offset,
        translation: frame.translation,
        prepaint_range: frame.prepaint_start.clone()..frame.prepaint_start.clone(),
        recording: None,
        dependencies: RenderDependencies::default(),
        input: Default::default(),
        view_layouts: FxHashMap::default(),
        // A list's layer is not tested for covering its content: it is
        // extended by the rows a scroll uncovers.
        content_bounds: frame.painted_region,
        surface_switches: Vec::new(),
        onto: None,
    }
}

/// Renders the rows a uniform list renders this frame with `render`: those
/// it shows, `visible`, on today's path, and the rows its layer lacks
/// otherwise, in order.
pub(crate) fn render_rows(
    rows: &Rows,
    visible: Range<usize>,
    mut render: impl FnMut(Range<usize>) -> SmallVec<[AnyElement; 64]>,
) -> SmallVec<[AnyElement; 64]> {
    let Some(plan) = &rows.0 else {
        return render(visible);
    };
    let mut items = SmallVec::new();
    for run in &plan.render {
        items.extend(render(run.clone()));
    }
    items
}

/// The indices of the rows [`render_rows`] rendered, in order.
pub(crate) fn row_indices(rows: &Rows, visible: Range<usize>) -> RowIndices {
    match &rows.0 {
        None => RowIndices::Visible(visible),
        Some(plan) => RowIndices::Planned(
            plan.render
                .iter()
                .flat_map(|run| run.clone())
                .collect::<Vec<_>>()
                .into_iter(),
        ),
    }
}

/// See [`row_indices`].
pub(crate) enum RowIndices {
    Visible(Range<usize>),
    Planned(std::vec::IntoIter<usize>),
}

impl Iterator for RowIndices {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        match self {
            RowIndices::Visible(range) => range.next(),
            RowIndices::Planned(rows) => rows.next(),
        }
    }
}

/// Prepaints a uniform list's row `ix` with `f`, noting what prepainting it
/// adds to the frame when it is painted into the list's layer.
pub(crate) fn prepaint_row<R>(
    window: &mut Window,
    cx: &mut App,
    ix: usize,
    f: impl FnOnce(&mut Window, &mut App) -> R,
) -> R {
    if !LIST_LAYERS || window.fast_layers.painting.is_none() {
        return f(window, cx);
    }
    let began = begin_row(window, cx, None, ix, false);
    let result = f(window, cx);
    if began {
        end_row(window, cx, None);
    }
    result
}

/// The frame of the list whose rows are prepainting into its layer, if it
/// is a `list` whose state has id `list`, or a uniform list for `None`.
fn prepainting_frame(window: &mut Window, list: Option<usize>) -> Option<&mut RowsFrame> {
    let id = window.fast_layers.painting.as_ref()?.id.clone();
    frame_mut(window, &id).filter(|frame| frame.list == list && frame.paint.is_none())
}

/// Notes that the row `ix` of the list whose rows are prepainting, a `list`
/// whose state has id `list` or a uniform list for `None`, begins to
/// prepaint. Returns whether it was noted: a row of a list nested in a row
/// is not.
fn begin_row(window: &mut Window, cx: &App, list: Option<usize>, ix: usize, placed: bool) -> bool {
    let start = window.prepaint_index();
    let masks = window
        .fast_layers
        .painting
        .as_ref()
        .map_or(0, |painting| painting.input.hitbox_masks.len());
    let Some(frame) = prepainting_frame(window, list) else {
        return false;
    };
    if frame.open_row.is_some() {
        return false;
    }
    frame.open_row = Some((ix, start, masks, reads_len(cx)));
    // A `list` shows the rows it places itself; the others, and a uniform
    // list's, show where they lie across the viewport.
    frame.open_row_hidden = !placed && !frame.visible.contains(&ix);
    // A row is prepainted in the rows' own space, as it is painted (see
    // `rows_region`): what it clips itself to moves with it, and the
    // list's own clip, which does not, is the viewport its hitboxes are
    // clipped to wherever the layer shows them. The row a `list` draws off
    // screen is no row of the layer's: it stays in the list's clip.
    frame.open_row_masked = !frame.loose.contains(&ix);
    if frame.open_row_masked {
        let region = rows_region(frame);
        window
            .content_mask_stack
            .push(ContentMask::rectangular(region));
    }
    true
}

/// The clip a `list` prepaints a row in: `clip`, its own, unless the row is
/// prepainted into its layer (see [`begin_row`]).
pub(crate) fn row_clip(window: &Window, clip: ContentMask<Pixels>) -> Option<ContentMask<Pixels>> {
    if !LIST_LAYERS {
        return Some(clip);
    }
    let open = window
        .fast_layers
        .painting
        .as_ref()
        .is_some_and(|painting| {
            window
                .fast_layers
                .layers
                .get(&painting.id)
                .and_then(|layer| layer.rows.frame.as_ref())
                .is_some_and(|frame| frame.open_row.is_some() && frame.open_row_masked)
        });
    (!open).then_some(clip)
}

/// Notes that the row [`begin_row`] noted is prepainted.
fn end_row(window: &mut Window, cx: &App, list: Option<usize>) {
    let end = window.prepaint_index();
    let Some(painting) = window.fast_layers.painting.as_ref() else {
        return;
    };
    let all_masks = &painting.input.hitbox_masks;
    let id = painting.id.clone();
    let Some(frame) = window
        .fast_layers
        .layers
        .get_mut(&id)
        .and_then(|layer| layer.rows.frame.as_mut())
        .filter(|frame| frame.list == list)
    else {
        return;
    };
    let Some((row, start, masks_start, reads_start)) = frame.open_row.take() else {
        return;
    };
    let hidden = mem::take(&mut frame.open_row_hidden);
    if mem::take(&mut frame.open_row_masked) {
        window.content_mask_stack.pop();
    }
    if hidden {
        hide_focus(&mut window.next_frame, &(start.clone()..end.clone()));
        note_hidden_hitboxes(
            &mut window.fast_layers.hidden_hitboxes,
            &(start.clone()..end.clone()),
        );
    }
    let masks = all_masks
        .get(masks_start..)
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    let engine = window.layout_engine.as_ref().unwrap();
    let layout_keys = match frame.layout_keys.as_mut() {
        Some((_, mark)) => {
            // The keys claimed since the row before it, but for those laying
            // out other rows claimed, and those laying it out claimed.
            let log = engine.retention.claimed_key_log();
            let row_layouts = &frame.row_layouts;
            let others = |at: usize| {
                row_layouts
                    .iter()
                    .any(|layout| layout.row != row && layout.keys.contains(&at))
            };
            let mut keys: Vec<u64> = (*mark..log.len())
                .filter(|at| !others(*at))
                .map(|at| log[at])
                .collect();
            if let Some(layout) = row_layouts.iter().rev().find(|layout| layout.row == row) {
                // Those it claimed since the row before it are taken above.
                let before = layout.keys.start..layout.keys.end.min(*mark);
                keys.extend_from_slice(log.get(before).unwrap_or_default());
            }
            *mark = log.len();
            keys
        }
        None => Vec::new(),
    };
    let mut reads: Vec<Range<usize>> = frame
        .row_layouts
        .iter()
        .filter(|layout| layout.row == row)
        .map(|layout| layout.reads.clone())
        .collect();
    reads.push(reads_start..reads_len(cx));
    frame.row_prepaints.push(RowPrepaint {
        row,
        range: start..end,
        masks,
        layout_keys,
        reads,
        entities: Rc::from([]),
    });
}

/// Where the layout keys a row of the list whose rows are prepainting into
/// its layer is about to claim, laid out outside any row's prepaint, start
/// in the recording of them, and where what it is about to read starts in
/// the log of the entities read. See [`note_row_layout`].
pub(crate) fn row_layout_start(window: &mut Window, cx: &App) -> Option<RowLayoutStart> {
    let id = window.fast_layers.painting.as_ref()?.id.clone();
    let frame = window.fast_layers.layers.get(&id)?.rows.frame.as_ref()?;
    if frame.paint.is_some() || frame.open_row.is_some() || frame.layout_keys.is_none() {
        return None;
    }
    Some(RowLayoutStart {
        keys: claimed_layout_keys(window).len(),
        reads: reads_len(cx),
        records: window.prepaint_index(),
    })
}

/// Where laying a row out starts. See [`row_layout_start`].
pub(crate) struct RowLayoutStart {
    keys: usize,
    reads: usize,
    records: PrepaintStateIndex,
}

/// Notes that laying out the row `row`, from `start` (see
/// [`row_layout_start`]), claimed the layout keys recorded since and read
/// the entities logged since: they are the row's, whichever row is
/// prepainted next.
pub(crate) fn note_row_layout(
    window: &mut Window,
    cx: &App,
    row: usize,
    start: Option<RowLayoutStart>,
) {
    let rendered = RENDERED_ROW.take();
    let Some(RowLayoutStart {
        keys: start,
        reads: reads_start,
        records,
    }) = start
    else {
        return;
    };
    let records = records..window.prepaint_index();
    let end = claimed_layout_keys(window).len();
    let Some(id) = window
        .fast_layers
        .painting
        .as_ref()
        .map(|painting| painting.id.clone())
    else {
        return;
    };
    if let Some(frame) = frame_mut(window, &id) {
        // What rendering the row, just before, read.
        let reads_start = match rendered {
            Some((rendered, from)) if rendered == row && from >= frame.reads_start => {
                from.min(reads_start)
            }
            _ => reads_start,
        };
        frame.row_layouts.push(RowLayout {
            row,
            keys: start..end,
            reads: reads_start..reads_len(cx),
            records,
        });
    }
}

/// Starts recording the layout keys a list's rows claim, returning where
/// the recording starts, twice: where it began, and where the last row ended.
fn record_layout_keys(window: &mut Window) -> (usize, usize) {
    let start = record_claimed_layout_keys(window);
    (start, start)
}

/// Ends what [`begin_uniform_list`] began, once the rows are prepainted.
pub(crate) fn end_rows(window: &mut Window, cx: &mut App, rows: Rows) {
    if rows.0.is_none() {
        return;
    }
    let Some(painting) = window.fast_layers.painting.take() else {
        debug_assert!(false, "a list's rows ended without beginning");
        return;
    };
    finish_prepaint(window, cx, &painting.id);
}

/// Ends the prepaint of the rows of the list `id`: what it added to the
/// frame and read is the frame's.
fn finish_prepaint(window: &mut Window, cx: &mut App, id: &GlobalElementId) {
    let end = window.prepaint_index();
    let Some(frame) = frame_mut(window, id) else {
        return;
    };
    frame.prepaint_range = frame.prepaint_start.clone()..end;
    if let Some((start, _)) = frame.layout_keys.take() {
        finish_recording_claimed_layout_keys(window, start);
    }
    let Some(frame) = frame_mut(window, id) else {
        return;
    };
    let recording = frame.recording.take();
    if let Some(recording) = recording {
        // What each row read is the row's; what the list read besides is
        // the record's. A row the list only measured, laying it out without
        // prepainting it, is not the layer's: what it read is no one's, as
        // a list without a layer measures it once and keeps its height.
        let mut row_reads = mem::take(&mut frame.row_prepaints);
        let mut excluded: Vec<Range<usize>> = row_reads
            .iter()
            .flat_map(|prepaint| prepaint.reads.iter().cloned())
            .chain(frame.row_layouts.iter().map(|layout| layout.reads.clone()))
            .collect();
        excluded.push(frame.replayed.clone());
        let rest = outside_spans(frame.reads_start..reads_len(cx), excluded);
        // The view holding the list stays among what the rows read: gpux
        // counts an entity updated without a notification as changed, and a
        // row that read its list's view is rendered again when that view is.
        for prepaint in &mut row_reads {
            prepaint.entities = cx.entities_read_in(prepaint.reads.iter().cloned());
        }
        let rest = cx.entities_read_in(rest);
        let all = cx.finish_recording_dependencies(recording).all;
        let dependencies = all.with_entities(rest);
        if let Some(frame) = frame_mut(window, id) {
            frame.dependencies = dependencies;
            frame.row_prepaints = row_reads;
        }
    }
}

/// Whether rows `a` and `b` draw the same thing where each lies, wherever
/// that is in the layer: one row painted again after a scroll moved it.
fn drawn_alike(a: &Row, b: &Row) -> bool {
    fn hash(row: &Row) -> u64 {
        let to_row = point(
            ScaledPixels(-row.slot.origin.x.0),
            ScaledPixels(-row.slot.origin.y.0),
        );
        let operations: Vec<PaintOperation> = row
            .part
            .scene
            .paint_operations
            .iter()
            .map(|operation| match operation {
                PaintOperation::Primitive(primitive) => {
                    let mut primitive = primitive.clone();
                    move_primitive(&mut primitive, to_row);
                    PaintOperation::Primitive(primitive)
                }
                PaintOperation::StartLayer(bounds) => PaintOperation::StartLayer(Bounds {
                    origin: bounds.origin + to_row,
                    size: bounds.size,
                }),
                // Rows painting into these are never held (see
                // `paint::uncompositable`).
                PaintOperation::StartPositionedLayer(bounds) => {
                    PaintOperation::StartPositionedLayer(Bounds {
                        origin: bounds.origin + to_row,
                        size: bounds.size,
                    })
                }
                PaintOperation::StartTopLayer(bounds) => PaintOperation::StartTopLayer(Bounds {
                    origin: bounds.origin + to_row,
                    size: bounds.size,
                }),
                PaintOperation::EndLayer => PaintOperation::EndLayer,
                PaintOperation::FastDropped(dropped) => {
                    PaintOperation::FastDropped(dropped.moved(to_row))
                }
                PaintOperation::FastClip(clip) => PaintOperation::FastClip(clip.moved(to_row)),
            })
            .collect();
        let (tiles, _) = part_tile_hashes(&operations, paint::TILE_SIZE);
        tiles.first().map_or(0, |(_, hash)| *hash)
    }
    a.part.scene.paint_operations.len() == b.part.scene.paint_operations.len() && hash(a) == hash(b)
}

/// The parts of `range` outside every one of `spans`.
fn outside_spans(range: Range<usize>, mut spans: Vec<Range<usize>>) -> Vec<Range<usize>> {
    spans.sort_by_key(|span| span.start);
    let mut parts = Vec::new();
    let mut cursor = range.start;
    for span in spans {
        if span.start > cursor {
            parts.push(cursor..span.start.min(range.end));
        }
        cursor = cursor.max(span.end);
    }
    if range.end > cursor {
        parts.push(cursor..range.end);
    }
    parts
}

fn frame_mut<'a>(window: &'a mut Window, id: &GlobalElementId) -> Option<&'a mut RowsFrame> {
    window
        .fast_layers
        .layers
        .get_mut(id)
        .and_then(|layer| layer.rows.frame.as_mut())
}

/// The id a `list` whose state `version` counts changes of has: the id of
/// the elements around it, and its state.
fn list_id(window: &Window, version: &StateVersion) -> GlobalElementId {
    let mut path: Vec<ElementId> = window.element_id_stack.to_vec();
    path.push(ElementId::NamedInteger(
        LIST_ID_NAME.into(),
        version.id() as u64,
    ));
    GlobalElementId::new(&path)
}

/// What the ids of the elements and views inside the scroll container `id`
/// start with: `id`, or for a `list`, which has no id of its own, the id of
/// the elements around it (see [`list_id`]).
pub(crate) fn content_prefix(id: &GlobalElementId) -> &[ElementId] {
    match id.split_last() {
        Some((ElementId::NamedInteger(name, _), around)) if name.as_ref() == LIST_ID_NAME => around,
        _ => id,
    }
}

/// Whether a view drawn into the rows the layer of the list `id` holds is
/// one `f` picks. A list's rows are rendered as it prepaints, not as the view
/// holding it renders, so their views are not among that view's nested ones;
/// the layer remembers them.
pub(crate) fn any_held_view(
    window: &Window,
    id: &GlobalElementId,
    f: impl Fn(EntityId) -> bool,
) -> bool {
    window
        .fast_layers
        .layers
        .get(id)
        .filter(|layer| layer.rows.list)
        .and_then(|layer| layer.record.as_ref())
        .is_some_and(|record| record.views.iter().copied().any(f))
}

/// The name of the last part of a `list`'s id.
const LIST_ID_NAME: &str = "fast-list";

/// The frame of the `list` whose rows are prepainting, if it is the one
/// whose state `version` counts changes of.
fn list_frame<'a>(window: &'a mut Window, version: &StateVersion) -> Option<&'a mut RowsFrame> {
    let id = window.fast_layers.painting.as_ref()?.id.clone();
    frame_mut(window, &id).filter(|frame| frame.list == Some(version.id()))
}

/// Decides what the `list` of `state`, laid out at `bounds`, does with its
/// rows this frame, before it lays them out and prepaints them.
pub(crate) fn begin_list(
    window: &mut Window,
    cx: &mut App,
    state: &crate::StateInner,
    bounds: Bounds<Pixels>,
) {
    // A row stuck to the top of the list does not scroll with the others.
    if !layered(window, cx, state.sticky_items.candidates.is_empty()) {
        if COMPILED && LIST_LAYERS && window.fast_mode.scroll_layers() {
            // Remembered as painted all the same, for its scrolls to be
            // told apart once it may have a layer.
            let version = state.fast_version.clone();
            let id = list_id(window, &version);
            invalidate::note_list_at_end(window, &version, scrolled_to_end(state));
            invalidate::painted_list(window, &id, &version);
        }
        return;
    }
    let version = state.fast_version.clone();
    let id = list_id(window, &version);
    let viewport = window.content_mask().bounds.intersect(&bounds);
    let scroll_top = state.scroll_top(&state.logical_scroll_top());
    let scroll_offset = paint::snap_scroll_offset(window, point(px(0.), -scroll_top));
    invalidate::note_list_at_end(window, &version, scrolled_to_end(state));
    // A view rendering again and splicing its list moves its rows to other
    // indices: they are not only rendered again, but painted afresh.
    let item_count = state.items.summary().count;
    if let Some(layer) = window.fast_layers.layers.get_mut(&id) {
        layer.rows.items_changed = layer.rows.item_count.replace(item_count) != Some(item_count);
        layer.rows.state_changed = layer
            .rows
            .state_version
            .is_some_and(|painted_at| painted_at != version.get());
    }
    // A list lays out its rows itself, and is not taken for changed when a
    // row is measured for the first time: its rows are checked where they
    // are placed.
    let decision = policy::decide(window, cx, &id, bounds, bounds.size, scroll_offset);
    let decision = input::decide(window, &id, decision, scroll_offset);
    // Remembered as painted, for its scrolls to be told apart.
    invalidate::painted_list(window, &id, &version);
    if decision == Decision::Bypass {
        return;
    }
    let layer = paint::layer_mut(window, &id);
    let mode = if decision == Decision::Composite && layer.rows.list && layer.record.is_some() {
        Mode::Extend
    } else {
        Mode::Repaint
    };
    let frame = RowsFrame {
        mode,
        list: Some(version.id()),
        skip_held: mode == Mode::Extend && state.pending_scroll.is_none() && !layer.rows.rerendered,
        needed: 0..0,
        retain: 0..0,
        stale: BTreeSet::new(),
        changed: BTreeSet::new(),
        visible: 0..0,
        leaving: BTreeSet::new(),
        carried: Vec::new(),
        slots: BTreeMap::new(),
        viewport,
        painted_region: viewport,
        scroll_offset,
        translation: Point::default(),
        prepaint_start: window.prepaint_index(),
        prepaint_range: window.prepaint_index()..window.prepaint_index(),
        placed: BTreeMap::new(),
        open_row_hidden: false,
        open_row_masked: false,
        left_to_layer: BTreeSet::new(),
        loose: BTreeSet::new(),
        placement: None,
        vetted_at: cx.entities.fast_access_log.write_generation(),
        recording: Some(cx.begin_recording_dependencies()),
        dependencies: RenderDependencies::default(),
        row_prepaints: Vec::new(),
        open_row: None,
        reads_start: reads_len(cx),
        replayed: 0..0,
        layout_keys: Some(record_layout_keys(window)),
        row_layouts: Vec::new(),
        order: Vec::new(),
        next: 0,
        anchor: None,
        prepainted: Vec::new(),
        extra: Vec::new(),
        paint: None,
    };
    let changed = match mode {
        Mode::Extend => changed_rows(window, cx, &id),
        Mode::Repaint => BTreeSet::new(),
    };
    window.fast_layers.painting = Some(marker(&id, &frame));
    let layer = paint::layer_mut(window, &id);
    layer.rows.frame = Some(RowsFrame { changed, ..frame });
    if mode == Mode::Extend {
        keep_reads(window, cx, &id);
    }
}

/// Notes that whether the `list` of `state` is scrolled to its end was read,
/// as [`crate::ListState::is_scrolled_to_end`] reads it, and what it was (see
/// [`invalidate::note_at_end_read`]).
pub(crate) fn note_at_end_read(state: &crate::StateInner) {
    invalidate::note_at_end_read(&state.fast_version, scrolled_to_end(state));
}

/// Whether scrolling the `list` of `state` to its end moves it. A view that
/// keeps its list at its end calls [`crate::ListState::scroll_to_end`] every
/// time it renders, which changes nothing once the list is there, though
/// laying the list out puts its offset in other terms: the list's version
/// does not move then.
pub(crate) fn moves_to_end(state: &std::cell::RefCell<crate::StateInner>) -> bool {
    state.try_borrow().map_or(true, |state| {
        let there = state.pending_scroll.is_none()
            && state.pending_programmatic_scroll.is_none()
            && scrolled_to_end(&state) == Some(true);
        !there
    })
}

/// Whether the `list` of `state` is scrolled to its end, or `None` if it
/// cannot scroll or the height of a row is not known yet, as
/// [`crate::ListState::is_scrolled_to_end`] answers.
pub(crate) fn scrolled_to_end(state: &crate::StateInner) -> Option<bool> {
    let bounds = state.last_layout_bounds?;
    let summary = state.items.summary();
    if summary.has_unknown_height {
        return None;
    }
    let padding = state.last_padding.unwrap_or_default();
    let content_height = summary.height + padding.top + padding.bottom;
    let scroll_max = (content_height - bounds.size.height).max(px(0.));
    if scroll_max <= px(0.) {
        return None;
    }
    let scroll_top = state.scroll_top(&state.logical_scroll_top());
    Some(scroll_top >= scroll_max)
}

/// Whether a `list`, whose state `version` counts changes of, laying out the
/// rows it shows, leaves out its row `ix`, whose size it knows when
/// `measured`: the frame only scrolled its layer, which holds the row as it
/// is.
pub(crate) fn keeps_row(
    window: &mut Window,
    version: &StateVersion,
    ix: usize,
    measured: bool,
    shown: bool,
) -> bool {
    if !LIST_LAYERS {
        return false;
    }
    if !measured {
        return false;
    }
    let Some(painting) = window.fast_layers.painting.as_ref() else {
        return false;
    };
    let Some(layer) = window.fast_layers.layers.get_mut(&painting.id) else {
        return false;
    };
    let rows = &mut layer.rows;
    let Some(frame) = rows.frame.as_mut() else {
        return false;
    };
    let kept = frame.list == Some(version.id())
        && frame.skip_held
        && rows.rows.get(&ix).is_some_and(|row| !row.suspect)
        && !frame.changed.contains(&ix);
    // `shown`: the list would render the row for its viewport, not only to
    // measure it.
    if kept && shown {
        frame.left_to_layer.insert(ix);
    }
    kept
}

/// Notes that a `list` of `state` is about to lay out and place its rows,
/// as it does again after an autoscroll: what the frame noted of the rows
/// laid out and placed so far is dropped.
pub(crate) fn begin_placing(window: &mut Window, state: &crate::StateInner) {
    if !LIST_LAYERS {
        return;
    }
    if let Some(frame) = list_frame(window, &state.fast_version) {
        frame.anchor = None;
        frame.placement = None;
        frame.placed.clear();
        frame.left_to_layer.clear();
        frame.loose.clear();
        frame.prepainted.clear();
        frame.row_prepaints.clear();
        // The row an autoscroll left open, with the clip it was given.
        frame.open_row_hidden = false;
        let masked = mem::take(&mut frame.open_row_masked);
        if frame.open_row.take().is_some() && masked {
            window.content_mask_stack.pop();
        }
    }
}

/// `scroll_top`, how far a `list` about to place its rows is scrolled, moved
/// to whole device pixels where layers are compiled, so that rows painted
/// into a layer and rows drawn without one land on the same pixels (see
/// [`paint::snap_scroll_offset`]).
pub(crate) fn snap_scroll_top(window: &Window, scroll_top: Pixels) -> Pixels {
    // With the switch off nothing is snapped: output is today's.
    if !LIST_LAYERS || !window.fast_mode.snapping() {
        return scroll_top;
    }
    paint::snap_scroll_offset(window, point(px(0.), scroll_top)).y
}

/// Notes where a `list` of `state` scrolled by `scroll_top` places its
/// rows, the first it shows being `row`: each at its top in the list less
/// `scroll_top`, down from `base`. The rows around those the list places
/// itself are placed likewise, for the list's layer.
pub(crate) fn anchor_rows(
    window: &mut Window,
    state: &crate::StateInner,
    row: usize,
    base: Point<Pixels>,
    scroll_top: Pixels,
) {
    if !LIST_LAYERS {
        return;
    }
    if let Some(frame) = list_frame(window, &state.fast_version) {
        frame.anchor = Some((
            row,
            point(base.x, list_row_top(state, base.y, scroll_top, row)),
        ));
        frame.placement = Some((base.y, scroll_top));
    }
}

/// Where a `list` of `state` scrolled by `scroll_top` places its row `row`
/// down from `base`, as [`crate::StateInner::prepaint_items`] works it out:
/// the same sum, so that a row the layer places lands on the device pixels
/// the list would draw it on.
fn list_row_top(state: &crate::StateInner, base: Pixels, scroll_top: Pixels, row: usize) -> Pixels {
    base + (state.item_top(row) - scroll_top)
}

/// Notes that a `list` of `state` is about to prepaint its row `ix` at
/// `origin`: the row begins to prepaint, the one before it prepainted, and
/// lies in its layer where the list placed it.
pub(crate) fn place_list_item(
    window: &mut Window,
    cx: &App,
    state: &crate::StateInner,
    ix: usize,
    origin: Point<Pixels>,
) {
    if !LIST_LAYERS {
        return;
    }
    let Some(id) = window.fast_layers.painting.as_ref().map(|p| p.id.clone()) else {
        return;
    };
    let list = Some(state.fast_version.id());
    if frame_mut(window, &id).is_none_or(|frame| frame.list != list) {
        return;
    }
    end_row(window, cx, list);
    if let Some(frame) = frame_mut(window, &id) {
        // The list places the rows it shows from the first on, but for
        // those it leaves to the layer; a row it places anywhere else is
        // the one it draws off screen.
        let in_flow = frame.anchor.is_some_and(|(first, _)| {
            ix >= first
                && (first..ix).all(|row| {
                    frame.placed.contains_key(&row) || frame.left_to_layer.contains(&row)
                })
        });
        if in_flow {
            frame.placed.insert(ix, origin.y);
        } else {
            frame.loose.insert(ix);
        }
        frame.prepainted.push(ix);
    }
    begin_row(window, cx, list, ix, true);
}

/// The rows the `list` of `frame` shows: those it placed in a run from the
/// first, and those of the run it left to its layer.
fn flow(frame: &RowsFrame) -> Range<usize> {
    let Some((first, _)) = frame.anchor else {
        return 0..0;
    };
    let mut end = first;
    while frame.placed.contains_key(&end) || frame.left_to_layer.contains(&end) {
        end += 1;
    }
    first..end
}

/// The rows of `slots` (each row's top and height) the list does not show
/// that lie across `viewport` all the same: under the padding a list ends
/// its rows at, or within what snapping the scroll offset moved. A list
/// without a layer does not draw them, so its layer neither holds nor
/// renders them, as it does not a row that draws outside its room.
fn hidden_in_view(
    slots: impl Iterator<Item = (usize, Pixels, Pixels)>,
    visible: &Range<usize>,
    viewport: Bounds<Pixels>,
) -> BTreeSet<usize> {
    slots
        .filter(|(row, top, height)| {
            !visible.contains(row) && *top < viewport.bottom() && *top + *height > viewport.top()
        })
        .map(|(row, ..)| row)
        .collect()
}

/// The height of a `list`'s row `row`: as the list measured it, or as the
/// layer holds it, or held it when it found it drawing outside its room.
fn row_height(
    state: &crate::StateInner,
    rows: &LayerRows,
    row: usize,
    scale_factor: f32,
) -> Option<Pixels> {
    let mut cursor = state.items.cursor::<crate::Count>(());
    cursor.seek(&crate::Count(row), sum_tree::Bias::Right);
    cursor
        .item()
        .and_then(|item| item.size())
        .map(|size| size.height)
        .or_else(|| {
            rows.rows
                .get(&row)
                .map(|held| held.slot.size.height)
                .or_else(|| rows.spilling.get(&row).copied())
                .map(|height| px(height.0 / scale_factor))
        })
}

/// Ends the layout and prepaint of the rows of the `list` of `state`, laid
/// out at `bounds` with `padding_top` above its rows: the rows around those it showed are rendered and
/// prepainted into its layer with `render_item`, as far as the layer is to
/// hold them, and the records of the rows it keeps are carried.
pub(crate) fn end_list(
    window: &mut Window,
    cx: &mut App,
    state: &crate::StateInner,
    render_item: &mut crate::RenderItemFn,
    bounds: Bounds<Pixels>,
    padding_top: Pixels,
    scroll_context: Option<crate::ScrollContainerContext>,
) {
    if !LIST_LAYERS {
        return;
    }
    let list = Some(state.fast_version.id());
    let id = match window.fast_layers.painting.as_ref() {
        Some(painting) => painting.id.clone(),
        None => {
            note_shown_without_layer(window, state, bounds, padding_top);
            return;
        }
    };
    if frame_mut(window, &id).is_none_or(|frame| frame.list != list) {
        return;
    }
    end_row(window, cx, list);
    let scale_factor = window.scale_factor();
    let Some(layer) = window.fast_layers.layers.get_mut(&id) else {
        return;
    };
    let rows = &mut layer.rows;
    let Some(frame) = rows.frame.as_mut() else {
        return;
    };
    let Some((anchor, anchor_origin)) = frame.anchor else {
        // No row was shown: nothing is painted into the layer.
        window.fast_layers.painting = None;
        policy::count_bypass(window, policy::Bypass::Ineligible);
        finish_prepaint(window, cx, &id);
        if let Some(layer) = window.fast_layers.layers.get_mut(&id) {
            layer.rows.frame = None;
        }
        return;
    };
    let item_count = state.items.summary().count;
    let viewport = frame.viewport;
    // The rows the list shows: those it renders without a layer.
    let visible = flow(frame);

    // The rows the layer holds stay where they are only if the first row
    // shown lies at a whole number of device pixels from where the layer
    // holds it, and every row it holds is as tall as the list measures it;
    // and, once the rows around it are placed, if each row it keeps lands on
    // the pixels it would painted afresh (see [`held_rows_land_alike`]).
    let mut translation = None;
    if frame.mode == Mode::Extend
        && let Some(held) = rows.rows.get(&anchor)
    {
        let x = anchor_origin.x.0 * scale_factor - held.slot.origin.x.0;
        let y = anchor_origin.y.0 * scale_factor - held.slot.origin.y.0;
        let whole = |value: f32| (value - value.round()).abs() < 0.01;
        let heights_hold = rows.rows.iter().all(|(row, held)| {
            let mut cursor = state.items.cursor::<crate::Count>(());
            cursor.seek(&crate::Count(*row), sum_tree::Bias::Right);
            cursor
                .item()
                .and_then(|item| item.size())
                .is_none_or(|size| {
                    (size.height.0 * scale_factor - held.slot.size.height.0).abs() < 0.01
                })
        });
        let in_list = rows
            .rows
            .last_key_value()
            .is_none_or(|(last, _)| *last < item_count);
        if whole(x) && whole(y) && heights_hold && in_list {
            translation = Some(point(ScaledPixels(x.round()), ScaledPixels(y.round())));
        }
    }
    let frame = rows.frame.as_mut().unwrap();
    if translation.is_none() {
        frame.mode = Mode::Repaint;
    }
    let frame_translation = translation.unwrap_or_default();
    frame.translation = frame_translation;
    let placed = mem::take(&mut frame.placed);
    let placement = frame.placement;

    // The rows shown and up to two viewports' height of rows on each side.
    let layer = window.fast_layers.layers.get_mut(&id).unwrap();
    let mode = layer
        .rows
        .frame
        .as_ref()
        .map_or(Mode::Repaint, |frame| frame.mode);
    let reach = overscan_reach(mode, layer.rows.reach, layer.record.is_some());
    layer.rows.reach = reach;
    let extent = viewport.size.height * reach;
    let (top, bottom) = (viewport.top() - extent, viewport.bottom() + extent);
    let available = crate::size(
        AvailableSpace::Definite(bounds.size.width),
        AvailableSpace::MinContent,
    );
    let mut rendered: FxHashMap<usize, AnyElement> = FxHashMap::default();
    let mut tops: BTreeMap<usize, (Pixels, Pixels)> = BTreeMap::new();
    let mut height_of = |row: usize,
                         window: &mut Window,
                         cx: &mut App,
                         rendered: &mut FxHashMap<usize, AnyElement>|
     -> Pixels {
        let rows = &window.fast_layers.layers[&id].rows;
        if let Some(height) = row_height(state, rows, row, scale_factor) {
            return height;
        }
        let mut element = render_item(row, window, cx);
        let size =
            crate::fast::layout_key::layout_as_list_item(&mut element, row, available, window, cx);
        rendered.insert(row, element);
        size.height
    };
    // A row lies where the list would place it as long as the list knows
    // the height of every row between it and the first row shown; past the
    // first it has not measured, which the layer measures itself, rows lie
    // one after another.
    let measured = |row: usize| {
        let mut cursor = state.items.cursor::<crate::Count>(());
        cursor.seek(&crate::Count(row), sum_tree::Bias::Right);
        cursor.item().is_some_and(|item| item.size().is_some())
    };
    let list_top = |row: usize| {
        placed.get(&row).copied().or_else(|| {
            placement.map(|(base, scroll_top)| list_row_top(state, base, scroll_top, row))
        })
    };
    let mut y = anchor_origin.y;
    let mut row = anchor;
    let mut known = true;
    while row < item_count && y < bottom {
        if known && let Some(top) = list_top(row) {
            y = top;
        }
        known &= measured(row);
        let height = height_of(row, window, cx, &mut rendered);
        tops.insert(row, (y, height));
        y += height;
        row += 1;
    }
    let end = row;
    let mut y = anchor_origin.y;
    let mut row = anchor;
    let mut known = true;
    while row > 0 && y > top {
        row -= 1;
        known &= measured(row);
        let height = height_of(row, window, cx, &mut rendered);
        y -= height;
        if known && let Some(top) = list_top(row) {
            y = top;
        }
        tops.insert(row, (y, height));
    }
    let needed = row..end;

    let layer = &window.fast_layers.layers[&id];
    let frame = layer.rows.frame.as_ref().unwrap();
    let (mode, changed) = (frame.mode, frame.changed.clone());
    let mut unheld = hidden_in_view(
        tops.iter()
            .map(|(row, (top, height))| (*row, *top, *height)),
        &visible,
        viewport,
    );
    unheld.extend(frame.loose.iter().copied());
    let sort = sort_rows(
        window,
        layer,
        mode,
        frame_translation,
        viewport,
        &visible,
        &changed,
        unheld,
    );
    let layer = &mut window.fast_layers.layers.get_mut(&id).unwrap().rows;
    let due_for_repaint = layer.due_for_repaint(&needed);
    let frame = layer.frame.as_mut().unwrap();
    frame.stale = sort.stale;
    frame.leaving = sort.leaving;
    frame.visible = visible;
    let spilling = sort.spilling;
    // The rows the list prepainted itself, those whose hovers changed among
    // them, are painted into the layer afresh.
    let prepainted: BTreeSet<usize> = frame.prepainted.iter().copied().collect();
    let kept: BTreeSet<usize> = match frame.mode {
        Mode::Extend => sort
            .unrendered
            .into_iter()
            .filter(|row| !prepainted.contains(row))
            .collect(),
        Mode::Repaint => BTreeSet::new(),
    };
    if frame.mode == Mode::Extend
        && (due_for_repaint
            || !held_rows_land_alike(
                &layer.rows,
                &kept,
                &needed,
                &tops,
                frame.translation,
                scale_factor,
            ))
    {
        frame.mode = Mode::Repaint;
    }
    let (held, unrendered) = match frame.mode {
        Mode::Extend => (kept.clone(), kept),
        // Painted afresh, but for the rows leaving the viewport.
        Mode::Repaint => (BTreeSet::new(), frame.leaving.clone()),
    };
    let plan = plan(&(&unrendered | &spilling), needed.clone());
    let slot = |(top, height): (Pixels, Pixels)| Bounds {
        origin: point(anchor_origin.x, top),
        size: crate::size(bounds.size.width, height),
    };
    frame.slots = plan
        .render
        .iter()
        .flat_map(|run| run.clone())
        .map(|row| (row, slot(tops[&row])))
        .collect();
    frame.retain = match frame.mode {
        Mode::Extend => retained_rows(&held, &needed),
        Mode::Repaint => needed.clone(),
    };
    frame.carried = held.range(frame.retain.clone()).copied().collect();
    frame.needed = needed;
    let mut painted_region = viewport;
    if let (Some(first), Some(last)) = (tops.values().next(), tops.values().last()) {
        painted_region = painted_region.union(&slot(*first)).union(&slot(*last));
    }
    frame.painted_region = painted_region;
    if frame.mode == Mode::Repaint {
        // Content space is window space as the rows are painted now.
        frame.translation = Point::default();
        frame.stale.clear();
    }
    let to_prepaint: Vec<(usize, Pixels)> = frame
        .slots
        .iter()
        .filter(|(row, _)| !prepainted.contains(row))
        .map(|(row, slot)| (*row, slot.origin.y))
        .collect();
    if let Some(painting) = window.fast_layers.painting.as_mut() {
        painting.painted_region = painted_region;
    }
    carry_prepaint_rows(window, &id);

    let mut extra = Vec::with_capacity(to_prepaint.len());
    let mut unplaced = Vec::new();
    for (row, top) in to_prepaint {
        let mut element = match rendered.remove(&row) {
            Some(element) => element,
            None => {
                let mut element = render_item(row, window, cx);
                let size = crate::fast::layout_key::layout_as_list_item(
                    &mut element,
                    row,
                    available,
                    window,
                    cx,
                );
                let (_, height) = tops[&row];
                let hidden = top + height <= viewport.top() || top >= viewport.bottom();
                if hidden && (size.height.0 - height.0).abs() * scale_factor >= 0.01 {
                    // The list's size for the row is out of date (it was
                    // measured at another scale, say), and the list corrects
                    // it when it shows the row: the layer does not hold a row
                    // where it would not be then.
                    unplaced.push(row);
                    continue;
                }
                element
            }
        };
        let origin = point(anchor_origin.x, top);
        let began = begin_row(window, cx, list, row, false);
        let clip = row_clip(window, ContentMask::rectangular(bounds));
        // In the list's scroll container, as the rows it shows are.
        let mut prepaint = |window: &mut Window| {
            window.with_content_mask(clip, |window| {
                element.prepaint_at(origin, window, cx);
            })
        };
        match scroll_context {
            Some(context) => window.with_scroll_container_context(context, prepaint),
            None => prepaint(window),
        }
        if began {
            end_row(window, cx, list);
        }
        extra.push((row, element));
    }
    if !extra.is_empty() {
        // Rows the list does not show ask for no autoscroll.
        window.take_autoscroll();
    }
    window.fast_layers.painting = None;
    finish_prepaint(window, cx, &id);
    if let Some(frame) = frame_mut(window, &id) {
        frame.extra = extra;
        for row in unplaced {
            frame.slots.remove(&row);
        }
    }
}

/// Whether each row of `rows` that a `list` keeps, those of `kept` in
/// `needed`, lands on the pixels it would if painted afresh when its content
/// is moved by `translation`: the row lies at `tops` (its top and height),
/// at the same fraction of a device pixel as it does moved, and neither its
/// top nor its bottom lies half way between device pixels.
///
/// A row's edges are rounded to device pixels as the row is painted, half
/// way toward zero. Rounding a position and then moving it by whole device
/// pixels lands where rounding the moved position does, unless the position
/// is half way: its rounding then depends on its sign, which the move can
/// flip, and on the last bits of how the list added up the rows' heights,
/// which differ from frame to frame. Rows whose height is not a whole number
/// of device pixels (30 px at a scale of 1.25) put edges there.
fn held_rows_land_alike(
    rows: &BTreeMap<usize, Row>,
    kept: &BTreeSet<usize>,
    needed: &Range<usize>,
    tops: &BTreeMap<usize, (Pixels, Pixels)>,
    translation: Point<ScaledPixels>,
    scale_factor: f32,
) -> bool {
    const EPSILON: f32 = 0.01;
    let half_way = |value: f32| ((value - value.floor()) - 0.5).abs() < EPSILON;
    rows.range(needed.clone())
        .filter(|(row, _)| kept.contains(row))
        .all(|(row, held)| {
            let Some((top, height)) = tops.get(row) else {
                return false;
            };
            let top = top.0 * scale_factor;
            let bottom = top + height.0 * scale_factor;
            let moved = held.slot.origin.y.0 + translation.y.0;
            (moved - top).abs() < EPSILON && !half_way(top) && !half_way(bottom)
        })
}

/// Notes which rows the `list` of `state`, laid out at `bounds` with
/// `padding_top` above its rows, showed on a frame that kept them off its
/// layer, if it has one: from its first row shown, each row that starts
/// above the height of its bounds, as it lays them out.
fn note_shown_without_layer(
    window: &mut Window,
    state: &crate::StateInner,
    bounds: Bounds<Pixels>,
    padding_top: Pixels,
) {
    if !COMPILED || window.fast_layers.layers.is_empty() {
        return;
    }
    let id = list_id(window, &state.fast_version);
    let frame = window.fast_layers.frame;
    let Some(layer) = window.fast_layers.layers.get_mut(&id) else {
        return;
    };
    let top = state.logical_scroll_top();
    let mut cursor = state.items.cursor::<crate::Count>(());
    cursor.seek(&crate::Count(top.item_ix), sum_tree::Bias::Right);
    let mut height = padding_top - top.offset_in_item;
    let mut end = top.item_ix;
    while let Some(item) = cursor.item()
        && height < bounds.size.height
    {
        height += item.size().map_or(bounds.size.height, |size| size.height);
        end += 1;
        cursor.next();
    }
    layer.rows.visible = Some((frame, top.item_ix..end));
}

/// Starts painting the rows of the `list` of `state`.
pub(crate) fn begin_paint_list(window: &mut Window, cx: &mut App, state: &crate::ListState) {
    if !LIST_LAYERS {
        return;
    }
    if !COMPILED || window.fast_layers.layers.is_empty() {
        return;
    }
    let version = state.0.borrow().fast_version.clone();
    let id = list_id(window, &version);
    begin_paint_rows(window, cx, Some(&id));
}

/// Ends painting the rows of the `list` of `state`: the rows rendered for
/// its layer alone are painted, and the layer is composited.
pub(crate) fn end_paint_list(window: &mut Window, cx: &mut App, state: &crate::ListState) {
    if !LIST_LAYERS {
        return;
    }
    if !COMPILED || window.fast_layers.layers.is_empty() {
        return;
    }
    let version = state.0.borrow().fast_version.clone();
    let id = list_id(window, &version);
    // Laying the list out moved its version if it changed what the list lays
    // out next from; a change from here on is one from outside.
    if let Some(layer) = window.fast_layers.layers.get_mut(&id) {
        layer.rows.state_version = Some(version.get());
    }
    finish_list_paint(window, cx, &id);
}

/// Paints the rows rendered for the layer of the list `id` alone, and ends
/// painting its rows.
fn finish_list_paint(window: &mut Window, cx: &mut App, id: &GlobalElementId) {
    let Some(frame) = frame_mut(window, id) else {
        return;
    };
    let extra = mem::take(&mut frame.extra);
    for (row, mut element) in extra {
        paint_row(window, cx, Some(row), |window, cx| {
            element.paint(window, cx)
        });
    }
    end_paint_rows(window, cx, Some(id));
}

/// Starts painting the rows of the list `id` as its prepaint decided, and
/// carries the paint records of the rows its layer keeps.
pub(crate) fn begin_paint_rows(window: &mut Window, cx: &mut App, id: Option<&GlobalElementId>) {
    if !LIST_LAYERS {
        return;
    }
    let Some(id) = id else {
        return;
    };
    if window.fast_layers.layers.is_empty() {
        return;
    }
    let Some(frame) = frame_mut(window, id) else {
        return;
    };
    if frame.paint.is_some() {
        return;
    }
    let region = rows_region(frame);
    let has_rows = !frame.slots.is_empty();
    let background = if has_rows {
        paint::bake_background(window)
    } else {
        None
    };
    let mut state = PaintState {
        background,
        swapped: background.is_some(),
        scene: Scene::default(),
        records_start: window.paint_index(),
        recording: None,
        reads_start: 0,
        current: None,
        spans: Vec::new(),
        painted_with: paint::PaintedWith::current(window),
        tab_stops_start: window.next_frame.tab_stops.paint_index(),
        in_frame: Vec::new(),
    };
    let mut onto = None;
    if state.swapped {
        // gpux keeps a second mask for the paint phase, in device pixels,
        // which follows the first (see `paint::repaint`).
        window
            .content_mask_stack
            .push(ContentMask::rectangular(region));
        window
            .paint_content_mask_stack
            .push(ContentMask::rectangular(
                region.scale(window.scale_factor()),
            ));
        onto = Some(paint::PaintingOnto {
            surface: crate::fast::composition::current_surface(window),
            in_top_layer: window.next_frame.scene.in_top_layer(),
        });
        mem::swap(&mut window.next_frame.scene, &mut state.scene);
        state.recording = Some(cx.begin_recording_dependencies());
        state.reads_start = reads_len(cx);
    }
    let frame = frame_mut(window, id).unwrap();
    let mut marker = marker(id, frame);
    marker.onto = onto;
    frame.paint = Some(state);
    window.fast_layers.painting = Some(marker);
    carry_paint_rows(window, id);
    check_hovers(window, id);
}

/// Asks for another frame if a row the layer of the list `id` keeps this
/// frame was painted by a hover that the frame's hit test, made after its
/// prepaint, finds changed. A row is kept only where no hitbox of the frame
/// can change what it read (see [`stale_rows`]), so none is; were one, it
/// would be rendered again on the next frame.
fn check_hovers(window: &mut Window, id: &GlobalElementId) {
    let Some(layer) = window.fast_layers.layers.get(id) else {
        return;
    };
    let Some(frame) = layer.rows.frame.as_ref() else {
        return;
    };
    // A row the list does not show can show a stale hover: it is checked
    // again before it shows.
    let rows = &layer.rows.rows;
    let changed = frame
        .carried
        .iter()
        .filter(|ix| frame.visible.contains(ix) && !frame.leaving.contains(ix))
        .any(|ix| {
            rows.get(ix).is_some_and(|row| {
                !crate::fast::dependencies::hovers_unchanged(window, &row.hovers)
            })
        });
    debug_assert!(!changed, "a row was kept under a hover that changed");
    if changed {
        window.request_animation_frame();
    }
}

/// Paints a row of a list with `f`: into the list's layer if its prepaint
/// rendered the row for it, not at all if the layer keeps the row, into the
/// frame otherwise, as the row a `list` draws off screen is, once the layer
/// is composited. `ix` is the row, or, for a uniform list, the next row it
/// rendered.
pub(crate) fn paint_row(
    window: &mut Window,
    cx: &mut App,
    ix: Option<usize>,
    f: impl FnOnce(&mut Window, &mut App),
) {
    if !LIST_LAYERS {
        return f(window, cx);
    }
    let Some(id) = window.fast_layers.painting.as_ref().map(|p| p.id.clone()) else {
        return f(window, cx);
    };
    let Some(layer) = window.fast_layers.layers.get_mut(&id) else {
        return f(window, cx);
    };
    let Some(frame) = layer.rows.frame.as_mut() else {
        return f(window, cx);
    };
    let Some(paint) = frame.paint.as_ref() else {
        return f(window, cx);
    };
    if paint.current.is_some() {
        // A row of a list nested in a row being painted.
        return f(window, cx);
    }
    let swapped = paint.swapped;
    let row = match ix {
        Some(row) => row,
        None => {
            let Some(row) = frame.order.get(frame.next).copied() else {
                return f(window, cx);
            };
            frame.next += 1;
            row
        }
    };
    if frame.loose.contains(&row) {
        // The row a `list` draws off screen is drawn after the rows it
        // shows, over them: the layer, which holds those, is finished and
        // composited first.
        finish_list_paint(window, cx, &id);
        return f(window, cx);
    }
    let row_shown = shows_row(frame, row);
    if !frame.slots.contains_key(&row) {
        if frame.carried.binary_search(&row).is_ok() {
            // The layer keeps the row as it is.
            return;
        }
        if !swapped {
            return paint_into_frame(window, cx, &id, row, f);
        }
        // Into the frame, around the layer's scene and painted region.
        let region = window.content_mask_stack.pop();
        swap_scenes(window, &id);
        paint_into_frame(window, cx, &id, row, f);
        swap_scenes(window, &id);
        window.content_mask_stack.extend(region);
        return;
    }
    if !swapped {
        // No layer holds the rows, which no background could be baked
        // under: the frame is painted the rows the list shows, as without
        // one.
        if row_shown {
            paint_into_frame(window, cx, &id, row, f);
        }
        return;
    }
    let start = window.next_frame.scene.paint_operations.len();
    let hovers_start = crate::fast::dependencies::begin_recording(window);
    let paint_start = window.paint_index();
    let reads_start = reads_len(cx);
    set_current_row(window, &id, Some(row));
    {
        f(window, cx);
    }
    set_current_row(window, &id, None);
    // A row the list does not show handles no text input: a list without a
    // layer does not draw it, and the platform would be handed the handler
    // of an input nobody sees. The row is not held, so that it asks again
    // once it shows.
    let handlers = paint_start.input_handlers_index;
    let unheld = !row_shown && window.next_frame.input_handlers.len() > handlers;
    if unheld {
        window.next_frame.input_handlers.truncate(handlers);
    }
    let paint_end = window.paint_index();
    let end = window.next_frame.scene.paint_operations.len();
    // The reads stay in the log for the view holding the list, whose hovers
    // they are too.
    let hovers = crate::fast::dependencies::finish_recording(window, hovers_start);
    let shown = Rc::new(Cell::new(row_shown));
    listen_while_shown(
        &mut window.next_frame,
        &(paint_start.clone()..paint_end.clone()),
        &shown,
    );
    if !row_shown {
        hide_records(
            &mut window.next_frame,
            &(paint_start.clone()..paint_end.clone()),
        );
    }
    if let Some(paint) = frame_mut(window, &id).and_then(|frame| frame.paint.as_mut()) {
        paint.spans.push(RowPaint {
            row,
            operations: start..end,
            paint: paint_start..paint_end,
            hovers,
            reads: reads_start..reads_len(cx),
            shown,
            unheld,
        });
    }
}

/// Whether a row prepainted into the layer of the list of `frame` can be
/// hit above or below the room the list gives it: a hitbox of its, clipped as
/// the row clips it, reaches over the rows around it.
///
/// A frame shown from a list's layer holds the hitboxes of the rows the
/// layer keeps before those of the rows it renders, not row after row as a
/// list without a layer inserts them, and a point under two hitboxes hits
/// the later one. While each row is hit inside its own room, which no other
/// row shares, the order tells nothing; a list whose rows reach over one
/// another keeps today's path.
fn rows_overflow(window: &Window, frame: &RowsFrame) -> bool {
    const EPSILON: Pixels = px(0.01);
    frame.row_prepaints.iter().any(|prepaint| {
        let Some(slot) = frame.slots.get(&prepaint.row) else {
            return false;
        };
        let range = prepaint.range.start.hitboxes_index..prepaint.range.end.hitboxes_index;
        let Some(hitboxes) = window.next_frame.hitboxes.get(range) else {
            return false;
        };
        hitboxes.iter().zip(&prepaint.masks).any(|(hitbox, mask)| {
            let hit = hitbox.bounds.intersect(&mask.bounds);
            !hit.is_empty()
                && (hit.top() < slot.top() - EPSILON || hit.bottom() > slot.bottom() + EPSILON)
        })
    })
}

/// Paints the row `row` of the list `id` into the frame with `f`, noting
/// which of the frame's records are its own (see [`order_tab_stops`]).
fn paint_into_frame(
    window: &mut Window,
    cx: &mut App,
    id: &GlobalElementId,
    row: usize,
    f: impl FnOnce(&mut Window, &mut App),
) {
    let start = window.paint_index();
    f(window, cx);
    let end = window.paint_index();
    if let Some(paint) = frame_mut(window, id).and_then(|frame| frame.paint.as_mut()) {
        paint.in_frame.push((row, start..end));
    }
}

/// Puts the tab stops of the rows of the list `id` in the order of its rows.
///
/// The tab key walks the stops of one group in the order they were painted
/// in. A list without a layer paints the rows it shows from the first to the
/// last; one with a layer carries the rows it keeps and then paints those it
/// renders, whichever they are, which would leave the keyboard walking the
/// rows out of order. The rows' stops lie one after another in the frame's
/// (nothing else is painted between a list's rows), so they are laid out
/// again by row, each row's as it painted them, and those of the rows the
/// list does not show left out of the order (see [`hide_records`]). The row
/// a `list` draws off screen comes after the others, as the list draws it.
fn order_tab_stops(
    window: &mut Window,
    id: &GlobalElementId,
    frame: &RowsFrame,
    paint: &mut PaintState,
) {
    use crate::tab_stop::TabStopOperation;
    let tab_stops = &mut window.next_frame.tab_stops;
    let region = paint.tab_stops_start..tab_stops.paint_index();
    if region.is_empty() {
        return;
    }
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    // Each row's stops, where they lie now, and whether the list shows it.
    let mut rows: BTreeMap<(bool, usize), (Range<usize>, bool)> = BTreeMap::new();
    let stops =
        |paint: &Range<PaintIndex>| paint.start.tab_handle_index..paint.end.tab_handle_index;
    let key = |row: usize| (frame.loose.contains(&row), row);
    for ix in &frame.carried {
        if let Some(row) = layer.rows.rows.get(ix) {
            rows.insert(key(*ix), (stops(&row.paint), frame.visible.contains(ix)));
        }
    }
    for (row, range) in &paint.in_frame {
        rows.insert(key(*row), (stops(range), true));
    }
    for span in &paint.spans {
        rows.insert(key(span.row), (stops(&span.paint), span.shown.get()));
    }
    let covered: usize = rows.values().map(|(range, _)| range.len()).sum();
    let inside = rows
        .values()
        .all(|(range, _)| range.start >= region.start && range.end <= region.end);
    if covered != region.len() || !inside {
        debug_assert!(false, "a list's rows do not account for its tab stops");
        return;
    }
    // Out of the order, and off the end of the history.
    let operations: Vec<TabStopOperation> =
        tab_stops.insertion_history.drain(region.clone()).collect();
    for (offset, operation) in operations.iter().enumerate() {
        let TabStopOperation::Insert(handle) = operation else {
            continue;
        };
        if tab_stops
            .by_id
            .get(&handle.id)
            .is_some_and(|node| node.node_insertion_index == region.start + offset)
            && let Some(node) = tab_stops.by_id.remove(&handle.id)
        {
            tab_stops.order.remove(&node, ());
        }
    }
    let mut moved: BTreeMap<usize, Range<usize>> = BTreeMap::new();
    for ((_, row), (range, shown)) in &rows {
        let start = tab_stops.paint_index();
        let own = &operations[range.start - region.start..range.end - region.start];
        if *shown {
            tab_stops.replay(own);
        } else {
            tab_stops.insertion_history.extend_from_slice(own);
        }
        moved.insert(*row, start..tab_stops.paint_index());
    }
    let place = |paint: &mut Range<PaintIndex>, stops: &Range<usize>| {
        paint.start.tab_handle_index = stops.start;
        paint.end.tab_handle_index = stops.end;
    };
    for ix in &frame.carried {
        if let (Some(row), Some(stops)) = (layer.rows.rows.get_mut(ix), moved.get(ix)) {
            place(&mut row.paint, stops);
        }
    }
    for span in &mut paint.spans {
        if let Some(stops) = moved.get(&span.row) {
            place(&mut span.paint, stops);
        }
    }
    for (row, range) in &mut paint.in_frame {
        if let Some(stops) = moved.get(row) {
            place(range, stops);
        }
    }
}

/// The row of a list's layer an element was drawn in, if it was drawn in one,
/// and whether its list shows the row. What a frame tells of its elements
/// ([`shown_nodes`]) leaves out those of the rows a list does not show, as
/// a list without a layer does not draw them, and tells of the others row
/// by row, as such a list draws them.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct NodeRow {
    /// The layer's key and the row.
    row: Option<(u32, usize)>,
    hidden: bool,
    /// The row a `list` draws off screen, after the rows it shows.
    loose: bool,
}

/// The row of a list's layer the element being prepainted lies in.
pub(crate) fn node_row(window: &Window) -> NodeRow {
    if !LIST_LAYERS {
        return NodeRow::default();
    }
    window
        .fast_layers
        .painting
        .as_ref()
        .and_then(|painting| window.fast_layers.layers.get(&painting.id))
        .and_then(|layer| {
            let frame = layer.rows.frame.as_ref()?;
            let (row, ..) = frame.open_row.as_ref()?;
            let row = *row;
            Some(NodeRow {
                row: Some((layer.key.0, row)),
                hidden: frame.open_row_hidden,
                loose: frame.loose.contains(&row),
            })
        })
        .unwrap_or_default()
}

/// How many of the hitboxes the frame being drawn holds over `hitboxes` are
/// those of rows a list's layer holds and its list does not show: an element
/// around a list tells how many hitboxes lie inside it, and a list without a
/// layer has none for the rows off screen.
pub(crate) fn hidden_hitboxes(window: &Window, hitboxes: &Range<usize>) -> usize {
    // Only the elements around a list hold hitboxes of its rows.
    let Some(hidden) = window.fast_layers.hidden_hitboxes.as_ref() else {
        return 0;
    };
    if hitboxes.start >= hidden.end || hitboxes.end <= hidden.start {
        return 0;
    }
    let inside = |prepaint: &Range<PrepaintStateIndex>| {
        let start = prepaint.start.hitboxes_index.max(hitboxes.start);
        let end = prepaint.end.hitboxes_index.min(hitboxes.end);
        end.saturating_sub(start)
    };
    let number = window.fast_layers.frame;
    window
        .fast_layers
        .layers
        .values()
        .map(|layer| {
            let Some(frame) = layer.rows.frame.as_ref() else {
                // Drawn from the last frame, with the view holding the list.
                if layer.rows.told_frame != Some(number) {
                    return 0;
                }
                return layer
                    .rows
                    .told
                    .iter()
                    .filter(|row| !row.shown)
                    .map(|row| inside(&row.prepaint))
                    .sum();
            };
            let carried: usize = frame
                .carried
                .iter()
                .filter(|row| !frame.visible.contains(row))
                .filter_map(|row| layer.rows.rows.get(row))
                .map(|row| inside(&row.prepaint))
                .sum();
            let prepainted: usize = frame
                .row_prepaints
                .iter()
                .filter(|prepainted| !shows_row(frame, prepainted.row))
                .map(|prepainted| inside(&prepainted.range))
                .sum();
            carried + prepainted
        })
        .sum()
}

/// The elements of `nodes` a frame tells of, in order: all but those drawn
/// in rows a list's layer holds and the list does not show, and those of a
/// list's rows by row, in whatever order the frame carried and drew them,
/// the row it draws off screen last.
pub(crate) fn shown_nodes(
    nodes: &[crate::window::DevtoolsFrameNode],
) -> impl Iterator<Item = &crate::window::DevtoolsFrameNode> {
    let mut shown = Vec::with_capacity(nodes.len());
    let mut rest = nodes;
    while let Some(first) = rest.first() {
        let Some((list, _)) = first.fast_row.row else {
            shown.push(first);
            rest = &rest[1..];
            continue;
        };
        // The rows of one list lie one after another.
        let run = rest
            .iter()
            .take_while(|node| node.fast_row.row.is_some_and(|(of, _)| of == list))
            .count();
        let mut rows: Vec<_> = rest[..run]
            .iter()
            .filter(|node| !node.fast_row.hidden)
            .collect();
        rows.sort_by_key(|node| {
            let row = node.fast_row.row.map(|(_, row)| row);
            (node.fast_row.loose, row)
        });
        shown.extend(rows);
        rest = &rest[run..];
    }
    shown.into_iter()
}

/// A row of a list's layer with records in a frame: where they lie in it,
/// and whether the list shows the row.
///
/// A frame shown from a list's layer holds the records of rows its list
/// does not show (kept for the scrolls to come, and made to answer nothing:
/// see [`hide_records`], [`listen_while_shown`], [`NodeRow`]), and holds
/// those of the rows it does show in the order it carried and drew them. A
/// frame drawn without the layer holds the records of the rows shown, row
/// after row. What a frame is described by (`fast::verify::describe_frame`) tells
/// the first as the second.
#[derive(Clone)]
pub(crate) struct ToldRow {
    #[cfg_attr(
        not(any(test, feature = "test-support", feature = "diagnostics")),
        allow(dead_code, reason = "read where frames are described")
    )]
    pub(crate) row: usize,
    pub(crate) shown: bool,
    /// The row a `list` draws off screen, after the rows it shows.
    #[cfg_attr(
        not(any(test, feature = "test-support", feature = "diagnostics")),
        allow(dead_code, reason = "read where frames are described")
    )]
    pub(crate) loose: bool,
    pub(crate) prepaint: Range<PrepaintStateIndex>,
    /// What laying the row out added to the frame before it was prepainted,
    /// if the frame laid it out: the element states it reached.
    pub(crate) layout: Option<Range<PrepaintStateIndex>>,
    /// `None` for a row that was not painted: with no background to bake
    /// under its rows, a list paints only those it shows.
    pub(crate) paint: Option<Range<PaintIndex>>,
    /// What the row's mouse listeners listen by (see
    /// [`listen_while_shown`]): `None` for a row painted into the frame,
    /// whose listeners are as it added them.
    #[cfg_attr(
        not(any(test, feature = "test-support", feature = "diagnostics")),
        allow(dead_code, reason = "read where frames are described")
    )]
    pub(crate) listens: Option<Rc<Cell<bool>>>,
}

/// The rows of each list's layer with records in the frame `window` last
/// drew, by layer.
#[cfg(any(test, feature = "test-support", feature = "diagnostics"))]
pub(crate) fn told_rows(window: &Window) -> Vec<&[ToldRow]> {
    let frame = window.fast_layers.frame;
    let mut lists: Vec<_> = window
        .fast_layers
        .layers
        .values()
        .filter(|layer| layer.rows.told_frame.is_some_and(|told| told + 1 == frame))
        .filter(|layer| !layer.rows.told.is_empty())
        .collect();
    lists.sort_by_key(|layer| layer.key.0);
    lists
        .into_iter()
        .map(|layer| layer.rows.told.as_slice())
        .collect()
}

/// Notes the rows of the list `id` with records in the frame being drawn,
/// once its rows are painted: those it carried, and those it prepainted.
fn note_told(window: &mut Window, id: &GlobalElementId, frame: &RowsFrame, paint: &PaintState) {
    let number = window.fast_layers.frame;
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    let rows = &mut layer.rows;
    let mut told: BTreeMap<usize, ToldRow> = BTreeMap::new();
    for ix in &frame.carried {
        if let Some(row) = rows.rows.get(ix) {
            told.insert(
                *ix,
                ToldRow {
                    row: *ix,
                    shown: frame.visible.contains(ix),
                    loose: false,
                    prepaint: row.prepaint.clone(),
                    layout: None,
                    paint: Some(row.paint.clone()),
                    listens: Some(row.shown.clone()),
                },
            );
        }
    }
    for prepainted in &frame.row_prepaints {
        let span = paint.spans.iter().find(|span| span.row == prepainted.row);
        let painted = span.map(|span| &span.paint).or_else(|| {
            let in_frame = paint
                .in_frame
                .iter()
                .find(|(row, _)| *row == prepainted.row);
            in_frame.map(|(_, range)| range)
        });
        let layout = frame
            .row_layouts
            .iter()
            .rev()
            .find(|layout| layout.row == prepainted.row);
        told.insert(
            prepainted.row,
            ToldRow {
                row: prepainted.row,
                shown: shows_row(frame, prepainted.row),
                loose: frame.loose.contains(&prepainted.row),
                prepaint: prepainted.range.clone(),
                layout: layout.map(|layout| layout.records.clone()),
                paint: painted.cloned(),
                listens: span.map(|span| span.shown.clone()),
            },
        );
    }
    rows.told = told.into_values().collect();
    rows.told_frame = Some(number);
}

/// Whether the list of `frame` shows its row `row`, which it paints: it lies
/// across the viewport, or is one a `list` places itself wherever it lies
/// (the row holding the focus, drawn off screen for the keyboard to reach).
fn shows_row(frame: &RowsFrame, row: usize) -> bool {
    frame.visible.contains(&row) || (frame.list.is_some() && frame.prepainted.contains(&row))
}

/// Has the mouse listeners painting a row added to `frame` over `paint`
/// listen only while `shown`: the frames the list shows the row in. A row
/// the layer holds off screen is not drawn by a list without one, whose
/// frame has none of its listeners: one that fires wherever the mouse goes
/// down outside its element would fire for every click otherwise.
fn listen_while_shown(
    frame: &mut crate::window::WindowFrame,
    paint: &Range<PaintIndex>,
    shown: &Rc<Cell<bool>>,
) {
    let listeners = paint.start.mouse_listeners_index..paint.end.mouse_listeners_index;
    for listener in &mut frame.mouse_listeners[listeners] {
        if let Some(mut inner) = listener.take() {
            let shown = shown.clone();
            *listener = Some(Box::new(move |event, phase, window, cx| {
                if shown.get() {
                    inner(event, phase, window, cx)
                }
            }));
        }
    }
}

/// Notes that the frame being drawn holds, over `prepaint`, the hitboxes of
/// a row its list does not show (see [`hidden_hitboxes`]).
fn note_hidden_hitboxes(hidden: &mut Option<Range<usize>>, prepaint: &Range<PrepaintStateIndex>) {
    let hitboxes = prepaint.start.hitboxes_index..prepaint.end.hitboxes_index;
    if hitboxes.is_empty() {
        return;
    }
    *hidden = Some(match hidden.take() {
        Some(hidden) => hidden.start.min(hitboxes.start)..hidden.end.max(hitboxes.end),
        None => hitboxes,
    });
}

/// Takes the focus out of what prepainting a row the list does not show
/// added to `frame` over `prepaint`. A list without a layer does not draw
/// the row: an element in it that held the focus is no part of the frame,
/// which then holds no focus, and the keyboard reaches nothing in it.
pub(crate) fn hide_focus(
    frame: &mut crate::window::WindowFrame,
    prepaint: &Range<PrepaintStateIndex>,
) {
    let tree = &mut frame.dispatch_tree;
    let nodes = prepaint.start.dispatch_tree_index..prepaint.end.dispatch_tree_index;
    for index in nodes {
        let Some(focus_id) = tree.nodes.get(index).and_then(|node| node.focus_id) else {
            continue;
        };
        if tree
            .focusable_node_ids
            .get(&focus_id)
            .is_some_and(|node| node.as_usize() == index)
        {
            tree.focusable_node_ids.remove(&focus_id);
        }
        if frame.focus == Some(focus_id) {
            frame.focus = None;
        }
    }
}

/// Takes what painting a row the list does not show added to `frame` over
/// `paint` out of what the frame answers by, leaving the records where they
/// lie for the next frame to carry: its tab stops are no stops (the
/// keyboard does not walk into rows off screen), and its elements have no
/// bounds to be found by.
pub(crate) fn hide_records(frame: &mut crate::window::WindowFrame, paint: &Range<PaintIndex>) {
    let tab_stops = &mut frame.tab_stops;
    for index in paint.start.tab_handle_index..paint.end.tab_handle_index {
        let Some(crate::tab_stop::TabStopOperation::Insert(handle)) =
            tab_stops.insertion_history.get(index)
        else {
            continue;
        };
        // Its latest place in the order, unless an element drawn since
        // tracks the same focus.
        let id = handle.id;
        if tab_stops
            .by_id
            .get(&id)
            .is_some_and(|node| node.node_insertion_index == index)
            && let Some(node) = tab_stops.by_id.remove(&id)
        {
            tab_stops.order.remove(&node, ());
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    for index in paint.start.debug_bounds_index..paint.end.debug_bounds_index {
        let Some((selector, bounds)) = frame.debug_bounds_records.get(index) else {
            continue;
        };
        if frame.debug_bounds.get(selector) == Some(bounds) {
            frame.debug_bounds.remove(selector);
        }
    }
}

fn swap_scenes(window: &mut Window, id: &GlobalElementId) {
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    if let Some(paint) = layer
        .rows
        .frame
        .as_mut()
        .and_then(|frame| frame.paint.as_mut())
    {
        mem::swap(&mut window.next_frame.scene, &mut paint.scene);
    }
}

fn set_current_row(window: &mut Window, id: &GlobalElementId, row: Option<usize>) {
    if let Some(paint) = frame_mut(window, id).and_then(|frame| frame.paint.as_mut()) {
        paint.current = row;
    }
}

/// Ends painting the rows of the list `id`: the rows painted into its layer
/// are recorded, those it drops forgotten, and the layer is composited.
pub(crate) fn end_paint_rows(window: &mut Window, cx: &mut App, id: Option<&GlobalElementId>) {
    if !LIST_LAYERS {
        return;
    }
    let Some(id) = id else {
        return;
    };
    if window.fast_layers.layers.is_empty() {
        return;
    }
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    let Some(mut frame) = layer.rows.frame.take() else {
        return;
    };
    let Some(mut paint) = frame.paint.take() else {
        return;
    };
    order_tab_stops(window, id, &frame, &mut paint);
    note_told(window, id, &frame, &paint);
    let surface_switches = window
        .fast_layers
        .painting
        .take()
        .map(|painting| painting.surface_switches)
        .unwrap_or_default();
    let records = paint.records_start.clone()..window.paint_index();
    if frame.slots.is_empty() {
        match frame.mode {
            Mode::Extend => {
                let layer = window.fast_layers.layers.get_mut(id).unwrap();
                if layer.rows.rows.len() == frame.carried.len() {
                    // Nothing new: the layer's content stands.
                    crate::fast::fast_stat!(
                        window.fast_stats.frame,
                        list_rows_kept += frame.carried.len() as u64
                    );
                    count_extended_frame(0);
                    finish_records(window, id, &frame, records);
                    paint::composite_at(window, id, frame.translation);
                    paint::repaint_if_painted_otherwise(window, id);
                    return;
                }
                // Rows are dropped: the content changes.
                paint.background = paint::bake_background(window);
            }
            // No row to paint: the layer holds nothing.
            Mode::Repaint => {
                if let Some(layer) = window.fast_layers.layers.get_mut(id) {
                    layer.record = None;
                }
                clear_rows(window, id, &frame);
                policy::count_bypass(window, policy::Bypass::Ineligible);
                return;
            }
        }
    }
    let Some(background) = paint.background else {
        // The rows were painted into the frame: so is what the layer held.
        let layer = window.fast_layers.layers.get_mut(id).unwrap();
        if let Some(record) = layer.record.take()
            && frame.mode == Mode::Extend
        {
            // Those the list shows: it draws no other without a layer.
            let kept: Vec<Rc<Scene>> = frame
                .carried
                .iter()
                .filter(|ix| frame.visible.contains(ix))
                .filter_map(|ix| layer.rows.rows.get(ix))
                .map(|row| row.part.scene.clone())
                .collect();
            drop(record);
            for scene in kept {
                paint::draw_into_frame(window, &scene.paint_operations, frame.translation);
            }
        }
        clear_rows(window, id, &frame);
        policy::defer_unbaked(window, id);
        policy::count_bypass(window, policy::Bypass::Ineligible);
        return;
    };

    // What painting each row read is the row's, as what prepainting it read
    // is (see [`finish_prepaint`]).
    let mut row_paint_reads: FxHashMap<usize, Rc<[EntityId]>> = FxHashMap::default();
    let paint_dependencies = match paint.recording.take() {
        Some(recording) => {
            let spans: Vec<Range<usize>> =
                paint.spans.iter().map(|span| span.reads.clone()).collect();
            let rest = outside_spans(paint.reads_start..reads_len(cx), spans);
            for span in &paint.spans {
                let entities = cx.entities_read_in([span.reads.clone()]);
                row_paint_reads.insert(span.row, entities);
            }
            let rest = cx.entities_read_in(rest);
            let all = cx.finish_recording_dependencies(recording).all;
            Some(all.with_entities(rest))
        }
        // Nothing was painted into the layer: nothing was read for it.
        None => None,
    };
    if paint.swapped {
        mem::swap(&mut window.next_frame.scene, &mut paint.scene);
        window.paint_content_mask_stack.pop();
        window.content_mask_stack.pop();
    }
    let rendered_rows = paint.spans.len();
    let rendered_operations = paint.scene.paint_operations.len();
    let mut painted = paint.scene;
    paint::take_scene_requests(&mut painted, &mut window.next_frame.scene);

    // What no tile can hold (see `paint::uncompositable`), or what the rows
    // painted onto another composition surface, is drawn into the frame as
    // it was painted, with the rows the layer kept, and the list goes back
    // to today's path for a while. So it does when a row can be hit outside
    // the room the list gives it (see `rows_overflow`). Only the rows the
    // list shows are drawn, as it draws them without a layer: a row off
    // screen that draws outside its own room would show.
    if !surface_switches.is_empty()
        || paint::uncompositable(&painted)
        || rows_overflow(window, &frame)
    {
        let layer = window.fast_layers.layers.get_mut(id).unwrap();
        let kept: Vec<Rc<Scene>> = match (layer.record.take(), frame.mode) {
            (Some(_), Mode::Extend) => frame
                .carried
                .iter()
                .filter(|ix| frame.visible.contains(ix))
                .filter_map(|ix| layer.rows.rows.get(ix))
                .map(|row| row.part.scene.clone())
                .collect(),
            _ => Vec::new(),
        };
        for scene in kept {
            paint::draw_into_frame(window, &scene.paint_operations, frame.translation);
        }
        let operations = &painted.paint_operations;
        let mut hidden: Vec<Range<usize>> = paint
            .spans
            .iter()
            .filter(|span| !span.shown.get())
            .map(|span| span.operations.clone())
            .collect();
        hidden.sort_by_key(|range| range.start);
        let mut draw = |window: &mut Window, range: Range<usize>| {
            for shown in outside_spans(range, hidden.clone()) {
                paint::draw_into_frame(window, &operations[shown], Point::default());
            }
        };
        let mut drawn = 0;
        for (at, surface) in &surface_switches {
            draw(window, drawn..*at);
            crate::fast::composition::switch_surface(window, *surface);
            drawn = *at;
        }
        draw(window, drawn..operations.len());
        clear_rows(window, id, &frame);
        policy::defer_unbaked(window, id);
        policy::count_bypass(window, policy::Bypass::Ineligible);
        return;
    }

    let scale_factor = window.scale_factor();
    let translation = frame.translation;
    let to_content = point(
        ScaledPixels(-translation.x.0),
        ScaledPixels(-translation.y.0),
    );
    let views = invalidate::content_views(window, &frame.prepaint_range);
    let viewport = window.snapped_content_mask().bounds;
    let mut row_prepaints = mem::take(&mut frame.row_prepaints);
    let row_views: FxHashMap<usize, Rc<[EntityId]>> = row_prepaints
        .iter()
        .map(|prepainted| {
            let views = invalidate::content_views(window, &prepainted.range);
            (prepainted.row, views)
        })
        .collect();

    let layer = window.fast_layers.layers.get_mut(id).unwrap();
    let rows = &mut layer.rows;
    let for_hover = paint
        .spans
        .iter()
        .filter(|span| frame.stale.contains(&span.row))
        .count();
    // The rows held before a repaint, to tell whether it changed them.
    let mut repainted_over = BTreeMap::new();
    match frame.mode {
        Mode::Repaint => {
            repainted_over = mem::take(&mut rows.rows);
            rows.clear();
        }
        Mode::Extend => {
            rows.added_since_repaint += paint
                .spans
                .iter()
                .filter(|span| !rows.rows.contains_key(&span.row))
                .count();
            let carried: BTreeSet<usize> = frame.carried.iter().copied().collect();
            rows.rows.retain(|row, _| carried.contains(row));
            if rows.rerendered {
                for row in rows.rows.values_mut() {
                    row.suspect = true;
                }
            }
            // Those rendered again this frame are held anew below.
            for row in &frame.changed {
                if let Some(row) = rows.rows.get_mut(row) {
                    row.suspect = true;
                }
            }
            count_extended_frame(for_hover);
        }
    }
    let mut operations = painted.paint_operations;
    for span in paint.spans {
        let Some(slot) = frame.slots.get(&span.row) else {
            continue;
        };
        let slot = slot.scale(scale_factor);
        let slot = Bounds {
            origin: slot.origin + to_content,
            size: slot.size,
        };
        // Each row's operations are taken, in content space, once.
        let mut row_operations: Vec<PaintOperation> = operations[span.operations]
            .iter_mut()
            .map(|operation| mem::replace(operation, PaintOperation::EndLayer))
            .collect();
        for operation in &mut row_operations {
            match operation {
                PaintOperation::Primitive(primitive) => move_primitive(primitive, to_content),
                PaintOperation::StartLayer(bounds)
                | PaintOperation::StartPositionedLayer(bounds)
                | PaintOperation::StartTopLayer(bounds) => {
                    bounds.origin = bounds.origin + to_content
                }
                PaintOperation::FastDropped(dropped) => *dropped = dropped.moved(to_content),
                PaintOperation::FastClip(clip) => *clip = clip.moved(to_content),
                PaintOperation::EndLayer => {}
            }
        }
        let (tile_hashes, reach) = part_tile_hashes(&row_operations, paint::TILE_SIZE);
        // A row drawing above or below its own room is held only while the
        // list shows it (see `RowSort::spilling`). A row's edges are drawn
        // on the device pixels around them.
        let spills = reach.is_some_and(|reach| {
            reach.top().0 < slot.top().0.floor() || reach.bottom().0 > slot.bottom().0.ceil()
        });
        if spills || span.unheld {
            rows.spilling.insert(span.row, slot.size.height);
            if !span.shown.get() {
                continue;
            }
        } else {
            rows.spilling.remove(&span.row);
        }
        let has_paths = row_operations
            .iter()
            .any(|operation| matches!(operation, PaintOperation::Primitive(Primitive::Path(_))));
        let mut scene = Scene::default();
        scene.paint_operations = row_operations;
        let (prepaint, hitboxes, layout_keys, read) =
            match row_prepaints.iter_mut().rev().find(|p| p.row == span.row) {
                Some(prepainted) => {
                    let range = &prepainted.range;
                    let inserted = &window.next_frame.hitboxes
                        [range.start.hitboxes_index..range.end.hitboxes_index];
                    let mut hitboxes = inserted.to_vec();
                    if prepainted.masks.len() == hitboxes.len() {
                        for (hitbox, mask) in hitboxes.iter_mut().zip(&prepainted.masks) {
                            hitbox.content_mask = *mask;
                        }
                    } else {
                        debug_assert!(false, "a hitbox inserted by a row went unnoted");
                    }
                    (
                        range.clone(),
                        hitboxes,
                        mem::take(&mut prepainted.layout_keys),
                        prepainted.entities.clone(),
                    )
                }
                None => {
                    debug_assert!(
                        false,
                        "a row painted into a layer was not prepainted into it"
                    );
                    let end = frame.prepaint_range.end.clone();
                    (end.clone()..end, Vec::new(), Vec::new(), Rc::from([]))
                }
            };
        let read = match row_paint_reads.get(&span.row) {
            Some(painted) => crate::fast::dependencies::merge_sorted(&read, painted),
            None => read,
        };
        let dependencies = frame.dependencies.entities_only(read);
        let views = row_views
            .get(&span.row)
            .cloned()
            .unwrap_or_else(|| Rc::from([]));
        let has_states = prepaint.start.accessed_element_states_index
            != prepaint.end.accessed_element_states_index
            || span.paint.start.accessed_element_states_index
                != span.paint.end.accessed_element_states_index;
        rows.rows.insert(
            span.row,
            Row {
                slot,
                part: LayerPart {
                    bounds: Some(reach.unwrap_or_default()),
                    scene: Rc::new(scene),
                },
                tile_hashes,
                has_paths,
                translation,
                foreign_hovers: span
                    .hovers
                    .iter()
                    .any(|(id, _)| !hitboxes.iter().any(|hitbox| hitbox.id == *id)),
                hitboxes,
                hovers: span.hovers,
                has_states,
                layout_keys,
                dependencies,
                views,
                prepaint,
                paint: span.paint,
                suspect: false,
                dropped: false,
                shown: span.shown,
            },
        );
    }
    rows.list = true;
    rows.repaints_shown_rows = frame.list.is_some();
    // A repaint for a change of the content that left every row it painted
    // again as the layer held it — the view holding the list notified for
    // something else, say — changed nothing.
    let unchanged = frame.mode == Mode::Repaint
        && !repainted_over.is_empty()
        && frame
            .slots
            .keys()
            .all(|ix| match (rows.rows.get(ix), repainted_over.get(ix)) {
                (Some(row), Some(before)) => {
                    row.slot.size == before.slot.size
                        && row.has_paths == before.has_paths
                        && drawn_alike(row, before)
                }
                _ => false,
            });
    drop(repainted_over);
    let visible_operations: usize = rows
        .rows
        .range(frame.visible.clone())
        .map(|(_, row)| row.part.scene.paint_operations.len())
        .sum();
    let work = (rendered_rows as f32 / frame.visible.len().max(1) as f32)
        .max(rendered_operations as f32 / visible_operations.max(1) as f32);

    // The content: the rows in order.
    let content = LayerContent::from_parts(rows.rows.values().map(|row| row.part.clone()));
    let mut region = Bounds {
        origin: viewport.origin + to_content,
        size: viewport.size,
    };
    for row in rows.rows.values() {
        region = region.union(&row.slot);
    }
    // A row's hash is of what it draws, taken once when it is painted;
    // where the content stacks it among the other rows is hashed with it.
    let stacked: Vec<Vec<_>> = rows
        .rows
        .values()
        .zip(content.part_order_hashes())
        .map(|(row, orders)| {
            row.tile_hashes
                .iter()
                .map(|(tile, hash)| (*tile, hash.rotate_left(17) ^ orders))
                .collect()
        })
        .collect();
    let hashes = combine_tile_hashes(stacked.iter().map(Vec::as_slice), paint::TILE_SIZE, region);
    let has_paths = rows.rows.values().any(|row| row.has_paths);

    let old = layer.record.take();
    let generation = layer.next_generation();
    let dirty = match &old {
        Some(old) if old.background == background => dirty_tiles(&old.tile_hashes, &hashes),
        _ => paint::all_tiles(&hashes),
    };
    let dependencies = match (&old, frame.mode) {
        // Checked when the frame chose to extend the layer: the stretches
        // of writes the record took for its own lie behind it, and are not
        // gathered frame after frame.
        (Some(old), Mode::Extend) => old
            .dependencies
            .written_up_to(frame.vetted_at)
            .union(&frame.dependencies),
        _ => frame.dependencies.clone(),
    };
    let dependencies = match &paint_dependencies {
        Some(painted) => dependencies.union(painted),
        None => dependencies,
    };
    let rows = &layer.rows;
    let painted_region = rows.rows.values().fold(frame.viewport, |region, row| {
        let slot = Bounds {
            origin: row.slot.origin - to_content,
            size: row.slot.size,
        };
        region.union(&Bounds {
            origin: point(
                px(slot.origin.x.0 / scale_factor),
                px(slot.origin.y.0 / scale_factor),
            ),
            size: crate::size(
                px(slot.size.width.0 / scale_factor),
                px(slot.size.height.0 / scale_factor),
            ),
        })
    });
    let dirtied = dirty.len();
    layer.record = Some(LayerRecord {
        content,
        generation,
        painted_region,
        viewport: frame.viewport,
        scroll_offset: frame.scroll_offset,
        translation,
        prepaint_range: frame.prepaint_range.clone(),
        paint_range: records.clone(),
        tile_hashes: hashes,
        dirty_tiles: dirty,
        background,
        hovers: Rc::from([]),
        dependencies,
        views,
        has_paths,
        paths: Rc::from([]),
        view_layouts: Rc::default(),
        content_bounds: painted_region,
        uncompositable: false,
        painted_with: Some(paint.painted_with),
    });
    finish_records(window, id, &frame, records);
    if unchanged {
        policy::note_unchanged_repaint(window, id);
    }
    policy::note_work(window, id, work);
    crate::fast::fast_stat!(
        window.fast_stats.frame,
        list_rows_rendered += rendered_rows as u64
    );
    crate::fast::fast_stat!(
        window.fast_stats.frame,
        list_rows_kept += frame.carried.len() as u64
    );
    // A frame is counted once: shown from its layer's tiles, the rows
    // painted afresh or kept, or drawn into the frame as without one.
    let inserted = paint::insert_layer(window, id, translation, dirtied);
    if frame.mode == Mode::Extend {
        paint::repaint_if_painted_otherwise(window, id);
    }
    if inserted {
        match frame.mode {
            Mode::Repaint => {
                crate::fast::fast_stat!(window.fast_stats.frame, layers_repainted += 1)
            }
            Mode::Extend => {
                crate::fast::fast_stat!(window.fast_stats.frame, layers_composited += 1)
            }
        }
    } else {
        policy::count_bypass(window, policy::Bypass::Ineligible);
    }
}

/// Notes, for the layer of the list `id`, where the records of its rows lie
/// in the frame being drawn, whose `frame` drew them, and how far the
/// positions they hold lag behind.
fn finish_records(
    window: &mut Window,
    id: &GlobalElementId,
    frame: &RowsFrame,
    records: Range<PaintIndex>,
) {
    let frame_number = window.fast_layers.frame;
    let scale_factor = window.scale_factor();
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    let translation = frame.translation;
    if let Some(record) = layer.record.as_mut() {
        record.prepaint_range = frame.prepaint_range.clone();
        record.paint_range = records;
    }
    let stale = layer
        .rows
        .rows
        .values()
        .find(|row| row.translation != translation)
        .map_or(Point::default(), |row| row.delta(translation, scale_factor));
    let input = &mut layer.input;
    input.ranges_frame = Some(frame_number);
    input.viewport = frame.viewport;
    input.stale = stale;
    let rows = &mut layer.rows;
    rows.last_translation = Some(translation);
    rows.visible = Some((frame_number, frame.visible.clone()));
    // What is remembered of the rows off screen reaches as far as the
    // layer does.
    rows.spilling.retain(|row, _| frame.needed.contains(row));
    for (ix, row) in &rows.rows {
        row.shown.set(shows_row(frame, *ix));
    }
    for ix in &frame.leaving {
        if let Some(row) = rows.rows.get_mut(ix) {
            row.dropped = true;
        }
    }
}

/// Forgets the rows of the layer of the list `id`, whose content is gone,
/// after `frame` showed them without the layer.
fn clear_rows(window: &mut Window, id: &GlobalElementId, frame: &RowsFrame) {
    let frame_number = window.fast_layers.frame;
    if let Some(layer) = window.fast_layers.layers.get_mut(id) {
        layer.rows.clear();
        layer.rows.visible = Some((frame_number, frame.visible.clone()));
    }
}
