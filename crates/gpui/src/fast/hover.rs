//! What a pointer's move compares an element's hover with.
//!
//! An element with a hover style repaints when a move of the pointer changes
//! whether it is hovered. What the move is compared with is kept in the
//! element's state ([`ElementHoverState`]), and upstream writes it only when
//! a move changes it. But a hover changes without a move too: the wheel
//! scrolls another row under the pointer, and the frame that follows paints
//! that row hovered and the row the pointer was over unhovered, by the frame's
//! own hit test. The state still says the old row is hovered and the new one
//! is not, so when the pointer then moves back onto the old row neither
//! differs from what the state holds, nothing is notified, and the window
//! goes on showing the new row hovered until something else repaints it.
//!
//! So what is painted is what the state holds: [`painted`] writes it where
//! the element registers the listener that reads it. A move is then compared
//! with what the window shows.
//!
//! gpux's own, not gpui-fast's: fast mode's retained rows are rendered again
//! by the hovers they read (`crate::fast::dependencies`) and showed the right
//! row, so a window with fast mode on differed for a frame from one with it
//! off.

use crate::{ElementHoverState, Hitbox, HitboxId, Window};
use std::{cell::RefCell, rc::Rc};

/// Notes whether the element of `state`, being painted, is painted hovered.
/// An element without a hover style has no state, and its hitbox is not
/// asked: asking is a read of the hover, which what is retained is painted
/// again for.
pub(crate) fn painted(
    state: Option<&Rc<RefCell<ElementHoverState>>>,
    hitbox: &Hitbox,
    window: &Window,
) {
    if let Some(state) = state {
        state.borrow_mut().element = hitbox.is_hovered(window);
    }
}

/// Notes whether the element of `state`, being painted, is painted with its
/// group hovered.
pub(crate) fn painted_group(
    state: Option<&Rc<RefCell<ElementHoverState>>>,
    group: HitboxId,
    window: &Window,
) {
    if let Some(state) = state {
        state.borrow_mut().group = group.is_hovered(window);
    }
}
