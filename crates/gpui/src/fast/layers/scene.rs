//! What the core hands a renderer about scroll layers: the tiles it
//! composites, as polychrome sprites whose texture id lies in a range no
//! atlas allocates, and the content those tiles are rasterized from.
//!
//! Adapted from gpui-fast's `fast/layers/scene.rs`. Every gpux primitive
//! carries a transformation, so moving one into content or tile space moves
//! its transformation with it; gpux's own primitives (visual effects, HTML
//! islands and elements, screen-effect regions) and its positioned and top
//! layers never reach a layer's content (the container bypasses), and are
//! moved like the rest only so the functions here stay total.

use crate::{
    AtlasTextureId, AtlasTextureKind, Bounds, PaintColor, Point, ScaledPixels, Scene, TileId,
    point,
    scene::{PaintOperation, Primitive, TransformationMatrix},
    size,
};
use std::rc::Rc;

/// A live scroll layer, stable while it lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerKey(pub u32);

/// A tile of a layer's content space, in units of the tile size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileCoord {
    /// The tile's column; tile `x` starts at `x * tile_size` device pixels.
    pub x: i32,
    /// The tile's row; tile `y` starts at `y * tile_size` device pixels.
    pub y: i32,
}

/// The first texture index of layer tiles; atlases allocate indices from 0.
pub const LAYER_TILE_TEXTURE_BASE: u32 = 0xF000_0000;
pub(crate) const LAYER_KEY_LIMIT: u32 = 0x0100_0000;

/// What [`crate::PolychromeSprite::pad`] holds on the sprite of a layer tile:
/// the tile's texture holds premultiplied colour, as everything drawn into a
/// render target does, where an atlas texture holds straight colour. `0` on
/// every other sprite.
pub const LAYER_TILE_PREMULTIPLIED: u32 = 1;

/// The tile textures a window's layers may use, in bytes: 64 MB on desktop,
/// 32 MB in the browser and on mobile, where GPU memory is tighter and
/// shared with the page or the system. The core keeps a layer only while
/// the tiles over its viewport fit in it, and a renderer keeps no more.
pub const LAYER_TILE_BUDGET_BYTES: u64 = if cfg!(any(
    target_family = "wasm",
    target_os = "android",
    target_os = "ios"
)) {
    32 << 20
} else {
    64 << 20
};
const TILE_COORD_BIAS: i32 = 2048;

/// The texture id the tiles of `layer` are composited with.
pub fn layer_tile_texture_id(layer: LayerKey) -> AtlasTextureId {
    debug_assert!(layer.0 < LAYER_KEY_LIMIT);
    AtlasTextureId {
        index: LAYER_TILE_TEXTURE_BASE + layer.0,
        kind: AtlasTextureKind::Polychrome,
    }
}

/// The tile id a composited `tile` carries.
pub fn layer_tile_id(tile: TileCoord) -> TileId {
    debug_assert!((-TILE_COORD_BIAS..TILE_COORD_BIAS).contains(&tile.x));
    debug_assert!((-TILE_COORD_BIAS..TILE_COORD_BIAS).contains(&tile.y));
    let x = (tile.x + TILE_COORD_BIAS) as u32;
    let y = (tile.y + TILE_COORD_BIAS) as u32;
    TileId((x << 16) | y)
}

/// The layer and tile a composited sprite stands for, if it is one.
pub fn decode_layer_tile(texture: AtlasTextureId, tile: TileId) -> Option<(LayerKey, TileCoord)> {
    if texture.kind != AtlasTextureKind::Polychrome
        || texture.index < LAYER_TILE_TEXTURE_BASE
        || texture.index - LAYER_TILE_TEXTURE_BASE >= LAYER_KEY_LIMIT
    {
        return None;
    }
    let layer = LayerKey(texture.index - LAYER_TILE_TEXTURE_BASE);
    let x = (tile.0 >> 16) as i32 - TILE_COORD_BIAS;
    let y = (tile.0 & 0xFFFF) as i32 - TILE_COORD_BIAS;
    Some((layer, TileCoord { x, y }))
}

/// The scroll layers a frame composites, for the renderer to rasterize their
/// tiles from.
#[derive(Default)]
pub struct SceneLayers {
    /// One per layer the frame composites.
    pub frames: Vec<LayerFrame>,
}

