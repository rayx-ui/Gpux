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
//! Measured leaves are given their new closure each frame, and their
//! closures are still taken when the frame is cleared: nothing a frame
//! captured outlives it. A leaf other than text is dirtied each frame, as in
//! the pool. A text leaf is left clean when its measurement stands: carried
//! from last frame's element at its key when that measured text shaped the
//! same way (see [`crate::fast::text`]), or replayed, when its new closure
//! gives every size Taffy took from the node since it was last dirtied
//! ([`MeasureLog`]). Text measured at another width than the one it is laid
//! out at is measured again there once the layout is computed
//! ([`settle_text_leaves`]).

use std::{
    cell::{Cell, RefCell},
    hash::{Hash as _, Hasher as _},
    mem,
    rc::Rc,
};

use collections::{FxHashMap, FxHashSet, FxHasher};
use smallvec::SmallVec;

use crate::{
    AbsoluteLength, AlignContent, AlignItems, AlignSelf, App, AvailableSpace, DefiniteLength,
    Display, Edges, FlexDirection, FlexWrap, GridLocation, GridTemplate, JustifyContent,
    LayoutDirection, LayoutId, Length, Overflow, Pixels, Point, Position, Size, Style,
    TaffyLayoutEngine, Window,
    fast::text::TextMeasureInputs,
    taffy::{NodeContext, ToTaffy as _},
    util::round_to_device_pixel,
};

/// What a Taffy call that cannot fail by construction panics with.
const EXPECT_MESSAGE: &str = "we should avoid taffy layout errors by construction if possible";

/// How many frames a node of a list row is kept unclaimed before it is
/// released. A list lays out only the rows it shows, so a row scrolled out
/// and back in would otherwise be laid out from scratch.
pub(crate) const LIST_ROW_NODE_LINGER_FRAMES: u64 = 240;

/// How many nodes of list rows may linger unclaimed, in percent of those
/// the list rows of the frame claimed: the rows scrolled out most recently.
/// What lingers costs memory for each node (a text leaf keeps the lines it
/// measured), and a list scrolled fast passes many rows in
/// [`LIST_ROW_NODE_LINGER_FRAMES`].
pub(crate) const LIST_ROW_NODE_LINGER_SHARE: usize = 50;

/// How many nodes of list rows may linger whatever the frame claimed.
pub(crate) const LIST_ROW_NODE_LINGER_MIN: usize = 256;

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
    /// When the nodes of list rows left unclaimed were claimed, gathered
    /// when they are swept. See [`linger_from`].
    linger_scratch: Vec<u64>,
    /// The style each node [`stretch_auto_size_to_fill`] stretched asked
    /// for: a stretched size looks like an explicit one, and the next
    /// frame's request is compared with what was asked.
    unstretched_styles: FxHashMap<LayoutId, taffy::style::Style>,
    /// How many requests were converted to Taffy styles.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) conversions: u64,
    /// The text leaves requested this frame: their closures are taken when
    /// it is cleared, without dirtying them.
    text_leaves: FxHashSet<LayoutId>,
    /// Text leaves whose measurement was carried or replayed when they were
    /// requested, with the width it was last taken at when known, to be
    /// settled once the layout they belong to is computed.
    pending_settles: Vec<(LayoutId, Option<Option<Pixels>>)>,
    /// Text leaves measured by the layout being computed, with the width
    /// each was last measured at.
    measured_text: FxHashMap<LayoutId, Option<Pixels>>,
    /// The keys claimed or made while a recording is open, for a retained
    /// view to keep its nodes by. See [`TaffyLayoutEngine::record_claimed_keys`].
    claimed_key_log: Vec<u64>,
    /// How many recordings of claimed keys are open, nested.
    open_key_recordings: usize,
    /// Counts the writes that change what a layout computes: a node made,
    /// a style or a child list written, a measured leaf dirtied. A subtree
    /// laid out again without any is laid out as it was.
    pub(crate) layout_changes: u64,
    /// Counts the measured leaves dirtied to be measured again.
    pub(crate) remeasures: u64,
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
    /// For a text leaf, what it was last measured from, and the sizes Taffy
    /// took from it since it was last dirtied.
    text: Option<TextLeaf>,
}

