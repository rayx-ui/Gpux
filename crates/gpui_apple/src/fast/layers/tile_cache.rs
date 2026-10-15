//! The textures scroll layer tiles are rasterized into, kept from frame to
//! frame so that a frame that only scrolled draws them again without
//! rasterizing anything.
//!
//! A tile's texture holds the layer's content of one generation. The core
//! bumps a layer's generation whenever it repaints the content and lists the
//! tiles whose pixels changed (`LayerFrame::dirty_tiles`); every other tile
//! of the previous generation stays as it is. A tile the cache has missed a
//! generation of could have changed in it, so it is rasterized again.
//!
//! Adapted from gpui-fast's `gpui_apple/src/fast/layers/tile_cache.rs`. The
//! bookkeeping is `gpui_wgpu`'s `fast::layers::tile_cache`, with Metal
//! textures in place of wgpu's.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpux::{LAYER_TILE_BUDGET_BYTES, LayerKey, SceneLayers, TileCoord};
use std::collections::{HashMap, HashSet};

/// Frames a layer that no frame composites keeps its tiles for; the core
/// drops such a layer after as many.
const LAYER_KEEP_FRAMES: u64 = 120;

/// Frames a tile no frame composites keeps its texture for: a scroll moves
/// the tiles it leaves behind out of the viewport for good, unless it turns
/// back right away, and the budget alone would keep every tile it passed.
pub(crate) const TILE_IDLE_FRAMES: u64 = 4;

/// Textures of released tiles kept for new tiles to reuse.
const POOL_LIMIT: usize = 4;

/// A window's scroll layer tiles.
pub(crate) struct TileCache {
    tiles: HashMap<(LayerKey, TileCoord), Tile>,
    layers: HashMap<LayerKey, SeenLayer>,
    pool: Vec<TileTexture>,
    frame: u64,
    pub(crate) budget_bytes: u64,
    /// The size frames were last drawn at: tiles are released when it changes.
    drawn_size: Option<gpux::Size<gpux::DevicePixels>>,
    /// Set by a command buffer that rasterized tiles and failed, as when the
    /// GPU is lost: its tiles' pixels are undefined.
    failed: Arc<AtomicBool>,
    /// How many tiles have been rasterized, for the tests.
    #[cfg(test)]
    pub(crate) rasterized: usize,
}

impl Default for TileCache {
    fn default() -> Self {
        Self {
            tiles: HashMap::new(),
            layers: HashMap::new(),
            pool: Vec::new(),
            frame: 0,
            budget_bytes: LAYER_TILE_BUDGET_BYTES,
            drawn_size: None,
            failed: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            rasterized: 0,
        }
    }
}

struct Tile {
    /// The generation of the content the texture holds, or will hold once
    /// this frame's rasterization runs.
    generation: u64,
    tile_size: u32,
    valid: bool,
    /// The last frame that composited or rasterized the tile.
    last_used: u64,
    texture: Option<TileTexture>,
}

struct SeenLayer {
    generation: u64,
    last_frame: u64,
}

struct TileTexture {
    size: u32,
    texture: metal::Texture,
}

fn tile_bytes(tile_size: u32) -> u64 {
    tile_size as u64 * tile_size as u64 * 4
}

impl TileCache {
    /// The texture of `tile` of `layer`, if the cache holds its content.
    pub(crate) fn texture(&self, layer: LayerKey, tile: TileCoord) -> Option<&metal::TextureRef> {
        self.tiles
            .get(&(layer, tile))
            .filter(|tile| tile.valid)
            .and_then(|tile| tile.texture.as_ref())
            .map(|texture| texture.texture.as_ref())
    }

    #[cfg(test)]
    pub(crate) fn holds(&self, layer: LayerKey, tile: TileCoord) -> bool {
        self.tiles
            .get(&(layer, tile))
            .is_some_and(|tile| tile.valid)
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.is_idle()
    }

    /// The flag a failed tile command buffer sets.
    pub(crate) fn failure_flag(&self) -> Arc<AtomicBool> {
        self.failed.clone()
    }

    /// Whether a tile command buffer failed since the last call.
    pub(crate) fn take_failure(&self) -> bool {
        self.failed.swap(false, Ordering::AcqRel)
    }

