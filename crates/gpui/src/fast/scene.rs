//! Putting a finished scene in drawing order without moving its primitives
//! more than once, and without allocating once its scratch has grown.
//!
//! Adopted from gpui-fast's `crates/gpui/src/fast/scene.rs` (`b8f0c02`): a
//! counting sort by draw order, falling back to packed keys when orders spread
//! too wide, and a packed `(order, texture, tile, index)` sort for sprites,
//! which groups the sprites of one draw order by atlas texture. Both keep the
//! order primitives came in among equal keys, as the stable sorts they
//! replace did. gpux sorts its own families (visual effects, HTML islands and
//! elements, screen-effect regions) the same way.

use crate::{
    DrawOrder, HtmlElementPrimitive, HtmlIslandPrimitive, MonochromeSprite, PaintSurface, Path,
    PolychromeSprite, Quad, ScaledPixels, Scene, ScreenEffectRegionPrimitive, Shadow,
    SubpixelSprite, Underline, VisualEffectPrimitive,
};
use std::mem;

/// Room to sort a scene's primitives in, kept from one frame to the next so
/// that a frame does not allocate megabytes to put what it drew in order.
#[derive(Default)]
pub(crate) struct SortScratch {
    permutation: Permutation,
    shadows: Vec<Shadow>,
    visual_effects: Vec<VisualEffectPrimitive>,
    quads: Vec<Quad>,
    html_islands: Vec<HtmlIslandPrimitive>,
    html_elements: Vec<HtmlElementPrimitive>,
    screen_effect_regions: Vec<ScreenEffectRegionPrimitive>,
    paths: Vec<Path<ScaledPixels>>,
    underlines: Vec<Underline>,
    monochrome_sprites: Vec<MonochromeSprite>,
    subpixel_sprites: Vec<SubpixelSprite>,
    polychrome_sprites: Vec<PolychromeSprite>,
    surfaces: Vec<PaintSurface>,
}

/// The order to gather a kind of primitive in, and what working it out takes.
#[derive(Default)]
pub(crate) struct Permutation {
    /// The index of the primitive to put at each place.
    pub(crate) order: Vec<u32>,
    /// For each draw order, how many primitives have it, and then where the
    /// next of them goes.
    pub(crate) counts: Vec<u32>,
    /// Draw orders packed with indices, when they are too spread out to count.
    pub(crate) keys: Vec<u64>,
    /// Sprites' draw orders, textures and tiles packed with indices.
    pub(crate) wide_keys: Vec<u128>,
}

/// How much wider than the number of primitives their draw orders may spread
/// before [`sort_by_draw_order`] sorts them rather than counting them, so that
/// the counts are never much longer than the primitives.
pub(crate) const MAX_ORDERS_PER_PRIMITIVE: usize = 4;

/// Puts `items` in draw order, keeping the order they came in among those of
/// one draw order, as a stable sort does.
///
/// A primitive's draw order is one past the greatest among the primitives
/// before it that it overlaps, or its layer's, so a scene of thousands of
/// primitives has a few dozen of them. They are counted rather than compared:
/// one pass counts how many primitives have each order, the next puts each
/// where its order's run is up to. Orders spread wider than
/// [`MAX_ORDERS_PER_PRIMITIVE`] allows are sorted as packed keys instead. A
/// scene with no primitive out of order is left as it is.
pub(crate) fn sort_by_draw_order<T: Clone>(
    items: &mut Vec<T>,
    permutation: &mut Permutation,
    gathered: &mut Vec<T>,
    draw_order: impl Fn(&T) -> DrawOrder,
) {
    if items.len() < 2 {
        return;
    }
    let mut max = 0;
    let mut sorted = true;
    let mut previous = 0;
    for item in items.iter() {
        let order = draw_order(item);
        sorted &= order >= previous;
        previous = order;
        max = max.max(order);
    }
    if sorted {
        return;
    }
    let Permutation {
        order,
        counts,
        keys,
        ..
    } = permutation;
    if max as usize > items.len() * MAX_ORDERS_PER_PRIMITIVE {
        keys.clear();
        keys.extend(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| (u64::from(draw_order(item)) << 32) | index as u64),
        );
        keys.sort_unstable();
        order.clear();
        order.extend(keys.iter().map(|&key| key as u32));
    } else {
        counts.clear();
        counts.resize(max as usize + 1, 0);
        for item in items.iter() {
            counts[draw_order(item) as usize] += 1;
        }
        let mut start = 0;
        for count in counts.iter_mut() {
            let primitives = *count;
            *count = start;
            start += primitives;
        }
        order.clear();
        order.resize(items.len(), 0);
        for (index, item) in items.iter().enumerate() {
            let next = &mut counts[draw_order(item) as usize];
            order[*next as usize] = index as u32;
            *next += 1;
        }
    }
    gather(items, order, gathered);
}

