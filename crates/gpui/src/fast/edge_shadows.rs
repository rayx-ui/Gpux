//! Ordering at a clip's edge: shadows whose box has left their clip while
//! what they draw still reaches in, and what lies under and over them.
//!
//! A scene gives each primitive a draw order from the bounds it keeps: the
//! primitive's box, clipped to its content mask. A primitive whose clipped
//! box is empty is dropped. A drop shadow draws three blur radii past its
//! box, so that rule drops a shadow whose box has just left a viewport though
//! part of its blur is still inside: the blur vanishes a few pixels early as
//! the element scrolls out, and comes back late as it scrolls in.
//!
//! Such a shadow is kept here, drawn clipped to its mask as any primitive is.
//! Where it is stacked must not depend on how far its content is scrolled: a
//! blur falls on neighbours its shadow's box never overlaps, over those of a
//! lower draw order and under the others, and a draw order worked out from
//! what the clip leaves in view changes as rows enter and leave it. A layer's
//! tiles are painted once and scrolled, and a frame drawn without them has to
//! equal them wherever the content stands.
//!
//! So at a clip's edge a scene orders primitives as it would were the clip
//! not there:
//!
//! - a primitive the clip drops keeps an entry of its whole bounds and the
//!   order it would have had ([`Dropped`], an operation of its own among the
//!   paint operations, so that everything replaying them keeps it);
//! - a primitive the clip cuts is ordered over the dropped primitives, and
//!   the other cut ones, that its whole bounds lie over, and is raised past
//!   them where the scene's tree, which keeps clipped bounds, put it lower;
//! - a kept shadow is ordered over what lies under its box, dropped or
//!   drawn, and what is painted after it over its box is raised past it: the
//!   element casting it, whose edge can show while the shadow's box, offset
//!   away from it, does not.
//!
//! A primitive that lies wholly inside its clip meets nothing outside it,
//! and is ordered by the scene's tree alone, as before. In a scene whose
//! primitives share one clip, every primitive kept has exactly the order it
//! has with the clip not there.
//!
//! The entries are kept by clip: primitives of one content mask are ordered
//! against each other, not against what another clip dropped where they
//! happen to lie in the window. What is painted outside a clip (the
//! background behind a viewport, an overlay over it) lies under or over a
//! kept shadow where it lies under or over the blur that shows, and under
//! the primitives the clip drops where it lies behind the clip's nearest
//! edge.

use crate::{
    Bounds, DrawOrder, ScaledPixels, Scene,
    fast::{bounds_tree::BoundsTree, layers::scene::drawn_bounds},
    point,
    scene::{
        LayerOrder, POSITIONED_DRAW_ORDER_BASE, PaintOperation, Primitive,
        TOP_LAYER_DRAW_ORDER_BASE,
    },
    size,
};
use collections::FxHashMap;
use smallvec::SmallVec;

/// How far outside its mask a dropped primitive is still given an entry. A
/// primitive further off lies under nothing that shows, short of a blur
/// reaching that far. An entry costs about a tenth of a microsecond, which
/// a page of text scrolled without a layer pays for every glyph within
/// reach.
const REACH: ScaledPixels = ScaledPixels(2048.);

/// How many times a primitive's bounds are inserted again to raise it past
/// what it lies over. Each insertion raises it by one; content whose rows
/// each overlap the one before for hundreds of rows gives up here.
const MAX_RAISE: usize = 256;

/// How many entries of one mask are kept in a list before they are put in a
/// tree. A clipped cell whose text runs past it has a few; a scrolling
/// container has what it scrolls out of view.
const LISTED: usize = 24;

/// How many times a scene works out what was painted behind a mask's edge
/// ([`behind`]), a pass over its paint operations each. Scrolling containers
/// are few; a table of a thousand clipped cells is past this with its first
/// rows, and its cells' entries start at the band's first order, which
/// matters to nothing a cell holds.
const BEHIND_BUDGET: usize = 64;

