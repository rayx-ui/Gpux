//! Accessibility support, provided by [AccessKit][accesskit].
//!
//! There are user-facing guide-level docs [here](crate::_accessibility).
//!
//! ## Architecture
//!
//! ```text
//!                              ┌────────────────────────────────┐   ┌─────────────────────┐
//!                           ┌─▶│ AccessKit Adapter (MacOS)      │◀─▶│ MacOS System APIs   │
//!                           │  └────────────────────────────────┘   └─────────────────────┘
//!                           │
//! ┌──────┐   ┌───────────┐  │  ┌────────────────────────────────┐   ┌─────────────────────┐
//! │ GPUI │◀─▶│ AccessKit │◀─┼─▶│ AccessKit Adapter (Windows)    │◀─▶│ Windows System APIs │
//! └──────┘   └───────────┘  │  └────────────────────────────────┘   └─────────────────────┘
//!                           │
//!                           │  ┌────────────────────────────────┐   ┌─────────────────────┐
//!                           └─▶│ AccessKit Adapter (Linux)      │◀─▶│ dbus                │
//!                              └────────────────────────────────┘   └─────────────────────┘
//! ```
//!
//! In order for GPUI apps to be usable for people using assistive technology,
//! we must do a few things:
//! - Inform the system when the UI changes meaningfully. This includes:
//!   - Reporting new/removed/changed UI elements
//!   - *Not* reporting irrelevant UI changes, e.g. an invisible `div()` being
//!     added.
//!   - Reporting the appearance and capabilities of each UI element. For example:
//!     - What does this piece of text say?
//!     - How far along is this progress bar?
//!     - Can this node be focused?
//!     - Can this node have a value directly assigned? (e.g. a slider)
//! - Allowing the system to interact with the UI by dispatching actions to
//!   nodes. Note that AccessKit has its own [`Action`] type, which is not the
//!   [`crate::Action`] trait.
//! - Activate and deactivate accessibility features when requested by the
//!   system.
//!
//! Activating and deactivating at the right time is trivial, so I won't go into
//! detail here. The other two are almost orthogonal in implementation.
//!
//! The state for both lives in the [`A11y`] struct in this module.
//!
//! ### Reporting UI changes
//!
//! Every frame, we build a [`TreeUpdate`] and send it to the platform-specific
//! adapter. A [`TreeUpdate`] is a representation of a subset of the UI tree.
//! When the adapter receives the update, it diffs it against the previous
//! update, and calls platform-specific APIs to inform screen readers about the
//! changes. Nodes may have been created, destroyed, or updated.
//!
//! Each node has an ID, and this ID *should* be stable across frames. If a
//! node's ID changes, then, from AccessKit's point of view, it is a different
//! node.
//!
//! We derive the node ID from the [`GlobalElementId`] in
//! [`GlobalElementId::accesskit_node_id`]. Nodes without [`GlobalElementId`]s
//! cannot produce an AccessKit [`NodeId`], and so are not included in the
//! accessibility tree. We try to warn when using accessibility APIs on
//! [`div()`] without setting an ID.
//!
//! This all happens in [`Drawable::prepaint`]. The [`A11y`] struct maintains a
//! stack of nodes during prepainting, which we can use to calculate the
//! [`NodeId`]s, and record parent-child relationships. Once all [`Element`]s in
//! a frame have been prepainted, we send the resulting [`TreeUpdate`] object to
//! the adapter and the screen reader can announce the changes.
//!
//! #### Synthetic children
//!
//! Additionally, some nodes can register "synthetic children" using
//! [`Element::a11y_synthetic_children`]. Normally, one accesskit node is pushed
//! for every [`Element`] with a role and id. However, sometimes a single
//! element may want to produce many accesskit nodes. These extra nodes are
//! referred to as "synthetic children" of the element providing a non-default
//! [`Element::a11y_synthetic_children`] implementation.
//!
//! The user is provided a builder-style API using [`A11ySubtreeBuilder`], which
//! allows them to create push nodes that are children of the current node, as
//! well as modify the current node itself.
//!
//! GPUI calls this callback *after* prepainting (and just before popping the
//! corresponding element), since this step may need prepaint information to be
//! available. In the future, we may want to add prepaint information more
//! generally to [`Element::write_a11y_info`], but for now that's not necessary.
//!
//! ### Responding to actions
//!
//! On adapter creation, we provide a callback to the adapter, which can be used
//! to dispatch actions. This callback forwards to [`A11y::action_listeners`], a
//! mapping from [`NodeId`]s to action handlers (basically just `Box<dyn
//! Fn()>`).
//!
//! This is populated in:
//! - [`Window::on_a11y_action`], which is called by:
//! - [`Interactivity::paint`], which is called by:
//! - [`StatefulInteractiveElement::on_a11y_action`], which is a public-facing API
//!
//! These are cleared at the start of a frame, and re-populated during painting.
//!
//! [`NodeId`]: accesskit::NodeId

use crate::*;

pub(crate) mod debug;
pub mod roles;

use super::WindowInvalidator;
use crate::{App, Bounds, FocusId, Pixels, SharedString, Window};
use accesskit::{Action, NodeId, TreeUpdate};
use collections::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// The fixed AccessKit node ID used for the root of every window's a11y tree.
pub(crate) const ROOT_NODE_ID: NodeId = NodeId(0);

/// The tree information every gpux tree update carries: the window root and gpux as toolkit.
pub(crate) fn tree_info() -> accesskit::TreeInfo {
    accesskit::TreeInfo {
        root: ROOT_NODE_ID,
        toolkit_name: Some("gpux".to_string()),
        toolkit_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    }
}

/// A listener for an accessibility action on a specific node.
pub(crate) type A11yActionListener =
    Box<dyn FnMut(Option<&accesskit::ActionData>, &mut Window, &mut App) + 'static>;

/// When gpux exposes its windows to screen readers, set per application with
/// [`Application::with_accessibility_mode`](crate::Application::with_accessibility_mode) or
/// [`App::set_accessibility_mode`], or read once from `GPUX_ACCESSIBILITY` (`auto`, `always`,
/// `off`) when the application is created. Explicit calls win over the variable.
///
/// A window builds its accessibility tree for a frame when an activation source asks for it: the
/// platform source (a screen-reader adapter that activated the tree) or the inspection source
/// ([`Window::begin_a11y_inspection`], which tests and the devtools use). The mode decides how the
/// platform source counts; inspection works in every mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AccessibilityMode {
    /// The tree follows the platform's assistive technology and inspection (the default).
    #[default]
    Auto,
    /// The tree is built on every frame of every window, with no assistive technology. It still
    /// reaches the platform's adapter only while that adapter has activated it.
    Always,
    /// Screen readers are refused: an adapter that was active receives one last tree holding
    /// only the window's root, then nothing. Inspection still builds the tree.
    Off,
}

impl AccessibilityMode {
    /// The environment variable read once when an application is created.
    pub const ENV: &'static str = "GPUX_ACCESSIBILITY";

    /// Parses a `GPUX_ACCESSIBILITY` value (`auto`, `always`, `off`, in any case).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "always" => Some(Self::Always),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    /// The mode `GPUX_ACCESSIBILITY` names, or `Auto` when it is unset or unknown (an unknown
    /// value is logged).
    pub(crate) fn from_env() -> Self {
        match std::env::var(Self::ENV) {
            Ok(value) => Self::parse(&value).unwrap_or_else(|| {
                log::warn!(
                    "{}={value:?} is not one of auto, always, off; using auto",
                    Self::ENV
                );
                Self::Auto
            }),
            Err(_) => Self::Auto,
        }
    }
}

/// Keeps a window's inspection source on while it lives; see
/// [`Window::begin_a11y_inspection`].
///
/// Dropping the last guard of a window turns the source off and requests a frame, so the window
/// stops building its tree unless another source asks for it.
#[must_use = "inspection stops when the guard is dropped"]
pub struct A11yInspection {
    state: Option<Rc<InspectionState>>,
    invalidator: Option<WindowInvalidator>,
}

/// The inspection source a window shares with its guards.
#[derive(Default)]
pub(crate) struct InspectionState {
    /// The number of live guards.
    count: Cell<usize>,
    /// A guard started or stopped while the window was drawing, so the redraw it
    /// requests waits for the end of that draw.
    refresh_after_frame: Cell<bool>,
}

impl InspectionState {
    /// Requests a frame now, or after the draw in progress.
    fn request_frame(&self, invalidator: &WindowInvalidator) {
        if invalidator.not_drawing() {
            invalidator.set_dirty(true);
        } else {
            self.refresh_after_frame.set(true);
        }
    }
}

impl A11yInspection {
    /// Whether accessibility is disabled for the application
    /// ([`Application::new_inaccessible`](crate::Application::new_inaccessible)), in which case
    /// this guard turns nothing on and the window never builds a tree.
    pub fn accessibility_disabled(&self) -> bool {
        self.state.is_none()
    }
}

impl Clone for A11yInspection {
    fn clone(&self) -> Self {
        if let Some(state) = &self.state {
            state.count.set(state.count.get() + 1);
        }
        Self {
            state: self.state.clone(),
            invalidator: self.invalidator.clone(),
        }
    }
}

impl Drop for A11yInspection {
    fn drop(&mut self) {
        let Some(state) = &self.state else {
            return;
        };
        let remaining = state.count.get().saturating_sub(1);
        state.count.set(remaining);
        if remaining == 0
            && let Some(invalidator) = &self.invalidator
        {
            state.request_frame(invalidator);
        }
    }
}

