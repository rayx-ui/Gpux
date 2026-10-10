//! Scroll layers: a scroll container's content painted once into cached tiles
//! and composited at the scroll offset on frames where only the offset
//! changed. Adapted from gpui-fast's `fast/layers/`.
//!
//! `scene` is the contract with renderers. `policy` decides, per container
//! and frame, between today's path, painting into the layer and compositing
//! it; `invalidate` tells a scroll-only frame from one whose content changed;
//! `paint`, `tiles` and `background` paint the content into the layer's
//! tiles; `reuse` and `input` carry what the content registered (hitboxes,
//! listeners, the dispatch subtree) to where a composited layer shows it.
//!
//! The mechanism is one of the window's [`FastMode`](crate::FastMode)
//! switches (`scroll_layers`), where gpui-fast keeps a switch of its own.

pub(crate) mod background;
pub(crate) mod input;
pub(crate) mod invalidate;
pub(crate) mod lists;
pub(crate) mod paint;
pub(crate) mod policy;
pub(crate) mod record;
pub(crate) mod reuse;
pub mod scene;
pub(crate) mod tiles;
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod verify;
pub(crate) mod work;

use crate::{App, GlobalElementId, Window};
use collections::FxHashMap;

/// Whether a renderer on this target implements scroll-layer tiles. Where it
/// does not, [`FastMode::normalized`](crate::fast::mode::FastMode::normalized)
/// keeps scroll layers off and every layer entry point is a no-op.
pub(crate) const COMPILED: bool = cfg!(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos",
    target_os = "windows"
));

/// A window's scroll layers, by the scroll container's global id.
#[derive(Default)]
pub(crate) struct WindowLayers {
    pub(crate) layers: FxHashMap<GlobalElementId, Layer>,
    /// The key the next layer gets, so keys stay unique in the window.
    pub(crate) next_key: u32,
    /// The scrolls and offset reads of the frame being drawn.
    pub(crate) scrolls: invalidate::ScrollLog,
    /// The layer being painted, if any.
    pub(crate) painting: Option<paint::Painting>,
    /// What every scroll container decides, in tests of one stream that
    /// cannot wait for the policy to decide it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) forced_decision: Option<policy::Decision>,
    /// Keeps every layer whatever its content costs to keep painting, in
    /// tests that are after what layers draw, not after when they pay.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) never_demote: bool,
    /// How many layers were demoted, for tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) demotions: u64,
    /// How many layers were painted again before input reached their
    /// content, for tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) input_rebuilds: u64,
    /// Counts the frames drawn, for layers to tell how long ago something was.
    pub(crate) frame: u64,
    /// The window's size and scale factor the layers were painted at.
    pub(crate) window_size: Option<(crate::Size<crate::Pixels>, f32)>,
}

/// The cached content of one scroll container and what it takes to
/// composite it and route input into it.
pub(crate) struct Layer {
    pub(crate) key: scene::LayerKey,
    /// The generation of the latest content, counted for the layer rather
    /// than its record: a renderer keeps tiles by key and generation, so a
    /// record dropped and painted again must not start over at a
    /// generation the renderer holds tiles of. See [`Layer::next_generation`].
    pub(crate) generation: u64,
    /// The content as last painted, once it has been.
    pub(crate) record: Option<record::LayerRecord>,
    pub(crate) policy: policy::LayerPolicy,
    pub(crate) input: input::LayerInput,
    pub(crate) rows: lists::LayerRows,
    /// The last frame the layer's tiles were composited in.
    pub(crate) last_composited_frame: u64,
    /// What the frame being drawn prepainted the container for, for its
    /// paint to finish.
    pub(crate) prepainted: Option<paint::Prepainted>,
}

impl Layer {
    /// A generation this layer's content has never had.
    pub(crate) fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }
}

/// Whether layers may be used in `window` this frame: the switch is on (which
/// implies view retention and a renderer with tiles), and nothing draws the
/// frame from scratch or needs every element painted.
pub(crate) fn active(window: &Window, cx: &App) -> bool {
    window.fast_mode.scroll_layers()
        && !window.refreshing
        && !cx.has_active_drag()
        && !window.a11y.is_active()
        && !window.is_inspector_picking(cx)
}

/// Ends the frame being drawn: the scrolls before it are taken in, and the
/// layers not composited for long, or painted for another window size or
/// scale factor, are dropped.
pub(crate) fn finish_frame(window: &mut Window) {
    policy::drop_layers_on_resize(window);
    let layers = &mut window.fast_layers;
    let frame = layers.frame;
    policy::finish_frame(layers);
    layers.layers.retain(|_, layer| policy::keep(layer, frame));
    let live = &layers.layers;
    layers
        .scrolls
        .finish_frame(frame, |id| live.contains_key(id));
    layers.frame += 1;
}

/// Drops every layer of `window`: scroll layers were switched off.
pub(crate) fn release(window: &mut Window) {
    window.fast_layers.layers.clear();
    window.fast_layers.painting = None;
}

/// How many layers `window` keeps and tiles they hold, for tests that check
/// switching scroll layers off released them.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn owned(window: &Window) -> usize {
    window
        .fast_layers
        .layers
        .values()
        .map(|layer| {
            1 + layer
                .record
                .as_ref()
                .map_or(0, |record| record.tile_hashes.len())
        })
        .sum()
}