/// What a kept text leaf keeps from one frame to the next: data, never a
/// closure.
struct TextLeaf {
    inputs: Rc<TextMeasureInputs>,
    log: Rc<MeasureLog>,
    /// The scale factor its sizes were snapped to device pixels at: Taffy
    /// caches them in device pixels, so another scale factor measures it
    /// afresh whatever its text.
    scale_factor: f32,
    /// The text system's font generation it was measured under: fonts
    /// installed since can shape the same text differently, so it is
    /// measured afresh.
    font_generation: usize,
}

/// The measurements Taffy took of one text leaf since the leaf was last
/// dirtied: the constraints of each and the size it came out at, in the order
/// they were last taken.
///
/// Taffy caches a node's sizes under the constraints it computed them for,
/// until the node is dirtied. Every size it holds for the leaf came from one
/// of these, so a new closure giving the same size under each leaves the
/// leaf's cache, and those above it, as they are. Recording stops, and the
/// log vouches for nothing, once it holds more than a leaf measured under
/// steady constraints needs.
#[derive(Default)]
pub(crate) struct MeasureLog {
    entries: RefCell<SmallVec<[MeasureLogEntry; 4]>>,
    overflowed: Cell<bool>,
}

/// One measurement in a [`MeasureLog`].
type MeasureLogEntry = (Size<Option<Pixels>>, Size<AvailableSpace>, Size<Pixels>);

/// How many measurements a [`MeasureLog`] keeps before giving up. Taffy
/// caches ten results per node.
const MEASURE_LOG_CAPACITY: usize = 16;

impl MeasureLog {
    /// Records a measurement Taffy took, moving it to the end if one under
    /// the same constraints was recorded.
    pub(crate) fn record(
        &self,
        known: Size<Option<Pixels>>,
        available: Size<AvailableSpace>,
        size: Size<Pixels>,
    ) {
        if self.overflowed.get() {
            return;
        }
        let mut entries = self.entries.borrow_mut();
        if let Some(ix) = entries
            .iter()
            .position(|(k, a, _)| *k == known && *a == available)
        {
            entries.remove(ix);
        } else if entries.len() == MEASURE_LOG_CAPACITY {
            self.overflowed.set(true);
            entries.clear();
            return;
        }
        entries.push((known, available, size));
    }

    /// Whether the log holds every size Taffy took from its leaf since the
    /// leaf was last dirtied.
    fn vouches(&self) -> bool {
        !self.overflowed.get() && !self.entries.borrow().is_empty()
    }