/// Per-window accessibility state.
///
/// Manages the AccessKit tree that is built each frame and the mappings
/// needed to dispatch incoming action requests back to the right elements.
pub(crate) struct A11y {
    /// Whether accessibility has been [forcibly disabled] for this window.
    ///
    /// [forcibly disabled]: crate::Application::new_inaccessible
    force_disabled: bool,
    /// The platform source: whether a screen-reader adapter has activated the tree.
    ///
    /// Updated by AccessKit using callbacks provided to the adapter. Can change
    /// halfway through a frame.
    platform_flag: Arc<AtomicBool>,
    /// The inspection source: the number of live [`A11yInspection`] guards.
    inspection: Rc<InspectionState>,
    /// The application's [`AccessibilityMode`], shared by all of its windows.
    mode: Rc<Cell<AccessibilityMode>>,
    /// Whether the tree is built for *this specific frame*.
    ///
    /// At the start of each frame, [`Self::sync_active_flag`] combines the
    /// sources and the mode into this flag, which decides whether we construct
    /// a [`TreeUpdate`] for this frame. It's important that this value is stable
    /// within a frame, because the builder API exposed by this type maintains a
    /// stack of nodes and each must be pushed and popped exactly once.
    active_this_frame: bool,
    /// Whether the platform source covered the start of this frame (the platform
    /// flag was on and the mode is not [`AccessibilityMode::Off`]). The frame's
    /// tree reaches the adapter only when the platform source still covers its
    /// end ([`Self::platform_covers_frame_end`]).
    platform_this_frame: bool,
    /// The mode in effect for this frame, sampled with the sources.
    mode_this_frame: AccessibilityMode,
    /// Whether the adapter holds more than the root, so `Off` sends it a
    /// root-only tree once.
    platform_exposed: bool,
    /// Whether this frame's tree is built for inspection (containers and the id map).
    inspecting_this_frame: bool,
    /// Whether this state builds a frame fast mode's verification draws from scratch beside
    /// the real one; its tree never reaches the platform.
    shadow: bool,
    pub(crate) nodes: A11yNodeBuilder,
    pub(crate) focus_ids: FxHashMap<NodeId, FocusId>,
    pub(crate) node_bounds: FxHashMap<NodeId, Bounds<Pixels>>,
    /// Beside the builder's open nodes: each one's bounds in window pixels, recorded in its
    /// `Push` event when it pops, hidden or not, so a row a layer held off screen keeps them
    /// when it is replayed shown.
    open_bounds: SmallVec<[Bounds<Pixels>; 16]>,
    /// The action handlers the frame being drawn registered as it painted, in paint order;
    /// a reused stretch of paint moves the last frame's here (see [`crate::PaintIndex`]).
    pub(crate) listeners: Vec<A11yListener>,
    /// The handlers of the last frame drawn, which requests are dispatched to.
    pub(crate) rendered_listeners: Vec<A11yListener>,
    /// The last frame's event log, which reused content replays.
    pub(crate) rendered_events: Vec<A11yEvent>,
    /// How far content replayed now has moved since it recorded its events (a composited
    /// layer's scroll since the last frame).
    pub(crate) replay_offset: Point<Pixels>,
    /// The containers replay opened that the window is not inspecting: their pushes and pops
    /// are skipped. Beside each replayed open node.
    replay_skipped: Vec<bool>,
    /// The scroll containers replay opened, with the one open before each.
    replay_scroll_parents: Vec<Option<u32>>,
    /// The inspection container that held focus in the last frame.
    last_container_focus: Option<NodeId>,
    /// While inspecting: each node of the last frame's tree mapped to the global element id
    /// of the element it belongs to (text leaves and synthetic children map to their owner).
    id_map: FxHashMap<NodeId, GlobalElementId>,
    /// The scroll containers the frame's nodes sit in, innermost last in each chain.
    pub(crate) scroll_targets: Vec<A11yScrollTarget>,
    /// The scroll container open around the element being prepainted (an index into
    /// [`Self::scroll_targets`]).
    pub(crate) current_scroll_target: Option<u32>,
    /// The innermost scroll container of each node, for `ScrollIntoView`.
    pub(crate) node_scroll_targets: FxHashMap<NodeId, u32>,
    /// The window's title, used to label the root node so assistive
    /// technology can tell windows apart.
    window_title: Option<SharedString>,
    /// The focus id we most recently reported as having no accessibility node,
    /// used to log at most once per focus change rather than every frame.
    last_focus_without_node: Option<FocusId>,
    /// Retains the last tree update (and, in debug builds, per-node provenance)
    /// so it can be dumped via [`crate::Window::debug_a11y_tree_json`].
    debug: debug::A11yDebug,
    /// Maps a view's [`EntityId`] to its `Render` type name
    #[cfg(debug_assertions)]
    pub(crate) view_type_names: FxHashMap<EntityId, &'static str>,
}

impl A11y {
    pub(crate) fn new(
        platform_flag: Arc<AtomicBool>,
        mode: Rc<Cell<AccessibilityMode>>,
        force_disabled: bool,
        window_title: Option<SharedString>,
    ) -> Self {
        Self {
            force_disabled,
            platform_flag,
            inspection: Rc::new(InspectionState::default()),
            mode,
            active_this_frame: false,
            platform_this_frame: false,
            mode_this_frame: AccessibilityMode::Auto,
            platform_exposed: false,
            inspecting_this_frame: false,
            shadow: false,
            nodes: A11yNodeBuilder::new(),
            focus_ids: FxHashMap::default(),
            node_bounds: FxHashMap::default(),
            open_bounds: SmallVec::new(),
            listeners: Vec::new(),
            rendered_listeners: Vec::new(),
            rendered_events: Vec::new(),
            replay_offset: Point::default(),
            replay_skipped: Vec::new(),
            replay_scroll_parents: Vec::new(),
            last_container_focus: None,
            id_map: FxHashMap::default(),
            scroll_targets: Vec::new(),
            current_scroll_target: None,
            node_scroll_targets: FxHashMap::default(),
            window_title,
            last_focus_without_node: None,
            debug: debug::A11yDebug::default(),
            #[cfg(debug_assertions)]
            view_type_names: FxHashMap::default(),
        }
    }

    /// Logs (once per focus change) that the focused element is not exposed to
    /// assistive technology because it has no accessibility node. When this
    /// happens, screen readers fall back to announcing the whole window instead
    /// of the focused element. The fix is to give the element both an
    /// `.id(...)` and a `.role(...)`.
    pub(crate) fn note_focus_without_node(&mut self, focus_id: FocusId, reason: &str) {
        if self.last_focus_without_node != Some(focus_id) {
            self.last_focus_without_node = Some(focus_id);
            log::info!(
                "a11y: focused element ({focus_id:?}) has no accessibility node \
                 ({reason}); assistive technology will announce the whole window \
                 instead. Give it both an `.id(...)` and a `.role(...)` to expose it."
            );
        }
    }

    pub(crate) fn set_window_title(&mut self, title: impl Into<SharedString>) {
        self.window_title = Some(title.into());
    }

    /// Samples the sources and the mode for the frame that is starting, so that
    /// [`Self::is_active`] is stable for the whole frame. A source switched in the
    /// middle of a frame takes effect on the next one.
    ///
    /// See the docs for [`Self::platform_flag`] and [`Self::active_this_frame`]
    /// for more commentary.
    ///
    /// Returns whether the frame must be drawn from scratch: it is the first to build the tree,
    /// or the first to build it for inspection, so content drawn from last frame recorded no
    /// tree (or no containers and id map) to carry.
    pub(crate) fn sync_active_flag(&mut self) -> bool {
        let (was_active, was_inspecting) = (self.active_this_frame, self.inspecting_this_frame);
        let enabled = self.is_enabled();
        let mode = self.mode.get();
        self.mode_this_frame = mode;
        self.platform_this_frame = enabled && self.platform_source_on(mode);
        self.inspecting_this_frame = enabled && self.inspection.count.get() > 0;
        self.active_this_frame = self.platform_this_frame
            || self.inspecting_this_frame
            || (enabled && mode == AccessibilityMode::Always);
        (self.active_this_frame && !was_active) || (self.inspecting_this_frame && !was_inspecting)
    }

    fn platform_source_on(&self, mode: AccessibilityMode) -> bool {
        mode != AccessibilityMode::Off && self.platform_flag.load(Ordering::SeqCst)
    }

    /// Whether any source asks for the tree right now (not only at the start of the frame).
    pub(crate) fn any_source_on(&self) -> bool {
        if !self.is_enabled() {
            return false;
        }
        let mode = self.mode.get();
        mode == AccessibilityMode::Always
            || self.inspection.count.get() > 0
            || self.platform_source_on(mode)
    }

    /// Whether the platform source still covers the end of a frame whose start it
    /// covered; only then does the frame's tree reach the adapter.
    pub(crate) fn platform_covers_frame_end(&self) -> bool {
        !self.shadow
            && self.platform_this_frame
            && self.is_enabled()
            && self.platform_source_on(self.mode_this_frame)
    }

    /// Whether this frame's tree is built for inspection.
    #[allow(dead_code)]
    pub(crate) fn is_inspecting(&self) -> bool {
        self.inspecting_this_frame
    }

    /// Called at the end of every frame with whether its tree reached the adapter.
    /// Returns the root-only tree to send when [`AccessibilityMode::Off`] has just
    /// hidden the window from an adapter that was showing it.
    pub(crate) fn end_platform_frame(&mut self, sent_full_tree: bool) -> Option<TreeUpdate> {
        if self.shadow {
            return None;
        }
        if sent_full_tree {
            self.platform_exposed = true;
            return None;
        }
        let adapter_active = self.is_enabled() && self.platform_flag.load(Ordering::SeqCst);
        if !adapter_active {
            // An inactive adapter forgets the tree; the next activation starts from its root.
            self.platform_exposed = false;
            return None;
        }
        if self.platform_exposed && self.mode.get() == AccessibilityMode::Off {
            self.platform_exposed = false;
            return Some(self.root_only_tree());
        }
        None
    }

    /// A tree holding only the window's root, focused.
    pub(crate) fn root_only_tree(&self) -> TreeUpdate {
        let mut root = accesskit::Node::new(accesskit::Role::Window);
        if let Some(title) = &self.window_title {
            root.set_label(title.to_string());
        }
        TreeUpdate {
            nodes: vec![(ROOT_NODE_ID, root)],
            tree: Some(tree_info()),
            tree_id: accesskit::TreeId::ROOT,
            focus: ROOT_NODE_ID,
        }
    }