impl SceneLayers {
    /// Forgets the frame's layers, as [`Scene::clear`] forgets its primitives.
    pub fn clear(&mut self) {
        self.frames.clear();
    }
}

/// A layer the frame composites: the content its tiles are rasterized from.
#[derive(Clone)]
pub struct LayerFrame {
    /// The layer, stable while it lives.
    pub key: LayerKey,
    /// Bumps whenever `content` is replaced.
    pub generation: u64,
    /// The opaque colour every tile is cleared with before its content is
    /// drawn, premultiplied encoded sRGB as every gpux paint colour is.
    pub background: PaintColor,
    /// The side of a tile, in device pixels.
    pub tile_size: u32,
    /// The painted content in content space.
    pub content: LayerContent,
    /// The tiles whose content changed in this generation.
    pub dirty_tiles: Vec<TileCoord>,
}

impl LayerFrame {
    /// The rectangle `tile` covers in content space.
    pub fn tile_bounds(&self, tile: TileCoord) -> Bounds<ScaledPixels> {
        let side = self.tile_size as f32;
        Bounds {
            origin: point(
                ScaledPixels(tile.x as f32 * side),
                ScaledPixels(tile.y as f32 * side),
            ),
            size: size(ScaledPixels(side), ScaledPixels(side)),
        }
    }

    /// The primitives of `content` that are visible over `tile`, translated
    /// into the tile's space (its top-left corner at the origin), in drawing
    /// order, finished and ready to batch.
    ///
    /// Each is drawn at the order it has in the content as a whole
    /// ([`LayerContent::orders`]), not at one worked out among the
    /// primitives over the tile: a shadow's blur falls on neighbours its box
    /// does not overlap, over those of a lower order and under the others,
    /// and an order worked out per tile changes at the tile's edge when what
    /// lies under the shadow's box does not reach into the tile.
    pub fn tile_scene(&self, tile: TileCoord) -> Scene {
        let bounds = self.tile_bounds(tile);
        let delta = point(
            ScaledPixels(-bounds.origin.x.0),
            ScaledPixels(-bounds.origin.y.0),
        );
        let orders = self.content.orders();
        let mut scene = Scene::default();
        for (part_index, part) in self.content.parts.iter().enumerate() {
            let operations = &part.scene.paint_operations;
            if part.bounds.is_some_and(|part| !part.intersects(&bounds)) {
                continue;
            }
            let orders = orders.and_then(|orders| orders[part_index].0.as_deref());
            for (index, operation) in operations.iter().enumerate() {
                let PaintOperation::Primitive(primitive) = operation else {
                    continue;
                };
                if visible_bounds(primitive).intersects(&bounds) {
                    let order = match orders {
                        Some(orders) => orders[index],
                        None => crate::fast::edge_shadows::order_of(primitive),
                    };
                    crate::fast::edge_shadows::insert_at(
                        &mut scene,
                        translate_primitive(primitive, delta),
                        order,
                    );
                }
            }
        }
        scene.finish();
        scene
    }
}

/// A layer's content, in content space: one or more parts drawn one after
/// another. A scrolling `div`'s content is one part, a finished scene; a
/// virtual list's is one part per row, so that a frame adding or changing
/// rows hands the renderer the rows it kept as they were, without copying
/// them.
#[derive(Clone, Default)]
pub struct LayerContent {
    parts: Rc<[LayerPart]>,
    /// The orders of each part's paint operations in the content, and a
    /// hash of them, once they were asked for. See
    /// [`LayerContent::orders`].
    orders: Rc<std::cell::OnceCell<Vec<PartOrders>>>,
}

/// The order of each of a part's paint operations among the parts it is
/// ordered with, and a hash of them; none, and no hash, where those are the
/// orders the operations hold, as they are for a part that meets no other.
type PartOrders = (Option<Rc<[crate::DrawOrder]>>, u64);