    /// The width the last recorded measurement was taken at, as
    /// [`measured_width`] reads it, if one is recorded.
    fn last_width(&self) -> Option<Option<Pixels>> {
        let entries = self.entries.borrow();
        let (known, available, _) = entries.last()?;
        Some(measured_width(*known, *available))
    }
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
        if self.open_key_recordings > 0 {
            self.claimed_key_log.push(key);
        }
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
                text: None,
            },
        );
        self.claimed_this_frame += 1;
        self.layout_changes += 1;
        if self.open_key_recordings > 0 {
            self.claimed_key_log.push(key);
        }
    }

    /// The keys claimed or made since the outermost recording open began, in
    /// order: what [`Self::record_claimed_keys`] returns positions in.
    pub(crate) fn claimed_key_log(&self) -> &[u64] {
        &self.claimed_key_log
    }

    /// Starts recording the keys of the nodes claimed or made from now on,
    /// returning where the recording starts. Recordings nest.
    pub(crate) fn record_claimed_keys(&mut self) -> usize {
        self.open_key_recordings += 1;
        self.claimed_key_log.len()
    }

    /// Ends the recording started at `start`, returning the keys it saw.
    pub(crate) fn finish_recording_claimed_keys(&mut self, start: usize) -> Vec<u64> {
        let keys = self.claimed_key_log[start.min(self.claimed_key_log.len())..].to_vec();
        self.open_key_recordings = self.open_key_recordings.saturating_sub(1);
        if self.open_key_recordings == 0 {
            self.claimed_key_log.clear();
        }
        keys
    }

    /// Keeps the nodes under `keys` for another frame without requesting
    /// them, for a subtree drawn from last frame: they are there when it is
    /// built next. Keys already claimed this frame, or no longer kept, are
    /// passed over.
    pub(crate) fn keep_retained(&mut self, keys: &[u64]) {
        let frame = self.frame;
        for key in keys {
            if let Some(node) = self.retained.get_mut(key)
                && node.claimed_in_frame != frame
            {
                node.claimed_in_frame = frame;
                self.claimed_this_frame += 1;
                if self.open_key_recordings > 0 {
                    self.claimed_key_log.push(*key);
                }
            }
        }
    }

    /// Keeps the nodes under `keys`, as [`Self::keep_retained`] does, only if
    /// every one is still kept and unclaimed this frame: a layout reused
    /// without being requested needs all of its nodes as they were.
    pub(crate) fn try_keep_retained(&mut self, keys: &[u64]) -> bool {
        let frame = self.frame;
        let all_there = keys.iter().all(|key| {
            self.retained
                .get(key)
                .is_some_and(|node| node.claimed_in_frame != frame)
        });
        if all_there {
            self.keep_retained(keys);
        }
        all_there
    }

    /// The text leaves among the nodes under `keys` that were kept this
    /// frame without being requested, with what they were last measured
    /// from.
    fn kept_text_leaves(
        &self,
        keys: &[u64],
    ) -> Vec<(LayoutId, Rc<TextMeasureInputs>, Rc<MeasureLog>)> {
        keys.iter()
            .filter_map(|key| {
                let node = self.retained.get(key)?;
                let text = node.text.as_ref()?;
                (node.claimed_in_frame == self.frame && !self.text_leaves.contains(&node.id))
                    .then(|| (node.id, text.inputs.clone(), text.log.clone()))
            })
            .collect()
    }

    /// Claims the nodes under `keys`, claimed already this frame by a view
    /// built for a splice that did not happen, for the view built around it
    /// that takes it over: they are recorded as that view's own.
    pub(crate) fn adopt_claimed(&mut self, keys: &[u64]) {
        let frame = self.frame;
        for key in keys {
            let Some(node) = self.retained.get_mut(key) else {
                continue;
            };
            if node.claimed_in_frame != frame {
                node.claimed_in_frame = frame;
                self.claimed_this_frame += 1;
            }
            if self.open_key_recordings > 0 {
                self.claimed_key_log.push(*key);
            }
        }
    }

    /// Undoes [`Self::try_keep_retained`] for `keys`, so the subtree can be
    /// laid out again this frame and claim its nodes itself.
    pub(crate) fn release_kept(&mut self, keys: &[u64]) {
        let frame = self.frame;
        for key in keys {
            if let Some(node) = self.retained.get_mut(key)
                && node.claimed_in_frame == frame
            {
                node.claimed_in_frame = frame.wrapping_sub(1);
                self.claimed_this_frame -= 1;
            }
        }
    }

    /// How many nodes made this frame are not kept: a subtree that made any
    /// cannot have its layout reused, since they go when the frame ends.
    pub(crate) fn transient_count(&self) -> usize {
        self.transient.len()
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
            // A text leaf was left clean at clear for the next text at its
            // key to carry, replay or dirty. Another element is none of
            // those: what Taffy cached for the text is not its size.
            if let Some(node) = engine.retention.retained.get_mut(&key)
                && node.text.take().is_some()
            {
                engine.taffy.mark_dirty(id.into()).expect(EXPECT_MESSAGE);
            }
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

/// What a text leaf's request found it could do with the node it claimed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TextOutcome {
    /// Last frame's measurement stands for this element's text and is now
    /// its own: the leaf stays clean.
    Carried,
    /// The new closure gives every size Taffy took from the leaf: it stays
    /// clean.
    Replayed,
    /// The text changed its size: the leaf is dirtied and measured afresh.
    Remeasured,
    /// The node was not a text leaf: it is made one, and measured.
    Fresh,
}