    /// Starts the inspection source; see [`Window::begin_a11y_inspection`].
    /// Returns the guard and whether it is the first live guard.
    pub(crate) fn begin_inspection(
        &mut self,
        invalidator: &WindowInvalidator,
    ) -> (A11yInspection, bool) {
        if !self.is_enabled() {
            return (
                A11yInspection {
                    state: None,
                    invalidator: None,
                },
                false,
            );
        }
        let count = &self.inspection.count;
        let first = count.get() == 0;
        count.set(count.get() + 1);
        if first {
            self.inspection.request_frame(invalidator);
        }
        (
            A11yInspection {
                state: Some(self.inspection.clone()),
                invalidator: Some(invalidator.clone()),
            },
            first,
        )
    }

    /// Whether a guard asked for a frame while the window was drawing; the window
    /// requests it once the draw is over.
    pub(crate) fn take_refresh_after_frame(&self) -> bool {
        self.inspection.refresh_after_frame.replace(false)
    }

    /// The last frame's complete tree, while a source asks for the tree.
    pub(crate) fn last_tree(&self) -> Option<&TreeUpdate> {
        if !self.any_source_on() {
            return None;
        }
        self.debug.last_tree_update()
    }

    /// Forgets the last frame's tree (a frame was drawn without one, or a new
    /// inspection must not read a stale one).
    pub(crate) fn forget_last_tree(&mut self) {
        self.debug.forget_last_tree_update();
        self.id_map.clear();
    }

    /// Opens a scroll container around the children being prepainted; returns the container
    /// that was open before it, to restore afterwards.
    pub(crate) fn enter_scroll_target(
        &mut self,
        scroll: A11yScroll,
        context: ScrollContainerContext,
    ) -> Option<u32> {
        let reach = |scrolls: bool, content: Pixels, port: Pixels| {
            if scrolls {
                (content - port).max(Pixels::ZERO)
            } else {
                Pixels::ZERO
            }
        };
        let max = Point::new(
            reach(
                context.overflow.x == crate::Overflow::Scroll,
                context.content_bounds.size.width,
                context.scrollport_bounds.size.width,
            ),
            reach(
                context.overflow.y == crate::Overflow::Scroll,
                context.content_bounds.size.height,
                context.scrollport_bounds.size.height,
            ),
        );
        self.enter_scroll(
            scroll,
            context.scrollport_bounds,
            context.scroll_offset,
            max,
        )
    }

    fn enter_scroll(
        &mut self,
        scroll: A11yScroll,
        scrollport: Bounds<Pixels>,
        offset: Point<Pixels>,
        max: Point<Pixels>,
    ) -> Option<u32> {
        self.nodes.events.push(A11yEvent::EnterScroll {
            scroll: scroll.clone(),
            scrollport,
            offset,
            max,
        });
        let parent = self.current_scroll_target;
        self.scroll_targets.push(A11yScrollTarget {
            scroll,
            scrollport,
            offset,
            max,
            parent,
        });
        self.current_scroll_target = Some(self.scroll_targets.len() as u32 - 1);
        parent
    }

    /// Closes the scroll container [`Self::enter_scroll_target`] opened, restoring `parent`.
    pub(crate) fn exit_scroll_target(&mut self, parent: Option<u32>) {
        self.nodes.events.push(A11yEvent::ExitScroll);
        self.current_scroll_target = parent;
    }

    /// Pushes an element's node (see [`A11yNodeBuilder::push_element`]), recording its bounds
    /// for actions and the scroll container it sits in.
    pub(crate) fn push_element(
        &mut self,
        id: NodeId,
        node: accesskit::Node,
        path: &GlobalElementId,
        container: bool,
        bounds: Bounds<Pixels>,
    ) -> bool {
        let hidden = self.nodes.in_hidden();
        if !self.nodes.push_element(id, node, path, container) {
            return false;
        }
        self.open_bounds.push(bounds);
        if !hidden {
            self.node_bounds.insert(id, bounds);
            if let Some(target) = self.current_scroll_target {
                self.node_scroll_targets.insert(id, target);
            }
        }
        true
    }

    /// Sets where the element being prepainted paints (a fixed or sticky element paints away
    /// from its layout bounds), where actions reach its node.
    pub(crate) fn set_visual_bounds(&mut self, id: NodeId, bounds: Bounds<Pixels>) {
        if let Some(open) = self.open_bounds.last_mut() {
            *open = bounds;
        }
        if let Some(target) = self.node_bounds.get_mut(&id) {
            *target = bounds;
        }
    }

    /// Pops the node of the element being prepainted.
    pub(crate) fn pop_node(&mut self, _id: NodeId) {
        let bounds = self.open_bounds.pop();
        self.nodes.pop(bounds);
    }

    /// Registers the handler of `action` on `node`, painted now.
    pub(crate) fn add_listener(
        &mut self,
        node: NodeId,
        action: A11yListenerAction,
        listener: A11yActionListener,
    ) {
        let shown = !self.nodes.in_hidden();
        self.listeners.push(A11yListener {
            node,
            action,
            listener: Some(listener),
            shown,
        });
    }

    /// Replays the last frame's events `range` into the tree being built, moved by
    /// [`Self::replay_offset`]; `focus` is the window's focus, which focusable nodes are
    /// checked against again. Replay records the events again for the next frame.
    pub(crate) fn replay(
        &mut self,
        range: Range<usize>,
        focus: Option<FocusId>,
        scale_factor: f32,
    ) {
        if range.is_empty() || !self.active_this_frame {
            return;
        }
        let events = mem::take(&mut self.rendered_events);
        let offset = self.replay_offset;
        let moved = offset != Point::default();
        let move_rect = |rect: accesskit::Rect| {
            if moved {
                let dx = (offset.x.as_f32() * scale_factor) as f64;
                let dy = (offset.y.as_f32() * scale_factor) as f64;
                accesskit::Rect::new(rect.x0 + dx, rect.y0 + dy, rect.x1 + dx, rect.y1 + dy)
            } else {
                rect
            }
        };
        let move_node = |mut node: accesskit::Node| {
            if moved && let Some(rect) = node.bounds() {
                node.set_bounds(move_rect(rect));
            }
            node
        };
        for event in events.get(range).unwrap_or_default() {
            match event {
                A11yEvent::Push {
                    id,
                    node,
                    bounds,
                    path,
                    container,
                } => {
                    let skip = *container && !self.nodes.is_inspecting();
                    self.replay_skipped.push(skip);
                    if skip {
                        // Kept in the log, so the copy is as long as what it copied (fast
                        // mode shifts the records nested in it by that), and skipped again.
                        self.nodes.events.push(event.clone());
                        continue;
                    }
                    let node = move_node(node.clone().unwrap_or_else(|| {
                        accesskit::Node::new(accesskit::Role::GenericContainer)
                    }));
                    let bounds = bounds.map(|bounds| bounds + offset).unwrap_or_default();
                    let path = path.clone().unwrap_or_default();
                    if !self.push_element(*id, node, &path, *container, bounds) {
                        // A duplicate: it never opened, so its pop is skipped too. Logged as
                        // it was, for the copy to be as long as what it copied.
                        *self.replay_skipped.last_mut().unwrap() = true;
                        self.nodes.events.push(event.clone());
                    }
                }
                A11yEvent::Pop => {
                    if self.replay_skipped.pop().unwrap_or(true) {
                        self.nodes.events.push(A11yEvent::Pop);
                    } else {
                        let id = *self
                            .nodes
                            .ids_stack
                            .last()
                            .expect("a replayed node is open");
                        self.pop_node(id);
                    }
                }
                A11yEvent::TextLeaf { node } => {
                    if let (Some(text), Some(rect)) = (node.value(), node.bounds()) {
                        let text = text.to_string();
                        self.nodes.push_text_leaf(&text, move_rect(rect));
                    }
                }
                A11yEvent::Leaf { id, node } => {
                    self.nodes.push_leaf(*id, move_node(node.clone()));
                }
                A11yEvent::Relation(relation) => self.nodes.push_relation(relation.clone()),
                A11yEvent::Focusable {
                    id,
                    focus: node_focus,
                } => {
                    self.set_focusable_node(*id, *node_focus, focus == Some(*node_focus));
                }
                A11yEvent::ActiveDescendant { id } => self.set_active_descendant(*id),
                A11yEvent::EnterScroll {
                    scroll,
                    scrollport,
                    offset: scroll_offset,
                    max,
                } => {
                    let parent = self.enter_scroll(
                        scroll.clone(),
                        *scrollport + offset,
                        *scroll_offset,
                        *max,
                    );
                    self.replay_scroll_parents.push(parent);
                }
                A11yEvent::ExitScroll => {
                    let parent = self.replay_scroll_parents.pop().flatten();
                    self.exit_scroll_target(parent);
                }
            }
        }
        self.rendered_events = events;
    }

    /// Moves the last frame's handlers `range` into the frame being drawn, where painting
    /// what registered them would register them again.
    pub(crate) fn reuse_listeners(&mut self, range: Range<usize>) {
        if range.is_empty() || !self.active_this_frame {
            return;
        }
        let hidden = self.nodes.in_hidden();
        let end = range.end.min(self.rendered_listeners.len());
        for index in range.start.min(end)..end {
            let entry = &mut self.rendered_listeners[index];
            self.listeners.push(A11yListener {
                node: entry.node,
                action: entry.action,
                listener: entry.listener.take(),
                shown: !hidden,
            });
        }
    }

    /// A fresh state with this one's sources and mode, for a frame verification draws from
    /// scratch beside the real one (`fast::verify::retention`), sampled for that frame.
    #[cfg(feature = "diagnostics")]
    pub(crate) fn shadow(&self) -> A11y {
        let mut shadow = A11y::new(
            self.platform_flag.clone(),
            self.mode.clone(),
            self.force_disabled,
            self.window_title.clone(),
        );
        shadow.inspection = self.inspection.clone();
        shadow.shadow = true;
        shadow
    }