/// A part of a layer's content.
#[derive(Clone)]
pub(crate) struct LayerPart {
    /// Where the part's primitives can draw, if known: a tile it misses
    /// draws nothing of it.
    pub(crate) bounds: Option<Bounds<ScaledPixels>>,
    /// The part's paint operations, in drawing order. A row's scene holds
    /// nothing else.
    pub(crate) scene: Rc<Scene>,
    /// What tells the part from every other there is or was.
    id: u64,
    /// Where anything of the part can meet anything of another part so
    /// that one is ordered by the other (see [`ordering_extent`]).
    extent: Bounds<ScaledPixels>,
    /// The scene the part was painted in with others, and its place among
    /// them: its operations hold the orders that scene gave them.
    painted: Option<(Rc<PaintPass>, u32)>,
    /// The part's orders as they were last worked out, kept with the part
    /// and shared by its copies: a row keeps them from one content to the
    /// next while the rows it is ordered after stay.
    stacked: Rc<std::cell::RefCell<Option<Stacked>>>,
}

/// A part's orders, and what they were worked out after.
struct Stacked {
    /// A hash of the parts ordered before this one, in order: those whose
    /// orders its own follow from.
    after: u64,
    orders: PartOrders,
}

thread_local! {
    static NEXT_PART: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    /// How many paint operations [`LayerContent::orders`] has ordered, for
    /// the tests that hold a row scrolled in to its own cost.
    #[cfg(test)]
    pub(crate) static ORDERED_OPERATIONS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
    /// How many times [`LayerContent::orders`] has compared a part's extent
    /// with others', for the tests that hold it to a few a part.
    #[cfg(test)]
    pub(crate) static COMPARED_EXTENTS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

impl LayerPart {
    pub(crate) fn new(bounds: Option<Bounds<ScaledPixels>>, scene: Rc<Scene>) -> Self {
        let id = NEXT_PART.with(|next| next.replace(next.get() + 1));
        LayerPart {
            bounds,
            extent: ordering_extent(&scene.paint_operations),
            scene,
            id,
            painted: None,
            stacked: Rc::default(),
        }
    }

    /// A part whose operations were painted in `pass`, after the parts it
    /// has given so far, and hold the orders they were given there.
    pub(crate) fn painted(
        bounds: Option<Bounds<ScaledPixels>>,
        scene: Rc<Scene>,
        pass: &Rc<PaintPass>,
    ) -> Self {
        let part = Self::new(bounds, scene);
        let mut made = pass.made.borrow_mut();
        let place = made.len() as u32;
        made.push(part.extent);
        LayerPart {
            painted: Some((pass.clone(), place)),
            ..part
        }
    }
}

/// The rows a list painted into one scene, one after another. That scene
/// ordered them as painted one after another, which is what
/// [`LayerContent::orders`] wants of them: rows of one pass that meet no
/// other row keep the orders they were painted at, without being painted
/// again to find them out. A `list` paints the rows it shows every frame.
#[derive(Default)]
pub(crate) struct PaintPass {
    /// The extents of the parts the pass has given, in the order they were
    /// painted. A content need not hold them all: a part that meets one the
    /// content does not hold was ordered by something that is not there.
    made: std::cell::RefCell<Vec<Bounds<ScaledPixels>>>,
    /// The extents of what was painted in the scene and became no part.
    apart: std::cell::RefCell<Vec<Bounds<ScaledPixels>>>,
}

impl PaintPass {
    /// Notes `operations`, painted in the pass, which become no part.
    /// `moved`: how far the pass's parts were moved from where they were
    /// painted and these were not, since a part's extent is where the part
    /// stands in the content.
    pub(crate) fn leave_out(&self, operations: &[PaintOperation], moved: Point<ScaledPixels>) {
        if !operations.is_empty() {
            self.apart
                .borrow_mut()
                .push(translate_bounds(ordering_extent(operations), moved));
        }
    }
}

/// Where the paint operations of `scene` reach as far as draw orders go:
/// two scenes whose extents lie clear of one another are ordered, painted
/// one after the other, each as it is ordered alone.
///
/// An order follows from the bounds a primitive meets: its own, a clip's or
/// a layer's, and those of a primitive a clip dropped (see
/// `crate::fast::edge_shadows`). A blur orders nothing but what follows the
/// scrolling container its shadow is painted in, where it shows, which is
/// inside that container's clip.
fn ordering_extent(operations: &[PaintOperation]) -> Bounds<ScaledPixels> {
    use crate::fast::edge_shadows::ClipOp;
    let mut extent: Option<Bounds<ScaledPixels>> = None;
    let mut reach = |bounds: Bounds<ScaledPixels>| {
        extent = Some(match extent {
            Some(extent) => extent.union(&bounds),
            None => bounds,
        });
    };
    for operation in operations {
        match operation {
            PaintOperation::Primitive(primitive) => reach(primitive.bounds()),
            PaintOperation::FastDropped(dropped) => reach(dropped.bounds),
            PaintOperation::FastClip(ClipOp::Push(bounds))
            | PaintOperation::StartLayer(bounds)
            | PaintOperation::StartPositionedLayer(bounds)
            | PaintOperation::StartTopLayer(bounds) => reach(*bounds),
            PaintOperation::FastClip(_) | PaintOperation::EndLayer => {}
        }
    }
    extent.unwrap_or_default()
}

impl From<Scene> for LayerContent {
    fn from(scene: Scene) -> Self {
        Self::from(Rc::new(scene))
    }
}

impl From<Rc<Scene>> for LayerContent {
    fn from(scene: Rc<Scene>) -> Self {
        LayerContent {
            parts: Rc::new([LayerPart::new(None, scene)]),
            orders: Rc::default(),
        }
    }
}

impl LayerContent {
    /// Content made of `parts`, drawn in order.
    pub(crate) fn from_parts(parts: impl IntoIterator<Item = LayerPart>) -> Self {
        LayerContent {
            parts: parts.into_iter().collect(),
            orders: Rc::default(),
        }
    }

    /// The draw order of each paint operation of every part, part by part,
    /// in the content as a whole, with a hash of each part's; `None` for
    /// content of one part, whose scene holds them.
    ///
    /// A part is a scene of its own, a list's row, ordered when it was
    /// painted among whatever was painted with it. What the rows are to one
    /// another is worked out here: the order each operation gets when the
    /// parts are painted one after another.
    ///
    /// Parts that meet nothing of one another are ordered each as it is
    /// alone, so only the parts whose extents meet are painted together
    /// ([`ordering_extent`]), and a part keeps its orders while the parts
    /// it was ordered after are the same. A row scrolled into a list costs
    /// its own ordering, and that of the rows it reaches over or under, not
    /// the content's: ordering the whole content for every row cost more
    /// than painting the list without a layer.
    pub(crate) fn orders(&self) -> Option<&[PartOrders]> {
        if self.parts.len() <= 1 {
            return None;
        }
        Some(self.orders.get_or_init(|| stack(&self.parts)))
    }

    /// A hash of the orders each part's paint operations have in the
    /// content as a whole, part by part: what a part draws does not change
    /// when another comes or goes, but where its primitives are stacked can.
    pub(crate) fn part_order_hashes(&self) -> Vec<u64> {
        match self.orders() {
            Some(orders) => orders.iter().map(|(_, hash)| *hash).collect(),
            None => vec![0; self.parts.len()],
        }
    }

    /// The content's scene, when it is one part, as a `div`'s is.
    pub fn scene(&self) -> Option<&Scene> {
        match &*self.parts {
            [part] => Some(&part.scene),
            _ => None,
        }
    }

    /// [`LayerContent::orders`], spelled out for every part.
    #[cfg(test)]
    pub(crate) fn orders_in_full(&self) -> Option<Vec<Vec<crate::DrawOrder>>> {
        let orders = self.orders()?;
        Some(
            self.parts
                .iter()
                .zip(orders)
                .map(|(part, (orders, _))| match orders {
                    Some(orders) => orders.to_vec(),
                    None => orders_of(&part.scene.paint_operations).collect(),
                })
                .collect(),
        )
    }

    /// The content's parts, in order.
    #[cfg(test)]
    pub(crate) fn parts(&self) -> &[LayerPart] {
        &self.parts
    }

    /// The scenes of the content's parts, in order.
    #[cfg(test)]
    pub(crate) fn part_scenes(&self) -> impl Iterator<Item = &Rc<Scene>> {
        self.parts.iter().map(|part| &part.scene)
    }

    /// Whether `self` and `other` are the same content, shared.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.parts, &other.parts)
    }

    /// Every paint operation of the content, in drawing order.
    pub(crate) fn operations(&self) -> impl Iterator<Item = &PaintOperation> {
        self.parts
            .iter()
            .flat_map(|part| part.scene.paint_operations.iter())
    }
}

