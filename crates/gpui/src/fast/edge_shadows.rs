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
//! So at a clip's edge a scene orders primitives as it would were that clip
//! not there, every other clip staying where it is:
//!
//! - a primitive the clip drops keeps an entry of its bounds and the order
//!   it would have had ([`Dropped`], an operation of its own among the paint
//!   operations, so that everything replaying them keeps it);
//! - a primitive the clip cuts is ordered over the dropped primitives, and
//!   the other cut ones, that its bounds lie over, and is raised past them
//!   where the scene's tree, which keeps clipped bounds, put it lower;
//! - a kept shadow is ordered over what lies under its box, dropped or
//!   drawn, and what is painted after it over its box is raised past it: the
//!   element casting it, whose edge can show while the shadow's box, offset
//!   away from it, does not.
//!
//! The window tells the scene each clip as it is pushed and popped
//! ([`ClipOp`], paint operations too), so the scene knows the clips a
//! primitive is painted in one by one, not only their intersection, which a
//! content mask is. With a clip not there, a primitive covers its bounds
//! within the clips pushed inside that one: a child of a card that clips its
//! content, cut by the viewport the card scrolls in, reaches as far as the
//! card, not as far as its own bounds. The clips around the one lifted are
//! left out: what a scrolling container holds is ordered alike wherever the
//! container itself is clipped, as the content of its layer is.
//!
//! A primitive that lies wholly inside its mask meets nothing outside it,
//! and is ordered by the scene's tree alone, as before. In a scene whose
//! primitives share one clip, every primitive kept has exactly the order it
//! has with the clip not there.
//!
//! What was painted before a clip was pushed lies behind everything in it:
//! a primitive the clip drops, with nothing of the clip's under it, is
//! ordered over what was painted there before, where it would come into
//! view. What is painted beside a shadow, inside its clip or after it, and
//! does not lie over its box is not ordered by it, kept or not: a blur and
//! such a neighbour are drawn by their draw orders, as they always were.

use crate::{
    Bounds, ContentMask, DrawOrder, Pixels, ScaledPixels, Scene, Window,
    fast::{bounds_tree::BoundsTree, layers::scene::drawn_bounds},
    point,
    scene::{
        LayerOrder, POSITIONED_DRAW_ORDER_BASE, PaintOperation, Primitive,
        TOP_LAYER_DRAW_ORDER_BASE, TransformationMatrix,
    },
    size,
};
use collections::FxHashMap;
use smallvec::SmallVec;

/// How far outside a clip a dropped primitive is still given an entry. A
/// primitive further off lies under nothing that shows, short of a blur
/// reaching that far. An entry costs about a tenth of a microsecond, which
/// a page of text scrolled without a layer pays for every glyph within
/// reach.
const REACH: ScaledPixels = ScaledPixels(2048.);

/// How many times a primitive's bounds are inserted again to raise it past
/// what it lies over. Each insertion raises it by one; content whose rows
/// each overlap the one before for hundreds of rows gives up here.
const MAX_RAISE: usize = 256;

/// How many entries of one clip are kept in a list before they are put in a
/// tree. A clipped cell whose text runs past it has a few; a scrolling
/// container has what it scrolls out of view.
const LISTED: usize = 24;

/// How many times a scene works out what was painted behind a clip
/// ([`behind`]), a pass over what was painted before it each. Scrolling
/// containers are few; a table of a thousand clipped cells is past this with
/// its first rows, and its cells' entries start at the band's first order,
/// which matters to nothing a cell holds.
const BEHIND_BUDGET: usize = 64;

/// A primitive a scene dropped, its bounds clipped to its mask being empty:
/// what is left of it among the paint operations, so that what is painted
/// over it at a clip's edge is ordered as with the clip not there.
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
        Dropped {
            bounds: moved(self.bounds, delta),
            mask: moved(self.mask, delta),
        }
    }
}

/// A clip pushed or popped while a scene is painted: what the window
/// intersects into the content mask, on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ClipOp {
    /// The clip's bounds, as the masks of what is painted in it hold them.
    Push(Bounds<ScaledPixels>),
    Pop,
}

impl ClipOp {
    /// The operation with its clip moved by `delta`.
    pub(crate) fn moved(self, delta: crate::Point<ScaledPixels>) -> ClipOp {
        match self {
            ClipOp::Push(clip) => ClipOp::Push(moved(clip, delta)),
            ClipOp::Pop => ClipOp::Pop,
        }
    }
}