/// Requests the layout of a text element under layout retention: a measured
/// leaf in the default style, whose measurement is carried over from last
/// frame's text at its key, or replayed, when it still stands. See
/// [`crate::fast::text::layout_text`].
pub(crate) fn request_text_node(
    window: &mut Window,
    inputs: Rc<TextMeasureInputs>,
    cx: &mut App,
) -> LayoutId {
    window.invalidator.debug_assert_prepaint();
    let rem_size = window.rem_size();
    let scale_factor = window.scale_factor();
    let font_generation = window
        .text_system()
        .line_layout_cache
        .font_generation
        .load(std::sync::atomic::Ordering::Acquire);
    let key = crate::fast::layout_key::layout_key(window);
    let mut engine = window
        .layout_engine
        .take()
        .expect("text is laid out with the layout engine in place");
    let measured_under = MeasuredUnder {
        scale_factor,
        font_generation,
    };
    let id = request_text_leaf(
        &mut engine,
        key,
        inputs,
        rem_size,
        measured_under,
        window,
        cx,
    );
    window.layout_engine = Some(engine);
    id
}

/// The node context of a text leaf: `inputs` measured, each size recorded in
/// `log`.
fn text_context(inputs: &Rc<TextMeasureInputs>, log: &Rc<MeasureLog>) -> NodeContext {
    NodeContext::measuring_text(inputs.clone(), log.clone())
}

/// Whether `inputs` measure to every size in `log` under the constraints it
/// was recorded under, measured in the order they were last taken, so what
/// the element keeps is what the last one left.
fn replays(
    log: &MeasureLog,
    inputs: &TextMeasureInputs,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let entries = log.entries.borrow().clone();
    entries.iter().all(|(known, available, size)| {
        crate::fast::text::measure_text(inputs, *known, *available, window, cx) == *size
    })
}

/// What a text leaf's sizes depend on beyond its own inputs.
#[derive(Clone, Copy, PartialEq)]
struct MeasuredUnder {
    scale_factor: f32,
    font_generation: usize,
}

fn request_text_leaf(
    engine: &mut TaffyLayoutEngine,
    key: Option<u64>,
    inputs: Rc<TextMeasureInputs>,
    rem_size: Pixels,
    measured_under: MeasuredUnder,
    window: &mut Window,
    cx: &mut App,
) -> LayoutId {
    let MeasuredUnder {
        scale_factor,
        font_generation,
    } = measured_under;
    let style = Style::default();
    let fingerprint = layout_fingerprint(&style, rem_size, scale_factor);
    let (key, id) = match engine.retention.claim(key) {
        Claim::Reused(key, id) => (key, id),
        claim => {
            let key = match claim {
                Claim::Vacant(key) => Some(key),
                _ => None,
            };
            let log = Rc::<MeasureLog>::default();
            let context = text_context(&inputs, &log);
            let id = request_node(
                engine,
                key,
                &style,
                rem_size,
                scale_factor,
                &[],
                Some(context),
            );
            engine.retention.text_leaves.insert(id);
            if let Some(node) = key.and_then(|key| engine.retention.retained.get_mut(&key))
                && node.id == id
            {
                node.text = Some(TextLeaf {
                    inputs,
                    log,
                    scale_factor,
                    font_generation,
                });
            }
            return id;
        }
    };
    crate::fast::fast_stat!(engine.fast_stats, nodes_claimed += 1);
    apply_requested_style(engine, key, id, fingerprint, &style, rem_size, scale_factor);
    let previous = engine
        .retention
        .retained
        .get(&key)
        .and_then(|node| node.text.as_ref())
        .map(|text| {
            (
                text.inputs.clone(),
                text.log.clone(),
                MeasuredUnder {
                    scale_factor: text.scale_factor,
                    font_generation: text.font_generation,
                },
            )
        });
    let (log, outcome) = match previous {
        Some((_, _, previous_under)) if previous_under != measured_under => {
            crate::fast::text::forget_measurement(&inputs);
            (Rc::default(), TextOutcome::Remeasured)
        }
        Some((previous, log, _)) => {
            if crate::fast::text::adopt_measurement(&inputs, &previous) {
                (log, TextOutcome::Carried)
            } else if log.vouches() && replays(&log, &inputs, window, cx) {
                (log, TextOutcome::Replayed)
            } else {
                crate::fast::text::forget_measurement(&inputs);
                (Rc::default(), TextOutcome::Remeasured)
            }
        }
        None => (Rc::default(), TextOutcome::Fresh),
    };
    let context = text_context(&inputs, &log);
    match outcome {
        TextOutcome::Carried | TextOutcome::Replayed => {
            set_measure_keeping_clean(engine, id, context);
            engine
                .retention
                .pending_settles
                .push((id, log.last_width()));
            if outcome == TextOutcome::Carried {
                crate::fast::fast_stat!(engine.fast_stats, leaves_carried += 1);
            } else {
                crate::fast::fast_stat!(engine.fast_stats, leaves_replayed += 1);
            }
        }
        TextOutcome::Remeasured | TextOutcome::Fresh => {
            engine.update_node_topology(id, &[], Some(context));
            if outcome == TextOutcome::Remeasured {
                crate::fast::fast_stat!(engine.fast_stats, leaves_remeasured += 1);
            }
        }
    }
    engine.retention.text_leaves.insert(id);
    if let Some(node) = engine.retention.retained.get_mut(&key) {
        node.text = Some(TextLeaf {
            inputs,
            log,
            scale_factor,
            font_generation,
        });
    }
    id
}