    /// Ends a frame [`Self::shadow`] began, as a real frame ends.
    #[cfg(feature = "diagnostics")]
    pub(crate) fn end_shadow(&mut self) {
        // The window's `draw_roots` began and ended the shadow's tree as a frame's.
        self.swap_frames();
    }

    /// What the last frame's tree is, for fast mode's frame description: every node in tree
    /// order with its properties, the focus, the focusable nodes, the nodes with action
    /// handlers and, while inspecting, the elements the nodes belong to.
    #[cfg(any(test, feature = "test-support", feature = "diagnostics"))]
    pub(crate) fn describe(&self, lines: &mut Vec<String>) {
        let Some(update) = self.debug.last_tree_update() else {
            lines.push("a11y none".into());
            return;
        };
        let nodes: FxHashMap<NodeId, &accesskit::Node> =
            update.nodes.iter().map(|(id, node)| (*id, node)).collect();
        lines.push(format!(
            "a11y focus {:?} container {:?}",
            update.focus, self.last_container_focus
        ));
        let mut stack = vec![(ROOT_NODE_ID, 0usize)];
        while let Some((id, depth)) = stack.pop() {
            let Some(node) = nodes.get(&id) else {
                lines.push(format!("a11y {depth} {id:?} missing"));
                continue;
            };
            lines.push(format!("a11y {depth} {id:?} {node:?}"));
            stack.extend(
                node.children()
                    .iter()
                    .rev()
                    .map(|child| (*child, depth + 1)),
            );
        }
        let mut focusables: Vec<String> = self
            .focus_ids
            .keys()
            .filter(|id| self.nodes.has_node(**id))
            .map(|id| format!("a11y focusable {id:?}"))
            .collect();
        focusables.sort();
        lines.extend(focusables);
        let mut listeners: Vec<String> = self
            .rendered_listeners
            .iter()
            .filter(|entry| entry.shown && self.nodes.has_node(entry.node))
            .map(|entry| format!("a11y listener {:?} {:?}", entry.node, entry.action))
            .collect();
        listeners.sort();
        lines.extend(listeners);
        // Where actions land (Click at the center, ScrollIntoView through the containers).
        let mut targets: Vec<String> = self
            .node_bounds
            .iter()
            .filter(|(id, _)| self.nodes.has_node(**id))
            .map(|(id, bounds)| {
                let scroll = self.node_scroll_targets.get(id).and_then(|index| {
                    self.scroll_targets
                        .get(*index as usize)
                        .map(|target| (target.scrollport, target.offset))
                });
                format!("a11y bounds {id:?} {bounds:?} scroll {scroll:?}")
            })
            .collect();
        targets.sort();
        lines.extend(targets);
        let mut elements: Vec<String> = self
            .id_map
            .iter()
            .map(|(id, path)| format!("a11y element {id:?} {path}"))
            .collect();
        elements.sort();
        lines.extend(elements);
    }

    /// A frame that builds no tree records nothing: content reused from it replays nothing.
    pub(crate) fn clear_frame(&mut self) {
        self.nodes.events.clear();
        self.listeners.clear();
    }

    /// The last frame's records become the rendered ones and the frame being drawn starts
    /// empty, as the window's frames do.
    pub(crate) fn swap_frames(&mut self) {
        mem::swap(&mut self.listeners, &mut self.rendered_listeners);
        self.listeners.clear();
        mem::swap(&mut self.nodes.events, &mut self.rendered_events);
        self.nodes.events.clear();
    }

    /// Where the frame's records stand, for [`Self::rollback`].
    pub(crate) fn checkpoint(&self) -> A11yCheckpoint {
        let (
            all_nodes,
            events,
            id_map_log,
            pending_relations,
            stack_depth,
            top_children,
            top_ordinal,
            focus,
            active_descendant,
        ) = self.nodes.checkpoint();
        A11yCheckpoint {
            all_nodes,
            events,
            id_map_log,
            pending_relations,
            scroll_targets: self.scroll_targets.len(),
            current_scroll_target: self.current_scroll_target,
            stack_depth,
            top_children,
            top_ordinal,
            focus,
            focus_handle: self.nodes.focus_handle,
            active_descendant,
        }
    }

    /// Discards what was recorded since `checkpoint`, as a prepaint that is tried again
    /// discards its other records.
    pub(crate) fn rollback(&mut self, checkpoint: A11yCheckpoint) {
        if !self.active_this_frame {
            return;
        }
        let nodes = &mut self.nodes;
        debug_assert_eq!(
            nodes.ids_stack.len(),
            checkpoint.stack_depth,
            "a transaction is balanced"
        );
        for (id, _) in nodes.all_nodes.drain(checkpoint.all_nodes..) {
            nodes.seen_ids.remove(&id);
            self.focus_ids.remove(&id);
            self.node_bounds.remove(&id);
            self.node_scroll_targets.remove(&id);
            nodes.containers.remove(&id);
            nodes.row_tags.remove(&id);
        }
        nodes.events.truncate(checkpoint.events);
        nodes.id_map_log.truncate(checkpoint.id_map_log);
        nodes
            .pending_relations
            .truncate(checkpoint.pending_relations);
        if let Some(top) = nodes.nodes_stack.last_mut() {
            let mut children = top.children().to_vec();
            children.truncate(checkpoint.top_children);
            top.set_children(children);
        }
        if let Some(ordinal) = nodes.text_owner_ordinal() {
            *ordinal = checkpoint.top_ordinal;
        }
        nodes.focus = checkpoint.focus;
        nodes.focus_handle = checkpoint.focus_handle;
        nodes.active_descendant = checkpoint.active_descendant;
        self.scroll_targets.truncate(checkpoint.scroll_targets);
        self.current_scroll_target = checkpoint.current_scroll_target;
    }

    /// The inspection container that holds the window's focus in the last frame, if the
    /// focused element is one (it is not the tree's focus; see [`Self::set_focus`]).
    pub(crate) fn container_focus(&self) -> Option<NodeId> {
        self.any_source_on()
            .then_some(self.last_container_focus)
            .flatten()
    }

    /// The global element id the node `id` of the last inspected frame belongs to.
    pub(crate) fn element_id(&self, id: NodeId) -> Option<&GlobalElementId> {
        if !self.any_source_on() {
            return None;
        }
        self.id_map.get(&id)
    }

    /// Every node of the last inspected frame with the element it belongs to.
    pub(crate) fn element_ids(&self) -> impl Iterator<Item = (NodeId, &GlobalElementId)> {
        self.any_source_on()
            .then_some(&self.id_map)
            .into_iter()
            .flat_map(|map| map.iter().map(|(id, path)| (*id, path)))
    }

    pub(crate) fn is_enabled(&self) -> bool {
        !self.force_disabled
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active_this_frame
    }

    pub(crate) fn set_focusable(&mut self, node_id: NodeId, focus_id: FocusId) {
        self.nodes.events.push(A11yEvent::Focusable {
            id: node_id,
            focus: focus_id,
        });
        if !self.nodes.in_hidden() {
            self.focus_ids.insert(node_id, focus_id);
        }
    }

    /// Registers a focusable node, and the node as focused when `focused`.
    pub(crate) fn set_focusable_node(&mut self, node_id: NodeId, focus_id: FocusId, focused: bool) {
        self.set_focusable(node_id, focus_id);
        if focused && !self.nodes.in_hidden() {
            self.set_focus(node_id);
        }
    }

    /// Report `node_id` as the currently-focused node, if it is present in the
    /// tree.
    ///
    /// Must only be called once per frame.
    pub(crate) fn set_focus(&mut self, node_id: NodeId) {
        // A focused node must have been registered as focusable this frame.
        if !self.focus_ids.contains_key(&node_id) {
            if cfg!(debug_assertions) {
                panic!("set_focus called for a node that was not registered with set_focusable");
            } else {
                log::warn!(
                    "a11y: set_focus called for a node that was not registered with \
                     set_focusable ({node_id:?})"
                );
            }
        }
        let handle = self.focus_ids.get(&node_id).copied();
        if self.nodes.is_container(node_id) {
            // An inspection container never takes the tree's focus (screen readers see the
            // tree as without it), but inspection knows it holds the window's focus, unless
            // a node tracking the same focus handle holds it (a control's host around its
            // field).
            if handle.is_none() || self.nodes.focus_handle != handle {
                self.nodes.container_focus = Some(node_id);
            }
        } else if self.nodes.has_node(node_id) {
            // The focused element is properly exposed; reset the dedup so a
            // later focus on a node-less element logs again.
            self.last_focus_without_node = None;
            self.nodes.set_focus_by(node_id, handle);
            // A container around it that claimed the same handle gives the focus up.
            if handle.is_some()
                && self
                    .nodes
                    .container_focus
                    .is_some_and(|container| self.focus_ids.get(&container).copied() == handle)
            {
                self.nodes.container_focus = None;
            }
        } else {
            // The element registered a focus handle and an id, but never got a
            // node because it has no role.
            if let Some(focus_id) = self.focus_ids.get(&node_id).copied() {
                self.note_focus_without_node(focus_id, "it has an id but no role");
            }
        }
    }

    pub(crate) fn set_active_descendant(&mut self, node_id: NodeId) {
        self.nodes
            .events
            .push(A11yEvent::ActiveDescendant { id: node_id });
        if self.nodes.in_hidden() {
            return;
        }
        // The active descendant must be a descendant of the focused container,
        // not the focused node itself.
        if self.nodes.node_is_focused(node_id) {
            if cfg!(debug_assertions) {
                panic!("set_active_descendant called on the focused node");
            } else {
                log::warn!("a11y: set_active_descendant called on the focused node ({node_id:?})");
            }
            return;
        }
        if self.nodes.has_node(node_id)
            && !self.nodes.is_container(node_id)
            && self.nodes.focus_is_ancestor_of_current()
        {
            self.nodes.set_active_descendant(node_id);
        }
    }

