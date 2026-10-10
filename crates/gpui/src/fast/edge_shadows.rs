//! Shadows whose box has left its clip while what they draw still reaches in.
//!
//! A scene gives each primitive a draw order from the bounds it keeps: the
//! primitive's box, clipped to its content mask. A primitive whose clipped
//! box is empty is dropped. A drop shadow draws three blur radii past its
//! box, so that rule drops a shadow whose box has just left a viewport though
//! part of its blur is still inside: the blur vanishes a few pixels early as
//! the element scrolls out, and comes back late as it scrolls in.
//!
//! Such a shadow is kept here, drawn clipped to its mask as any primitive is,
//! and stacked as it is while its box shows:
//!
//! - over what was painted before it under its box and draws inside the clip
//!   (the backgrounds it lies on);
//! - under what is painted after it over its box (the element casting it,
//!   whose edge can show while the shadow's box, offset away from it, does
//!   not);
//! - and, among primitives its box does not overlap, as primitives of equal
//!   draw order are: by kind, then in paint order.
//!
//! The bounds a scene's tree keeps are clipped to masks, and cannot tell
//! what lay under a box outside the clip from what the blur merely reaches.
//! The paint operations keep each primitive whole, with its own bounds and
//! mask, so the order of a kept shadow is worked out from those: one pass
//! over the operations painted so far, made only for these shadows, which
//! are the few within a blur's reach of a clip's edge.
//!
//! A tile of a scroll layer orders a shadow the same way: its scene is the
//! primitives drawing over the tile, inserted whole
//! ([`crate::fast::layers::scene::LayerFrame::tile_scene`]), so a frame drawn
//! from tiles and one drawn without layers show the same blur at a
//! viewport's edge.
//!
//! Nothing else changes: a scene that keeps no such shadow orders every
//! primitive as it did before.

use crate::{
    Bounds, DrawOrder, ScaledPixels, Scene,
    fast::layers::scene::drawn_bounds,
    point,
    scene::{LayerOrder, PaintOperation, Primitive},
};

/// A shadow a scene kept for what it draws inside its mask (see the
/// module's documentation).
#[derive(Clone, Copy, Debug)]
struct EdgeShadow {
    /// The shadow's box, outside its mask.
    bounds: Bounds<ScaledPixels>,
    /// What it draws inside its mask.
    visible: Bounds<ScaledPixels>,
    mask: Bounds<ScaledPixels>,
    order: DrawOrder,
}

/// The shadows the scene being built kept for what they draw inside their
/// masks.
#[derive(Default)]
pub(crate) struct EdgeShadows {
    shadows: Vec<EdgeShadow>,
}

impl EdgeShadows {
    /// Forgets the scene's shadows, as [`Scene::clear`] forgets its
    /// primitives.
    pub(crate) fn clear(&mut self) {
        self.shadows.clear();
    }

    /// How many shadows the scene kept for what they draw inside their
    /// masks.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.shadows.len()
    }
}

/// The run of draw orders an order belongs to: normal content, positioned
/// layers or the top layer. Orders of different bands are not worked out
/// from one another.
fn band(order: DrawOrder) -> DrawOrder {
    if order > crate::scene::TOP_LAYER_DRAW_ORDER_BASE {
        crate::scene::TOP_LAYER_DRAW_ORDER_BASE
    } else if order > crate::scene::POSITIONED_DRAW_ORDER_BASE {
        crate::scene::POSITIONED_DRAW_ORDER_BASE
    } else {
        0
    }
}

pub(crate) fn order_of(primitive: &Primitive) -> DrawOrder {
    match primitive {
        Primitive::Shadow(shadow) => shadow.order,
        Primitive::VisualEffect(effect) => effect.order,
        Primitive::Quad(quad) => quad.order,
        Primitive::HtmlIsland(island) => island.order,
        Primitive::HtmlElement(element) => element.order,
        Primitive::ScreenEffectRegion(region) => region.order,
        Primitive::Path(path) => path.order,
        Primitive::Underline(underline) => underline.order,
        Primitive::MonochromeSprite(sprite) => sprite.order,
        Primitive::SubpixelSprite(sprite) => sprite.order,
        Primitive::PolychromeSprite(sprite) => sprite.order,
        Primitive::Surface(surface) => surface.order,
    }
}