/// A primitive a scene dropped, its bounds clipped to its mask being empty:
/// what is left of it among the paint operations, so that what is painted
/// over it at its clip's edge is ordered as with the clip not there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dropped {
    /// The primitive's whole bounds.
    pub(crate) bounds: Bounds<ScaledPixels>,
    /// The bounds of its content mask.
    pub(crate) mask: Bounds<ScaledPixels>,
}

impl Dropped {
    /// The entry moved by `delta`, its mask with it.
    pub(crate) fn moved(self, delta: crate::Point<ScaledPixels>) -> Dropped {
        let moved = |bounds: Bounds<ScaledPixels>| Bounds {
            origin: bounds.origin + delta,
            size: bounds.size,
        };
        Dropped {
            bounds: moved(self.bounds),
            mask: moved(self.mask),
        }
    }
}

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

/// The primitives of one content mask and one band of draw orders that a
/// scene meets at the mask's edges: those it dropped, those the mask cuts
/// and the shadows it kept, by their whole bounds, each with its order.
struct Lifted {
    mask: Bounds<ScaledPixels>,
    /// The band's first draw order, less one.
    base: DrawOrder,
    /// The entries while they are few.
    listed: SmallVec<[(Bounds<ScaledPixels>, DrawOrder); 4]>,
    /// The entries once they are many.
    tree: Option<BoundsTree<ScaledPixels>>,
    /// The highest order among what was painted outside the mask behind
    /// each of its edges (below, above, left, right), once worked out.
    behind: [Option<DrawOrder>; 4],
}

impl Lifted {
    /// `bounds` where the tree holds them: the tree's grid starts at the
    /// origin, and what lies above or left of it shares its first cells, so
    /// the entries are kept moved to where everything within [`REACH`] of
    /// the mask is right and down of the origin.
    fn in_tree(&self, bounds: &Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        Bounds {
            origin: point(
                bounds.origin.x - self.mask.origin.x + REACH,
                bounds.origin.y - self.mask.origin.y + REACH,
            ),
            size: bounds.size,
        }
    }

    /// The highest order among the entries `bounds` meet, or 0.
    fn max_over(&mut self, bounds: &Bounds<ScaledPixels>) -> DrawOrder {
        let moved = self.in_tree(bounds);
        match &mut self.tree {
            Some(tree) => tree.max_over(&moved),
            None => self
                .listed
                .iter()
                .filter(|(entry, _)| entry.intersects(bounds))
                .map(|(_, order)| *order)
                .max()
                .unwrap_or(0),
        }
    }
}

/// What a scene being built keeps of the primitives at its clips' edges.
#[derive(Default)]
pub(crate) struct EdgeShadows {
    shadows: Vec<EdgeShadow>,
    lifted: Vec<Lifted>,
    /// Where each mask's entries are in `lifted`, by the mask's bounds and
    /// the band.
    by_mask: FxHashMap<([u32; 4], DrawOrder), usize>,
    /// Trees to fill again, kept from the scenes before.
    spare: Vec<BoundsTree<ScaledPixels>>,
    /// How many more times [`behind`] may pass over the paint operations.
    behind_budget: usize,
}

impl EdgeShadows {
    /// Forgets the scene's entries, as [`Scene::clear`] forgets its
    /// primitives.
    pub(crate) fn clear(&mut self) {
        self.shadows.clear();
        self.by_mask.clear();
        for lifted in self.lifted.drain(..) {
            if let Some(mut tree) = lifted.tree {
                tree.forget();
                self.spare.push(tree);
            }
        }
        self.behind_budget = BEHIND_BUDGET;
    }