    /// Notes the size the frame about to be drawn has, releasing every tile
    /// when it is not the last frame's: the window was resized, or a
    /// headless renderer was asked for another size.
    pub(crate) fn drawn_at(&mut self, size: gpux::Size<gpux::DevicePixels>) {
        if self
            .drawn_size
            .replace(size)
            .is_some_and(|last| last != size)
        {
            self.clear();
        }
    }

    /// Starts a frame that composites the tiles `composited` of the layers in
    /// `layers`. Returns the tiles to rasterize before they are drawn, each
    /// with the index of its layer in `layers.frames`: every composited tile
    /// the cache does not hold, or holds from an older generation. A dirty
    /// tile the frame does not composite is only marked stale, and waits
    /// until a frame shows it: a scrolling list dirties the tiles at the edge
    /// of its overscan on most frames. They count as held from here on.
    pub(crate) fn begin_frame(
        &mut self,
        layers: &SceneLayers,
        composited: impl Iterator<Item = (LayerKey, TileCoord)>,
    ) -> Vec<(usize, TileCoord)> {
        self.frame += 1;
        let frame = self.frame;
        let mut planned = HashSet::new();

        for layer in &layers.frames {
            let seen = self.layers.entry(layer.key).or_insert(SeenLayer {
                generation: layer.generation.wrapping_sub(2),
                last_frame: frame,
            });
            seen.last_frame = frame;
            if seen.generation == layer.generation {
                continue;
            }
            let previous = seen.generation;
            let consecutive = previous.wrapping_add(1) == layer.generation;
            seen.generation = layer.generation;
            let dirty: HashSet<TileCoord> = layer.dirty_tiles.iter().copied().collect();
            for ((key, coord), tile) in &mut self.tiles {
                if *key != layer.key {
                    continue;
                }
                if consecutive
                    && tile.valid
                    && tile.generation == previous
                    && tile.tile_size == layer.tile_size
                    && !dirty.contains(coord)
                {
                    tile.generation = layer.generation;
                } else {
                    tile.valid = false;
                }
            }
        }

        let mut indices = HashMap::new();
        for (index, layer) in layers.frames.iter().enumerate() {
            indices.insert(layer.key, index);
        }
        for (key, coord) in composited {
            // A tile of a layer the frame does not describe cannot be
            // rasterized; it draws nothing.
            let Some(&index) = indices.get(&key) else {
                continue;
            };
            let layer = &layers.frames[index];
            match self.tiles.get_mut(&(key, coord)) {
                Some(tile)
                    if tile.valid
                        && tile.generation == layer.generation
                        && tile.tile_size == layer.tile_size =>
                {
                    tile.last_used = frame;
                }
                _ => {
                    planned.insert((index, coord));
                }
            }
        }

        let mut planned: Vec<(usize, TileCoord)> = planned.into_iter().collect();
        planned.sort();
        for &(index, coord) in &planned {
            let layer = &layers.frames[index];
            let tile = self.tiles.entry((layer.key, coord)).or_insert(Tile {
                generation: layer.generation,
                tile_size: layer.tile_size,
                valid: false,
                last_used: frame,
                texture: None,
            });
            tile.generation = layer.generation;
            tile.tile_size = layer.tile_size;
            tile.valid = true;
            tile.last_used = frame;
        }

        self.drop_unseen_layers();
        self.release_idle_tiles();
        planned
    }

    /// Releases the tiles no frame composited for [`TILE_IDLE_FRAMES`],
    /// keeping their textures in the pool for new tiles.
    fn release_idle_tiles(&mut self) {
        let frame = self.frame;
        let pool = &mut self.pool;
        self.tiles.retain(|_, tile| {
            if tile.last_used + TILE_IDLE_FRAMES >= frame {
                return true;
            }
            if let Some(texture) = tile.texture.take()
                && pool.len() < POOL_LIMIT
            {
                pool.push(texture);
            }
            false
        });
    }