/// The width a measurement under these constraints was taken at: the width
/// it was given, or the definite width it was offered, or none when it was
/// asked for its content size.
fn measured_width(known: Size<Option<Pixels>>, available: Size<AvailableSpace>) -> Option<Pixels> {
    known.width.or(match available.width {
        AvailableSpace::Definite(width) => Some(width),
        AvailableSpace::MinContent | AvailableSpace::MaxContent => None,
    })
}

/// Whether `id` is a text leaf requested this frame, whose closure is taken
/// at clear without dirtying it.
pub(crate) fn is_text_leaf(retention: &LayoutRetention, id: LayoutId) -> bool {
    retention.text_leaves.contains(&id)
}

/// Notes that the layout being computed measured `id` under these
/// constraints, when it is a text leaf.
pub(crate) fn note_measured(
    retention: &mut LayoutRetention,
    id: LayoutId,
    known: Size<Option<Pixels>>,
    available: Size<AvailableSpace>,
) {
    if retention.text_leaves.contains(&id) {
        retention
            .measured_text
            .insert(id, measured_width(known, available));
    }
}

/// Whether computing the layout at `root` lays `id` out.
fn reaches(taffy: &taffy::TaffyTree<NodeContext>, root: LayoutId, id: LayoutId) -> bool {
    let root: taffy::NodeId = root.into();
    let mut node: taffy::NodeId = id.into();
    loop {
        if node == root {
            return true;
        }
        match taffy.parent(node) {
            Some(parent) => node = parent,
            None => return false,
        }
    }
}

