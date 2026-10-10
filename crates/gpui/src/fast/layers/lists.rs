//! Per-row list layers: what `list` and `uniform_list` keep in a layer beyond
//! a `div`'s content. Not ported yet: until it is, lists draw on today's
//! path and these are the neutral answers the other layer modules ask for.
//! Adapted from gpui-fast's `fast/layers/lists.rs`.

use std::ops::Range;

use crate::{
    ElementId, EntityId, GlobalElementId, HitboxId, PaintIndex, PrepaintStateIndex, Window,
};

/// What a layer keeps for a list's rows.
#[derive(Default)]
pub(crate) struct LayerRows {
    /// Whether the container is a `list`.
    pub(crate) list: bool,
    /// Whether the list renders the rows it shows again on every frame.
    pub(crate) repaints_shown_rows: bool,
    /// Whether the list's items changed since the layer was painted.
    pub(crate) items_changed: bool,
    /// Whether this frame composites the layer while the view holding the
    /// list renders again.
    pub(crate) rerendered: bool,
}

impl LayerRows {
    /// Ends the frame being drawn.
    pub(crate) fn finish_frame(&mut self) {}

    /// Follows the rows' prepaint ranges to where a reused view copied them.
    pub(crate) fn follow_prepaint(
        &mut self,
        _shift: &dyn Fn(&Range<PrepaintStateIndex>) -> Range<PrepaintStateIndex>,
    ) {
    }

    /// Follows the rows' paint ranges to where a reused view copied them.
    pub(crate) fn follow_paint(
        &mut self,
        _shift: &dyn Fn(&Range<PaintIndex>) -> Range<PaintIndex>,
    ) {
    }

    /// Whether a row the layer holds inserted the hitbox `id`.
    pub(crate) fn holds_hitbox(&self, _id: HitboxId) -> bool {
        false
    }
}

/// What the ids of the elements and views inside the scroll container `id`
/// start with: `id` itself, for a `div`.
pub(crate) fn content_prefix(id: &GlobalElementId) -> &[ElementId] {
    id
}

/// Whether a view drawn into the rows the layer of the list `id` holds is one
/// `f` picks.
pub(crate) fn any_held_view(_: &Window, _: &GlobalElementId, _: impl Fn(EntityId) -> bool) -> bool {
    false
}

/// Whether `view` is a row the layer `id` holds without showing it.
pub(crate) fn held_row_view(_: &Window, _: &GlobalElementId, _: EntityId) -> bool {
    false
}
