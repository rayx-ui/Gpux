//! What a reusable subtree read while it was drawn, so it is drawn again only
//! while what it read is unchanged.
//!
//! Adapted from gpui-fast's `fast/dependencies.rs` and the hover reads of
//! `fast/retained.rs` (`note_hover_read`, `hovers_unchanged`). This sprint
//! covers hovers, which gpux's cached views (`AnyView::cached`) read through
//! hover styles: a cached view painted while hovered must not be replayed
//! once the pointer has left it, whether a mouse move, a wheel event, or a
//! press moved the pointer.

use std::cell::{Cell, RefCell};

use crate::{EntityId, HitboxId, Window};

/// The hovers read while reusable subtrees were being drawn.
#[derive(Default)]
pub(crate) struct HoverReads {
    /// How many recordings are open, innermost last.
    depth: Cell<usize>,
    /// Every hover read since the outermost recording opened, in order:
    /// a nested recording's reads stay for the recordings around it.
    reads: RefCell<Vec<(HitboxId, bool)>>,
    /// Views replayed this frame although a hover they read changed once the
    /// frame's hitboxes were known, too late to build them again in it.
    stale_replays: RefCell<Vec<EntityId>>,
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

/// Notes that the view `entity` was replayed with hovers that changed once
/// this frame's hitboxes were known: an overlay now covers it, or uncovers
/// it, with the pointer still. [`redraw_stale_replays`] draws the frame again.
pub(crate) fn note_stale_replay(window: &Window, entity: EntityId) {
    window
        .fast_hover_reads
        .stale_replays
        .borrow_mut()
        .push(entity);
}

/// Whether the frame just drawn replayed a view whose hovers changed; if so,
/// marks those views and their ancestors dirty for drawing again, so they are
/// built with this frame's hovers. The window draws the frame again before it
/// is shown, so no frame shows a stale hover. Nothing about the views changed,
/// so they are not notified: their observers do not run, and the platform is
/// not asked for another frame.
pub(crate) fn redraw_stale_replays(window: &mut Window) -> bool {
    let stale = std::mem::take(&mut *window.fast_hover_reads.stale_replays.borrow_mut());
    for entity in &stale {
        window.mark_view_dirty(*entity);
    }
    !stale.is_empty()
}