/// Measures again, at the size it was laid out at, every text leaf of the
/// layout just computed at `root` whose measurement was last taken at
/// another width.
///
/// A text leaf keeps what its last measurement produced, and paints it.
/// Upstream lays every leaf out afresh each frame, and Taffy's last question
/// to a leaf is the one that places it. A kept leaf keeps the layout Taffy
/// placed it at, and Taffy answers that question from its cache, while it can
/// still ask the leaf for its size under other constraints: the last
/// measurement is then a probe (a `flex_1` text measured at width zero, and
/// painted a glyph to a line). The same holds for a measurement carried or
/// replayed at request time. As gpui-fast's fix `2e2c73a`.
pub(crate) fn settle_text_leaves(
    engine: &mut TaffyLayoutEngine,
    root: LayoutId,
    window: &mut Window,
    cx: &mut App,
) {
    let retention = &mut engine.retention;
    let mut pending = mem::take(&mut retention.pending_settles);
    let mut settle: SmallVec<[(LayoutId, Option<Option<Pixels>>); 16]> = retention
        .measured_text
        .drain()
        .map(|(id, width)| (id, Some(width)))
        .collect();
    // A leaf settles once: by the width it was measured at, else by the one
    // it was carried at. Looked up by set, as the leaves requested this frame
    // number in the thousands.
    let mut settled: FxHashSet<LayoutId> = settle.iter().map(|(id, _)| *id).collect();
    pending.retain(|&(id, width)| {
        if !reaches(&engine.taffy, root, id) {
            return true;
        }
        if settled.insert(id) {
            settle.push((id, width));
        }
        false
    });
    engine.retention.pending_settles = pending;
    let scale_factor = window.scale_factor();
    for (id, width) in settle {
        let Ok(layout) = engine.taffy.layout(id.into()) else {
            continue;
        };
        let content = Size {
            width: Pixels(
                (layout.size.width
                    - layout.padding.left
                    - layout.padding.right
                    - layout.border.left
                    - layout.border.right)
                    .max(0.)
                    / scale_factor,
            ),
            height: Pixels(
                (layout.size.height
                    - layout.padding.top
                    - layout.padding.bottom
                    - layout.border.top
                    - layout.border.bottom)
                    .max(0.)
                    / scale_factor,
            ),
        };
        if width == Some(Some(content.width)) {
            continue;
        }
        crate::fast::fast_stat!(window.fast_stats.frame, leaves_measured += 1);
        measure_node(
            engine,
            id,
            Size {
                width: Some(content.width),
                height: Some(content.height),
            },
            Size {
                width: AvailableSpace::Definite(content.width),
                height: AvailableSpace::Definite(content.height),
            },
            window,
            cx,
        );
    }
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
    retention.layout_changes += 1;
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
        // When each node lingering was claimed, and how many nodes of list
        // rows the frame claimed, for `linger_from`.
        let scratch = &mut retention.linger_scratch;
        scratch.clear();
        let mut claimed_rows = 0;
        retention.retained.retain(|_, node| {
            if node.claimed_in_frame == frame {
                claimed_rows += node.lingers as usize;
                return true;
            }
            if node.lingers && frame - node.claimed_in_frame < LIST_ROW_NODE_LINGER_FRAMES {
                scratch.push(node.claimed_in_frame);
                return true;
            }
            unstretched_styles.remove(&node.id);
            taffy.remove(node.id.into()).expect(EXPECT_MESSAGE);
            released += 1;
            false
        });
        lingering = scratch.len() as u64;
        // Only a list scrolled past more rows than may linger pays for a
        // second sweep.
        if let Some(linger_from) = linger_from(scratch, claimed_rows) {
            retention.retained.retain(|_, node| {
                if node.claimed_in_frame == frame || node.claimed_in_frame >= linger_from {
                    return true;
                }
                unstretched_styles.remove(&node.id);
                taffy.remove(node.id.into()).expect(EXPECT_MESSAGE);
                released += 1;
                lingering -= 1;
                false
            });
        }
    }
    retention.text_leaves.clear();
    retention.pending_settles.clear();
    retention.measured_text.clear();
    retention.claimed_this_frame = 0;
    retention.frame += 1;
    let _ = (released, lingering);
    crate::fast::fast_stat!(engine.fast_stats, nodes_released += released);
    crate::fast::fast_stat!(engine.fast_stats, nodes_lingering += lingering);
}

/// The earliest frame a node of a list row left unclaimed may have been
/// claimed in to linger, given when each such node was claimed (`claims`,
/// reordered) and how many nodes of list rows the frame claimed, if fewer
/// than all of them may: the nodes of the rows scrolled out most recently
/// linger, at most [`LIST_ROW_NODE_LINGER_SHARE`] of those the frame
/// claimed. Rows scrolled out before them were laid out too long ago to
/// come back soon, and a fast scroll would otherwise keep every row it
/// passed in [`LIST_ROW_NODE_LINGER_FRAMES`].
fn linger_from(claims: &mut [u64], claimed_rows: usize) -> Option<u64> {
    let limit = (claimed_rows * LIST_ROW_NODE_LINGER_SHARE / 100).max(LIST_ROW_NODE_LINGER_MIN);
    if claims.len() <= limit {
        return None;
    }
    let first_kept = claims.len() - limit;
    Some(*claims.select_nth_unstable(first_kept).1)
}

/// How many kept or transient nodes hold a measurement closure: none once
/// a frame is cleared.
#[cfg(test)]
pub(crate) fn closures_held(engine: &TaffyLayoutEngine) -> usize {
    let retention = &engine.retention;
    retention
        .retained
        .values()
        .map(|node| node.id)
        .chain(retention.transient.iter().copied())
        .filter(|id| {
            engine
                .taffy
                .get_node_context((*id).into())
                .is_some_and(|context| context.measure.is_some())
        })
        .count()
}

