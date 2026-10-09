//! The path of keys from the root of the element tree that matches an
//! element to the layout node it had last frame.
//!
//! Adapted from gpui-fast's `fast/layout_key.rs`. Keys are derived only in a
//! frame that began with layout retention on; otherwise every function here
//! is a no-op and the layout engine's ordinal pool runs as before.

use std::{
    hash::{Hash, Hasher},
    mem,
};

use collections::FxHasher;
use smallvec::SmallVec;

use crate::{AnyElement, App, AvailableSpace, ElementId, Pixels, SharedString, Size, Window};

/// What a window keeps to match elements to the layout nodes they had last
/// frame.
pub(crate) struct WindowLayout {
    /// Whether the frame being drawn keys its layout; fixed when it begins.
    active: bool,
    /// Running hashes of the path from the root of the element tree down to
    /// the element currently requesting layout. See [`push_layout_key`].
    key_stack: SmallVec<[LayoutKeyFrame; 32]>,
    /// How many unidentified element trees have been laid out under the
    /// current scope. Window roots, prompts, drags, tooltips and deferred
    /// draws each start a tree of their own and need keys that do not collide.
    root_index: u32,
    /// The layout key of the element currently being prepainted, which
    /// anything it lays out from there hangs off. See [`enter_prepaint_scope`].
    prepaint_scope: u64,
}

impl Default for WindowLayout {
    fn default() -> Self {
        WindowLayout {
            active: false,
            key_stack: SmallVec::new(),
            root_index: 0,
            prepaint_scope: LAYOUT_ROOT_SEED,
        }
    }
}

impl WindowLayout {
    /// Starts the key paths of a frame from the root, keying them only when
    /// `active`.
    pub(crate) fn begin_frame(&mut self, active: bool) {
        debug_assert!(self.key_stack.is_empty());
        self.active = active;
        self.key_stack.clear();
        self.root_index = 0;
        self.prepaint_scope = LAYOUT_ROOT_SEED;
    }
}

/// The key an element's layout node was requested under, kept with the
/// element from request layout to prepaint. Meaningless in a frame that does
/// not key its layout.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LayoutKey(u64);

/// One level of [`push_layout_key`]'s path stack.
struct LayoutKeyFrame {
    /// Hash of the path from the root of the element tree to this element.
    key: u64,
    /// How many children without an [`ElementId`] have been entered so far,
    /// which is what identifies the next one.
    next_unidentified_child: u32,
}

/// Where layout key paths start.
const LAYOUT_ROOT_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// Separates the elements laid out during an element's prepaint from that
/// same element's children, which are keyed from the same point during
/// request layout and would otherwise land on the same keys.
const LAYOUT_PREPAINT_SALT: u64 = 0x5BF0_3635_931A_2E77;