    /// Clear per-frame state and push the root node to start a new frame.
    pub(crate) fn begin_frame(&mut self) {
        self.focus_ids.clear();
        self.node_bounds.clear();
        self.open_bounds.clear();
        self.listeners.clear();
        self.replay_skipped.clear();
        self.replay_scroll_parents.clear();
        self.replay_offset = Point::default();
        self.scroll_targets.clear();
        self.current_scroll_target = None;
        self.node_scroll_targets.clear();
        self.nodes
            .begin_frame_for(self.window_title.as_ref(), self.inspecting_this_frame);
    }

    /// Finalize the tree and produce a [`TreeUpdate`] for the platform adapter.
    pub(crate) fn end_frame(&mut self, frame: debug::FrameDebugInfo) -> TreeUpdate {
        let update = self.nodes.finalize();
        self.last_container_focus = self.nodes.container_focus;
        self.id_map.clear();
        if self.nodes.is_inspecting() {
            self.id_map.extend(self.nodes.id_map_log.iter().cloned());
        }
        self.debug.capture(
            &update,
            self.nodes.focus,
            self.nodes.active_descendant,
            self.window_title.as_ref(),
            frame,
        );
        #[cfg(debug_assertions)]
        self.debug.capture_node_info(&self.nodes.node_info);
        update
    }

    pub(crate) fn debug_tree_json(&self) -> Option<String> {
        self.debug.to_json()
    }
}

/// Which requests a registered handler answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum A11yListenerAction {
    Action(Action),
    Custom(i32),
}

/// A handler a frame registered as it painted.
pub(crate) struct A11yListener {
    pub(crate) node: NodeId,
    pub(crate) action: A11yListenerAction,
    pub(crate) listener: Option<A11yActionListener>,
    /// Whether the node is in the tree (a list row held off screen is not).
    pub(crate) shown: bool,
}

/// A scroll container that `ScrollIntoView` can move as a scroll of it would: a tracked
/// [`ScrollHandle`] through its programmatic path, an untracked one as its wheel listener
/// moves the offset its element keeps.
#[derive(Clone)]
pub(crate) struct A11yScrollTarget {
    pub(crate) scroll: A11yScroll,
    /// The scrollport in window coordinates.
    pub(crate) scrollport: Bounds<Pixels>,
    /// The scroll offset the frame drew with (non-positive, as [`ScrollHandle::offset`]).
    pub(crate) offset: Point<Pixels>,
    /// How far the container scrolls on each axis (zero on an axis it does not scroll).
    pub(crate) max: Point<Pixels>,
    /// The scroll container this one sits in.
    pub(crate) parent: Option<u32>,
}

#[derive(Clone)]
pub(crate) enum A11yScroll {
    /// A container that tracks a scroll handle, painted by `view`.
    Handle {
        handle: ScrollHandle,
        view: EntityId,
    },
    /// A container that keeps its own offset: `id` names it (for fast mode to tell the scroll
    /// apart, as from a wheel's) and `view` painted it.
    Offset {
        offset: Rc<std::cell::RefCell<Point<Pixels>>>,
        id: Option<GlobalElementId>,
        view: EntityId,
    },
}

/// Builder API for synthetic children. See the docs for
/// [`Element::a11y_synthetic_children`].
pub struct A11ySubtreeBuilder<'a> {
    parent_id: NodeId,
    nodes: &'a mut A11yNodeBuilder,
    /// Provenance of the real element whose `a11y_synthetic_children` is
    /// running.
    #[cfg(debug_assertions)]
    creator: debug::NodeCreator,
}

impl<'a> A11ySubtreeBuilder<'a> {
    pub(crate) fn new(parent_id: NodeId, nodes: &'a mut A11yNodeBuilder) -> Self {
        Self {
            parent_id,
            nodes,
            #[cfg(debug_assertions)]
            creator: debug::NodeCreator::default(),
        }
    }

    #[cfg(debug_assertions)]
    pub(crate) fn with_creator(mut self, creator: debug::NodeCreator) -> Self {
        self.creator = creator;
        self
    }

    /// Derive a [`NodeId`] for a synthetic child.
    ///
    /// The generated ID is based on the hash of `key`, as well as the parent's
    /// ID. This means that `key`s must be unique within the same
    /// [`Element::a11y_synthetic_children`] call, but may be duplicated across
    /// different calls.
    pub fn synthetic_node_id(&self, key: impl Hash) -> NodeId {
        let mut hasher = std::hash::DefaultHasher::default();
        self.parent_id.0.hash(&mut hasher);
        key.hash(&mut hasher);
        NodeId(hasher.finish())
    }

    /// Append a synthetic leaf node as a child of this element's node.
    ///
    /// Returns `false` if a node with this id is already present in the tree,
    /// in which case the node is discarded.
    pub fn push_child(&mut self, id: NodeId, node: accesskit::Node) -> bool {
        let pushed = self.nodes.push_leaf(id, node);
        #[cfg(debug_assertions)]
        if pushed {
            self.nodes.record_node_info(
                id,
                debug::NodeDebugInfo {
                    synthetic: true,
                    view: self.creator.view,
                    element_id: self.creator.element_id.clone(),
                    source_location: self.creator.source_location,
                },
            );
        }
        pushed
    }

    /// A mutable reference to the parent node.
    pub fn parent_node(&mut self) -> &mut accesskit::Node {
        self.nodes
            .current_node_mut()
            .expect("A11ySubtreeBuilder exists only while its element's node is on the stack")
    }
}

pub(crate) struct A11yNodeBuilder {
    ids_stack: SmallVec<[NodeId; 16]>,
    nodes_stack: SmallVec<[accesskit::Node; 16]>,
    /// This is the exact type required by accesskit, so we can't just make it a
    /// `HashMap<NodeId, Node>` to remove the need for `seen_ids`
    all_nodes: Vec<(NodeId, accesskit::Node)>,
    seen_ids: FxHashSet<NodeId>,
    /// The node that GPUI considers focused. Note that this may be different to
    /// what is reported to accesskit - see [`Self::active_descendant`]
    focus: Option<NodeId>,
    /// The focus handle of [`Self::focus`]: nested elements tracking one handle (a control's
    /// host and the text field inside it) each claim the focus, and the innermost wins.
    focus_handle: Option<FocusId>,
    /// If a node calls `.aria_active_descendant()`, AND an ancestor is focused,
    /// override it as the focused node. This supports the "active descendant"
    /// pattern, which allows a focused container to act as if a descendant is
    /// focused.
    active_descendant: Option<NodeId>,
    /// Beside [`Self::ids_stack`]: how many text leaves each open node holds so far,
    /// which numbers the next leaf's id; `None` for an inspection container, whose text
    /// counts toward the nearest node around it that is not one, so that turning
    /// inspection on or off keeps every leaf's id.
    text_ordinals: SmallVec<[Option<u32>; 16]>,
    /// Whether this frame's tree is built for inspection: generic containers and the
    /// id map are recorded only then.
    inspecting: bool,
    /// Beside [`Self::ids_stack`] while inspecting: the global element id of each open
    /// node (`None` for the root), which text leaves and synthetic children map to.
    paths_stack: SmallVec<[Option<GlobalElementId>; 16]>,
    /// While inspecting, every node pushed this frame with the global element id it
    /// maps to, in push order.
    pub(crate) id_map_log: Vec<(NodeId, GlobalElementId)>,
    /// The generic containers pushed this frame for inspection. They never take
    /// focus, so the screen-reader view of the tree stays what it is without them.
    containers: FxHashSet<NodeId>,
    /// Relations that name other elements by `accessibility_id`, resolved at the end
    /// of the frame.
    pub(crate) pending_relations: Vec<PendingRelation>,
    /// Relation targets already reported unknown in this window (debug builds log
    /// each once).
    unknown_relation_targets: FxHashSet<SharedString>,
    /// The inspection container holding the window's focus this frame.
    container_focus: Option<NodeId>,
    /// This frame's event log (see [`A11yEvent`]).
    pub(crate) events: Vec<A11yEvent>,
    /// Beside [`Self::ids_stack`]: where each open node's `Push` event is in
    /// [`Self::events`] (`usize::MAX` for the root), filled in when it is popped.
    open_events: SmallVec<[usize; 16]>,
    /// Beside [`Self::ids_stack`]: whether each open node is hidden (see [`Self::hidden`]).
    hidden_stack: SmallVec<[bool; 16]>,
    /// Whether what is pushed now belongs to a row a list's layer holds off screen: it is
    /// recorded, for the frames that show the row, but contributes nothing to this tree.
    pub(crate) hidden: bool,
    /// The row of a list's layer being drawn, if any; its nodes are told row after row.
    pub(crate) row: Option<(u32, usize, bool)>,
    /// The row of each node pushed in a list's row, to order a list's rows as a list
    /// without a layer draws them.
    row_tags: FxHashMap<NodeId, (u32, usize, bool)>,
    #[cfg(debug_assertions)]
    node_info: FxHashMap<NodeId, debug::NodeDebugInfo>,
}

/// What building one frame's tree did, in order: fast mode replays a stretch of the last frame's
/// log through the builder wherever it reuses the content that recorded it (cached view replay,
/// retained paint, splice stretches, composited layers and list rows), so the tree of a reused
/// frame is built exactly as drawing the content from scratch builds it. Replay records the
/// events again, for the frames after.
#[derive(Clone)]
pub(crate) enum A11yEvent {
    /// An element's node, filled in with its final properties (children excluded) when it is
    /// popped, with the bounds actions use.
    Push {
        id: NodeId,
        node: Option<accesskit::Node>,
        bounds: Option<Bounds<Pixels>>,
        path: Option<GlobalElementId>,
        container: bool,
    },
    Pop,
    /// A text leaf; its id is derived again where it is replayed.
    TextLeaf {
        node: accesskit::Node,
    },
    /// A synthetic child.
    Leaf {
        id: NodeId,
        node: accesskit::Node,
    },
    Relation(PendingRelation),
    /// A focusable node; whether it holds focus is decided against the window's focus when
    /// replayed.
    Focusable {
        id: NodeId,
        focus: FocusId,
    },
    ActiveDescendant {
        id: NodeId,
    },
    EnterScroll {
        scroll: A11yScroll,
        scrollport: Bounds<Pixels>,
        offset: Point<Pixels>,
        max: Point<Pixels>,
    },
    ExitScroll,
}