    /// How many shadows the scene kept for what they draw inside their
    /// masks.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.shadows.len()
    }

    /// The entries of the primitives of `mask` in the band of `base`.
    fn lifted(&mut self, mask: Bounds<ScaledPixels>, base: DrawOrder) -> usize {
        // Primitives of one mask come one after another.
        if let Some(last) = self.lifted.last()
            && last.mask == mask
            && last.base == base
        {
            return self.lifted.len() - 1;
        }
        let key = (
            [
                mask.origin.x.0.to_bits(),
                mask.origin.y.0.to_bits(),
                mask.size.width.0.to_bits(),
                mask.size.height.0.to_bits(),
            ],
            base,
        );
        let next = self.lifted.len();
        let index = *self.by_mask.entry(key).or_insert(next);
        if index == next {
            if next == 0 {
                // A scene built without having been cleared.
                self.behind_budget = BEHIND_BUDGET;
            }
            self.lifted.push(Lifted {
                mask,
                base,
                listed: SmallVec::new(),
                tree: None,
                behind: [None; 4],
            });
        }
        index
    }

    /// Enters `bounds` at `order` among the entries at `index`.
    fn enter(&mut self, index: usize, bounds: Bounds<ScaledPixels>, order: DrawOrder) {
        let lifted = &mut self.lifted[index];
        let moved = lifted.in_tree(&bounds);
        if let Some(tree) = &mut lifted.tree {
            return tree.insert_at(moved, order);
        }
        lifted.listed.push((bounds, order));
        if lifted.listed.len() > LISTED {
            let mut tree = self.spare.pop().unwrap_or_default();
            // Everything within reach of the mask, at once.
            tree.cover(&Bounds {
                origin: point(ScaledPixels(0.), ScaledPixels(0.)),
                size: size(
                    lifted.mask.size.width + REACH + REACH,
                    lifted.mask.size.height + REACH + REACH,
                ),
            });
            let listed = std::mem::take(&mut lifted.listed);
            for (bounds, order) in &listed {
                tree.insert_at(lifted.in_tree(bounds), *order);
            }
            lifted.tree = Some(tree);
        }
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

/// Adds `primitive` to `scene` at `order`, worked out elsewhere: the scene
/// draws it, without ordering it or anything after it by it. For a scene
/// drawn from another's primitives at the orders they have there, as a
/// tile's is.
pub(crate) fn insert_at(scene: &mut Scene, mut primitive: Primitive, order: DrawOrder) {
    match &mut primitive {
        Primitive::Shadow(shadow) => {
            shadow.order = order;
            scene.shadows.push(*shadow);
        }
        Primitive::VisualEffect(effect) => {
            effect.order = order;
            scene.visual_effects.push(effect.clone());
        }
        Primitive::Quad(quad) => {
            quad.order = order;
            scene.quads.push(quad.clone());
        }
        Primitive::HtmlIsland(island) => {
            island.order = order;
            scene.html_islands.push(island.clone());
        }
        Primitive::HtmlElement(element) => {
            element.order = order;
            scene.html_elements.push(element.clone());
        }
        Primitive::ScreenEffectRegion(region) => {
            region.order = order;
            scene.screen_effect_regions.push(region.clone());
        }
        Primitive::Path(path) => {
            path.order = order;
            path.id = crate::scene::PathId(scene.paths.len());
            scene.paths.push(path.clone());
        }
        Primitive::Underline(underline) => {
            underline.order = order;
            scene.underlines.push(*underline);
        }
        Primitive::MonochromeSprite(sprite) => {
            sprite.order = order;
            scene.monochrome_sprites.push(*sprite);
        }
        Primitive::SubpixelSprite(sprite) => {
            sprite.order = order;
            scene.subpixel_sprites.push(*sprite);
        }
        Primitive::PolychromeSprite(sprite) => {
            sprite.order = order;
            scene.polychrome_sprites.push(*sprite);
        }
        Primitive::Surface(surface) => {
            surface.order = order;
            scene.surfaces.push(surface.clone());
        }
    }
    scene
        .paint_operations
        .push(PaintOperation::Primitive(primitive));
}

/// The band the scene orders in now, and the order everything takes inside
/// a layer of one order.
fn context(scene: &Scene) -> (DrawOrder, Option<DrawOrder>) {
    match scene.layer_stack.last().copied() {
        None => (0, None),
        Some(LayerOrder::Positioned) => (POSITIONED_DRAW_ORDER_BASE, None),
        Some(LayerOrder::Top) => (TOP_LAYER_DRAW_ORDER_BASE, None),
        Some(LayerOrder::Fixed(order)) => (band(order), Some(order)),
    }
}

/// The scene's tree for the band of `base`.
fn tree(scene: &mut Scene, base: DrawOrder) -> &mut BoundsTree<ScaledPixels> {
    match base {
        TOP_LAYER_DRAW_ORDER_BASE => &mut scene.top_layer_primitive_bounds,
        POSITIONED_DRAW_ORDER_BASE => &mut scene.positioned_primitive_bounds,
        _ => &mut scene.primitive_bounds,
    }
}

/// The highest order among the primitives painted so far, outside `mask`,
/// in the band of `base`, that draw behind the edge of `mask` nearest to
/// `bounds`, which lie outside it: the background behind a viewport, which
/// what the viewport drops lies over as what it shows does.
fn behind(scene: &mut Scene, index: usize, bounds: &Bounds<ScaledPixels>) -> DrawOrder {
    let lifted = &scene.fast_edge_shadows.lifted[index];
    let (mask, base) = (lifted.mask, lifted.base);
    let one = ScaledPixels(1.);
    let across = |top: ScaledPixels| Bounds {
        origin: point(mask.left(), top),
        size: size(mask.size.width, one),
    };
    let down = |left: ScaledPixels| Bounds {
        origin: point(left, mask.top()),
        size: size(one, mask.size.height),
    };
    let (side, strip) = if bounds.top() >= mask.bottom() {
        (0, across(mask.bottom() - one))
    } else if bounds.bottom() <= mask.top() {
        (1, across(mask.top()))
    } else if bounds.right() <= mask.left() {
        (2, down(mask.left()))
    } else {
        (3, down(mask.right() - one))
    };
    if let Some(order) = lifted.behind[side] {
        return order;
    }
    let Some(budget) = scene.fast_edge_shadows.behind_budget.checked_sub(1) else {
        return base;
    };
    scene.fast_edge_shadows.behind_budget = budget;
    let mut highest = base;
    for operation in &scene.paint_operations {
        let PaintOperation::Primitive(primitive) = operation else {
            continue;
        };
        let order = order_of(primitive);
        let painted_in = primitive.content_mask().bounds;
        if order > highest
            && band(order) == base
            && !within(&painted_in, &mask)
            && primitive.bounds().intersect(&painted_in).intersects(&strip)
        {
            highest = order;
        }
    }
    scene.fast_edge_shadows.lifted[index].behind[side] = Some(highest);
    highest
}

/// Takes in a primitive whose bounds, clipped to its mask, are empty, which
/// a scene drops. A shadow that still draws inside its mask is kept, ordered
/// over what lies under its box; anything else leaves an entry of its whole
/// bounds, at the order it would have had.
pub(crate) fn keep(scene: &mut Scene, mut primitive: Primitive) {
    let bounds = primitive.bounds();
    let mask = primitive.content_mask().bounds;
    let visible = match &primitive {
        Primitive::Shadow(_) => drawn_bounds(&primitive).intersect(&mask),
        _ => Bounds::default(),
    };
    let Primitive::Shadow(shadow) = &mut primitive else {
        return dropped(scene, Dropped { bounds, mask });
    };
    if visible.is_empty() {
        return dropped(scene, Dropped { bounds, mask });
    }
    let mut edge = EdgeShadow {
        bounds,
        visible,
        mask,
        order: 0,
    };
    let (base, fixed) = context(scene);
    let index = scene.fast_edge_shadows.lifted(mask, base);
    edge.order = match fixed {
        // Everything in the layer is drawn at its order.
        Some(order) => order,
        None => {
            let over = scene.fast_edge_shadows.lifted[index].max_over(&bounds);
            order_over(scene, &edge, base).max(over) + 1
        }
    };
    scene.fast_edge_shadows.enter(index, bounds, edge.order);
    shadow.order = edge.order;
    scene.shadows.push(*shadow);
    scene.fast_edge_shadows.shadows.push(edge);
    scene
        .paint_operations
        .push(PaintOperation::Primitive(primitive));
}

/// Notes a primitive the scene dropped: an entry of its whole bounds at the
/// order it would have had, and an operation among the paint operations.
/// One whose mask is empty, or that lies far from its mask, lies under
/// nothing that shows, and leaves nothing.
pub(crate) fn dropped(scene: &mut Scene, dropped: Dropped) {
    let Dropped { bounds, mask } = dropped;
    if mask.is_empty() {
        return;
    }
    let near = Bounds {
        origin: point(mask.left() - REACH, mask.top() - REACH),
        size: size(
            mask.size.width + REACH + REACH,
            mask.size.height + REACH + REACH,
        ),
    };
    if !bounds.intersects(&near) {
        return;
    }
    let (base, fixed) = context(scene);
    let index = scene.fast_edge_shadows.lifted(mask, base);
    let order = match fixed {
        Some(order) => order,
        None => {
            // What it lies over inside its clip lies over what is behind
            // the clip already.
            match scene.fast_edge_shadows.lifted[index].max_over(&bounds) {
                0 => behind(scene, index, &bounds) + 1,
                over => over + 1,
            }
        }
    };
    scene.fast_edge_shadows.enter(index, bounds, order);
    scene
        .paint_operations
        .push(PaintOperation::FastDropped(dropped));
}

/// The highest order among the primitives painted so far, in the band of
/// `base`, that lie under the box of `shadow`, or `base` when none does.
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
    highest
}