/// Measures every kept text leaf at width zero, as a flex probe would, and
/// notes it as `compute_layout` does, then settles the layout of the tree
/// each belongs to. Returns how many leaves were probed.
#[cfg(test)]
pub(crate) fn probe_and_settle_text_leaves(window: &mut Window, cx: &mut App) -> usize {
    let mut engine = window.layout_engine.take().unwrap();
    let leaves: Vec<(LayoutId, NodeContext)> = engine
        .retention
        .retained
        .values()
        .filter_map(|node| {
            let text = node.text.as_ref()?;
            Some((node.id, text_context(&text.inputs, &text.log)))
        })
        .collect();
    let leaves: Vec<LayoutId> = leaves
        .into_iter()
        .map(|(id, context)| {
            // The frame that measured it was cleared: give it its closure
            // back for the probe, as the next frame would.
            set_measure_keeping_clean(&mut engine, id, context);
            id
        })
        .collect();
    for id in &leaves {
        // A flex child allowed to shrink to nothing is offered width zero.
        let (known, available) = (
            Size {
                width: Some(Pixels::ZERO),
                height: None,
            },
            Size {
                width: AvailableSpace::Definite(Pixels::ZERO),
                height: AvailableSpace::MaxContent,
            },
        );
        engine.retention.text_leaves.insert(*id);
        measure_node(&mut engine, *id, known, available, window, cx);
        note_measured(&mut engine.retention, *id, known, available);
        let mut root: taffy::NodeId = (*id).into();
        while let Some(parent) = engine.taffy.parent(root) {
            root = parent;
        }
        settle_text_leaves(&mut engine, root.into(), window, cx);
    }
    engine.clear();
    window.layout_engine = Some(engine);
    leaves.len()
}

/// Gives a measured node `context`'s closure without dirtying it, for a
/// measurement that still stands.
fn set_measure_keeping_clean(engine: &mut TaffyLayoutEngine, id: LayoutId, context: NodeContext) {
    match engine.taffy.get_node_context_mut(id.into()) {
        Some(kept) => {
            kept.measure = context.measure;
            #[cfg(feature = "diagnostics")]
            {
                kept.kind = context.kind;
            }
        }
        None => engine
            .taffy
            .set_node_context(id.into(), Some(context))
            .expect(EXPECT_MESSAGE),
    }
    engine.note_requested_node(id, &[], true);
}

/// Keeps the nodes under `keys` for a subtree drawn from last frame, as
/// [`LayoutRetention::keep_retained`] does, or, with `all_or_none`, only if
/// every one is there, as [`LayoutRetention::try_keep_retained`] does.
///
/// Each text leaf kept is given back a closure measuring the text it was
/// last measured from, as if it had been requested: the subtree is laid out
/// as it was only while its constraints are, and a parent laid out under
/// other constraints measures its leaves again. Without a closure, Taffy
/// would size them as empty.
pub(crate) fn keep_retained_nodes(
    engine: &mut TaffyLayoutEngine,
    keys: &[u64],
    all_or_none: bool,
) -> bool {
    if all_or_none {
        if !engine.retention.try_keep_retained(keys) {
            return false;
        }
    } else {
        engine.retention.keep_retained(keys);
    }
    for (id, inputs, log) in engine.retention.kept_text_leaves(keys) {
        set_measure_keeping_clean(engine, id, text_context(&inputs, &log));
        engine.retention.text_leaves.insert(id);
    }
    true
}

/// Whether computing layout at `root` lays `id` out. Element ancestry does
/// not imply layout ancestry: list rows and other children laid out on their
/// own are trees of their own.
pub(crate) fn layout_reaches(engine: &TaffyLayoutEngine, root: LayoutId, id: LayoutId) -> bool {
    reaches(&engine.taffy, root, id)
}