/// Where a frame's records stand, to roll back to (see [`crate::Window::transact`]).
pub(crate) struct A11yCheckpoint {
    all_nodes: usize,
    events: usize,
    id_map_log: usize,
    pending_relations: usize,
    scroll_targets: usize,
    current_scroll_target: Option<u32>,
    stack_depth: usize,
    top_children: usize,
    top_ordinal: u32,
    focus: Option<NodeId>,
    focus_handle: Option<FocusId>,
    active_descendant: Option<NodeId>,
}

/// An accessibility relation of a node, naming its targets by `accessibility_id`.
#[derive(Clone, Debug)]
pub(crate) struct PendingRelation {
    pub(crate) node: NodeId,
    pub(crate) kind: RelationKind,
    pub(crate) targets: SmallVec<[SharedString; 1]>,
}

/// The relations an element can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RelationKind {
    LabelledBy,
    DescribedBy,
    Controls,
    ErrorMessage,
}

/// The accessibility relations an element names, by the `accessibility_id` of their targets.
/// The window resolves them to node ids at the end of each frame; see
/// [`Element::queue_a11y_relations`].
pub struct A11yRelationQueue<'a> {
    node: NodeId,
    nodes: &'a mut A11yNodeBuilder,
}

impl A11yRelationQueue<'_> {
    pub(crate) fn push(&mut self, kind: RelationKind, targets: &[SharedString]) {
        if targets.is_empty() {
            return;
        }
        let relation = PendingRelation {
            node: self.node,
            kind,
            targets: targets.iter().cloned().collect(),
        };
        self.nodes.push_relation(relation);
    }
}

impl A11yNodeBuilder {
    fn new() -> Self {
        Self {
            ids_stack: SmallVec::new(),
            nodes_stack: SmallVec::new(),
            all_nodes: Vec::new(),
            seen_ids: FxHashSet::default(),
            focus: None,
            focus_handle: None,
            active_descendant: None,
            text_ordinals: SmallVec::new(),
            inspecting: false,
            paths_stack: SmallVec::new(),
            id_map_log: Vec::new(),
            containers: FxHashSet::default(),
            pending_relations: Vec::new(),
            unknown_relation_targets: FxHashSet::default(),
            container_focus: None,
            events: Vec::new(),
            open_events: SmallVec::new(),
            hidden_stack: SmallVec::new(),
            hidden: false,
            row: None,
            row_tags: FxHashMap::default(),
            #[cfg(debug_assertions)]
            node_info: FxHashMap::default(),
        }
    }

    /// Whether the node on top of the stack, and so what is pushed under it, is hidden.
    fn in_hidden(&self) -> bool {
        self.hidden || self.hidden_stack.last().copied().unwrap_or(false)
    }