/// Puts sprites in draw order, those of one draw order grouped by the atlas
/// texture they come from and then by tile, keeping the order they came in
/// among sprites of one tile, as a stable sort does.
///
/// The draw order, texture, tile and index of each sprite are packed into one
/// integer, so the sort compares integers lying next to each other rather
/// than reaching into a sprite for its key on every comparison. Sprites
/// already in that order are left as they are.
pub(crate) fn sort_sprites<T: Clone>(
    items: &mut Vec<T>,
    permutation: &mut Permutation,
    gathered: &mut Vec<T>,
    key: impl Fn(&T) -> (DrawOrder, u32, u32),
) {
    if items.len() < 2 {
        return;
    }
    let Permutation {
        order, wide_keys, ..
    } = permutation;
    wide_keys.clear();
    wide_keys.extend(items.iter().enumerate().map(|(index, item)| {
        let (draw_order, texture, tile) = key(item);
        (u128::from(draw_order) << 96)
            | (u128::from(texture) << 64)
            | (u128::from(tile) << 32)
            | index as u128
    }));
    if wide_keys.is_sorted() {
        return;
    }
    wide_keys.sort_unstable();
    order.clear();
    order.extend(wide_keys.iter().map(|&key| key as u32));
    gather(items, order, gathered);
}

/// Puts the item at `order[i]` at place `i`.
///
/// Sorting indices and then gathering once moves each item a single time,
/// where sorting the items themselves moves them as often as the sort needs
/// to compare them — and these items are 112 to 168 bytes each.
fn gather<T: Clone>(items: &mut Vec<T>, order: &[u32], gathered: &mut Vec<T>) {
    gathered.clear();
    gathered.extend(order.iter().map(|&index| items[index as usize].clone()));
    mem::swap(items, gathered);
}

/// What [`Scene::finish`] does: puts every primitive in drawing order.
/// Sprites of one order are grouped by the atlas texture they come from:
/// a batch draws from one texture, and tile ids, which each texture numbers
/// from zero, would interleave them.
#[inline]
pub(crate) fn sort_in_drawing_order(scene: &mut Scene) {
    let scratch = &mut scene.sort_scratch;
    macro_rules! sort {
        ($field:ident) => {
            sort_by_draw_order(
                &mut scene.$field,
                &mut scratch.permutation,
                &mut scratch.$field,
                |primitive| primitive.order,
            )
        };
    }
    macro_rules! sort_sprites {
        ($field:ident) => {
            sort_sprites(
                &mut scene.$field,
                &mut scratch.permutation,
                &mut scratch.$field,
                |sprite| {
                    (
                        sprite.order,
                        sprite.tile.texture_id.index,
                        sprite.tile.tile_id.0,
                    )
                },
            )
        };
    }
    sort!(shadows);
    sort!(visual_effects);
    sort!(quads);
    sort!(html_islands);
    sort!(html_elements);
    sort!(screen_effect_regions);
    sort!(paths);
    sort!(underlines);
    sort_sprites!(monochrome_sprites);
    sort_sprites!(subpixel_sprites);
    sort_sprites!(polychrome_sprites);
    sort!(surfaces);
}
