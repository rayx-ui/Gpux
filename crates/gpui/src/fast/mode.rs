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
/// about. Every mechanism is on by default; normalization turns off what a
/// target does not compile, and the variables below turn each one off.
pub(crate) const DEFAULT_LAYOUT_RETENTION: bool = true;
/// See [`DEFAULT_LAYOUT_RETENTION`].
pub(crate) const DEFAULT_VIEW_RETENTION: bool = true;
/// See [`DEFAULT_LAYOUT_RETENTION`].
pub(crate) const DEFAULT_SPLICE: bool = true;
/// See [`DEFAULT_LAYOUT_RETENTION`].
pub(crate) const DEFAULT_SCROLL_LAYERS: bool = true;

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
        Self::from_environment_over(FastMode::compiled_default(), lookup)
    }

    /// [`Self::from_environment`] for a window whose compiled default is
    /// `default`, which lets tests of the variables' precedence stand apart
    /// from what the defaults are.
    pub(crate) fn from_environment_over(
        default: FastMode,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> FastMode {
        let base = match lookup(FAST_MODE_VAR).as_deref().map(str::trim) {
            Some("0") => return FastMode::OFF,
            Some("1") => FastMode::ALL,
            _ => default,
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
    /// Whether what scroll layers snap is snapped with them off, in a
    /// window drawn as the reference of one with them on.
    #[cfg(any(test, feature = "test-support"))]
    snapping_forced: bool,
    /// What decided the kind of sprite text was painted as in the last frame
    /// drawn with a retention mechanism on. See [`Window::refresh_for_text_policy`].
    text_policy: Option<TextPolicy>,
}

/// What decides whether a window paints text as subpixel or grayscale
/// sprites: scenes a retention mechanism replays carry the choice made when
/// they were painted.
#[derive(Clone, Copy, PartialEq)]
struct TextPolicy {
    mode: crate::TextRenderingMode,
    background: crate::WindowBackgroundAppearance,
    subpixel_supported: bool,
}

impl WindowFastMode {
    /// The mode of a window being created on `platform_window`, read from
    /// the process environment and normalized for it.
    pub(crate) fn for_window(platform_window: &dyn crate::PlatformWindow) -> Self {
        Self {
            mode: counted(
                FastMode::OFF,
                FastMode::from_environment(|name| std::env::var(name).ok())
                    .normalized_for(layers_drawn_by(platform_window)),
            ),
            #[cfg(any(test, feature = "test-support"))]
            snapping_forced: false,
            text_policy: None,
        }
    }

    /// The effective mode.
    #[inline(always)]
    pub(crate) fn get(&self) -> FastMode {
        self.mode
    }

    /// Whether layout retention is on.
    #[inline(always)]
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
    pub(crate) fn splice(&self) -> bool {
        self.mode.splice
    }

    /// Whether scroll layers are on, which also enables their snapping.
    #[inline(always)]
    pub(crate) fn scroll_layers(&self) -> bool {
        self.mode.scroll_layers
    }

    /// Whether scroll offsets, glyph origins and paths are snapped to what
    /// a layer's tiles can hold: with scroll layers on, or in a test's
    /// reference window (see [`Window::force_layer_snapping`]).
    #[inline(always)]
    pub(crate) fn snapping(&self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        if self.snapping_forced {
            return true;
        }
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

/// Whether scroll layers can be on in a window on `platform_window`: the
/// target compiles them and the window's renderer rasterizes their tiles.
fn layers_drawn_by(platform_window: &dyn crate::PlatformWindow) -> bool {
    crate::fast::layers::COMPILED && platform_window.rasterizes_layer_tiles()
}

/// `current`, the mode a window goes to from `previous`, with the window
/// counted for the hooks of scroll layers that have no window to ask: they
/// work while a window of the thread has scroll layers on.
fn counted(previous: FastMode, current: FastMode) -> FastMode {
    match (previous.scroll_layers, current.scroll_layers) {
        (false, true) => crate::fast::layers::turned_on(),
        (true, false) => crate::fast::layers::turned_off(),
        _ => {}
    }
    current
}

impl Drop for WindowFastMode {
    fn drop(&mut self) {
        counted(self.mode, FastMode::OFF);
    }
}

/// Releases what the mechanisms that `previous` had on and `current` has off
/// own. Each mechanism adds its release here as it lands.
fn release_disabled(window: &mut Window, previous: FastMode, current: FastMode) {
    if previous.scroll_layers && !current.scroll_layers {
        crate::fast::layers::release(window);
    }
}

impl Window {
    /// Draws the frame about to be drawn from scratch when what decides the
    /// kind of sprite text is painted as changed since the last frame: the
    /// application's text rendering mode, the window's background appearance,
    /// or what its renderer can do. Frames gpux draws without fast mode paint
    /// every element again and follow such a change by themselves; a scene
    /// that retention replays would keep the old kind of sprite until its
    /// content changed.
    pub(crate) fn refresh_for_text_policy(&mut self) {
        if !self.fast_mode.layout_retention() {
            self.fast_mode.text_policy = None;
            return;
        }
        let policy = TextPolicy {
            mode: self.text_rendering_mode.get(),
            background: self.platform_window.background_appearance(),
            subpixel_supported: self.platform_window.is_subpixel_rendering_supported(),
        };
        if let Some(previous) = self.fast_mode.text_policy.replace(policy)
            && previous != policy
        {
            self.refresh();
        }
    }

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
    /// The mode is [normalized](FastMode::normalized) first, and scroll
    /// layers stay off in a window whose renderer does not rasterize their
    /// tiles. If that changes anything, what the mechanisms turned off kept between frames is
    /// released and the window is refreshed, so the next frame is drawn from
    /// scratch.
    pub fn set_fast_mode(&mut self, mode: FastMode) {
        let previous = self.fast_mode.get();
        let current = mode.normalized_for(layers_drawn_by(self.platform_window.as_ref()));
        if previous == current {
            return;
        }
        self.fast_mode.mode = counted(previous, current);
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
        self.fast_glyph_bounds.forget();
        self.refresh();
    }

    /// Snaps scroll offsets, glyph origins and paths as scroll layers do,
    /// with scroll layers off: what a window drawn with them on is compared
    /// with, since the snapping moves what is drawn by fractions of a
    /// device pixel whether or not a layer is composited.
    #[cfg(any(test, feature = "test-support"))]
    pub fn force_layer_snapping(&mut self, forced: bool) {
        if self.fast_mode.snapping_forced != forced {
            self.fast_mode.snapping_forced = forced;
            self.refresh();
        }
    }

    /// What this window's fast-mode mechanisms hold on to between frames.
    #[cfg(any(test, feature = "test-support"))]
    pub fn fast_owned_state(&self) -> FastOwnedState {
        let engine = self.layout_engine.as_ref();
        FastOwnedState {
            kept_layout_nodes: engine.map_or(0, |engine| engine.retention.kept_count()),
            layout_engine_nodes: engine.map_or(0, |engine| engine.node_count()),
            layer_tiles: crate::fast::layers::owned(self),
            ..FastOwnedState::default()
        }
    }
}
