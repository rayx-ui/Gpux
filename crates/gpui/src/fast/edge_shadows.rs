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

/// A primitive a scene dropped, its bounds clipped to its mask being empty:
/// what is left of it among the paint operations, so that what is painted
/// over it at a clip's edge is ordered as with the clip not there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dropped {
    /// The primitive's whole bounds.
    pub(crate) bounds: Bounds<ScaledPixels>,
    /// What it draws, its mask aside: its bounds, or more for a shadow.
    pub(crate) drawn: Bounds<ScaledPixels>,
    /// The bounds of its content mask.
    pub(crate) mask: Bounds<ScaledPixels>,
}

impl Dropped {
    /// The entry moved by `delta`, its mask with it.
    pub(crate) fn moved(self, delta: crate::Point<ScaledPixels>) -> Dropped {
        Dropped {
            bounds: moved(self.bounds, delta),
            drawn: moved(self.drawn, delta),
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
    /// The content of a scrolling container begins.
    BeginScroll,
    /// The content of a scrolling container ends: what is painted from here
    /// on lies over all of it, the blurs of its shadows too.
    EndScroll,
}

impl ClipOp {
    /// What closes what this operation opens, if it opens something.
    pub(crate) fn closing(self) -> Option<ClipOp> {
        match self {
            ClipOp::Push(_) => Some(ClipOp::Pop),
            ClipOp::BeginScroll => Some(ClipOp::EndScroll),
            ClipOp::Pop | ClipOp::EndScroll => None,
        }
    }

    /// The operation with its clip moved by `delta`.
    pub(crate) fn moved(self, delta: crate::Point<ScaledPixels>) -> ClipOp {
        match self {
            ClipOp::Push(clip) => ClipOp::Push(moved(clip, delta)),
            other => other,
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
#[derive(Clone, Copy)]
struct Clip {
    bounds: Bounds<ScaledPixels>,
    /// The clips pushed so far, intersected: the mask of what is painted now.
    mask: Bounds<ScaledPixels>,
    /// The clip's entries in each band, in [`EdgeShadows::lifted`].
    entries: [usize; 3],
    /// How many bounds each band's tree held when the clip was pushed: what
    /// was painted before it, and lies behind what it holds.
    before: [usize; 3],
    /// How many paint operations the scene held when the clip was pushed.
    pushed_at: usize,
}

/// The content of a scrolling container the scene is painting.
#[derive(Clone, Copy)]
struct Scrolling {
    /// How many clips were pushed when the content began: those are around
    /// it, and what is painted after it is painted in them.
    clips: usize,
    /// Where the blurs of the content's shadows begin in
    /// [`EdgeShadows::blurs`].
    blurs: usize,
}

/// What a shadow painted in a scrolling container's content draws past its
/// box: what is painted after the content lies over it (see [`lay_over`]).
struct Blur {
    /// What the shadow draws, clipped to its mask.
    visible: Bounds<ScaledPixels>,
    order: DrawOrder,
    /// The edges of the clips around the content it lies at, as a primitive
    /// of its extent would.
    at: SmallVec<[Edge; 1]>,
}

/// Something closed that the next operation may open again where it was: a
/// range of paint operations replayed after another closes and reopens what
/// lay open between them, and goes on as if it had not.
#[derive(Clone, Copy)]
enum Closed {
    Clip(Clip),
    Scrolling(Scrolling),
}

/// The primitives a scene meets at one clip's edges, in one band of draw
/// orders: those the clip drops, those it cuts and the shadows kept at it,
/// by the bounds they have with the clip not there, each with its order.
struct Lifted {
    /// The clip.
    clip: Bounds<ScaledPixels>,
    /// The band's first draw order, less one.
    base: DrawOrder,
    /// How many bounds the band's tree and how many paint operations the
    /// scene held when the clip was pushed.
    pushed: (usize, usize),
    /// How many paint operations the scene held when each entry was entered,
    /// entry by entry.
    entered_at: SmallVec<[usize; 4]>,
    /// The entries while they are few.
    listed: SmallVec<[(Bounds<ScaledPixels>, DrawOrder); 4]>,
    /// The entries once they are many.
    tree: Option<Box<BoundsTree<ScaledPixels>>>,
}

/// What a stretch of a clip's edge is found by in [`EdgeShadows::behind`],
/// and a mask in [`EdgeShadows::untold_by_mask`]: its bits.
fn key_of(stretch: &Bounds<ScaledPixels>) -> [u32; 4] {
    [
        stretch.origin.x.0.to_bits(),
        stretch.origin.y.0.to_bits(),
        stretch.size.width.0.to_bits(),
        stretch.size.height.0.to_bits(),
    ]
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
    fn max_before(&mut self, operations: usize, bounds: &Bounds<ScaledPixels>) -> DrawOrder {
        let count = self
            .entered_at
            .partition_point(|entered| *entered < operations);
        let moved = self.in_tree(bounds);
        match &mut self.tree {
            Some(tree) => tree.max_behind(count, &moved).0,
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
    /// The clips the window did not tell of: masks primitives came with
    /// that are not what the clips pushed leave (see [`sight`]).
    untold: Vec<Clip>,
    /// Where each is in `untold`, by its mask.
    untold_by_mask: FxHashMap<[u32; 4], usize>,
    /// The one the last primitive with such a mask was painted in.
    untold_last: usize,
    /// The scrolling containers whose content is being painted.
    scrolling: Vec<Scrolling>,
    /// What was closed since the last primitive was painted, in the order
    /// it was closed.
    closed: Vec<Closed>,
    /// The blurs of the shadows painted in the scrolling containers open
    /// now, or closed and not yet laid under what follows.
    blurs: Vec<Blur>,
    /// How many shadows were kept for what they draw inside their masks.
    kept: usize,
    lifted: Vec<Lifted>,
    /// The highest order among what was painted behind a clip under each
    /// stretch of its edges asked about so far, by the clip's entries in
    /// `lifted`.
    behind: FxHashMap<(usize, [u32; 4]), DrawOrder>,
    /// Trees to fill again, kept from the scenes before: boxed as a
    /// [`Lifted`] holds one, which is mostly none.
    #[allow(clippy::vec_box)]
    spare: Vec<Box<BoundsTree<ScaledPixels>>>,
    /// How many bounds [`behind`] has looked at, for the tests that hold it
    /// to a few a clip.
    #[cfg(test)]
    pub(crate) behind_work: usize,
}

impl EdgeShadows {
    /// Forgets the scene's entries, as [`Scene::clear`] forgets its
    /// primitives.
    pub(crate) fn clear(&mut self) {
        self.clips.clear();
        self.untold.clear();
        self.untold_by_mask.clear();
        self.untold_last = NONE;
        self.scrolling.clear();
        self.closed.clear();
        self.blurs.clear();
        self.kept = 0;
        self.behind.clear();
        for lifted in self.lifted.drain(..) {
            if let Some(mut tree) = lifted.tree {
                tree.forget();
                self.spare.push(tree);
            }
        }
        #[cfg(test)]
        {
            self.behind_work = 0;
        }
    }

    /// How many clips the window did not tell of the scene has taken in.
    #[cfg(test)]
    pub(crate) fn untold_count(&self) -> usize {
        self.untold.len()
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
        pushed: (usize, usize),
    ) -> usize {
        self.lifted.push(Lifted {
            clip,
            base,
            pushed,
            entered_at: SmallVec::new(),
            listed: SmallVec::new(),
            tree: None,
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
        let pushed = (clip.before[band], clip.pushed_at);
        let index = self.add(clip.bounds, base, pushed);
        self.clips[level].entries[band] = index;
        index
    }

    /// The entries of the clip the window did not tell of that the
    /// primitive being taken in is painted in (see [`sight`]), in the band
    /// of `base`.
    fn of_untold(&mut self, base: DrawOrder) -> Option<usize> {
        let band = band_index(base);
        let clip = self.untold.get(self.untold_last)?;
        if clip.entries[band] != NONE {
            return Some(clip.entries[band]);
        }
        let pushed = (clip.before[band], clip.pushed_at);
        let index = self.add(clip.bounds, base, pushed);
        self.untold[self.untold_last].entries[band] = index;
        Some(index)
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

/// Tells the scene the window is painting into that the content of a
/// scrolling container begins (`begins`) or has ended.
pub(crate) fn scroll(window: &mut Window, painting: bool, begins: bool) {
    if !painting {
        return;
    }
    let op = match begins {
        true => ClipOp::BeginScroll,
        false => ClipOp::EndScroll,
    };
    clip_op(&mut window.next_frame.scene, op);
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
    match op {
        ClipOp::Push(bounds) => {
            let edges = &mut scene.fast_edge_shadows;
            let mask = match edges.clips.last() {
                Some(outer) => outer.mask.intersect(&bounds),
                None => bounds,
            };
            // A clip pushed again where it was just popped, with nothing
            // painted between, is that clip going on.
            if let Some(Closed::Clip(clip)) = edges.closed.last()
                && clip.bounds == bounds
                && clip.mask == mask
            {
                let Some(Closed::Clip(clip)) = edges.closed.pop() else {
                    unreachable!()
                };
                edges.clips.push(clip);
                return;
            }
            settle(scene);
            let clip = pushed(scene, bounds, mask);
            scene.fast_edge_shadows.clips.push(clip);
        }
        ClipOp::Pop => {
            let edges = &mut scene.fast_edge_shadows;
            // A range of operations replayed on its own can pop what it did
            // not push.
            if let Some(clip) = edges.clips.pop() {
                edges.closed.push(Closed::Clip(clip));
            }
        }
        ClipOp::BeginScroll => {
            let edges = &mut scene.fast_edge_shadows;
            // Content that ended and begins again at once, with nothing
            // painted between, is that content going on.
            if let Some(Closed::Scrolling(scrolling)) = edges.closed.last()
                && scrolling.clips == edges.clips.len()
            {
                let scrolling = *scrolling;
                edges.closed.pop();
                edges.scrolling.push(scrolling);
                return;
            }
            settle(scene);
            let edges = &mut scene.fast_edge_shadows;
            edges.scrolling.push(Scrolling {
                clips: edges.clips.len(),
                blurs: edges.blurs.len(),
            });
        }
        ClipOp::EndScroll => {
            let edges = &mut scene.fast_edge_shadows;
            if let Some(scrolling) = edges.scrolling.pop() {
                edges.closed.push(Closed::Scrolling(scrolling));
            }
        }
    }
}

/// A clip of `bounds` pushed now, which leaves `mask` of what is painted in
/// it.
fn pushed(scene: &Scene, bounds: Bounds<ScaledPixels>, mask: Bounds<ScaledPixels>) -> Clip {
    Clip {
        bounds,
        mask,
        entries: [NONE; 3],
        before: [
            scene.primitive_bounds.inserted(),
            scene.positioned_primitive_bounds.inserted(),
            scene.top_layer_primitive_bounds.inserted(),
        ],
        pushed_at: scene.paint_operations.len(),
    }
}

/// Whether `mask` is what the clips the window pushed leave.
///
/// An empty mask is: what the clips leave of a primitive's bounds is empty
/// as well, the primitive is dropped by them, and what it lies under is
/// found through the clips told, one by one. Where edges fall between device
/// pixels an empty mask is not grown to the grid (`paint::cover_bounds`)
/// while the clips told are, so the two differ by a few millionths of a
/// pixel (2.625: an empty mask of 623.99994 against 624): taken for a clip
/// the window did not tell of, it stopped `levels` at once, and what the
/// primitive dropped under went unnoticed.
fn told(scene: &Scene, mask: &Bounds<ScaledPixels>) -> bool {
    mask.is_empty()
        || scene
            .fast_edge_shadows
            .clips
            .last()
            .is_some_and(|clip| clip.mask == *mask)
}

/// Takes in the mask of a primitive the scene keeps, or keeps an entry of.
/// A mask that is not what the clips the window pushed leave was pushed some
/// other way: the window's own bounds, which everything outside a clip is
/// painted in, are one, and the region a layer's content is painted in when
/// its operations are replayed. Such a mask is one clip more, innermost,
/// pushed when a primitive first comes with it and not popped: what was
/// painted before that is behind it, and everything that comes with the
/// mask is painted in it.
///
/// `inserted`: the primitive's bounds are in the scene's tree already. They
/// are not among what the clip found before it.
#[inline]
fn sight(scene: &mut Scene, mask: &Bounds<ScaledPixels>, inserted: bool) {
    if told(scene, mask) {
        return;
    }
    let edges = &scene.fast_edge_shadows;
    if edges
        .untold
        .get(edges.untold_last)
        .is_some_and(|clip| clip.mask == *mask)
    {
        return;
    }
    sight_another(scene, mask, inserted);
}

fn sight_another(scene: &mut Scene, mask: &Bounds<ScaledPixels>, inserted: bool) {
    let key = key_of(mask);
    if let Some(index) = scene.fast_edge_shadows.untold_by_mask.get(&key) {
        scene.fast_edge_shadows.untold_last = *index;
        return;
    }
    let mut clip = pushed(scene, *mask, *mask);
    if inserted && let (base, None) = context(scene) {
        clip.before[band_index(base)] -= 1;
    }
    let edges = &mut scene.fast_edge_shadows;
    edges.untold_last = edges.untold.len();
    edges.untold_by_mask.insert(key, edges.untold_last);
    edges.untold.push(clip);
}

/// Settles what was closed since the last primitive was painted, before the
/// next that leaves a paint operation is ordered: nothing is going on where
/// it was, so the clips are done with, and what is painted from here on lies
/// over the blurs of the scrolling containers whose content has ended.
///
/// A primitive that leaves no operation settles nothing: what replays the
/// operations does not meet it.
#[inline]
pub(crate) fn settle(scene: &mut Scene) {
    if !scene.fast_edge_shadows.closed.is_empty() {
        settle_closed(scene);
    }
}

fn settle_closed(scene: &mut Scene) {
    let mut index = 0;
    while let Some(closed) = scene.fast_edge_shadows.closed.get(index).copied() {
        if let Closed::Scrolling(scrolling) = closed {
            lay_over(scene, scrolling);
        }
        index += 1;
    }
    scene.fast_edge_shadows.closed.clear();
}

/// Has what is painted from here on lie over the blurs of the shadows of
/// `scrolling`, a scrolling container whose content has ended: an entry at
/// each shadow's order over what it draws past its box, in the scene's tree
/// and at the edges of the clips around the content it lies at.
///
/// The tree holds a shadow's box. A primitive painted later over its blur
/// alone is not ordered by it, and is drawn under the blur where its order
/// came out lower. Inside the content that is how neighbours are stacked,
/// with or without a layer. What is painted after the container (a bar laid
/// over its edge, its scrollbar) lies over all of it: over its layer when
/// it has one, and so over every blur in it.
fn lay_over(scene: &mut Scene, scrolling: Scrolling) {
    let from = scrolling.blurs.min(scene.fast_edge_shadows.blurs.len());
    let blurs: Vec<Blur> = scene.fast_edge_shadows.blurs.drain(from..).collect();
    let operations = scene.paint_operations.len();
    for blur in blurs {
        let base = band(blur.order);
        if !blur.visible.is_empty() {
            tree(scene, base).insert_at(blur.visible, blur.order - base);
        }
        for edge in blur.at {
            scene
                .fast_edge_shadows
                .enter(edge.entries, edge.lifted, blur.order, operations);
        }
    }
}

/// Notes what a shadow of `bounds`, painted in `mask` at `order` in the
/// content of a scrolling container, draws past its box (`drawn`), for
/// [`lay_over`].
fn note_blur(
    scene: &mut Scene,
    bounds: Bounds<ScaledPixels>,
    drawn: Bounds<ScaledPixels>,
    mask: Bounds<ScaledPixels>,
    order: DrawOrder,
) {
    let Some(scrolling) = scene.fast_edge_shadows.scrolling.last().copied() else {
        return;
    };
    if drawn == bounds {
        return;
    }
    let visible = drawn.intersect(&mask);
    // At the clips around the content: inside it nothing is ordered by a
    // blur.
    let levels = levels(scene, drawn, mask, Some(scrolling.clips));
    let at = edges(scene, &levels, band(order));
    if visible.is_empty() && at.is_empty() {
        return;
    }
    scene.fast_edge_shadows.blurs.push(Blur {
        visible,
        order,
        at: at.into_iter().collect(),
    });
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

/// A clip's edge a primitive lies at.
#[derive(Clone, Copy)]
struct Edge {
    /// The clip's entries, in [`EdgeShadows::lifted`].
    entries: usize,
    /// How many clips the window pushed lie around the clip.
    around: usize,
    /// The bounds the primitive has with the clip not there.
    lifted: Bounds<ScaledPixels>,
}

/// The clips a primitive of `bounds`, painted in `mask`, lies at an edge
/// of, innermost first: each with the bounds the primitive has with that
/// clip not there. That is its bounds within the clips pushed inside that
/// one and, of the clip itself, within [`REACH`] of it. A clip that leaves
/// those bounds whole is none of them: lifting it shows nothing more. And a
/// primitive some clip drops from further off than its reach lies at no
/// clip's edge, those inside that one neither: nothing of it shows with any
/// of them lifted.
///
/// Each is given as the clip's place among those the window pushed, or none
/// for a mask the window did not tell of, which is one clip more, innermost
/// (see [`sight`]).
///
/// With `outermost`, only the first so many clips the window pushed are
/// looked at, the others still cutting the bounds: the clips around a
/// scrolling container's content.
fn levels(
    scene: &Scene,
    bounds: Bounds<ScaledPixels>,
    mask: Bounds<ScaledPixels>,
    outermost: Option<usize>,
) -> SmallVec<[(Option<usize>, Bounds<ScaledPixels>); 2]> {
    let mut found = SmallVec::new();
    let clips = &scene.fast_edge_shadows.clips;
    // What the clips inside the one being looked at leave of the bounds.
    let mut inner = bounds;
    let untold = (!told(scene, &mask)).then_some((None, mask));
    let pushed = (0..clips.len())
        .rev()
        .map(|level| (Some(level), clips[level].bounds));
    for (level, clip) in untold.into_iter().chain(pushed) {
        if inner.is_empty() {
            break;
        }
        if !within(&inner, &clip) {
            let lifted = inner.intersect(&within_reach(&clip));
            if lifted.is_empty() {
                // Dropped, out of reach.
                found.clear();
                break;
            }
            let looked_at = match (level, outermost) {
                (_, None) => true,
                (Some(level), Some(outermost)) => level < outermost,
                (None, Some(_)) => false,
            };
            if looked_at {
                found.push((level, lifted));
            }
        }
        inner = inner.intersect(&clip);
    }
    found
}

/// The clips' edges a primitive lies at (see [`levels`]), with the entries
/// kept at each.
fn edges(
    scene: &mut Scene,
    levels: &[(Option<usize>, Bounds<ScaledPixels>)],
    base: DrawOrder,
) -> SmallVec<[Edge; 2]> {
    let edges = &mut scene.fast_edge_shadows;
    let all = edges.clips.len();
    levels
        .iter()
        .filter_map(|(level, lifted)| {
            Some(match *level {
                Some(level) => Edge {
                    entries: edges.of_clip(level, base),
                    around: level,
                    lifted: *lifted,
                },
                None => Edge {
                    entries: edges.of_untold(base)?,
                    around: all,
                    lifted: *lifted,
                },
            })
        })
        .collect()
}

/// The highest order among what was painted behind the clip at `edge`,
/// where a primitive the clip drops would come into view: under the stretch
/// of the clip's nearest edge its bounds lie along. Everything painted
/// before the clip was pushed lies behind what it holds.
///
/// That is what the scene's tree held then under the stretch
/// ([`BoundsTree::max_behind`], which finds it without passing over all that
/// was painted). The tree holds what shows. Where the stretch lies outside
/// what the clips around leave, what those clips had dropped or cut there
/// by then counts too. No count limits the search: an answer that gave out
/// would depend on how much was painted before, which is how far the
/// content is scrolled.
fn behind(scene: &mut Scene, edge: &Edge) -> DrawOrder {
    let lifted = &scene.fast_edge_shadows.lifted[edge.entries];
    let (clip, base, (before, operations)) = (lifted.clip, lifted.base, lifted.pushed);
    let bounds = &edge.lifted;
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
    let key = (edge.entries, key_of(&stretch));
    if let Some(order) = scene.fast_edge_shadows.behind.get(&key) {
        return *order;
    }
    let (highest, _looked) = tree(scene, base).max_behind(before, &stretch);
    let mut highest = base + highest;
    let edges = &mut scene.fast_edge_shadows;
    #[cfg(test)]
    {
        edges.behind_work += _looked;
    }
    // What a clip around entered inside what the clips around leave is in
    // the tree, at the order it was entered at.
    let around = &edges.clips[..edge.around.min(edges.clips.len())];
    if around
        .last()
        .is_some_and(|clip| !within(&stretch, &clip.mask))
    {
        let band_at = band_index(base);
        for around in around {
            if around.entries[band_at] != NONE {
                let entries = &mut edges.lifted[around.entries[band_at]];
                highest = highest.max(entries.max_before(operations, &stretch));
            }
        }
    }
    edges.behind.insert(key, highest);
    highest
}

/// The order of a primitive a clip drops, or of a shadow kept at it, that
/// lies at the clip edges `at`, innermost first: one past the highest among
/// what it lies over with each clip not there. With nothing under it at any
/// of them, one past what was painted behind the outermost, the clip that
/// drops it, where it would come into view.
fn order_at(scene: &mut Scene, at: &[Edge], base: DrawOrder) -> DrawOrder {
    let mut highest = 0;
    for edge in at {
        let over = scene.fast_edge_shadows.lifted[edge.entries].max_over(&edge.lifted);
        highest = highest.max(over);
    }
    if highest == 0 {
        highest = match at.last() {
            Some(edge) => behind(scene, edge),
            None => base,
        };
    }
    highest.max(base) + 1
}

/// Takes in a primitive whose bounds, clipped to its mask, are empty, which
/// a scene drops. A shadow that still draws inside its mask is kept, ordered
/// over what lies under its box; anything else leaves an entry of its
/// bounds, at the order it would have had.
pub(crate) fn keep(scene: &mut Scene, mut primitive: Primitive) {
    let bounds = primitive.bounds();
    let mask = primitive.content_mask().bounds;
    let drawn = match &primitive {
        Primitive::Shadow(_) => drawn_bounds(&primitive),
        _ => bounds,
    };
    if drawn.intersect(&mask).is_empty() {
        return dropped(
            scene,
            Dropped {
                bounds,
                drawn,
                mask,
            },
        );
    }
    settle(scene);
    sight(scene, &mask, false);
    let (base, fixed) = context(scene);
    let levels = levels(scene, bounds, mask, None);
    let at = edges(scene, &levels, base);
    let order = match fixed {
        // Everything in the layer is drawn at its order.
        Some(order) => order,
        None => order_at(scene, &at, base),
    };
    let operations = scene.paint_operations.len();
    for edge in at {
        scene
            .fast_edge_shadows
            .enter(edge.entries, edge.lifted, order, operations);
    }
    scene.fast_edge_shadows.kept += 1;
    note_blur(scene, bounds, drawn, mask, order);
    let Primitive::Shadow(shadow) = &mut primitive else {
        unreachable!("only a shadow draws past its bounds")
    };
    shadow.order = order;
    scene.shadows.push(*shadow);
    scene
        .paint_operations
        .push(PaintOperation::Primitive(primitive));
}

/// Notes a primitive the scene dropped: an entry of its bounds, at each
/// clip's edge it lies at, at the order it would have had, and an operation
/// among the paint operations. One that lies at no clip's edge (further
/// than [`REACH`] from the clip that drops it) lies under nothing that
/// shows, and leaves nothing.
pub(crate) fn dropped(scene: &mut Scene, dropped: Dropped) {
    let Dropped {
        bounds,
        drawn,
        mask,
    } = dropped;
    // Only a primitive that leaves an operation changes what the scene
    // holds: what replays the operations has to end up where this did.
    let levels = levels(scene, bounds, mask, None);
    if levels.is_empty() {
        return;
    }
    settle(scene);
    sight(scene, &mask, false);
    let (base, fixed) = context(scene);
    let at = edges(scene, &levels, base);
    let order = match fixed {
        Some(order) => order,
        None => order_at(scene, &at, base),
    };
    let operations = scene.paint_operations.len();
    for edge in at {
        scene
            .fast_edge_shadows
            .enter(edge.entries, edge.lifted, order, operations);
    }
    // A shadow's blur may show with a clip around its container not there.
    note_blur(scene, bounds, drawn, mask, order);
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
    let mask = primitive.content_mask().bounds;
    sight(scene, &mask, true);
    let shadow =
        matches!(primitive, Primitive::Shadow(_)) && !scene.fast_edge_shadows.scrolling.is_empty();
    // A primitive wholly inside its mask meets nothing outside it.
    if whole == bounds {
        if shadow {
            note_blur(scene, whole, drawn_bounds(primitive), mask, order);
        }
        return order;
    }
    let (base, fixed) = context(scene);
    let levels = levels(scene, whole, mask, None);
    let at = edges(scene, &levels, base);
    let mut under = 0;
    for edge in &at {
        under = under.max(scene.fast_edge_shadows.lifted[edge.entries].max_over(&edge.lifted));
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
    for edge in at {
        scene
            .fast_edge_shadows
            .enter(edge.entries, edge.lifted, order, operations);
    }
    if shadow {
        note_blur(scene, whole, drawn_bounds(primitive), mask, order);
    }
    order
}