    /// A queue for the relations of the node `node` names.
    pub(crate) fn relation_queue(&mut self, node: NodeId) -> A11yRelationQueue<'_> {
        A11yRelationQueue { node, nodes: self }
    }

    /// Whether this frame's tree is built for inspection.
    pub(crate) fn is_inspecting(&self) -> bool {
        self.inspecting
    }

    /// Pushes a node for an element and makes it the parent of what follows, recording
    /// its global element id while inspecting. `container` marks an inspection-only
    /// generic container.
    pub(crate) fn push_element(
        &mut self,
        id: NodeId,
        node: accesskit::Node,
        path: &GlobalElementId,
        container: bool,
    ) -> bool {
        if !self.push(id, node) {
            return false;
        }
        if let Some(&index) = self.open_events.last()
            && let Some(A11yEvent::Push {
                path: event_path,
                container: event_container,
                ..
            }) = self.events.get_mut(index)
        {
            *event_path = Some(path.clone());
            *event_container = container;
        }
        if self.in_hidden() {
            return true;
        }
        if self.inspecting {
            *self
                .paths_stack
                .last_mut()
                .expect("push keeps the stacks in step") = Some(path.clone());
            self.id_map_log.push((id, path.clone()));
        }
        if container {
            self.containers.insert(id);
            *self
                .text_ordinals
                .last_mut()
                .expect("push keeps the stacks in step") = None;
        }
        true
    }

    /// The text-leaf count of the nearest open node that is not an inspection container.
    fn text_owner_ordinal(&mut self) -> Option<&mut u32> {
        self.text_ordinals.iter_mut().rev().find_map(Option::as_mut)
    }

    /// Adds a `Label` leaf holding `text` under the current node. Its id derives from the
    /// nearest node around it that is not an inspection container and the leaf's position
    /// among that node's text leaves, so it stays the same across frames while the order of
    /// the text does, and whether or not inspection adds containers.
    pub(crate) fn push_text_leaf(&mut self, text: &str, bounds: accesskit::Rect) -> bool {
        // Numbered under the nearest node that is not an inspection container.
        let owner = self
            .text_ordinals
            .iter()
            .rposition(Option::is_some)
            .expect("the root counts its text");
        let parent = self.ids_stack[owner];
        let ordinal = self.text_ordinals[owner]
            .as_mut()
            .expect("the owner counts its text");
        let mut hasher = std::hash::DefaultHasher::default();
        parent.0.hash(&mut hasher);
        "gpux-text-leaf".hash(&mut hasher);
        ordinal.hash(&mut hasher);
        *ordinal += 1;
        let id = NodeId(hasher.finish());
        let mut node = accesskit::Node::new(accesskit::Role::Label);
        node.set_value(text);
        node.set_bounds(bounds);
        self.events.push(A11yEvent::TextLeaf { node: node.clone() });
        self.add_leaf(id, node)
    }

    /// Queues a relation, to be resolved at the end of the frame.
    pub(crate) fn push_relation(&mut self, relation: PendingRelation) {
        self.events.push(A11yEvent::Relation(relation.clone()));
        if !self.in_hidden() {
            self.pending_relations.push(relation);
        }
    }

    /// Whether `id` is a generic container pushed for inspection this frame.
    pub(crate) fn is_container(&self, id: NodeId) -> bool {
        self.containers.contains(&id)
    }

    /// Records provenance for a node already pushed this frame. Debug builds only.
    #[cfg(debug_assertions)]
    pub(crate) fn record_node_info(&mut self, id: NodeId, info: debug::NodeDebugInfo) {
        self.node_info.insert(id, info);
    }

    #[must_use]
    fn can_push(&mut self, id: NodeId) -> bool {
        debug_assert!(!self.ids_stack.is_empty(), "node pushed before push_root");

        if !self.seen_ids.insert(id) {
            debug_assert!(
                false,
                "Duplicate a11y node id: {id:?}. In a release build, this node would be silently discarded from the a11y tree."
            );
            return false;
        }

        true
    }

    /// Push a new node onto the stack. It becomes a child of the current
    /// top-of-stack node.
    ///
    /// Returns `true` if the node was successfully pushed.
    pub(crate) fn push(&mut self, id: NodeId, node: accesskit::Node) -> bool {
        let hidden = self.in_hidden();
        if !hidden && !self.can_push(id) {
            return false;
        }

        if !hidden && let Some(parent) = self.nodes_stack.last_mut() {
            parent.push_child(id);
        }
        if !hidden && let Some(row) = self.row {
            self.row_tags.insert(id, row);
        }
        self.open_events.push(self.events.len());
        self.events.push(A11yEvent::Push {
            id,
            node: None,
            bounds: None,
            path: None,
            container: false,
        });
        self.hidden_stack.push(hidden);
        self.ids_stack.push(id);
        self.nodes_stack.push(node);
        self.text_ordinals.push(Some(0));
        if self.inspecting {
            self.paths_stack.push(None);
        }
        true
    }

    /// Add a leaf node as a child of the current top-of-stack node, without
    /// pushing it onto the stack. Semantically equivalent to a [`Self::push`]
    /// followed by a [`Self::pop`].
    ///
    /// Returns `true` if the node was successfully pushed.
    pub(crate) fn push_leaf(&mut self, id: NodeId, node: accesskit::Node) -> bool {
        self.events.push(A11yEvent::Leaf {
            id,
            node: node.clone(),
        });
        self.add_leaf(id, node)
    }

    /// Adds a leaf to the tree being built, unless it is hidden.
    fn add_leaf(&mut self, id: NodeId, node: accesskit::Node) -> bool {
        if self.in_hidden() {
            return true;
        }
        if !self.can_push(id) {
            return false;
        }
        if let Some(row) = self.row {
            self.row_tags.insert(id, row);
        }

        if let Some(parent) = self.nodes_stack.last_mut() {
            parent.push_child(id);
        }
        self.all_nodes.push((id, node));
        // Text leaves and synthetic children map to their owner's element.
        if self.inspecting
            && let Some(Some(owner)) = self.paths_stack.last()
        {
            self.id_map_log.push((id, owner.clone()));
        }
        true
    }

    pub(crate) fn current_node_mut(&mut self) -> Option<&mut accesskit::Node> {
        self.nodes_stack.last_mut()
    }

    /// Pop the current node off the stack and finalize it into the all_nodes
    /// list.
    pub(crate) fn pop(&mut self, bounds: Option<Bounds<Pixels>>) {
        debug_assert!(self.ids_stack.len() > 1, "pop would remove the root node");

        let hidden = self.hidden_stack.pop().unwrap_or(false);
        let event = self.open_events.pop();
        if let (Some(id), Some(node)) = (self.ids_stack.pop(), self.nodes_stack.pop()) {
            if let Some(A11yEvent::Push {
                node: event_node,
                bounds: event_bounds,
                ..
            }) = event.and_then(|index| self.events.get_mut(index))
            {
                let mut recorded = node.clone();
                recorded.clear_children();
                *event_node = Some(recorded);
                *event_bounds = bounds;
            }
            if !hidden {
                self.all_nodes.push((id, node));
            }
        }
        self.events.push(A11yEvent::Pop);
        self.text_ordinals.pop();
        if self.inspecting {
            self.paths_stack.pop();
        }
    }

    /// Where the frame's records stand.
    fn checkpoint(
        &self,
    ) -> (
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        u32,
        Option<NodeId>,
        Option<NodeId>,
    ) {
        (
            self.all_nodes.len(),
            self.events.len(),
            self.id_map_log.len(),
            self.pending_relations.len(),
            self.ids_stack.len(),
            self.nodes_stack
                .last()
                .map_or(0, |node| node.children().len()),
            self.text_ordinals
                .iter()
                .rev()
                .find_map(|ordinal| *ordinal)
                .unwrap_or(0),
            self.focus,
            self.active_descendant,
        )
    }

    /// Push the root node to start a new frame.
    #[cfg(test)]
    fn begin_frame(&mut self, window_title: Option<&SharedString>) {
        self.begin_frame_for(window_title, false);
    }

    /// Push the root node to start a new frame, built for inspection when `inspecting`.
    fn begin_frame_for(&mut self, window_title: Option<&SharedString>, inspecting: bool) {
        self.all_nodes.clear();
        self.ids_stack.clear();
        self.nodes_stack.clear();
        self.seen_ids.clear();
        self.text_ordinals.clear();
        self.text_ordinals.push(Some(0));
        self.inspecting = inspecting;
        self.paths_stack.clear();
        if inspecting {
            self.paths_stack.push(None);
        }
        self.id_map_log.clear();
        self.containers.clear();
        self.pending_relations.clear();
        self.events.clear();
        self.open_events.clear();
        self.open_events.push(usize::MAX);
        self.hidden_stack.clear();
        self.hidden_stack.push(false);
        self.hidden = false;
        self.row = None;
        self.row_tags.clear();
        #[cfg(debug_assertions)]
        self.node_info.clear();
        let mut root_node = accesskit::Node::new(accesskit::Role::Window);
        if let Some(title) = window_title {
            root_node.set_label(title.to_string());
        }

        self.ids_stack.push(ROOT_NODE_ID);
        self.nodes_stack.push(root_node);
        self.focus = None;
        self.focus_handle = None;
        self.active_descendant = None;
        self.container_focus = None;
    }

    /// Returns whether a node with the given ID has been pushed in this frame.
    pub(crate) fn has_node(&self, id: NodeId) -> bool {
        id == ROOT_NODE_ID || self.seen_ids.contains(&id)
    }

    /// Returns whether `id` is the node currently reported as focused.
    pub(crate) fn node_is_focused(&self, id: NodeId) -> bool {
        self.focus == Some(id)
    }

    pub(crate) fn focus_is_ancestor_of_current(&self) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };

        // The current node is on top of the stack; everything below it is an
        // ancestor.
        let ancestor_count = self.ids_stack.len().saturating_sub(1);
        self.ids_stack[..ancestor_count].contains(&focus)
    }

    pub(crate) fn set_active_descendant(&mut self, id: NodeId) {
        if self
            .active_descendant
            .is_some_and(|existing| existing != id)
        {
            if cfg!(debug_assertions) {
                panic!("active descendant claimed by multiple nodes in one frame");
            } else {
                log::warn!(
                    "a11y: multiple nodes claimed the active descendant this frame; \
                     using last-wins ({id:?})"
                );
            }
        }
        self.active_descendant = Some(id);
    }

    #[cfg(test)]
    pub(crate) fn set_focus(&mut self, id: NodeId) {
        self.set_focus_by(id, None);
    }

    /// Reports `id` as focused through the focus handle `handle`. A later claim through the same
    /// handle (an element nested in another that tracks it) takes over; a claim through another
    /// handle means two elements both claimed the window's focus.
    pub(crate) fn set_focus_by(&mut self, id: NodeId, handle: Option<FocusId>) {
        let same_handle = handle.is_some() && handle == self.focus_handle;
        if self.focus.is_some() && !same_handle {
            if cfg!(debug_assertions) {
                panic!("set_focus called more than once in a single frame");
            } else {
                log::warn!(
                    "a11y: set_focus called more than once in a single frame; \
                     using last-wins ({id:?})"
                );
            }
        }
        self.focus = Some(id);
        self.focus_handle = handle;
    }

    fn finalize(&mut self) -> TreeUpdate {
        // Stack should contain only the root node
        debug_assert_eq!(self.ids_stack.len(), 1);
        debug_assert_eq!(self.ids_stack[0], ROOT_NODE_ID);

        if self.ids_stack.len() != 1 {
            log::error!(
                "a11y: Stack imbalance at end of frame: expected 1 (root), got {}. \
                 Some elements may have pushed without popping.",
                self.ids_stack.len()
            );
        }

        // Pop remaining nodes (should just be the root).
        while !self.ids_stack.is_empty() {
            if let (Some(id), Some(node)) = (self.ids_stack.pop(), self.nodes_stack.pop()) {
                self.all_nodes.push((id, node));
            }
        }

        let focus = match self.active_descendant {
            Some(id) if self.has_node(id) => id,
            Some(id) => {
                if cfg!(debug_assertions) {
                    panic!("active_descendant set to {id:?}, which is not in the tree");
                } else {
                    log::warn!("active_descendant set to {id:?}, which is not in the tree");
                    self.focus.unwrap_or(ROOT_NODE_ID)
                }
            }

            _ => self.focus.unwrap_or(ROOT_NODE_ID),
        };

        self.resolve_relations();
        self.order_list_rows();
        let nodes = std::mem::take(&mut self.all_nodes);
        let update = TreeUpdate {
            nodes,
            tree: Some(tree_info()),
            tree_id: accesskit::TreeId::ROOT,
            focus,
        };

        Self::repair_tree_update(update)
    }

    /// Orders each list's rows as a list without a layer draws them: a frame drawn from a
    /// list's layer carries and draws its rows in whatever order, the row drawn off screen
    /// after the rows shown last.
    fn order_list_rows(&mut self) {
        if self.row_tags.is_empty() {
            return;
        }
        let tags = &self.row_tags;
        for (_, node) in &mut self.all_nodes {
            let children = node.children();
            if children
                .iter()
                .filter(|child| tags.contains_key(child))
                .count()
                < 2
            {
                continue;
            }
            let mut ordered: Vec<NodeId> = children.to_vec();
            // Stable within runs of one list's rows, which lie one after another.
            let mut start = 0;
            while start < ordered.len() {
                let Some(&(list, ..)) = tags.get(&ordered[start]) else {
                    start += 1;
                    continue;
                };
                let end = start
                    + ordered[start..]
                        .iter()
                        .take_while(|child| tags.get(child).is_some_and(|tag| tag.0 == list))
                        .count();
                ordered[start..end].sort_by_key(|child| {
                    let (_, row, loose) = tags[child];
                    (loose, row)
                });
                start = end;
            }
            if ordered != children {
                node.set_children(ordered);
            }
        }
    }

    /// Resolves the frame's relations: every target `accessibility_id` becomes the id of
    /// the node that carries it, in the order given; unknown targets are dropped.
    fn resolve_relations(&mut self) {
        if self.pending_relations.is_empty() {
            return;
        }
        let mut by_author: FxHashMap<&str, NodeId> = FxHashMap::default();
        let mut index_of: FxHashMap<NodeId, usize> = FxHashMap::default();
        for (index, (id, node)) in self.all_nodes.iter().enumerate() {
            index_of.insert(*id, index);
            if let Some(author) = node.author_id() {
                by_author.entry(author).or_insert(*id);
            }
        }
        let mut resolved: Vec<(usize, RelationKind, Vec<NodeId>)> = Vec::new();
        for relation in &self.pending_relations {
            let Some(&index) = index_of.get(&relation.node) else {
                continue;
            };
            let mut ids = Vec::with_capacity(relation.targets.len());
            for target in &relation.targets {
                match by_author.get(target.as_ref()) {
                    Some(id) => ids.push(*id),
                    None => {
                        if cfg!(debug_assertions)
                            && self.unknown_relation_targets.insert(target.clone())
                        {
                            log::warn!(
                                "a11y: a {:?} relation names `{target}`, which no element in \
                                 the frame carries as its accessibility_id; dropping it",
                                relation.kind
                            );
                        }
                    }
                }
            }
            resolved.push((index, relation.kind, ids));
        }
        for (index, kind, ids) in resolved {
            let node = &mut self.all_nodes[index].1;
            match kind {
                RelationKind::LabelledBy => {
                    for id in ids {
                        node.push_labelled_by(id);
                    }
                }
                RelationKind::DescribedBy => {
                    for id in ids {
                        node.push_described_by(id);
                    }
                }
                RelationKind::Controls => {
                    for id in ids {
                        node.push_controlled(id);
                    }
                }
                RelationKind::ErrorMessage => {
                    if let Some(id) = ids.first() {
                        node.set_error_message(*id);
                    }
                }
            }
        }
    }

    /// Accesskit panics on invalid [`TreeUpdate`]s. This function defensively
    /// checks invariants that accesskit panics on, and tries to fix them.
    fn repair_tree_update(mut update: TreeUpdate) -> TreeUpdate {
        let node_ids: FxHashSet<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();

        // Focus must point to a node in the tree.
        if !node_ids.contains(&update.focus) {
            log::error!(
                "a11y: Focused node {:?} is not in the tree ({} nodes). \
                 Falling back to root. This is a bug in the a11y tree builder.",
                update.focus,
                update.nodes.len()
            );
            update.focus = ROOT_NODE_ID;
        }

        // Every child reference must point to a node in the update.
        for (id, node) in &mut update.nodes {
            let has_invalid_child = node
                .children()
                .iter()
                .any(|child_id| !node_ids.contains(child_id));
            if has_invalid_child {
                let children = node.children();
                let invalid_count = children
                    .iter()
                    .filter(|child_id| !node_ids.contains(child_id))
                    .count();
                log::error!(
                    "a11y: Node {:?} references {} children not present in the tree. \
                     Stripping invalid child references.",
                    id,
                    invalid_count
                );
                let valid: Vec<NodeId> = children
                    .iter()
                    .copied()
                    .filter(|child_id| node_ids.contains(child_id))
                    .collect();
                node.set_children(valid);
            }
        }

        update
    }
}

