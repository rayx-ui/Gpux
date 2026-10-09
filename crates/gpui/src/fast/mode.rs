//! The fast-mode switches: which mechanisms a window uses, read once from the
//! environment when the window is created and changeable at runtime.
//!
//! Adapted from gpui-fast's `view_retention` switch (`fast/retained.rs`,
//! `GPUI_VIEW_RETENTION`) and its scroll-layers switch (`fast/layers/mod.rs`,
//! `GPUI_SCROLL_LAYERS`, `COMPILED`). gpux gathers every mechanism into one
//! per-window value, adds a layout-retention switch and a master switch, and
//! keeps gpui-fast's variable names as aliases.

use crate::Window;

/// Which fast-mode mechanisms a window uses.
///
/// Each mechanism builds on the one before it: layout retention keeps Taffy
/// nodes across frames, view retention reuses whole views from what they
/// read, splice copies a view from its last frame around a rebuilt child, and
/// scroll layers composite scrolled content from cached tiles. A value is
/// [normalized](Self::normalized) whenever a window takes it, so a mechanism
/// is never on without the one it builds on.
///
/// A window's initial value comes from the compiled defaults and the
/// environment (see [`Window::fast_mode`]); [`Window::set_fast_mode`] changes
/// it at runtime. With every switch off the window draws exactly as gpux does
/// without fast mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FastMode {
    /// Keep Taffy layout nodes keyed by element identity across frames,
    /// rewriting a node only when its style, children or measurement changed.
    pub layout_retention: bool,
    /// Treat every view as a retained subtree, reused when nothing it read
    /// changed. Requires `layout_retention`.
    pub view_retention: bool,
    /// Copy a view that is dirty only because a nested view changed from its
    /// last frame around the rebuilt child. Requires `view_retention`.
    pub splice: bool,
    /// Paint scrolled content once into GPU tiles and composite them on
    /// scroll-only frames. Requires `view_retention` and a renderer that
    /// implements tiles. Also enables the device-pixel snapping of scroll
    /// offsets, glyph origins and paths that tiles need.
    pub scroll_layers: bool,
}

/// Whether layout retention is on in a window the environment says nothing
/// about. Every compiled default stays `false` until fast mode's final sprint,
/// so mechanisms land dark.
pub(crate) const DEFAULT_LAYOUT_RETENTION: bool = false;
/// See [`DEFAULT_LAYOUT_RETENTION`].
pub(crate) const DEFAULT_VIEW_RETENTION: bool = false;
/// See [`DEFAULT_LAYOUT_RETENTION`].
pub(crate) const DEFAULT_SPLICE: bool = false;
/// See [`DEFAULT_LAYOUT_RETENTION`].
pub(crate) const DEFAULT_SCROLL_LAYERS: bool = false;

/// Turns every mechanism off when `0`; when `1`, every mechanism starts on
/// before the per-mechanism variables apply.
pub(crate) const FAST_MODE_VAR: &str = "GPUX_FAST_MODE";
/// `0` or `1`: layout retention.
pub(crate) const LAYOUT_RETENTION_VAR: &str = "GPUX_LAYOUT_RETENTION";
/// `0` or `1`: view retention.
pub(crate) const VIEW_RETENTION_VAR: &str = "GPUX_VIEW_RETENTION";
/// `0` or `1`: scroll layers.
pub(crate) const SCROLL_LAYERS_VAR: &str = "GPUX_SCROLL_LAYERS";
/// gpui-fast's name for [`VIEW_RETENTION_VAR`], read when that is unset.
pub(crate) const VIEW_RETENTION_ALIAS: &str = "GPUI_VIEW_RETENTION";
/// gpui-fast's name for [`SCROLL_LAYERS_VAR`], read when that is unset.
pub(crate) const SCROLL_LAYERS_ALIAS: &str = "GPUI_SCROLL_LAYERS";

impl FastMode {
    /// Every mechanism off: the frame gpux draws without fast mode.
    pub const OFF: FastMode = FastMode {
        layout_retention: false,
        view_retention: false,
        splice: false,
        scroll_layers: false,
    };