/// The orders of `parts`, part by part (see [`LayerContent::orders`]).
fn stack(parts: &[LayerPart]) -> Vec<PartOrders> {
    if apart_in_order(parts) {
        // Each part is a set of its own, as a list's rows mostly are.
        for (index, part) in parts.iter().enumerate() {
            settle(parts, std::slice::from_ref(&index), &part.extent);
        }
    } else {
        let (gathered, sets) = gather(parts);
        for (members, extent) in &sets {
            settle(parts, &gathered[members.clone()], extent);
        }
    }
    parts
        .iter()
        .map(|part| {
            let stacked = part.stacked.borrow();
            let (orders, hash) = &stacked.as_ref().expect("every part was ordered").orders;
            (orders.clone(), *hash)
        })
        .collect()
}

/// Sees that the parts at `members` of `parts`, a set ordered together
/// whose extents `extent` holds, each hold their orders in it: those they
/// held, if the parts before each in the set are the parts they were
/// ordered after, or orders found now.
fn settle(parts: &[LayerPart], members: &[usize], extent: &Bounds<ScaledPixels>) {
    use std::hash::Hasher as _;
    // Each part's orders follow from the parts before it in its set.
    let mut after = collections::FxHasher::default();
    let mut kept = true;
    for index in members {
        let part = &parts[*index];
        kept &= part
            .stacked
            .borrow()
            .as_ref()
            .is_some_and(|stacked| stacked.after == after.finish());
        after.write_u64(part.id);
    }
    if !kept && !keep_painted(parts, members, extent) {
        order_together(parts, members);
    }
}