/// The layouts last computed for the kept nodes `keys`, to tell once layout
/// is computed again whether they came out the same.
pub(crate) fn retained_layouts(
    engine: &TaffyLayoutEngine,
    keys: &[u64],
) -> Vec<(LayoutId, taffy::Layout)> {
    keys.iter()
        .filter_map(|key| engine.retention.retained.get(key))
        .filter_map(|node| Some((node.id, *engine.taffy.layout(node.id.into()).ok()?)))
        .collect()
}

/// Whether the nodes [`retained_layouts`] returned still have the layouts
/// they had then.
pub(crate) fn layouts_unchanged(
    engine: &TaffyLayoutEngine,
    layouts: &[(LayoutId, taffy::Layout)],
) -> bool {
    layouts.iter().all(|(id, layout)| {
        engine
            .taffy
            .layout((*id).into())
            .is_ok_and(|now| now == layout)
    })
}

/// Runs a measured node's closure under these constraints, outside a layout
/// computation.
fn measure_node(
    engine: &mut TaffyLayoutEngine,
    id: LayoutId,
    known_dimensions: Size<Option<Pixels>>,
    available_space: Size<AvailableSpace>,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(measure) = engine
        .taffy
        .get_node_context_mut(id.into())
        .and_then(|context| context.measure.as_mut())
    {
        measure.call(known_dimensions, available_space, window, cx);
    }
}

/// Lays out again the subtree under `id`, a node already placed by its
/// parent this frame, within `available_space`, leaving it where its parent
/// put it, for a view built at the layout it kept.
///
/// Computing a layout from a node treats it as a root and moves it to the
/// origin; its absolute position, worked out before, is put back so that
/// the bounds of everything under it are found relative to it as before. A
/// root is sized by its own style, so a node its parent stretched would
/// shrink to its content: it is held at the size its parent gave it while it
/// is laid out, and given back the style it asked for afterwards.
pub(crate) fn relayout_in_place(
    engine: &mut TaffyLayoutEngine,
    id: LayoutId,
    available_space: Size<AvailableSpace>,
    window: &mut Window,
    cx: &mut App,
) {
    let scale_factor = window.scale_factor();
    let bounds = engine.layout_bounds(id, scale_factor);
    let origin = engine
        .absolute_outer_origins
        .get(&id)
        .copied()
        .expect("layout_bounds caches the absolute origin");
    let requested = engine.taffy.style(id.into()).expect(EXPECT_MESSAGE).clone();
    let given = engine.taffy.layout(id.into()).expect(EXPECT_MESSAGE).size;
    let mut held = requested.clone();
    held.size = taffy::geometry::Size {
        width: taffy::style::Dimension::length(given.width),
        height: taffy::style::Dimension::length(given.height),
    };
    held.min_size = taffy::geometry::Size {
        width: taffy::style::LengthPercentageAuto::length(given.width),
        height: taffy::style::LengthPercentageAuto::length(given.height),
    };
    held.max_size = held.min_size;
    held.box_sizing = taffy::style::BoxSizing::BorderBox;
    engine
        .taffy
        .set_style(id.into(), held)
        .expect(EXPECT_MESSAGE);
    engine.compute_layout(id, available_space, window, cx);
    engine
        .taffy
        .set_style(id.into(), requested)
        .expect(EXPECT_MESSAGE);
    let stack = &mut engine.layout_bounds_scratch_space;
    stack.push(id);
    while let Some(id) = stack.pop() {
        engine.absolute_layout_bounds.remove(&id);
        engine.absolute_outer_origins.remove(&id);
        stack.extend(
            engine
                .taffy
                .children(id.into())
                .expect(EXPECT_MESSAGE)
                .into_iter()
                .map(LayoutId::from),
        );
    }
    engine.absolute_outer_origins.insert(id, origin);
    engine.absolute_layout_bounds.insert(id, bounds);
}

/// Begins a frame for `window`'s layout: it keys its layout while layout
/// retention is on. A frame that switches retention on or off starts from a
/// fresh layout engine, so kept and pooled nodes never meet.
pub(crate) fn begin_frame(window: &mut Window) {
    let active = window.fast_mode.layout_retention();
    window.fast_layout.begin_frame(active);
    window
        .text_system()
        .line_layout_cache
        .fast_carry_held_lines
        .store(active, std::sync::atomic::Ordering::Relaxed);
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