/// Mixes `value` into `state`, well enough that path hashes built out of
/// small child indices do not collide in practice: the SplitMix64 finalizer
/// applied to the combined value.
fn mix(state: u64, value: u64) -> u64 {
    let mut z = state
        .rotate_left(27)
        .wrapping_add(value)
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Whether the frame being drawn keys its layout.
#[inline(always)]
pub(crate) fn keyed(window: &Window) -> bool {
    window.fast_layout.active
}

/// The key identifying the element currently requesting layout, matching it
/// with the Taffy node it had last frame; `None` in a frame that does not key
/// its layout, or outside an element.
#[inline(always)]
pub(crate) fn layout_key(window: &Window) -> Option<u64> {
    let layout = &window.fast_layout;
    if !layout.active {
        return None;
    }
    layout.key_stack.last().map(|frame| frame.key)
}

/// Begins an element, deriving the key its layout node is matched by across
/// frames.
///
/// The key is the hash of the path from the root: each element mixes either
/// its [`ElementId`], when it has one, or its index among its unidentified
/// siblings. Identified elements therefore keep their node when siblings are
/// inserted or reordered around them, while unidentified ones are matched by
/// position, the bargain the element state map makes. Because the parent's
/// key is always mixed in, a key encodes the whole ancestor path, and a node
/// is never matched to an element that moved to another parent.
#[inline]
pub(crate) fn push_layout_key(window: &mut Window, id: Option<&ElementId>) -> LayoutKey {
    let layout = &mut window.fast_layout;
    if !layout.active {
        return LayoutKey::default();
    }
    let component = match id {
        Some(id) => {
            let mut hasher = FxHasher::default();
            id.hash(&mut hasher);
            // Distinct from the positional case, so an element identified by
            // index 3 and one whose id hashes to 3 do not collide.
            mix(hasher.finish(), 1)
        }
        None => {
            let index = match layout.key_stack.last_mut() {
                Some(parent) => &mut parent.next_unidentified_child,
                None => &mut layout.root_index,
            };
            let component = mix(*index as u64, 2);
            *index += 1;
            component
        }
    };
    let parent = layout
        .key_stack
        .last()
        .map(|parent| parent.key)
        .unwrap_or_else(|| mix(layout.prepaint_scope, LAYOUT_PREPAINT_SALT));
    let key = mix(parent, component);
    layout.key_stack.push(LayoutKeyFrame {
        key,
        next_unidentified_child: 0,
    });
    LayoutKey(key)
}

/// Ends the element most recently begun by [`push_layout_key`].
#[inline(always)]
pub(crate) fn pop_layout_key(window: &mut Window) {
    let layout = &mut window.fast_layout;
    if layout.active {
        layout.key_stack.pop();
    }
}

/// Hangs elements laid out from here on the element whose prepaint is
/// running, keyed `key`, and returns what [`exit_prepaint_scope`] needs to
/// undo it.
///
/// The walk that assigns keys covers request layout only. Elements laid out
/// during prepaint (list items, which a list lays out only once it knows how
/// many fit, and cached views) arrive with no path, and would otherwise be
/// keyed by the order they happened to be laid out in. Keyed under the
/// element laying them out, an item with an [`ElementId`] keeps its nodes
/// wherever it moves within its list.
#[inline(always)]
pub(crate) fn enter_prepaint_scope(window: &mut Window, key: LayoutKey) -> (u64, u32) {
    let layout = &mut window.fast_layout;
    if !layout.active {
        return (layout.prepaint_scope, layout.root_index);
    }
    (
        mem::replace(&mut layout.prepaint_scope, key.0),
        mem::replace(&mut layout.root_index, 0),
    )
}

/// Restores what [`enter_prepaint_scope`] replaced.
#[inline(always)]
pub(crate) fn exit_prepaint_scope(window: &mut Window, enclosing: (u64, u32)) {
    let layout = &mut window.fast_layout;
    (layout.prepaint_scope, layout.root_index) = enclosing;
}

/// Lays `element` out as the item at `index` of a list, the way
/// [`AnyElement::layout_as_root`] does.
///
/// A list lays out only the items in view, so an item without an
/// [`ElementId`] would be matched to last frame's nodes by where it comes
/// among the items laid out this frame, and scrolling by one row would hand
/// every item its neighbour's nodes. Keyed by its index instead, an item
/// keeps its nodes while it stays in view; one with an id keeps being matched
/// by that, so it keeps its nodes when items are inserted ahead of it. Only
/// layout is keyed: element state is untouched. The nodes the item claims
/// linger after it scrolls out (see [`crate::fast::layout`]).
pub(crate) fn layout_as_list_item(
    element: &mut AnyElement,
    index: usize,
    available_space: Size<AvailableSpace>,
    window: &mut Window,
    cx: &mut App,
) -> Size<Pixels> {
    if !window.fast_layout.active {
        return element.layout_as_root(available_space, window, cx);
    }
    crate::fast::layout::laying_out_list_row(window, |window| {
        if element.fast_element_id().is_some() {
            return element.layout_as_root(available_space, window, cx);
        }
        // Named so that it cannot stand for an item given its index as id.
        let key =
            ElementId::NamedInteger(SharedString::new_static("gpux::list_item"), index as u64);
        push_layout_key(window, Some(&key));
        let size = element.layout_as_root(available_space, window, cx);
        pop_layout_key(window);
        size
    })
}