#[cfg(test)]
mod tests {
    // Import specific items rather than glob-importing `super`, which would pull
    // in gpui's own `test` attribute macro and shadow the standard one.
    use super::{A11y, A11yNodeBuilder, ROOT_NODE_ID};
    use crate::FocusId;
    use accesskit::{NodeId, Role};
    use std::sync::{Arc, atomic::AtomicBool};

    fn test_node() -> accesskit::Node {
        accesskit::Node::new(Role::GenericContainer)
    }

    fn new_builder() -> A11yNodeBuilder {
        let mut builder = A11yNodeBuilder::new();
        builder.begin_frame(None);
        builder
    }

    fn new_a11y() -> A11y {
        let mut a11y = A11y::new(
            Arc::new(AtomicBool::new(true)),
            Default::default(),
            false,
            None,
        );
        a11y.begin_frame();
        a11y
    }

    /// `Application::with_accessibility_mode` sets the mode every window of the application reads.
    #[test]
    fn a11y_activation_builder_sets_the_application_mode() {
        use crate::{
            Application, BackgroundExecutor, ForegroundExecutor, TestDispatcher, TestPlatform,
        };
        let dispatcher = TestDispatcher::new(0);
        let arc_dispatcher = std::sync::Arc::new(dispatcher);
        let platform = TestPlatform::new(
            BackgroundExecutor::new(arc_dispatcher.clone()),
            ForegroundExecutor::new(arc_dispatcher),
        );
        for mode in [
            super::AccessibilityMode::Always,
            super::AccessibilityMode::Off,
            super::AccessibilityMode::Auto,
        ] {
            let app = Application::with_platform(platform.clone()).with_accessibility_mode(mode);
            assert_eq!(app.0.borrow().accessibility_mode(), mode);
        }
    }

    #[test]
    fn accessibility_enabled_is_independent_of_activation() {
        for force_disabled in [false, true] {
            for active in [false, true] {
                let mut a11y = A11y::new(
                    Arc::new(AtomicBool::new(active)),
                    Default::default(),
                    force_disabled,
                    None,
                );
                a11y.sync_active_flag();

                assert_eq!(a11y.is_enabled(), !force_disabled);
                assert_eq!(a11y.is_active(), !force_disabled && active);
            }
        }
    }

    #[test]
    fn active_descendant_honored_when_container_focused() {
        let mut builder = new_builder();
        let container = NodeId(1);
        let item = NodeId(2);

        assert!(builder.push(container, test_node()));
        builder.set_focus(container);
        assert!(builder.push(item, test_node()));

        // The item is on top of the stack; the focused container is its
        // ancestor, so the claim is honored.
        assert!(builder.focus_is_ancestor_of_current());
        builder.set_active_descendant(item);

        builder.pop(None); // item
        builder.pop(None); // container
        let update = builder.finalize();
        assert_eq!(update.focus, item);
    }

    #[test]
    fn active_descendant_honored_for_deep_descendant() {
        let mut builder = new_builder();
        let container = NodeId(1);
        let group = NodeId(2);
        let item = NodeId(3);

        assert!(builder.push(container, test_node()));
        builder.set_focus(container);
        assert!(builder.push(group, test_node()));
        assert!(builder.push(item, test_node()));

        // The item is a grandchild of the focused container; depth doesn't
        // matter, the focused ancestor is still on the stack.
        assert!(builder.focus_is_ancestor_of_current());
        builder.set_active_descendant(item);

        builder.pop(None); // item
        builder.pop(None); // group
        builder.pop(None); // container
        let update = builder.finalize();
        assert_eq!(update.focus, item);
    }

    #[test]
    fn active_descendant_ignored_when_focus_in_other_subtree() {
        let mut builder = new_builder();
        let focused_container = NodeId(1);
        let focused_leaf = NodeId(2);
        let other_container = NodeId(3);
        let other_item = NodeId(4);

        // First subtree holds real focus.
        assert!(builder.push(focused_container, test_node()));
        assert!(builder.push(focused_leaf, test_node()));
        builder.set_focus(focused_leaf);
        builder.pop(None); // focused_leaf
        builder.pop(None); // focused_container

        // Second subtree: its item would claim the active descendant, but the
        // focus is not on any of its ancestors, so the gate rejects it.
        assert!(builder.push(other_container, test_node()));
        assert!(builder.push(other_item, test_node()));
        assert!(!builder.focus_is_ancestor_of_current());
        builder.pop(None); // other_item
        builder.pop(None); // other_container

        let update = builder.finalize();
        assert_eq!(update.focus, focused_leaf);
    }

    #[test]
    fn active_descendant_ignored_when_nothing_focused() {
        let mut builder = new_builder();
        let container = NodeId(1);
        let item = NodeId(2);

        assert!(builder.push(container, test_node()));
        assert!(builder.push(item, test_node()));

        // Nothing is focused (focus defaults to the root window node), so the
        // gate rejects the claim.
        assert!(!builder.focus_is_ancestor_of_current());
        builder.pop(None);
        builder.pop(None);

        let update = builder.finalize();
        assert_eq!(update.focus, ROOT_NODE_ID);
    }

    #[test]
    fn regular_focus_used_when_no_active_descendant() {
        let mut builder = new_builder();
        let focused = NodeId(1);

        assert!(builder.push(focused, test_node()));
        builder.set_focus(focused);
        builder.pop(None);

        let update = builder.finalize();
        assert_eq!(update.focus, focused);
    }

    #[test]
    fn focus_is_ancestor_excludes_self_and_non_ancestors() {
        let mut builder = new_builder();
        let container = NodeId(1);
        let item = NodeId(2);

        assert!(builder.push(container, test_node()));
        builder.set_focus(container);

        // With the focused container itself on top, it is not its own (strict)
        // ancestor, so the gate is false.
        assert!(!builder.focus_is_ancestor_of_current());

        assert!(builder.push(item, test_node()));
        // Now the focused container is a strict ancestor of the item on top.
        assert!(builder.focus_is_ancestor_of_current());

        builder.pop(None);
        builder.pop(None);
    }

    // The double-claim guard panics only in debug builds; in release it falls
    // back to last-wins with a warning.
    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "active descendant claimed by multiple nodes")
    )]
    fn multiple_active_descendant_claims_panic_in_debug() {
        let mut builder = new_builder();
        builder.set_active_descendant(NodeId(1));
        builder.set_active_descendant(NodeId(2));
    }

    // Setting focus twice in one frame means two elements both claimed window
    // focus; that panics in debug and falls back to last-wins in release.
    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "set_focus called more than once")
    )]
    fn setting_focus_twice_panics_in_debug() {
        let mut builder = new_builder();
        builder.set_focus(NodeId(1));
        builder.set_focus(NodeId(2));
    }

    // Focusing a node that was never registered as focusable is a bug: panic in
    // debug, warn in release.
    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "was not registered with set_focusable")
    )]
    fn set_focus_without_set_focusable() {
        let mut a11y = new_a11y();
        let node = NodeId(1);
        assert!(a11y.nodes.push(node, test_node()));
        // set_focusable was never called for `node`.
        a11y.set_focus(node);
    }

    // The focused node cannot also be its own active descendant: panic in
    // debug, warn in release.
    #[test]
    #[cfg_attr(debug_assertions, should_panic(expected = "on the focused node"))]
    fn set_active_descendant_on_focused_node() {
        let mut a11y = new_a11y();
        let node = NodeId(1);
        assert!(a11y.nodes.push(node, test_node()));
        a11y.set_focusable(node, FocusId::default());
        a11y.set_focus(node);
        a11y.set_active_descendant(node);
    }

    // Two sibling children of a focused container both claim the active
    // descendant (both pass the focus gate). The second claim is a bug: panic
    // in debug, last-wins + warn in release.
    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "active descendant claimed by multiple nodes")
    )]
    fn two_siblings_claiming_active_descendant() {
        let mut a11y = new_a11y();
        let container = NodeId(1);
        let first = NodeId(2);
        let second = NodeId(3);

        assert!(a11y.nodes.push(container, test_node()));
        a11y.set_focusable(container, FocusId::default());
        a11y.set_focus(container);

        assert!(a11y.nodes.push(first, test_node()));
        a11y.set_active_descendant(first);
        a11y.nodes.pop(None); // first

        assert!(a11y.nodes.push(second, test_node()));
        a11y.set_active_descendant(second);
        a11y.nodes.pop(None); // second

        a11y.nodes.pop(None); // container
    }

    // Node A is focused; node C (a child of the unfocused node B) claims the
    // active descendant. The final tree must still report A as focused.
    #[test]
    fn active_descendant_in_unfocused_subtree_keeps_real_focus() {
        let mut a11y = new_a11y();
        let a = NodeId(1);
        let b = NodeId(2);
        let c = NodeId(3);

        assert!(a11y.nodes.push(a, test_node()));
        a11y.set_focusable(a, FocusId::default());
        a11y.set_focus(a);
        a11y.nodes.pop(None); // a

        assert!(a11y.nodes.push(b, test_node()));
        assert!(a11y.nodes.push(c, test_node()));
        a11y.set_active_descendant(c);
        a11y.nodes.pop(None); // c
        a11y.nodes.pop(None); // b

        let update = a11y.end_frame(Default::default());
        assert_eq!(update.focus, a);
    }
}