/// Whether `parts` stand one after another down the content, or across it,
/// each beginning no earlier than the one before it ends: none meets
/// another, then, and nothing has to be put in order to see it.
fn apart_in_order(parts: &[LayerPart]) -> bool {
    #[cfg(test)]
    COMPARED_EXTENTS.with(|compared| compared.set(compared.get() + parts.len()));
    let in_order = |begins: fn(&Bounds<ScaledPixels>) -> ScaledPixels,
                    ends: fn(&Bounds<ScaledPixels>) -> ScaledPixels| {
        parts
            .windows(2)
            .all(|pair| begins(&pair[1].extent) >= ends(&pair[0].extent))
    };
    in_order(|extent| extent.top(), |extent| extent.bottom())
        || in_order(|extent| extent.left(), |extent| extent.right())
}

/// The sets of parts that are ordered together: the places of `parts`, set
/// by set and in the content's order within each, and for each set its
/// stretch of them with the box around its parts' extents. Parts of
/// different sets have extents that lie clear of one another.
///
/// A list's rows come one after another along the axis it scrolls on, so
/// the parts are first taken in runs along an axis: put in order of where
/// they begin on it, a part that begins no earlier than everything before it
/// ends meets none of them. Only the parts of one run, which are few, are
/// compared with one another. Comparing every row with every set made a
/// frame of a list of thirteen hundred rows cost twice what it had.
fn gather(
    parts: &[LayerPart],
) -> (
    Vec<usize>,
    Vec<(std::ops::Range<usize>, Bounds<ScaledPixels>)>,
) {
    let runs = |down: bool| {
        let begins = |index: usize| match down {
            true => parts[index].extent.top().0,
            false => parts[index].extent.left().0,
        };
        let ends = |index: usize| match down {
            true => parts[index].extent.bottom().0,
            false => parts[index].extent.right().0,
        };
        let mut order: Vec<usize> = (0..parts.len()).collect();
        order.sort_by(|a, b| begins(*a).total_cmp(&begins(*b)));
        let mut stops = Vec::new();
        let mut reach = f32::NEG_INFINITY;
        for (at, index) in order.iter().enumerate() {
            if at > 0 && begins(*index) >= reach {
                stops.push(at);
                reach = f32::NEG_INFINITY;
            }
            reach = reach.max(ends(*index));
        }
        stops.push(parts.len());
        (order, stops)
    };
    let (mut order, mut stops) = runs(true);
    if stops.len() * 2 < parts.len() {
        // Rows laid side by side share the lines they lie on.
        let across = runs(false);
        if across.1.len() > stops.len() {
            (order, stops) = across;
        }
    }
    #[cfg(test)]
    COMPARED_EXTENTS.with(|compared| compared.set(compared.get() + parts.len()));

    let mut gathered = Vec::with_capacity(parts.len());
    let mut sets = Vec::with_capacity(stops.len());
    let mut start = 0;
    for stop in stops {
        let run = &mut order[start..stop];
        start = stop;
        if let [index] = run {
            gathered.push(*index);
            sets.push((gathered.len() - 1..gathered.len(), parts[*index].extent));
            continue;
        }
        // The parts of a run that meet, by the box around those that met so
        // far: a part whose extent lies clear of that box meets none of
        // them.
        run.sort_unstable();
        let mut met: Vec<(Vec<usize>, Bounds<ScaledPixels>)> = Vec::new();
        for index in run.iter() {
            let extent = parts[*index].extent;
            let mut joined: Option<usize> = None;
            let mut at = 0;
            while at < met.len() {
                #[cfg(test)]
                COMPARED_EXTENTS.with(|compared| compared.set(compared.get() + 1));
                if !met[at].1.intersects(&extent) {
                    at += 1;
                    continue;
                }
                match joined {
                    None => {
                        met[at].0.push(*index);
                        met[at].1 = met[at].1.union(&extent);
                        joined = Some(at);
                        at += 1;
                    }
                    Some(first) => {
                        // The part joins two sets into one.
                        let (members, extent) = met.remove(at);
                        met[first].0.extend(members);
                        met[first].1 = met[first].1.union(&extent);
                    }
                }
            }
            if joined.is_none() {
                met.push((vec![*index], extent));
            }
        }
        for (mut members, extent) in met {
            members.sort_unstable();
            let first = gathered.len();
            gathered.extend(members);
            sets.push((first..gathered.len(), extent));
        }
    }
    (gathered, sets)
}