fn moved(bounds: Bounds<ScaledPixels>, delta: crate::Point<ScaledPixels>) -> Bounds<ScaledPixels> {
    Bounds {
        origin: bounds.origin + delta,
        size: bounds.size,
    }
}

fn band_index(base: DrawOrder) -> usize {
    match base {
        TOP_LAYER_DRAW_ORDER_BASE => 2,
        POSITIONED_DRAW_ORDER_BASE => 1,
        _ => 0,
    }
}

/// No entries yet.
const NONE: usize = usize::MAX;

/// A clip the scene is painting in.
struct Clip {
    bounds: Bounds<ScaledPixels>,
    /// The clips pushed so far, intersected: the mask of what is painted now.
    mask: Bounds<ScaledPixels>,
    /// Which push this is, counted from the scene's first.
    id: u32,
    /// The clip's entries in each band, in [`EdgeShadows::lifted`].
    entries: [usize; 3],
    /// How many bounds each band's tree held when the clip was pushed: what
    /// was painted before it, and lies behind what it holds.
    before: [usize; 3],
    /// How many paint operations the scene held when the clip was pushed.
    pushed_at: usize,
}

/// The primitives a scene meets at one clip's edges, in one band of draw
/// orders: those the clip drops, those it cuts and the shadows kept at it,
/// by the bounds they have with the clip not there, each with its order.
struct Lifted {
    /// The clip.
    clip: Bounds<ScaledPixels>,
    /// The band's first draw order, less one.
    base: DrawOrder,
    /// For a clip the window told of: which push it was, and how many bounds
    /// the band's tree and how many paint operations the scene held then.
    pushed: Option<(u32, usize, usize)>,
    /// How many paint operations the scene held when each entry was entered,
    /// entry by entry.
    entered_at: Vec<usize>,
    /// The entries while they are few.
    listed: SmallVec<[(Bounds<ScaledPixels>, DrawOrder); 4]>,
    /// The entries once they are many.
    tree: Option<BoundsTree<ScaledPixels>>,
    /// The highest order among what was painted behind the clip under each
    /// stretch of its edges asked about so far.
    behind: SmallVec<[(Bounds<ScaledPixels>, DrawOrder); 2]>,
}

impl Lifted {
    /// `bounds` where the tree holds them: the tree's grid starts at the
    /// origin, and what lies above or left of it shares its first cells, so
    /// the entries are kept moved to where everything within [`REACH`] of
    /// the clip is right and down of the origin.
    fn in_tree(&self, bounds: &Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        Bounds {
            origin: point(
                bounds.origin.x - self.clip.origin.x + REACH,
                bounds.origin.y - self.clip.origin.y + REACH,
            ),
            size: bounds.size,
        }
    }