    /// Releases the tiles of layers no frame has composited for a while.
    fn drop_unseen_layers(&mut self) {
        let frame = self.frame;
        let gone: Vec<LayerKey> = self
            .layers
            .iter()
            .filter(|(_, seen)| seen.last_frame + LAYER_KEEP_FRAMES < frame)
            .map(|(key, _)| *key)
            .collect();
        if gone.is_empty() {
            return;
        }
        for key in &gone {
            self.layers.remove(key);
        }
        let released: Vec<(LayerKey, TileCoord)> = self
            .tiles
            .keys()
            .filter(|(key, _)| gone.contains(key))
            .copied()
            .collect();
        for key in released {
            if let Some(texture) = self.tiles.remove(&key).and_then(|tile| tile.texture)
                && self.pool.len() < POOL_LIMIT
            {
                self.pool.push(texture);
            }
        }
    }

    /// Releases tiles, least recently used first, until the tiles and the
    /// pool fit in `budget_bytes`. Tiles this frame composites or
    /// rasterizes are kept even when they alone exceed the budget.
    pub(crate) fn evict_to_budget(&mut self, budget_bytes: u64) {
        let mut bytes: u64 = self
            .tiles
            .values()
            .filter(|tile| tile.valid || tile.texture.is_some())
            .map(|tile| tile_bytes(tile.tile_size))
            .sum::<u64>()
            + self
                .pool
                .iter()
                .map(|texture| tile_bytes(texture.size))
                .sum::<u64>();
        while bytes > budget_bytes
            && let Some(texture) = self.pool.pop()
        {
            bytes -= tile_bytes(texture.size);
        }
        if bytes <= budget_bytes {
            return;
        }
        let mut candidates: Vec<(u64, LayerKey, TileCoord)> = self
            .tiles
            .iter()
            .filter(|(_, tile)| tile.last_used != self.frame)
            .map(|((key, coord), tile)| (tile.last_used, *key, *coord))
            .collect();
        candidates.sort_by_key(|&(last_used, key, coord)| (last_used, key.0, coord));
        for (_, key, coord) in candidates {
            if bytes <= budget_bytes {
                break;
            }
            if let Some(tile) = self.tiles.remove(&(key, coord))
                && (tile.valid || tile.texture.is_some())
            {
                bytes -= tile_bytes(tile.tile_size);
            }
        }
    }

    /// Releases every tile and texture, as when the drawable's size or
    /// transparency changes, or the GPU fails.
    pub(crate) fn clear(&mut self) {
        self.tiles.clear();
        self.layers.clear();
        self.pool.clear();
    }

    /// Whether the cache holds nothing a frame without layers has to age.
    pub(crate) fn is_idle(&self) -> bool {
        self.tiles.is_empty() && self.layers.is_empty() && self.pool.is_empty()
    }

    /// Fits the cache in its budget and gives every tile in `planned` a
    /// texture of `format`.
    pub(crate) fn prepare(
        &mut self,
        device: &metal::DeviceRef,
        format: metal::MTLPixelFormat,
        layers: &SceneLayers,
        planned: &[(usize, TileCoord)],
    ) {
        self.evict_to_budget(self.budget_bytes);
        for &(index, coord) in planned {
            let layer = &layers.frames[index];
            let Some(tile) = self.tiles.get_mut(&(layer.key, coord)) else {
                continue;
            };
            if tile
                .texture
                .as_ref()
                .is_some_and(|texture| texture.size == tile.tile_size)
            {
                continue;
            }
            let size = tile.tile_size;
            let texture = match self.pool.iter().position(|texture| texture.size == size) {
                Some(position) => self.pool.swap_remove(position),
                None => create_tile_texture(device, format, size),
            };
            tile.texture = Some(texture);
        }
    }
}

/// A tile texture: a render target of the frame's format that the
/// polychrome sprite pipeline samples.
fn create_tile_texture(
    device: &metal::DeviceRef,
    format: metal::MTLPixelFormat,
    size: u32,
) -> TileTexture {
    let descriptor = metal::TextureDescriptor::new();
    descriptor.set_width(size as u64);
    descriptor.set_height(size as u64);
    descriptor.set_pixel_format(format);
    descriptor.set_storage_mode(metal::MTLStorageMode::Private);
    descriptor.set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
    let texture = device.new_texture(&descriptor);
    texture.set_label("scroll_layer_tile");
    TileTexture { size, texture }
}