    /// Every mechanism on, before normalization turns off what this target
    /// does not compile.
    pub const ALL: FastMode = FastMode {
        layout_retention: true,
        view_retention: true,
        splice: true,
        scroll_layers: true,
    };

    /// The mode a window starts in when the environment says nothing,
    /// normalized.
    pub fn compiled_default() -> FastMode {
        FastMode {
            layout_retention: DEFAULT_LAYOUT_RETENTION,
            view_retention: DEFAULT_VIEW_RETENTION,
            splice: DEFAULT_SPLICE,
            scroll_layers: DEFAULT_SCROLL_LAYERS,
        }
        .normalized()
    }

    /// This mode with every mechanism whose prerequisite is off turned off,
    /// and scroll layers off where no renderer implements them.
    pub fn normalized(self) -> FastMode {
        self.normalized_for(crate::fast::layers::COMPILED)
    }

    /// [`Self::normalized`] for a target that does or does not compile
    /// scroll layers.
    pub(crate) fn normalized_for(self, layers_compiled: bool) -> FastMode {
        let layout_retention = self.layout_retention;
        let view_retention = self.view_retention && layout_retention;
        FastMode {
            layout_retention,
            view_retention,
            splice: self.splice && view_retention,
            scroll_layers: self.scroll_layers && view_retention && layers_compiled,
        }
    }

    /// Whether every mechanism is off.
    pub fn is_off(self) -> bool {
        self == FastMode::OFF
    }

    /// The mode a window starts in, from the compiled defaults and the
    /// variables `lookup` returns, normalized.
    ///
    /// - `GPUX_FAST_MODE=0` turns everything off and nothing else is read;
    ///   `GPUX_FAST_MODE=1` starts every mechanism on.
    /// - `GPUX_LAYOUT_RETENTION`, `GPUX_VIEW_RETENTION` and
    ///   `GPUX_SCROLL_LAYERS` set one mechanism each, from `0` or `1`.
    /// - gpui-fast's `GPUI_VIEW_RETENTION` and `GPUI_SCROLL_LAYERS` are read
    ///   only when the `GPUX_*` variable is unset or not `0`/`1`. As in
    ///   gpui-fast, any value other than `0` turns them on.
    /// - Splice has no variable, as in gpui-fast.
    pub(crate) fn from_environment(lookup: impl Fn(&str) -> Option<String>) -> FastMode {
        let base = match lookup(FAST_MODE_VAR).as_deref().map(str::trim) {
            Some("0") => return FastMode::OFF,
            Some("1") => FastMode::ALL,
            _ => FastMode::compiled_default(),
        };
        let switch = |name: &str, alias: Option<&str>, default: bool| {
            match lookup(name).as_deref().map(str::trim) {
                Some("0") => return false,
                Some("1") => return true,
                Some(other) => log::warn!("{name}={other:?} is neither 0 nor 1; ignoring it"),
                None => {}
            }
            alias
                .and_then(|alias| lookup(alias))
                .map_or(default, |value| value.trim() != "0")
        };
        FastMode {
            layout_retention: switch(LAYOUT_RETENTION_VAR, None, base.layout_retention),
            view_retention: switch(
                VIEW_RETENTION_VAR,
                Some(VIEW_RETENTION_ALIAS),
                base.view_retention,
            ),
            splice: base.splice,
            scroll_layers: switch(
                SCROLL_LAYERS_VAR,
                Some(SCROLL_LAYERS_ALIAS),
                base.scroll_layers,
            ),
        }
        .normalized()
    }
}

/// A window's fast mode.
pub(crate) struct WindowFastMode {
    mode: FastMode,
}

impl WindowFastMode {
    /// The mode of a window being created, read from the process environment.
    pub(crate) fn from_process_environment() -> Self {
        Self {
            mode: FastMode::from_environment(|name| std::env::var(name).ok()),
        }
    }

    /// The effective mode.
    #[inline(always)]
    pub(crate) fn get(&self) -> FastMode {
        self.mode
    }