/// The order of `primitive`, which the scene keeps with `bounds` (clipped to
/// its mask) at `order`: raised past what it lies over at its clip's edge,
/// where the scene's tree, which keeps clipped bounds, put it no higher.
/// That is the primitives its mask cuts or drops that its whole bounds lie
/// over, and the shadows kept for their blur whose box it is painted over.
///
/// One past, not level: two translucent primitives drawn in the other order
/// blend to a colour a rounding step away.
pub(crate) fn raise(
    scene: &mut Scene,
    primitive: &Primitive,
    bounds: Bounds<ScaledPixels>,
    order: DrawOrder,
) -> DrawOrder {
    let whole = primitive.bounds();
    // A primitive wholly inside its mask meets nothing outside it.
    let cut = whole != bounds;
    if !cut && scene.fast_edge_shadows.shadows.is_empty() {
        return order;
    }
    let mask = primitive.content_mask().bounds;
    let (base, fixed) = context(scene);
    let mut under = scene
        .fast_edge_shadows
        .shadows
        .iter()
        .filter(|shadow| {
            shadow.order >= order && band(shadow.order) == base && overlaps(shadow, whole, &mask)
        })
        .map(|shadow| shadow.order)
        .max()
        .unwrap_or(0);
    let index = cut.then(|| scene.fast_edge_shadows.lifted(mask, base));
    if let Some(index) = index {
        under = under.max(scene.fast_edge_shadows.lifted[index].max_over(&whole));
    }
    // Everything in a layer of one order is drawn at it, what the layer
    // holds of its own at the clip's edge too: the layer is raised only to
    // what lies under it, not past it.
    let enough = |order: DrawOrder| match fixed {
        Some(_) => order >= under,
        None => order > under,
    };
    // The tree gives an order one past the highest it holds over the
    // bounds, which the bounds' own are among: each insertion raises it.
    let mut order = order;
    let tree = tree(scene, base);
    for _ in 0..MAX_RAISE {
        if enough(order) {
            break;
        }
        order = base + tree.insert(bounds);
    }
    if fixed.is_some_and(|fixed| fixed != order)
        && let Some(layer) = scene.layer_stack.last_mut()
    {
        // What the layer holds from here on is drawn with the primitive.
        *layer = LayerOrder::Fixed(order);
    }
    if let Some(index) = index {
        scene.fast_edge_shadows.enter(index, whole, order);
    }
    order
}
