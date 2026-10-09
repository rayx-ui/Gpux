//! What a reusable subtree read while it was drawn, so it is drawn again only
//! while what it read is unchanged.
//!
//! Adapted from gpui-fast's `fast/dependencies.rs` and the hover reads of
//! `fast/retained.rs` (`note_hover_read`, `hovers_unchanged`). This sprint
//! covers hovers, which gpux's cached views (`AnyView::cached`) read through
//! hover styles: a cached view painted while hovered must not be replayed
//! once the pointer has left it, whether a mouse move, a wheel event, or a
//! press moved the pointer, nor while an overlay this frame adds may cover
//! it, nor at all once it read the hover of a hitbox it did not insert.

use std::{
    cell::{Cell, RefCell},
    ops::Range,
};

use collections::FxHashSet;

use crate::{Bounds, HitboxId, Pixels, Window};

/// The hovers read while reusable subtrees were being drawn.
#[derive(Default)]
pub(crate) struct HoverReads {
    /// How many recordings are open, innermost last.
    depth: Cell<usize>,
    /// Every hover read since the outermost recording opened, in order:
    /// a nested recording's reads stay for the recordings around it.
    reads: RefCell<Vec<(HitboxId, bool)>>,
}

/// Notes that whatever is being drawn asked whether `hitbox` is hovered and
/// got `hovered`.
#[inline(always)]
pub(crate) fn note_hover_read(window: &Window, hitbox: HitboxId, hovered: bool) {
    let hovers = &window.fast_hover_reads;
    if hovers.depth.get() > 0 {
        hovers.reads.borrow_mut().push((hitbox, hovered));
    }
}

/// Starts recording the hovers read from here on; returns where they start.
pub(crate) fn begin_recording(window: &Window) -> usize {
    let hovers = &window.fast_hover_reads;
    hovers.depth.set(hovers.depth.get() + 1);
    hovers.reads.borrow().len()
}

/// Ends the recording [`begin_recording`] started at `start` and returns the
/// hovers read since. They stay recorded for the recordings around it.
pub(crate) fn finish_recording(window: &Window, start: usize) -> Vec<(HitboxId, bool)> {
    let hovers = &window.fast_hover_reads;
    let depth = hovers.depth.get().saturating_sub(1);
    hovers.depth.set(depth);
    let mut reads = hovers.reads.borrow_mut();
    let recorded = reads.get(start..).map(<[_]>::to_vec).unwrap_or_default();
    if depth == 0 {
        reads.clear();
    }
    recorded
}

/// Adds the hovers a subtree drawn again from its last frame was drawn by to
/// the recordings around it, which depend on them too.
pub(crate) fn note_replayed_hovers(window: &Window, reads: &[(HitboxId, bool)]) {
    let hovers = &window.fast_hover_reads;
    if hovers.depth.get() > 0 && !reads.is_empty() {
        hovers.reads.borrow_mut().extend_from_slice(reads);
    }
}

/// Whether every hover in `reads` is still as it was read, against the
/// window's current hit test.
pub(crate) fn hovers_unchanged(window: &Window, reads: &[(HitboxId, bool)]) -> bool {
    reads
        .iter()
        .all(|(hitbox, hovered)| hitbox.hovered_unrecorded(window) == *hovered)
}

/// Where a cached view's hovers can change, worked out when it is built.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum HoverArea {
    /// Every hover it read is of a hitbox it inserted itself, and those can
    /// be hit only within these bounds: the union of their masked bounds.
    Own(Option<Bounds<Pixels>>),
    /// It read the hover of a hitbox it did not insert, such as an
    /// ancestor's group (`group_hover`). That hitbox is inserted again, under
    /// a new id and perhaps at new bounds, whenever its owner is drawn, so
    /// nothing the view recorded says where its hovers can change.
    #[default]
    Foreign,
}

/// Where the hovers in `reads` can change, for a view that inserted the
/// hitboxes at `hitboxes` in the frame being drawn.
pub(crate) fn hover_area(
    window: &Window,
    reads: &[(HitboxId, bool)],
    hitboxes: Range<usize>,
) -> HoverArea {
    let own = &window.next_frame.hitboxes[hitboxes];
    if !reads.is_empty() {
        let ids: FxHashSet<HitboxId> = own.iter().map(|hitbox| hitbox.id).collect();
        if reads.iter().any(|(hitbox, _)| !ids.contains(hitbox)) {
            return HoverArea::Foreign;
        }
    }
    HoverArea::Own(
        own.iter()
            .map(|hitbox| hitbox.bounds.intersect(&hitbox.content_mask.bounds))
            .filter(|bounds| !bounds.is_empty())
            .reduce(|area, bounds| area.union(&bounds)),
    )
}

/// Whether a cached view drawn by `reads`, whose hovers can change in
/// `area`, may be replayed as far as its hovers go.
///
/// Its hovers must be as they were against the hit test prepaint sees, which
/// is last frame's hitboxes. This frame's hitboxes, known only after
/// prepaint, can cover or uncover the view's, but only where the pointer is:
/// with the pointer away from all of them, and none holding the pointer
/// captured, every hover it read is false whatever this frame adds. So a view
/// that read hovers is drawn again while the pointer is over its hitboxes,
/// and its replays always show the hovers a frame drawn from scratch shows.
/// A view that read the hover of a hitbox it did not insert is never
/// replayed: that hitbox's id and bounds belong to whoever inserted it.
pub(crate) fn may_replay_hovers(
    window: &Window,
    reads: &[(HitboxId, bool)],
    area: HoverArea,
) -> bool {
    if reads.is_empty() {
        return true;
    }
    let HoverArea::Own(area) = area else {
        return false;
    };
    let pointer_over = area.is_some_and(|area| area.contains(&window.mouse_position));
    let captured = window
        .captured_hitbox
        .is_some_and(|captured| reads.iter().any(|(hitbox, _)| *hitbox == captured));
    !pointer_over && !captured && hovers_unchanged(window, reads)
}