/// The order of each paint operation in `operations`: a primitive's, and
/// none for the rest.
fn orders_of(operations: &[PaintOperation]) -> impl Iterator<Item = crate::DrawOrder> + '_ {
    operations.iter().map(|operation| match operation {
        PaintOperation::Primitive(primitive) => crate::fast::edge_shadows::order_of(primitive),
        _ => 0,
    })
}

/// Keeps `orders` with each of the parts at `members` of `parts`, a set
/// ordered together, in turn: none for a part ordered as its operations
/// are.
fn keep_orders(
    parts: &[LayerPart],
    members: &[usize],
    orders: impl IntoIterator<Item = Option<Rc<[crate::DrawOrder]>>>,
) {
    use std::hash::Hasher as _;
    let mut after = collections::FxHasher::default();
    for (index, orders) in members.iter().zip(orders) {
        let part = &parts[*index];
        let hash = match &orders {
            Some(orders) => {
                let mut hasher = collections::FxHasher::default();
                for order in orders.iter() {
                    hasher.write_u32(*order);
                }
                // Never that of a part ordered as its operations are.
                hasher.finish() | 1
            }
            None => 0,
        };
        *part.stacked.borrow_mut() = Some(Stacked {
            after: after.finish(),
            orders: (orders, hash),
        });
        after.write_u64(part.id);
    }
}

/// If the parts at `members` of `parts`, a set whose extents `extent` holds,
/// were painted one after another in one scene, in this order, and met
/// nothing else painted there, keeps with each the orders it was painted
/// at: they are the orders [`order_together`] would find. Says whether it
/// did.
fn keep_painted(parts: &[LayerPart], members: &[usize], extent: &Bounds<ScaledPixels>) -> bool {
    let Some((pass, _)) = &parts[members[0]].painted else {
        return false;
    };
    let mut last = None;
    for index in members {
        match &parts[*index].painted {
            Some((other, place))
                if Rc::ptr_eq(pass, other) && last.is_none_or(|last| last < *place) =>
            {
                last = Some(*place)
            }
            _ => return false,
        }
    }
    if pass
        .apart
        .borrow()
        .iter()
        .any(|apart| apart.intersects(extent))
    {
        return false;
    }
    // Nor anything painted there that the content does not hold, or holds
    // elsewhere.
    let mut places = members
        .iter()
        .filter_map(|index| Some(parts[*index].painted.as_ref()?.1))
        .peekable();
    for (place, made) in pass.made.borrow().iter().enumerate() {
        if places.peek() == Some(&(place as u32)) {
            places.next();
        } else if made.intersects(extent) {
            return false;
        }
    }
    keep_orders(parts, members, members.iter().map(|_| None));
    true
}

