//! Taffy layout nodes kept across frames by element identity.
//!
//! Adapted from the claim and lifecycle half of gpui-fast's `fast/layout.rs`.
//! With layout retention on, [`TaffyLayoutEngine`] finds an element's node by
//! the key [`crate::fast::layout_key`] derives for it, instead of taking the
//! next node of its ordinal pool. A node kept this way stays where it is in
//! the tree, so inserting, removing or reordering keyed siblings leaves every
//! other node unwritten, and Taffy's cache keeps it clean.
//!
//! Measured leaves are given their new closure, and dirtied, each frame as in
//! the pool, and their closures are still taken when the frame is cleared:
//! nothing a frame captured outlives it.

use collections::FxHashMap;

use crate::{LayoutId, TaffyLayoutEngine, Window, taffy::NodeContext};

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
}

/// What [`LayoutRetention::claim`] found for a key.
enum Claim {
    /// A node from an earlier frame, now claimed for this one.
    Reused(LayoutId),
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
        Claim::Reused(node.id)
    }

    /// Records a node made this frame under `key`, or as transient.
    fn keep(&mut self, key: Option<u64>, id: LayoutId) {
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
            },
        );
        self.claimed_this_frame += 1;
    }
}

/// Adds a node to the layout tree for an element keyed `key`, reusing the
/// node kept under it when there is one. Its style and children are written
/// only when they differ from the node's, and a measured node is given its
/// new closure and dirtied, as the pool does. See
/// [`TaffyLayoutEngine::request_layout`].
pub(crate) fn request_node(
    engine: &mut TaffyLayoutEngine,
    key: Option<u64>,
    style: taffy::style::Style,
    children: &[LayoutId],
    context: Option<NodeContext>,
) -> LayoutId {
    let key = match engine.retention.claim(key) {
        Claim::Reused(id) => {
            crate::fast::fast_stat!(engine.fast_stats, nodes_claimed += 1);
            engine.update_pooled_node(id, style, children, context);
            return id;
        }
        Claim::Vacant(key) => Some(key),
        Claim::Unkeyed => None,
    };
    let measured = context.is_some();
    crate::fast::fast_stat!(engine.fast_stats, style_writes += 1);
    crate::fast::fast_stat!(engine.fast_stats, children_writes += !children.is_empty());
    let id: LayoutId = match context {
        Some(context) => engine.taffy.new_leaf_with_context(style, context),
        None => engine.taffy.new_leaf(style),
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
    engine.retention.keep(key, id);
    engine.note_requested_node(id, children, measured);
    id
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
        engine.taffy.remove(id.into()).expect(EXPECT_MESSAGE);
        released += 1;
    }
    // In a steady frame every kept node was claimed, and nothing is swept.
    if retention.retained.len() != retention.claimed_this_frame {
        let frame = retention.frame;
        let taffy = &mut engine.taffy;
        retention.retained.retain(|_, node| {
            if node.claimed_in_frame == frame {
                return true;
            }
            if node.lingers && frame - node.claimed_in_frame < LIST_ROW_NODE_LINGER_FRAMES {
                lingering += 1;
                return true;
            }
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