    /// The highest order among the entries entered before the scene held
    /// `operations` paint operations that `bounds` meet, or 0.
    fn max_before(&self, operations: usize, bounds: &Bounds<ScaledPixels>) -> DrawOrder {
        let count = self
            .entered_at
            .partition_point(|entered| *entered < operations);
        match &self.tree {
            Some(tree) => tree.max_among_first(count, &self.in_tree(bounds)),
            None => self.listed[..count.min(self.listed.len())]
                .iter()
                .filter(|(entry, _)| entry.intersects(bounds))
                .map(|(_, order)| *order)
                .max()
                .unwrap_or(0),
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
    /// The clips the window pushed and has not popped.
    clips: Vec<Clip>,
    /// How many clips were pushed so far.
    pushed: u32,
    /// How many shadows were kept for what they draw inside their masks.
    kept: usize,
    lifted: Vec<Lifted>,
    /// Where the entries of a clip the window did not tell of are in
    /// `lifted`, by the mask it shows as and the band.
    by_mask: FxHashMap<([u32; 4], DrawOrder), usize>,
    /// Trees to fill again, kept from the scenes before.
    spare: Vec<BoundsTree<ScaledPixels>>,
    /// How many times [`behind`] has passed over what was painted.
    behind_passes: usize,
}

impl EdgeShadows {
    /// Forgets the scene's entries, as [`Scene::clear`] forgets its
    /// primitives.
    pub(crate) fn clear(&mut self) {
        self.clips.clear();
        self.pushed = 0;
        self.kept = 0;
        self.by_mask.clear();
        for lifted in self.lifted.drain(..) {
            if let Some(mut tree) = lifted.tree {
                tree.forget();
                self.spare.push(tree);
            }
        }
        self.behind_passes = 0;
    }

    /// How many shadows the scene kept for what they draw inside their
    /// masks.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.kept
    }

    fn add(
        &mut self,
        clip: Bounds<ScaledPixels>,
        base: DrawOrder,
        pushed: Option<(u32, usize, usize)>,
    ) -> usize {
        self.lifted.push(Lifted {
            clip,
            base,
            pushed,
            entered_at: Vec::new(),
            listed: SmallVec::new(),
            tree: None,
            behind: SmallVec::new(),
        });
        self.lifted.len() - 1
    }

    /// The entries of the clip at `level` of the clips pushed, in the band
    /// of `base`.
    fn of_clip(&mut self, level: usize, base: DrawOrder) -> usize {
        let band = band_index(base);
        let clip = &self.clips[level];
        if clip.entries[band] != NONE {
            return clip.entries[band];
        }
        let pushed = (clip.id, clip.before[band], clip.pushed_at);
        let index = self.add(clip.bounds, base, Some(pushed));
        self.clips[level].entries[band] = index;
        index
    }

    /// The entries of a clip the window did not tell of, which shows as
    /// `mask`, in the band of `base`.
    fn of_mask(&mut self, mask: Bounds<ScaledPixels>, base: DrawOrder) -> usize {
        let key = (
            [
                mask.origin.x.0.to_bits(),
                mask.origin.y.0.to_bits(),
                mask.size.width.0.to_bits(),
                mask.size.height.0.to_bits(),
            ],
            base,
        );
        if let Some(index) = self.by_mask.get(&key) {
            return *index;
        }
        let index = self.add(mask, base, None);
        self.by_mask.insert(key, index);
        index
    }

    /// Enters `bounds` at `order` among the entries at `index`, in a scene
    /// holding `operations` paint operations.
    fn enter(
        &mut self,
        index: usize,
        bounds: Bounds<ScaledPixels>,
        order: DrawOrder,
        operations: usize,
    ) {
        let lifted = &mut self.lifted[index];
        lifted.entered_at.push(operations);
        let moved = lifted.in_tree(&bounds);
        if let Some(tree) = &mut lifted.tree {
            return tree.insert_at(moved, order);
        }
        lifted.listed.push((bounds, order));
        if lifted.listed.len() > LISTED {
            let mut tree = self.spare.pop().unwrap_or_default();
            // Everything within reach of the clip, at once.
            tree.cover(&Bounds {
                origin: point(ScaledPixels(0.), ScaledPixels(0.)),
                size: size(
                    lifted.clip.size.width + REACH + REACH,
                    lifted.clip.size.height + REACH + REACH,
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

/// Tells the scene the window is painting into that it pushed `mask`, before
/// intersecting it with the masks pushed so far.
pub(crate) fn push_clip(window: &mut Window, painting: bool, mask: &ContentMask<Pixels>) {
    if !painting {
        return;
    }
    let transformation = window.current_element_transform();
    // As the masks of what is painted in it hold it (see
    // `Window::snapped_content_mask`, `Window::paint_content_mask`).
    let clip = if transformation == TransformationMatrix::unit() {
        window.cover_bounds(mask.bounds)
    } else {
        transformation.apply_to_scaled_bounds(mask.bounds.scale(window.scale_factor()))
    };
    clip_op(&mut window.next_frame.scene, ClipOp::Push(clip));
}

/// Tells the scene the window is painting into that it popped the mask it
/// pushed last.
pub(crate) fn pop_clip(window: &mut Window) {
    clip_op(&mut window.next_frame.scene, ClipOp::Pop);
}

/// Takes in a clip pushed or popped, when the window paints and when paint
/// operations are replayed.
pub(crate) fn clip_op(scene: &mut Scene, op: ClipOp) {
    clip(scene, op);
    scene.paint_operations.push(PaintOperation::FastClip(op));
}

/// Pushes or pops a clip that is no part of what the scene replays: the
/// region a layer's content is painted in, which is the content's clip
/// while it is painted and nothing once it is drawn elsewhere.
pub(crate) fn clip(scene: &mut Scene, op: ClipOp) {
    let edges = &mut scene.fast_edge_shadows;
    match op {
        ClipOp::Push(bounds) => {
            let mask = match edges.clips.last() {
                Some(outer) => outer.mask.intersect(&bounds),
                None => bounds,
            };
            edges.pushed += 1;
            edges.clips.push(Clip {
                bounds,
                mask,
                id: edges.pushed,
                entries: [NONE; 3],
                before: [
                    scene.primitive_bounds.inserted(),
                    scene.positioned_primitive_bounds.inserted(),
                    scene.top_layer_primitive_bounds.inserted(),
                ],
                pushed_at: scene.paint_operations.len(),
            });
        }
        ClipOp::Pop => {
            // A range of operations replayed on its own can pop what it did
            // not push.
            edges.clips.pop();
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

/// `bounds` grown by [`REACH`] on every side.
fn within_reach(bounds: &Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(bounds.origin.x - REACH, bounds.origin.y - REACH),
        size: size(
            bounds.size.width + REACH + REACH,
            bounds.size.height + REACH + REACH,
        ),
    }
}

/// The clips a primitive of `bounds`, painted in `mask` and kept as
/// `clipped`, lies at an edge of, innermost first: each with the bounds the
/// primitive has with that clip not there, where they are not what it has
/// with it. That is its bounds within the clips pushed inside that one and,
/// of the clip itself, within [`REACH`] of it.
///
/// The clips are those the window told of; a mask that is not their
/// intersection was pushed some other way, and stands for one clip more,
/// innermost.
fn edges(
    scene: &mut Scene,
    bounds: Bounds<ScaledPixels>,
    clipped: Bounds<ScaledPixels>,
    mask: Bounds<ScaledPixels>,
    base: DrawOrder,
) -> SmallVec<[(usize, Bounds<ScaledPixels>); 2]> {
    let mut found = SmallVec::new();
    let edges = &mut scene.fast_edge_shadows;
    let lifts = |inner: &Bounds<ScaledPixels>, clip: &Bounds<ScaledPixels>| {
        let lifted = bounds.intersect(inner).intersect(&within_reach(clip));
        let differs = match clipped.is_empty() {
            true => !lifted.is_empty(),
            false => lifted != clipped,
        };
        differs.then_some(lifted)
    };
    // What the clips inside the one being looked at leave of the bounds.
    let mut inner = bounds;
    if edges.clips.last().is_none_or(|clip| clip.mask != mask) {
        if let Some(lifted) = lifts(&inner, &mask) {
            found.push((edges.of_mask(mask, base), lifted));
        }
        inner = inner.intersect(&mask);
    }
    for level in (0..edges.clips.len()).rev() {
        if inner.is_empty() {
            break;
        }
        let clip = edges.clips[level].bounds;
        if let Some(lifted) = lifts(&inner, &clip) {
            found.push((edges.of_clip(level, base), lifted));
        }
        inner = inner.intersect(&clip);
    }
    found
}

/// The highest order among what was painted behind the clip of the entries
/// at `index`, where a primitive of `bounds`, which the clip drops, would
/// come into view: under the stretch of the clip's nearest edge it lies
/// along. Everything painted before the clip was pushed lies behind what it
/// holds: what the scene's tree held then and, of what the clips around
/// this one cut or dropped, what they had entered by then, since the clip
/// may itself lie where they cut. For a clip the window did not tell of, it
/// is what was painted outside its mask.
fn behind(scene: &mut Scene, index: usize, bounds: &Bounds<ScaledPixels>) -> DrawOrder {
    let lifted = &scene.fast_edge_shadows.lifted[index];
    let (clip, base, pushed) = (lifted.clip, lifted.base, lifted.pushed);
    let one = ScaledPixels(1.);
    // The stretch: a pixel deep inside the clip, as long as the bounds are
    // along that edge.
    let along = |from: ScaledPixels, to: ScaledPixels, low: ScaledPixels, high: ScaledPixels| {
        let from = from.max(low).min(high - one);
        (from, (to.min(high) - from).max(one))
    };
    let stretch = if bounds.top() >= clip.bottom() || bounds.bottom() <= clip.top() {
        let (left, width) = along(bounds.left(), bounds.right(), clip.left(), clip.right());
        let top = if bounds.top() >= clip.bottom() {
            clip.bottom() - one
        } else {
            clip.top()
        };
        Bounds {
            origin: point(left, top),
            size: size(width, one),
        }
    } else {
        let (top, height) = along(bounds.top(), bounds.bottom(), clip.top(), clip.bottom());
        let left = if bounds.left() >= clip.right() {
            clip.right() - one
        } else {
            clip.left()
        };
        Bounds {
            origin: point(left, top),
            size: size(one, height),
        }
    };
    if let Some((_, order)) = lifted.behind.iter().find(|(asked, _)| *asked == stretch) {
        return *order;
    }
    if scene.fast_edge_shadows.behind_passes >= BEHIND_BUDGET {
        return base;
    }
    scene.fast_edge_shadows.behind_passes += 1;
    let highest = match pushed {
        Some((id, before, operations)) => {
            let mut highest = base + tree(scene, base).max_among_first(before, &stretch);
            let edges = &scene.fast_edge_shadows;
            let band = band_index(base);
            for around in edges.clips.iter().take_while(|around| around.id != id) {
                if around.entries[band] != NONE {
                    let entries = &edges.lifted[around.entries[band]];
                    highest = highest.max(entries.max_before(operations, &stretch));
                }
            }
            highest
        }
        None => {
            let mut highest = base;
            for operation in &scene.paint_operations {
                let PaintOperation::Primitive(primitive) = operation else {
                    continue;
                };
                let order = order_of(primitive);
                let painted_in = primitive.content_mask().bounds;
                if order > highest
                    && band(order) == base
                    && !within(&painted_in, &clip)
                    && primitive
                        .bounds()
                        .intersect(&painted_in)
                        .intersects(&stretch)
                {
                    highest = order;
                }
            }
            highest
        }
    };
    scene.fast_edge_shadows.lifted[index]
        .behind
        .push((stretch, highest));
    highest
}

/// The order of a primitive a clip drops, or of a shadow kept at it, that
/// lies at the clip edges `at`: one past the highest among what it lies over
/// with each clip not there, or among what was painted behind the clip where
/// nothing of the clip's lies under it.
fn order_at(scene: &mut Scene, at: &[(usize, Bounds<ScaledPixels>)], base: DrawOrder) -> DrawOrder {
    let mut highest = base;
    for (index, lifted) in at {
        // What it lies over inside the clip lies over what is behind the
        // clip already.
        let over = match scene.fast_edge_shadows.lifted[*index].max_over(lifted) {
            0 => behind(scene, *index, lifted),
            over => over,
        };
        highest = highest.max(over);
    }
    highest + 1
}

/// Takes in a primitive whose bounds, clipped to its mask, are empty, which
/// a scene drops. A shadow that still draws inside its mask is kept, ordered
/// over what lies under its box; anything else leaves an entry of its
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
    let (base, fixed) = context(scene);
    let at = edges(scene, bounds, Bounds::default(), mask, base);
    let order = match fixed {
        // Everything in the layer is drawn at its order.
        Some(order) => order,
        None => order_at(scene, &at, base),
    };
    let operations = scene.paint_operations.len();
    for (index, lifted) in at {
        scene
            .fast_edge_shadows
            .enter(index, lifted, order, operations);
    }
    scene.fast_edge_shadows.kept += 1;
    shadow.order = order;
    scene.shadows.push(*shadow);
    scene
        .paint_operations
        .push(PaintOperation::Primitive(primitive));
}

/// Notes a primitive the scene dropped: an entry of its bounds, at each
/// clip's edge it lies at, at the order it would have had, and an operation
/// among the paint operations. One that lies at no clip's edge (further
/// than [`REACH`] from the clip that drops it, or outside two clips at once)
/// lies under nothing that shows, and leaves nothing.
pub(crate) fn dropped(scene: &mut Scene, dropped: Dropped) {
    let Dropped { bounds, mask } = dropped;
    let (base, fixed) = context(scene);
    let at = edges(scene, bounds, Bounds::default(), mask, base);
    if at.is_empty() {
        return;
    }
    let order = match fixed {
        Some(order) => order,
        None => order_at(scene, &at, base),
    };
    let operations = scene.paint_operations.len();
    for (index, lifted) in at {
        scene
            .fast_edge_shadows
            .enter(index, lifted, order, operations);
    }
    scene
        .paint_operations
        .push(PaintOperation::FastDropped(dropped));
}

/// The order of `primitive`, which the scene keeps with `bounds` (clipped to
/// its mask) at `order`: raised past what it lies over at its clips' edges,
/// where the scene's tree, which keeps clipped bounds, put it no higher.
/// That is the primitives a clip cuts or drops, and the shadows kept there,
/// that it lies over with the clip not there.
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
    if whole == bounds {
        return order;
    }
    let mask = primitive.content_mask().bounds;
    let (base, fixed) = context(scene);
    let at = edges(scene, whole, bounds, mask, base);
    let mut under = 0;
    for (index, lifted) in &at {
        under = under.max(scene.fast_edge_shadows.lifted[*index].max_over(lifted));
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
        order = order.max(base + tree.insert(bounds));
    }
    if fixed.is_some_and(|fixed| fixed != order)
        && let Some(layer) = scene.layer_stack.last_mut()
    {
        // What the layer holds from here on is drawn with the primitive.
        *layer = LayerOrder::Fixed(order);
    }
    let operations = scene.paint_operations.len();
    for (index, lifted) in at {
        scene
            .fast_edge_shadows
            .enter(index, lifted, order, operations);
    }
    order
}