/// Orders the parts at `members` of `parts`, painted one after another, and
/// keeps each one's orders with it.
fn order_together(parts: &[LayerPart], members: &[usize]) {
    #[cfg(test)]
    ORDERED_OPERATIONS.with(|ordered| {
        let operations: usize = members
            .iter()
            .map(|index| parts[*index].scene.paint_operations.len())
            .sum();
        ordered.set(ordered.get() + operations)
    });
    keep_orders(parts, members, ordered_together(parts, members));
}

/// The orders of the parts at `members` of `parts` painted one after
/// another, part by part.
///
/// They are painted near the origin, wherever they stand in the content:
/// where a primitive is stacked does not change with where the content
/// stands, and a scene's trees keep everything further than their grids
/// span, some sixteen thousand pixels, in their last cells, where rows far
/// down a list would all lie.
fn ordered_together(parts: &[LayerPart], members: &[usize]) -> Vec<Option<Rc<[crate::DrawOrder]>>> {
    let origin = parts[members[0]].extent.origin;
    let delta = point(
        ScaledPixels(-origin.x.0.floor()),
        ScaledPixels(-origin.y.0.floor()),
    );
    let mut whole = Scene::default();
    let mut orders = Vec::with_capacity(members.len());
    for index in members {
        let operations = &parts[*index].scene.paint_operations;
        let before = whole.paint_operations.len();
        for operation in operations {
            match operation {
                PaintOperation::Primitive(primitive) => {
                    whole.insert_primitive(translate_primitive(primitive, delta))
                }
                PaintOperation::FastDropped(dropped) => {
                    crate::fast::edge_shadows::dropped(&mut whole, dropped.moved(delta))
                }
                PaintOperation::FastClip(clip) => {
                    crate::fast::edge_shadows::clip_op(&mut whole, clip.moved(delta))
                }
                PaintOperation::StartLayer(bounds) => {
                    whole.push_layer(translate_bounds(*bounds, delta))
                }
                PaintOperation::StartPositionedLayer(bounds) => {
                    whole.push_positioned_layer(translate_bounds(*bounds, delta))
                }
                PaintOperation::StartTopLayer(bounds) => {
                    whole.push_top_layer(translate_bounds(*bounds, delta))
                }
                PaintOperation::EndLayer => whole.pop_layer(),
            }
        }
        let ordered = &whole.paint_operations[before..];
        // An operation painted again is an operation again.
        debug_assert_eq!(ordered.len(), operations.len());
        // Mostly the orders the part was painted at, which need no keeping.
        orders.push(
            (ordered.len() == operations.len() && !orders_of(ordered).eq(orders_of(operations)))
                .then(|| orders_of(ordered).collect()),
        );
    }
    orders
}

fn translate_bounds(
    bounds: Bounds<ScaledPixels>,
    delta: Point<ScaledPixels>,
) -> Bounds<ScaledPixels> {
    Bounds {
        origin: bounds.origin + delta,
        size: bounds.size,
    }
}

/// The part of window (or content) space `primitive` can draw into: what
/// the renderer rasterizes for it, clipped to its content mask. A drop
/// shadow reaches three blur radii past its bounds and an inset one fills
/// its element's bounds (`vs_shadow`); every primitive's bounds are
/// transformed (`to_device_position_transformed`).
pub(crate) fn visible_bounds(primitive: &Primitive) -> Bounds<ScaledPixels> {
    drawn_bounds(primitive).intersect(&primitive.content_mask().bounds)
}

/// The part of window (or content) space `primitive` can draw into, its
/// content mask aside. See [`visible_bounds`].
pub(crate) fn drawn_bounds(primitive: &Primitive) -> Bounds<ScaledPixels> {
    match primitive {
        Primitive::Shadow(shadow) if shadow.inset != 0 => shadow
            .transformation
            .apply_to_scaled_bounds(shadow.element_bounds),
        Primitive::Shadow(shadow) => {
            let margin = ScaledPixels(3. * shadow.blur_radius.0.max(0.));
            shadow.transformation.apply_to_scaled_bounds(Bounds {
                origin: point(
                    shadow.bounds.origin.x - margin,
                    shadow.bounds.origin.y - margin,
                ),
                size: size(
                    shadow.bounds.size.width + margin * 2.,
                    shadow.bounds.size.height + margin * 2.,
                ),
            })
        }
        // `Primitive::bounds` applies the primitive's transformation.
        primitive => primitive.bounds(),
    }
}