    /// Whether layout retention is on.
    #[inline(always)]
    #[allow(
        dead_code,
        reason = "read by the layout-retention hooks of a later sprint"
    )]
    pub(crate) fn layout_retention(&self) -> bool {
        self.mode.layout_retention
    }

    /// Whether view retention is on.
    #[inline(always)]
    #[allow(
        dead_code,
        reason = "read by the view-retention hooks of a later sprint"
    )]
    pub(crate) fn view_retention(&self) -> bool {
        self.mode.view_retention
    }

    /// Whether splice is on.
    #[inline(always)]
    #[allow(dead_code, reason = "read by the splice hooks of a later sprint")]
    pub(crate) fn splice(&self) -> bool {
        self.mode.splice
    }

    /// Whether scroll layers are on, which also enables their snapping.
    #[inline(always)]
    #[allow(dead_code, reason = "read by the scroll-layer hooks of a later sprint")]
    pub(crate) fn scroll_layers(&self) -> bool {
        self.mode.scroll_layers
    }
}

/// What a window's fast-mode mechanisms hold on to between frames, for tests
/// that check a disabled mechanism released it.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FastOwnedState {
    /// Taffy nodes kept for elements by layout retention.
    pub kept_layout_nodes: usize,
    /// Retained view records kept by view retention and splice.
    pub retained_records: usize,
    /// Scroll layers and the tiles they keep.
    pub layer_tiles: usize,
    /// Every Taffy node the layout engine holds, kept or pooled.
    pub layout_engine_nodes: usize,
}

/// Releases what the mechanisms that `previous` had on and `current` has off
/// own. Each mechanism adds its release here as it lands.
fn release_disabled(window: &mut Window, previous: FastMode, current: FastMode) {
    let _ = (window, previous, current);
}

impl Window {
    /// The fast-mode mechanisms this window uses.
    ///
    /// A window starts with the compiled defaults, overridden by the
    /// environment when it is created: `GPUX_FAST_MODE=0` turns everything
    /// off; `GPUX_LAYOUT_RETENTION`, `GPUX_VIEW_RETENTION` and
    /// `GPUX_SCROLL_LAYERS` take `0` or `1`; gpui-fast's
    /// `GPUI_VIEW_RETENTION` and `GPUI_SCROLL_LAYERS` are read when the gpux
    /// name is unset.
    pub fn fast_mode(&self) -> FastMode {
        self.fast_mode.get()
    }

    /// Changes the fast-mode mechanisms this window uses, from the next frame.
    ///
    /// The mode is [normalized](FastMode::normalized) first. If that changes
    /// anything, what the mechanisms turned off kept between frames is
    /// released and the window is refreshed, so the next frame is drawn from
    /// scratch.
    pub fn set_fast_mode(&mut self, mode: FastMode) {
        let previous = self.fast_mode.get();
        let current = mode.normalized();
        if previous == current {
            return;
        }
        self.fast_mode.mode = current;
        release_disabled(self, previous, current);
        if self.invalidator.not_drawing() {
            self.refresh();
        } else {
            self.on_next_frame(|window, _| window.refresh());
        }
    }

    /// Drops everything this window keeps between frames to save work and
    /// refreshes it, so the next frame is drawn the way a window drawing its
    /// first frame would draw it. State the application can observe, such as
    /// element state, is kept. The oracle's reference window calls this
    /// before every frame.
    #[cfg(any(test, feature = "test-support"))]
    pub fn forget_retained_state(&mut self) {
        self.layout_engine = Some(crate::TaffyLayoutEngine::new());
        self.rendered_frame.scene.forget_orderings();
        self.next_frame.scene.forget_orderings();
        self.refresh();
    }

    /// What this window's fast-mode mechanisms hold on to between frames.
    #[cfg(any(test, feature = "test-support"))]
    pub fn fast_owned_state(&self) -> FastOwnedState {
        FastOwnedState {
            layout_engine_nodes: self
                .layout_engine
                .as_ref()
                .map_or(0, |engine| engine.node_count()),
            ..FastOwnedState::default()
        }
    }
}