/// Whether `inner` lies within `outer`.
fn within(inner: &Bounds<ScaledPixels>, outer: &Bounds<ScaledPixels>) -> bool {
    inner.left() >= outer.left()
        && inner.top() >= outer.top()
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

/// `bounds` clipped to `mask`, but let through the edges `mask` shares with
/// `clip`: what a primitive painted inside `clip` would cover were `clip` not
/// there.
fn through(
    bounds: Bounds<ScaledPixels>,
    mask: &Bounds<ScaledPixels>,
    clip: &Bounds<ScaledPixels>,
) -> Bounds<ScaledPixels> {
    let mut left = bounds.left();
    let mut top = bounds.top();
    let mut right = bounds.right();
    let mut bottom = bounds.bottom();
    if mask.left() > clip.left() {
        left = left.max(mask.left());
    }
    if mask.top() > clip.top() {
        top = top.max(mask.top());
    }
    if mask.right() < clip.right() {
        right = right.min(mask.right());
    }
    if mask.bottom() < clip.bottom() {
        bottom = bottom.min(mask.bottom());
    }
    Bounds::from_corners(point(left, top), point(right.max(left), bottom.max(top)))
}

/// Whether a primitive of `bounds` painted in `mask` lies under or over the
/// box of `shadow`, as it would were the shadow's clip not there.
///
/// A primitive painted inside the shadow's clip is taken through the edges
/// its own mask shares with it. One painted outside (the background behind a
/// viewport, an overlay over it) covers what it draws and no more: it lies
/// under or over the shadow where it lies under or over the blur that shows.
fn overlaps(
    shadow: &EdgeShadow,
    bounds: Bounds<ScaledPixels>,
    mask: &Bounds<ScaledPixels>,
) -> bool {
    if within(mask, &shadow.mask) {
        through(bounds, mask, &shadow.mask).intersects(&shadow.bounds)
    } else {
        bounds.intersect(mask).intersects(&shadow.visible)
    }
}

/// Takes in a primitive whose bounds, clipped to its mask, are empty, which a
/// scene drops: a shadow that still draws inside its mask is kept, ordered
/// over what lies under its box.
pub(crate) fn keep(scene: &mut Scene, mut primitive: Primitive) {
    let Primitive::Shadow(shadow) = &mut primitive else {
        return;
    };
    let mask = shadow.content_mask.bounds;
    let visible = drawn_bounds(&Primitive::Shadow(*shadow)).intersect(&mask);
    if visible.is_empty() {
        return;
    }
    let mut edge = EdgeShadow {
        bounds: shadow.transformation.apply_to_scaled_bounds(shadow.bounds),
        visible,
        mask,
        order: 0,
    };
    edge.order = match scene.layer_stack.last().copied() {
        // Everything in the layer is drawn at its order.
        Some(LayerOrder::Fixed(order)) => order,
        Some(LayerOrder::Positioned) => {
            order_over(scene, &edge, crate::scene::POSITIONED_DRAW_ORDER_BASE)
        }
        Some(LayerOrder::Top) => order_over(scene, &edge, crate::scene::TOP_LAYER_DRAW_ORDER_BASE),
        None => order_over(scene, &edge, 0),
    };
    shadow.order = edge.order;
    scene.shadows.push(*shadow);
    scene.fast_edge_shadows.shadows.push(edge);
    scene
        .paint_operations
        .push(PaintOperation::Primitive(primitive));
}

/// The order of `shadow` in the band starting at `base`: one past the
/// highest among the primitives painted so far, in that band, that lie
/// under its box.
fn order_over(scene: &Scene, shadow: &EdgeShadow, base: DrawOrder) -> DrawOrder {
    let mut highest = base;
    for operation in &scene.paint_operations {
        let PaintOperation::Primitive(primitive) = operation else {
            continue;
        };
        let order = order_of(primitive);
        if order > highest
            && band(order) == base
            && overlaps(shadow, primitive.bounds(), &primitive.content_mask().bounds)
        {
            highest = order;
        }
    }
    highest + 1
}

/// The order of `primitive`, which the scene keeps with `bounds` (clipped to
/// its mask) at `order`: raised past that of the shadows kept for their blur
/// whose box it is painted over, as the scene's tree puts it past them while
/// their boxes show. One past, not level: two translucent primitives drawn in
/// the other order blend to a colour a rounding step away.
pub(crate) fn raise(
    scene: &mut Scene,
    primitive: &Primitive,
    bounds: Bounds<ScaledPixels>,
    order: DrawOrder,
) -> DrawOrder {
    if scene.fast_edge_shadows.shadows.is_empty() {
        return order;
    }
    let base = match scene.layer_stack.last() {
        // The layer's order is the primitive's.
        Some(LayerOrder::Fixed(_)) => return order,
        Some(LayerOrder::Positioned) => crate::scene::POSITIONED_DRAW_ORDER_BASE,
        Some(LayerOrder::Top) => crate::scene::TOP_LAYER_DRAW_ORDER_BASE,
        None => 0,
    };
    let whole = primitive.bounds();
    let mask = primitive.content_mask().bounds;
    let Some(under) = scene
        .fast_edge_shadows
        .shadows
        .iter()
        .filter(|shadow| {
            shadow.order >= order && band(shadow.order) == base && overlaps(shadow, whole, &mask)
        })
        .map(|shadow| shadow.order)
        .max()
    else {
        return order;
    };
    // The tree gives an order one past the highest it holds over the
    // bounds, which the bounds' own are among: each insertion raises it.
    let tree = match base {
        crate::scene::TOP_LAYER_DRAW_ORDER_BASE => &mut scene.top_layer_primitive_bounds,
        crate::scene::POSITIONED_DRAW_ORDER_BASE => &mut scene.positioned_primitive_bounds,
        _ => &mut scene.primitive_bounds,
    };
    let mut order = order;
    while order <= under {
        order = base + tree.insert(bounds);
    }
    order
}