/// `primitive` moved by `delta`: every position it carries, its content mask
/// included, so it draws the same pixels `delta` away.
pub(crate) fn translate_primitive(primitive: &Primitive, delta: Point<ScaledPixels>) -> Primitive {
    let mut primitive = primitive.clone();
    move_primitive(&mut primitive, delta);
    primitive
}

/// Moves `primitive` by `delta`, as [`translate_primitive`] does, in place.
pub(crate) fn move_primitive(primitive: &mut Primitive, delta: Point<ScaledPixels>) {
    let mv = |bounds: &mut Bounds<ScaledPixels>| *bounds = translate_bounds(*bounds, delta);
    match primitive {
        Primitive::Shadow(shadow) => {
            mv(&mut shadow.bounds);
            mv(&mut shadow.element_bounds);
            mv(&mut shadow.content_mask.bounds);
            translate_transformation(&mut shadow.transformation, delta);
        }
        Primitive::Quad(quad) => {
            mv(&mut quad.bounds);
            mv(&mut quad.content_mask.bounds);
            translate_transformation(&mut quad.transformation, delta);
        }
        Primitive::Path(path) => {
            mv(&mut path.bounds);
            mv(&mut path.content_mask.bounds);
            // Where the path was begun and last drawn to: compared with the
            // rest of it.
            path.start = path.start + delta;
            path.current = path.current + delta;
            for vertex in &mut path.vertices {
                vertex.xy_position = vertex.xy_position + delta;
                mv(&mut vertex.content_mask.bounds);
            }
        }
        Primitive::Underline(underline) => {
            mv(&mut underline.bounds);
            mv(&mut underline.content_mask.bounds);
            translate_transformation(&mut underline.transformation, delta);
        }
        Primitive::MonochromeSprite(sprite) => {
            mv(&mut sprite.bounds);
            mv(&mut sprite.content_mask.bounds);
            translate_transformation(&mut sprite.transformation, delta);
        }
        Primitive::SubpixelSprite(sprite) => {
            mv(&mut sprite.bounds);
            mv(&mut sprite.content_mask.bounds);
            translate_transformation(&mut sprite.transformation, delta);
        }
        Primitive::PolychromeSprite(sprite) => {
            mv(&mut sprite.bounds);
            mv(&mut sprite.content_mask.bounds);
            translate_transformation(&mut sprite.transformation, delta);
        }
        Primitive::Surface(surface) => {
            mv(&mut surface.bounds);
            mv(&mut surface.content_mask.bounds);
        }
        // What follows keeps a container on today's path and never reaches
        // a layer's content; it is moved in device pixels only, its logical
        // bounds left as they are.
        Primitive::VisualEffect(effect) => {
            mv(&mut effect.bounds);
            mv(&mut effect.content_mask.bounds);
            translate_transformation(&mut effect.transformation, delta);
        }
        Primitive::ScreenEffectRegion(region) => {
            mv(&mut region.bounds);
            mv(&mut region.content_mask.bounds);
            translate_transformation(&mut region.transformation, delta);
        }
        Primitive::HtmlIsland(island) => {
            mv(&mut island.bounds);
            mv(&mut island.content_mask.bounds);
        }
        Primitive::HtmlElement(element) => {
            mv(&mut element.bounds);
            mv(&mut element.content_mask.bounds);
        }
    }
}

/// A primitive's transformation applies to window positions (`R·p + t`, see
/// `to_device_position_transformed` in the shaders), so for the primitive to
/// move by `delta` with it, `t` becomes `t + (I − R)·delta`. The unit
/// transformation stays the unit.
pub(crate) fn translate_transformation(
    matrix: &mut TransformationMatrix,
    delta: Point<ScaledPixels>,
) {
    let r = matrix.rotation_scale;
    let (dx, dy) = (delta.x.0, delta.y.0);
    matrix.translation[0] += dx - (r[0][0] * dx + r[0][1] * dy);
    matrix.translation[1] += dy - (r[1][0] * dx + r[1][1] * dy);
}
