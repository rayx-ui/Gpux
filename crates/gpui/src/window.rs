#[cfg(any(feature = "inspector", debug_assertions))]
use crate::Inspector;
use crate::{
    Action, ActionDispatchEvent, ActionDispatchOutcome, ActionDispatchReport, ActionInvocation,
    ActionInvocationSource, ActiveScreenEffect, ActiveScreenEffectSnapshot, AnyDrag, AnyElement,
    AnyImageCache, AnyTooltip, AnyView, App, AppContext, Arena, Asset, AsyncWindowContext,
    AvailableSpace, Background, BorderStyle, Bounds, BoxShadow, Capslock, Color, Context, Corners,
    CursorHideMode, CursorStyle, Decorations, DevicePixels, DispatchActionListener, DispatchNodeId,
    DispatchTree, DisplayId, Edges, Effect, EffectCapabilities, ElementEffect, Entity, EntityId,
    EventEmitter, FileDropEvent, FontId, GestureMapSnapshot, GestureMapVersion, GestureRecognizer,
    Global, GlobalElementId, GlyphId, GpuSpecs, HtmlElementContentFit, HtmlElementInputMode,
    HtmlElementLifetime, HtmlElementPrimitive, HtmlElementWheelBehavior, HtmlIslandInputMode,
    HtmlIslandPrimitive, InputEvent, InputHandler, IsZero, KeyBinding, KeyContext, KeyDownEvent,
    KeyEvent, KeymapSnapshot, Keystroke, KeystrokeEvent, LayoutId, Length, LineLayoutIndex,
    MAX_ACTIVE_SCREEN_EFFECTS, MAX_SCREEN_EFFECT_SNAPSHOTS, Modifiers, ModifiersChangedEvent,
    MonochromeSprite, MonochromeSpriteKind, MouseButton, Overflow, Path, Pixels, PlatformAtlas,
    PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow, Point, PointerEvent,
    PointerId, PolychromeSprite, Position, Priority, PromptButton, PromptLevel, Quad,
    RecognizedGesture, Render, RenderGlyphParams, RenderImage, RenderImageParams, RenderSvgParams,
    Replay, ResizeEdge, SMOOTH_SVG_SCALE_FACTOR, SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y,
    ScaledPixels, Scene, ScreenEffect, ScreenEffectId, ScreenEffectRegionId,
    ScreenEffectRegionPrimitive, ScreenEffectSnapshotId, ScreenEffectTarget, ScrollWheelEvent,
    Shadow, SharedString, Size, StickyStacking, StrikethroughStyle, Style, SubpixelSprite,
    SubscriberSet, Subscription, SystemWindowTab, SystemWindowTabController, TabStopMap,
    TaffyLayoutEngine, Task, TextRenderingMode, TextStyle, TextStyleRefinement, ThermalState,
    TransformationMatrix, Underline, UnderlineStyle, VisualEffectPrimitive, WindowAppearance,
    WindowBackgroundAppearance, WindowBounds, WindowControls, WindowDecorations, WindowOptions,
    WindowParams, WindowTextSystem, point, prelude::*, px, rems, size, transparent_black,
};
use anyhow::{Context as _, Result, anyhow};
use collections::{FxHashMap, FxHashSet};
#[cfg(target_os = "macos")]
use core_video::pixel_buffer::CVPixelBuffer;
use derive_more::{Deref, DerefMut};
use futures::FutureExt;
use futures::channel::oneshot;
use gpui_util::post_inc;
use gpui_util::{ResultExt, measure};
#[cfg(feature = "input-latency-histogram")]
use hdrhistogram::Histogram;
use parking_lot::RwLock;
use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};
use refineable::Refineable;
use scheduler::Instant;
use slotmap::SlotMap;
use smallvec::SmallVec;
use std::{
    any::{Any, TypeId},
    borrow::Cow,
    cell::{Cell, RefCell},
    cmp,
    collections::VecDeque,
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    marker::PhantomData,
    mem,
    ops::{DerefMut, Range},
    rc::Rc,
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
    time::Duration,
};
use uuid::Uuid;

mod prompts;

use crate::util::{
    atomic_incr_if_not_zero, ceil_to_device_pixel, floor_to_device_pixel, round_half_toward_zero,
    round_half_toward_zero_f64, round_stroke_to_device_pixel, round_to_device_pixel,
};
pub use prompts::*;

/// Default window size used when no explicit size is provided.
pub const DEFAULT_WINDOW_SIZE: Size<Pixels> = size(px(1536.), px(1095.));

/// A 6:5 aspect ratio minimum window size to be used for functional,
/// additional-to-main-Zed windows, like the settings and rules library windows.
pub const DEFAULT_ADDITIONAL_WINDOW_SIZE: Size<Pixels> = Size {
    width: Pixels(900.),
    height: Pixels(750.),
};

/// Represents the two different phases when dispatching events.
#[derive(Default, Copy, Clone, Debug, Eq, PartialEq)]
pub enum DispatchPhase {
    /// After the capture phase comes the bubble phase, in which mouse event listeners are
    /// invoked front to back and keyboard event listeners are invoked from the focused element
    /// to the root of the element tree. This is the phase you'll most commonly want to use when
    /// registering event listeners.
    #[default]
    Bubble,
    /// During the initial capture phase, mouse event listeners are invoked back to front, and keyboard
    /// listeners are invoked from the root of the tree downward toward the focused element. This phase
    /// is used for special purposes such as clearing the "pressed" state for click events. If
    /// you stop event propagation during this phase, you need to know what you're doing. Handlers
    /// outside of the immediate region may rely on detecting non-local events during this phase.
    Capture,
}

impl DispatchPhase {
    /// Returns true if this represents the "bubble" phase.
    #[inline]
    pub fn bubble(self) -> bool {
        self == DispatchPhase::Bubble
    }

    /// Returns true if this represents the "capture" phase.
    #[inline]
    pub fn capture(self) -> bool {
        self == DispatchPhase::Capture
    }
}

struct WindowInvalidatorInner {
    pub dirty: bool,
    pub draw_phase: DrawPhase,
    pub dirty_views: FxHashSet<EntityId>,
    pub update_count: usize,
}

#[derive(Clone)]
pub(crate) struct WindowInvalidator {
    inner: Rc<RefCell<WindowInvalidatorInner>>,
}

impl WindowInvalidator {
    pub fn new() -> Self {
        WindowInvalidator {
            inner: Rc::new(RefCell::new(WindowInvalidatorInner {
                dirty: true,
                draw_phase: DrawPhase::None,
                dirty_views: FxHashSet::default(),
                update_count: 0,
            })),
        }
    }

    pub fn invalidate_view(&self, entity: EntityId, cx: &mut App) -> bool {
        let mut inner = self.inner.borrow_mut();
        inner.update_count += 1;
        inner.dirty_views.insert(entity);
        if inner.draw_phase == DrawPhase::None {
            inner.dirty = true;
            cx.push_effect(Effect::Notify { emitter: entity });
            true
        } else {
            false
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.inner.borrow().dirty
    }

    pub fn set_dirty(&self, dirty: bool) {
        let mut inner = self.inner.borrow_mut();
        inner.dirty = dirty;
        if dirty {
            inner.update_count += 1;
        }
    }

    pub fn set_phase(&self, phase: DrawPhase) {
        self.inner.borrow_mut().draw_phase = phase
    }

    pub fn update_count(&self) -> usize {
        self.inner.borrow().update_count
    }

    pub fn take_views(&self) -> FxHashSet<EntityId> {
        mem::take(&mut self.inner.borrow_mut().dirty_views)
    }

    pub fn replace_views(&self, views: FxHashSet<EntityId>) {
        self.inner.borrow_mut().dirty_views = views;
    }

    pub fn not_drawing(&self) -> bool {
        self.inner.borrow().draw_phase == DrawPhase::None
    }

    #[track_caller]
    pub fn debug_assert_paint(&self) {
        debug_assert!(
            matches!(self.inner.borrow().draw_phase, DrawPhase::Paint),
            "this method can only be called during paint"
        );
    }

    #[track_caller]
    pub fn debug_assert_prepaint(&self) {
        debug_assert!(
            matches!(self.inner.borrow().draw_phase, DrawPhase::Prepaint),
            "this method can only be called during request_layout, or prepaint"
        );
    }

    #[track_caller]
    pub fn debug_assert_paint_or_prepaint(&self) {
        debug_assert!(
            matches!(
                self.inner.borrow().draw_phase,
                DrawPhase::Paint | DrawPhase::Prepaint
            ),
            "this method can only be called during request_layout, prepaint, or paint"
        );
    }
}

type AnyObserver = Box<dyn FnMut(&mut Window, &mut App) -> bool + 'static>;

type AnyActionDispatchObserver =
    Box<dyn FnMut(&ActionDispatchEvent, &mut Window, &mut App) -> bool + 'static>;

pub(crate) type AnyWindowFocusListener =
    Box<dyn FnMut(&WindowFocusEvent, &mut Window, &mut App) -> bool + 'static>;

pub(crate) struct WindowFocusEvent {
    pub(crate) previous_focus_path: SmallVec<[FocusId; 8]>,
    pub(crate) current_focus_path: SmallVec<[FocusId; 8]>,
}

impl WindowFocusEvent {
    pub fn is_focus_in(&self, focus_id: FocusId) -> bool {
        !self.previous_focus_path.contains(&focus_id) && self.current_focus_path.contains(&focus_id)
    }

    pub fn is_focus_out(&self, focus_id: FocusId) -> bool {
        self.previous_focus_path.contains(&focus_id) && !self.current_focus_path.contains(&focus_id)
    }
}

/// This is provided when subscribing for `Context::on_focus_out` events.
pub struct FocusOutEvent {
    /// A weak focus handle representing what was blurred.
    pub blurred: WeakFocusHandle,
}

slotmap::new_key_type! {
    /// A globally unique identifier for a focusable element.
    pub struct FocusId;
}

thread_local! {
    /// Fallback arena used when no app-specific arena is active.
    /// In production, each window draw sets CURRENT_ELEMENT_ARENA to the app's arena.
    pub(crate) static ELEMENT_ARENA: RefCell<Arena> = RefCell::new(Arena::new(1024 * 1024));

    /// Points to the current App's element arena during draw operations.
    /// This allows multiple test Apps to have isolated arenas, preventing
    /// cross-session corruption when the scheduler interleaves their tasks.
    static CURRENT_ELEMENT_ARENA: Cell<Option<*const RefCell<Arena>>> = const { Cell::new(None) };
}

/// Allocates an element in the current arena. Uses the app-specific arena if one
/// is active (during draw), otherwise falls back to the thread-local ELEMENT_ARENA.
pub(crate) fn with_element_arena<R>(f: impl FnOnce(&mut Arena) -> R) -> R {
    CURRENT_ELEMENT_ARENA.with(|current| {
        if let Some(arena_ptr) = current.get() {
            // SAFETY: The pointer is valid for the duration of the draw operation
            // that set it, and we're being called during that same draw.
            let arena_cell = unsafe { &*arena_ptr };
            f(&mut arena_cell.borrow_mut())
        } else {
            ELEMENT_ARENA.with_borrow_mut(f)
        }
    })
}

/// RAII guard that sets CURRENT_ELEMENT_ARENA for the duration of a draw operation.
/// When dropped, restores the previous arena (supporting nested draws).
pub(crate) struct ElementArenaScope {
    previous: Option<*const RefCell<Arena>>,
}

impl ElementArenaScope {
    /// Enter a scope where element allocations use the given arena.
    pub(crate) fn enter(arena: &RefCell<Arena>) -> Self {
        let previous = CURRENT_ELEMENT_ARENA.with(|current| {
            let prev = current.get();
            current.set(Some(arena as *const RefCell<Arena>));
            prev
        });
        Self { previous }
    }
}

impl Drop for ElementArenaScope {
    fn drop(&mut self) {
        CURRENT_ELEMENT_ARENA.with(|current| {
            current.set(self.previous);
        });
    }
}

/// Returned when the element arena has been used and so must be cleared before the next draw.
#[must_use]
pub struct ArenaClearNeeded {
    arena: *const RefCell<Arena>,
}

impl ArenaClearNeeded {
    /// Create a new ArenaClearNeeded that will clear the given arena.
    pub(crate) fn new(arena: &RefCell<Arena>) -> Self {
        Self {
            arena: arena as *const RefCell<Arena>,
        }
    }

    /// Clear the element arena.
    pub fn clear(self) {
        // SAFETY: The arena pointer is valid because ArenaClearNeeded is created
        // at the end of draw() and must be cleared before the next draw.
        let arena_cell = unsafe { &*self.arena };
        arena_cell.borrow_mut().clear();
    }
}

pub(crate) type FocusMap = RwLock<SlotMap<FocusId, FocusRef>>;
pub(crate) struct FocusRef {
    pub(crate) ref_count: AtomicUsize,
    pub(crate) tab_index: isize,
    pub(crate) tab_stop: bool,
}

impl FocusId {
    /// Obtains whether the element associated with this handle is currently focused.
    pub fn is_focused(&self, window: &Window) -> bool {
        window.focus == Some(*self)
    }

    /// Obtains whether the element associated with this handle contains the focused
    /// element or is itself focused.
    pub fn contains_focused(&self, window: &Window, cx: &App) -> bool {
        window
            .focused(cx)
            .is_some_and(|focused| self.contains(focused.id, window))
    }

    /// Obtains whether the element associated with this handle is contained within the
    /// focused element or is itself focused.
    pub fn within_focused(&self, window: &Window, cx: &App) -> bool {
        let focused = window.focused(cx);
        focused.is_some_and(|focused| focused.id.contains(*self, window))
    }

    /// Obtains whether this handle contains the given handle in the most recently rendered frame.
    pub(crate) fn contains(&self, other: Self, window: &Window) -> bool {
        window
            .rendered_frame
            .dispatch_tree
            .focus_contains(*self, other)
    }
}

/// A handle which can be used to track and manipulate the focused element in a window.
pub struct FocusHandle {
    pub(crate) id: FocusId,
    handles: Arc<FocusMap>,
    /// The index of this element in the tab order.
    pub tab_index: isize,
    /// Whether this element can be focused by tab navigation.
    pub tab_stop: bool,
}

impl std::fmt::Debug for FocusHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("FocusHandle({:?})", self.id))
    }
}

impl FocusHandle {
    pub(crate) fn new(handles: &Arc<FocusMap>) -> Self {
        let id = handles.write().insert(FocusRef {
            ref_count: AtomicUsize::new(1),
            tab_index: 0,
            tab_stop: false,
        });

        Self {
            id,
            tab_index: 0,
            tab_stop: false,
            handles: handles.clone(),
        }
    }

    pub(crate) fn for_id(id: FocusId, handles: &Arc<FocusMap>) -> Option<Self> {
        let lock = handles.read();
        let focus = lock.get(id)?;
        if atomic_incr_if_not_zero(&focus.ref_count) == 0 {
            return None;
        }
        Some(Self {
            id,
            tab_index: focus.tab_index,
            tab_stop: focus.tab_stop,
            handles: handles.clone(),
        })
    }

    /// Sets the tab index of the element associated with this handle.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = index;
        if let Some(focus) = self.handles.write().get_mut(self.id) {
            focus.tab_index = index;
        }
        self
    }

    /// Sets whether the element associated with this handle is a tab stop.
    ///
    /// When `false`, the element will not be included in the tab order.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        if let Some(focus) = self.handles.write().get_mut(self.id) {
            focus.tab_stop = tab_stop;
        }
        self
    }

    /// Converts this focus handle into a weak variant, which does not prevent it from being released.
    pub fn downgrade(&self) -> WeakFocusHandle {
        WeakFocusHandle {
            id: self.id,
            handles: Arc::downgrade(&self.handles),
        }
    }

    /// Moves the focus to the element associated with this handle.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(self, cx)
    }

    /// Obtains whether the element associated with this handle is currently focused.
    pub fn is_focused(&self, window: &Window) -> bool {
        self.id.is_focused(window)
    }

    /// Obtains whether the element associated with this handle contains the focused
    /// element or is itself focused.
    pub fn contains_focused(&self, window: &Window, cx: &App) -> bool {
        self.id.contains_focused(window, cx)
    }

    /// Obtains whether the element associated with this handle is contained within the
    /// focused element or is itself focused.
    pub fn within_focused(&self, window: &Window, cx: &mut App) -> bool {
        self.id.within_focused(window, cx)
    }

    /// Obtains whether this handle contains the given handle in the most recently rendered frame.
    pub fn contains(&self, other: &Self, window: &Window) -> bool {
        self.id.contains(other.id, window)
    }

    /// Dispatch an action on the element that rendered this focus handle.
    pub fn dispatch_action(
        &self,
        action: &dyn Action,
        window: &mut Window,
        cx: &mut App,
    ) -> ActionDispatchReport {
        if let Some(node_id) = window
            .rendered_frame
            .dispatch_tree
            .focusable_node_id(self.id)
        {
            window.dispatch_action_on_node_from(
                node_id,
                ActionInvocationSource::Programmatic,
                action,
                cx,
            )
        } else {
            window.dispatch_unhandled_action(ActionInvocationSource::Programmatic, action, cx)
        }
    }
}

impl Clone for FocusHandle {
    fn clone(&self) -> Self {
        Self::for_id(self.id, &self.handles).unwrap()
    }
}

impl PartialEq for FocusHandle {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for FocusHandle {}

impl Drop for FocusHandle {
    fn drop(&mut self) {
        self.handles
            .read()
            .get(self.id)
            .unwrap()
            .ref_count
            .fetch_sub(1, SeqCst);
    }
}

/// A weak reference to a focus handle.
#[derive(Clone, Debug)]
pub struct WeakFocusHandle {
    pub(crate) id: FocusId,
    pub(crate) handles: Weak<FocusMap>,
}

impl WeakFocusHandle {
    /// Attempts to upgrade the [WeakFocusHandle] to a [FocusHandle].
    pub fn upgrade(&self) -> Option<FocusHandle> {
        let handles = self.handles.upgrade()?;
        FocusHandle::for_id(self.id, &handles)
    }
}

impl PartialEq for WeakFocusHandle {
    fn eq(&self, other: &WeakFocusHandle) -> bool {
        self.id == other.id
    }
}

impl Eq for WeakFocusHandle {}

impl PartialEq<FocusHandle> for WeakFocusHandle {
    fn eq(&self, other: &FocusHandle) -> bool {
        self.id == other.id
    }
}

impl PartialEq<WeakFocusHandle> for FocusHandle {
    fn eq(&self, other: &WeakFocusHandle) -> bool {
        self.id == other.id
    }
}

/// Focusable allows users of your view to easily
/// focus it (using window.focus_view(cx, view))
pub trait Focusable: 'static {
    /// Returns the focus handle associated with this view.
    fn focus_handle(&self, cx: &App) -> FocusHandle;
}

impl<V: Focusable> Focusable for Entity<V> {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }
}

/// ManagedView is a view (like a Modal, Popover, Menu, etc.)
/// where the lifecycle of the view is handled by another view.
pub trait ManagedView: Focusable + EventEmitter<DismissEvent> + Render {}

impl<M: Focusable + EventEmitter<DismissEvent> + Render> ManagedView for M {}

/// Emitted by implementers of [`ManagedView`] to indicate the view should be dismissed, such as when a view is presented as a modal.
pub struct DismissEvent;

type FrameCallback = Box<dyn FnOnce(&mut Window, &mut App)>;

pub(crate) type AnyMouseListener =
    Box<dyn FnMut(&dyn Any, DispatchPhase, &mut Window, &mut App) + 'static>;

#[derive(Clone)]
pub(crate) struct CursorStyleRequest {
    pub(crate) hitbox_id: Option<HitboxId>,
    pub(crate) style: CursorStyle,
}

fn resolve_cursor_style(
    requests: &[CursorStyleRequest],
    hit_test: &HitTest,
    captured_hitbox: Option<HitboxId>,
) -> Option<CursorStyle> {
    if let Some(style) = requests
        .iter()
        .rev()
        .find_map(|request| request.hitbox_id.is_none().then_some(request.style))
    {
        return Some(style);
    }

    let style_for_hitbox = |hitbox_id| {
        requests
            .iter()
            .rev()
            .find_map(|request| (request.hitbox_id == Some(hitbox_id)).then_some(request.style))
    };

    if let Some(style) = captured_hitbox.and_then(style_for_hitbox) {
        return Some(style);
    }

    hit_test
        .ids
        .iter()
        .take(hit_test.hover_hitbox_count)
        .find_map(|hitbox_id| style_for_hitbox(*hitbox_id))
}

#[derive(Default, Eq, PartialEq)]
pub(crate) struct HitTest {
    pub(crate) ids: SmallVec<[HitboxId; 8]>,
    pub(crate) hover_hitbox_count: usize,
}

/// A type of window control area that corresponds to the platform window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowControlArea {
    /// An area that allows dragging of the platform window.
    Drag,
    /// An area that allows closing of the platform window.
    Close,
    /// An area that allows maximizing of the platform window.
    Max,
    /// An area that allows minimizing of the platform window.
    Min,
}

/// An identifier for a [Hitbox] which also includes [HitboxBehavior].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct HitboxId(u64);

const POSITIONED_HITBOX_ID_BASE: u64 = 1 << 62;
const TOP_LAYER_HITBOX_ID_BASE: u64 = 1 << 63;

#[cfg(feature = "test-support")]
impl HitboxId {
    /// A placeholder HitboxId exclusively for integration testing API's that
    /// need a hitbox but where the value of the hitbox does not matter. The
    /// alternative is to make the Hitbox optional but that complicates the
    /// implementation.
    pub const fn placeholder() -> Self {
        Self(0)
    }
}

impl HitboxId {
    /// Checks if the hitbox with this ID is currently hovered. Returns `false` during keyboard
    /// input modality so that keyboard navigation suppresses hover highlights. Except when handling
    /// `ScrollWheelEvent`, this is typically what you want when determining whether to handle mouse
    /// events or paint hover styles.
    ///
    /// See [`Hitbox::is_hovered`] for details.
    pub fn is_hovered(self, window: &Window) -> bool {
        // If this hitbox has captured the pointer, it's always considered hovered
        if window.captured_hitbox == Some(self) {
            return true;
        }
        if window.last_input_was_keyboard() {
            return false;
        }
        self.hit_test(window)
    }

    fn hit_test(self, window: &Window) -> bool {
        let hit_test = &window.mouse_hit_test;
        for id in hit_test.ids.iter().take(hit_test.hover_hitbox_count) {
            if self == *id {
                return true;
            }
        }
        false
    }

    /// Checks if the hitbox with this ID contains the mouse and should handle scroll events.
    /// Typically this should only be used when handling `ScrollWheelEvent`, and otherwise
    /// `is_hovered` should be used. See the documentation of `Hitbox::is_hovered` for details about
    /// this distinction.
    pub fn should_handle_scroll(self, window: &Window) -> bool {
        window.mouse_hit_test.ids.contains(&self)
    }

    fn next(mut self) -> HitboxId {
        HitboxId(self.0.wrapping_add(1))
    }

    fn positioned(self) -> HitboxId {
        HitboxId(POSITIONED_HITBOX_ID_BASE + self.0)
    }

    fn top_layer(self) -> HitboxId {
        HitboxId(TOP_LAYER_HITBOX_ID_BASE + self.0)
    }
}

/// A rectangular region that potentially blocks hitboxes inserted prior.
/// See [Window::insert_hitbox] for more details.
#[derive(Clone, Debug, Deref)]
pub struct Hitbox {
    /// A unique identifier for the hitbox.
    pub id: HitboxId,
    /// The bounds of the hitbox.
    #[deref]
    pub bounds: Bounds<Pixels>,
    /// The content mask when the hitbox was inserted.
    pub content_mask: ContentMask<Pixels>,
    /// Flags that specify hitbox behavior.
    pub behavior: HitboxBehavior,
}

impl Hitbox {
    /// Checks if the hitbox is currently hovered. Returns `false` during keyboard input modality
    /// so that keyboard navigation suppresses hover highlights. Except when handling
    /// `ScrollWheelEvent`, this is typically what you want when determining whether to handle mouse
    /// events or paint hover styles.
    ///
    /// This can return `false` even when the hitbox contains the mouse, if a hitbox in front of
    /// this sets `HitboxBehavior::BlockMouse` (`InteractiveElement::occlude`) or
    /// `HitboxBehavior::BlockMouseExceptScroll` (`InteractiveElement::block_mouse_except_scroll`),
    /// or if the current input modality is keyboard (see [`Window::last_input_was_keyboard`]).
    ///
    /// Handling of `ScrollWheelEvent` should typically use `should_handle_scroll` instead.
    /// Concretely, this is due to use-cases like overlays that cause the elements under to be
    /// non-interactive while still allowing scrolling. More abstractly, this is because
    /// `is_hovered` is about element interactions directly under the mouse - mouse moves, clicks,
    /// hover styling, etc. In contrast, scrolling is about finding the current outer scrollable
    /// container.
    pub fn is_hovered(&self, window: &Window) -> bool {
        self.id.is_hovered(window)
    }

    /// Checks if the hitbox contains the mouse and should handle scroll events. Typically this
    /// should only be used when handling `ScrollWheelEvent`, and otherwise `is_hovered` should be
    /// used. See the documentation of `Hitbox::is_hovered` for details about this distinction.
    ///
    /// This can return `false` even when the hitbox contains the mouse, if a hitbox in front of
    /// this sets `HitboxBehavior::BlockMouse` (`InteractiveElement::occlude`).
    pub fn should_handle_scroll(&self, window: &Window) -> bool {
        self.id.should_handle_scroll(window)
    }
}

/// How the hitbox affects mouse behavior.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum HitboxBehavior {
    /// Normal hitbox mouse behavior, doesn't affect mouse handling for other hitboxes.
    #[default]
    Normal,

    /// All hitboxes behind this hitbox will be ignored and so will have `hitbox.is_hovered() ==
    /// false` and `hitbox.should_handle_scroll() == false`. Typically for elements this causes
    /// skipping of all mouse events, hover styles, and tooltips. This flag is set by
    /// [`InteractiveElement::occlude`].
    ///
    /// For mouse handlers that check those hitboxes, this behaves the same as registering a
    /// bubble-phase handler for every mouse event type:
    ///
    /// ```ignore
    /// window.on_pointer_event(move |_: &EveryMouseEventTypeHere, phase, window, cx| {
    ///     if phase == DispatchPhase::Capture && hitbox.is_hovered(window) {
    ///         cx.stop_propagation();
    ///     }
    /// })
    /// ```
    ///
    /// This has effects beyond event handling - any use of hitbox checking, such as hover
    /// styles and tooltips. These other behaviors are the main point of this mechanism. An
    /// alternative might be to not affect mouse event handling - but this would allow
    /// inconsistent UI where clicks and moves interact with elements that are not considered to
    /// be hovered.
    BlockMouse,

    /// All hitboxes behind this hitbox will have `hitbox.is_hovered() == false`, even when
    /// `hitbox.should_handle_scroll() == true`. Typically for elements this causes all mouse
    /// interaction except scroll events to be ignored - see the documentation of
    /// [`Hitbox::is_hovered`] for details. This flag is set by
    /// [`InteractiveElement::block_mouse_except_scroll`].
    ///
    /// For mouse handlers that check those hitboxes, this behaves the same as registering a
    /// bubble-phase handler for every mouse event type **except** `ScrollWheelEvent`:
    ///
    /// ```ignore
    /// window.on_pointer_event(move |_: &EveryMouseEventTypeExceptScroll, phase, window, cx| {
    ///     if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
    ///         cx.stop_propagation();
    ///     }
    /// })
    /// ```
    ///
    /// See the documentation of [`Hitbox::is_hovered`] for details of why `ScrollWheelEvent` is
    /// handled differently than other mouse events. If also blocking these scroll events is
    /// desired, then a `cx.stop_propagation()` handler like the one above can be used.
    ///
    /// This has effects beyond event handling - this affects any use of `is_hovered`, such as
    /// hover styles and tooltips. These other behaviors are the main point of this mechanism.
    /// An alternative might be to not affect mouse event handling - but this would allow
    /// inconsistent UI where clicks and moves interact with elements that are not considered to
    /// be hovered.
    BlockMouseExceptScroll,

    /// The hitbox is absent from pointer hit testing while its layout, paint,
    /// and semantics remain available.
    Ignore,
}

/// An identifier for a tooltip.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct TooltipId(usize);

impl TooltipId {
    /// Checks if the tooltip is currently hovered.
    pub fn is_hovered(&self, window: &Window) -> bool {
        window
            .tooltip_bounds
            .as_ref()
            .is_some_and(|tooltip_bounds| {
                tooltip_bounds.id == *self
                    && tooltip_bounds.bounds.contains(&window.mouse_position())
            })
    }
}

pub(crate) struct TooltipBounds {
    id: TooltipId,
    bounds: Bounds<Pixels>,
}

#[derive(Clone)]
pub(crate) struct TooltipRequest {
    id: TooltipId,
    tooltip: AnyTooltip,
}

pub(crate) struct DeferredDraw {
    current_view: EntityId,
    priority: usize,
    parent_node: DispatchNodeId,
    element_id_stack: SmallVec<[ElementId; 32]>,
    text_style_stack: Vec<TextStyleRefinement>,
    content_mask: Option<ContentMask<Pixels>>,
    rem_size: Pixels,
    element: Option<AnyElement>,
    absolute_offset: Point<Pixels>,
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
}

pub(crate) struct WindowFrame {
    pub(crate) focus: Option<FocusId>,
    pub(crate) window_active: bool,
    pub(crate) element_states: FxHashMap<(GlobalElementId, TypeId), ElementStateBox>,
    accessed_element_states: Vec<(GlobalElementId, TypeId)>,
    pub(crate) mouse_listeners: Vec<Option<AnyMouseListener>>,
    pub(crate) dispatch_tree: DispatchTree,
    pub(crate) scene: Scene,
    pub(crate) hitboxes: Vec<Hitbox>,
    pub(crate) hitbox_dispatch_nodes: FxHashMap<HitboxId, DispatchNodeId>,
    pub(crate) window_control_hitboxes: Vec<(WindowControlArea, Hitbox)>,
    pub(crate) deferred_draws: Vec<DeferredDraw>,
    pub(crate) input_handlers: Vec<Option<PlatformInputHandler>>,
    pub(crate) tooltip_requests: Vec<Option<TooltipRequest>>,
    pub(crate) cursor_styles: Vec<CursorStyleRequest>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) debug_bounds: FxHashMap<String, Bounds<Pixels>>,
    pub(crate) devtools_nodes: Vec<DevtoolsElementSnapshot>,
    pub(crate) devtools_property_setters: FxHashMap<usize, Vec<DevtoolsPropertySetter>>,
    pub(crate) devtools_node_stack: Vec<usize>,
    pub(crate) next_devtools_node_id: usize,
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) next_inspector_instance_ids: FxHashMap<Rc<crate::InspectorElementPath>, usize>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) inspector_hitboxes: FxHashMap<HitboxId, crate::InspectorElementId>,
    pub(crate) tab_stops: TabStopMap,
}

#[derive(Clone, Default)]
pub(crate) struct PrepaintStateIndex {
    hitboxes_index: usize,
    tooltips_index: usize,
    deferred_draws_index: usize,
    dispatch_tree_index: usize,
    accessed_element_states_index: usize,
    line_layout_index: LineLayoutIndex,
}

#[derive(Clone, Default)]
pub(crate) struct PaintIndex {
    scene_index: usize,
    mouse_listeners_index: usize,
    input_handlers_index: usize,
    cursor_styles_index: usize,
    accessed_element_states_index: usize,
    tab_handle_index: usize,
    line_layout_index: LineLayoutIndex,
}

impl WindowFrame {
    pub(crate) fn new(dispatch_tree: DispatchTree) -> Self {
        WindowFrame {
            focus: None,
            window_active: false,
            element_states: FxHashMap::default(),
            accessed_element_states: Vec::new(),
            mouse_listeners: Vec::new(),
            dispatch_tree,
            scene: Scene::default(),
            hitboxes: Vec::new(),
            hitbox_dispatch_nodes: FxHashMap::default(),
            window_control_hitboxes: Vec::new(),
            deferred_draws: Vec::new(),
            input_handlers: Vec::new(),
            tooltip_requests: Vec::new(),
            cursor_styles: Vec::new(),

            #[cfg(any(test, feature = "test-support"))]
            debug_bounds: FxHashMap::default(),

            devtools_nodes: Vec::new(),
            devtools_property_setters: FxHashMap::default(),
            devtools_node_stack: Vec::new(),
            next_devtools_node_id: 0,

            #[cfg(any(feature = "inspector", debug_assertions))]
            next_inspector_instance_ids: FxHashMap::default(),

            #[cfg(any(feature = "inspector", debug_assertions))]
            inspector_hitboxes: FxHashMap::default(),
            tab_stops: TabStopMap::default(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.element_states.clear();
        self.accessed_element_states.clear();
        self.mouse_listeners.clear();
        self.dispatch_tree.clear();
        self.scene.clear();
        self.input_handlers.clear();
        self.tooltip_requests.clear();
        self.cursor_styles.clear();
        self.hitboxes.clear();
        self.hitbox_dispatch_nodes.clear();
        self.window_control_hitboxes.clear();
        self.deferred_draws.clear();
        self.tab_stops.clear();
        self.focus = None;

        #[cfg(any(test, feature = "test-support"))]
        {
            self.debug_bounds.clear();
        }

        self.devtools_nodes.clear();
        self.devtools_property_setters.clear();
        self.devtools_node_stack.clear();
        self.next_devtools_node_id = 0;

        #[cfg(any(feature = "inspector", debug_assertions))]
        {
            self.next_inspector_instance_ids.clear();
            self.inspector_hitboxes.clear();
        }
    }

    pub(crate) fn cursor_style(&self, window: &Window) -> Option<CursorStyle> {
        resolve_cursor_style(
            &self.cursor_styles,
            &window.mouse_hit_test,
            window.captured_hitbox,
        )
    }

    pub(crate) fn hit_test(&self, position: Point<Pixels>) -> HitTest {
        let mut set_hover_hitbox_count = false;
        let mut hit_test = HitTest::default();
        let mut hitboxes = self
            .hitboxes
            .iter()
            .filter(|hitbox| {
                hitbox.behavior != HitboxBehavior::Ignore
                    && hitbox
                        .bounds
                        .intersect(&hitbox.content_mask.bounds)
                        .contains(&position)
            })
            .collect::<SmallVec<[_; 16]>>();
        hitboxes.sort_by_key(|hitbox| std::cmp::Reverse(hitbox.id.0));

        for hitbox in hitboxes {
            hit_test.ids.push(hitbox.id);
            if !set_hover_hitbox_count && hitbox.behavior == HitboxBehavior::BlockMouseExceptScroll
            {
                hit_test.hover_hitbox_count = hit_test.ids.len();
                set_hover_hitbox_count = true;
            }
            if hitbox.behavior == HitboxBehavior::BlockMouse {
                break;
            }
        }
        if !set_hover_hitbox_count {
            hit_test.hover_hitbox_count = hit_test.ids.len();
        }
        hit_test
    }

    pub(crate) fn focus_path(&self) -> SmallVec<[FocusId; 8]> {
        self.focus
            .map(|focus_id| self.dispatch_tree.focus_path(focus_id))
            .unwrap_or_default()
    }

    pub(crate) fn finish(&mut self, prev_frame: &mut Self) {
        for element_state_key in &self.accessed_element_states {
            if let Some((element_state_key, element_state)) =
                prev_frame.element_states.remove_entry(element_state_key)
            {
                self.element_states.insert(element_state_key, element_state);
            }
        }

        self.scene.finish();
    }
}

/// Read-only geometry of an element rendered in a GPUI frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DevtoolsBounds {
    /// The left edge in window-local pixels.
    pub x: f32,
    /// The top edge in window-local pixels.
    pub y: f32,
    /// The width in pixels.
    pub width: f32,
    /// The height in pixels.
    pub height: f32,
}

impl From<Bounds<Pixels>> for DevtoolsBounds {
    fn from(bounds: Bounds<Pixels>) -> Self {
        Self {
            x: bounds.origin.x.as_f32(),
            y: bounds.origin.y.as_f32(),
            width: bounds.size.width.as_f32(),
            height: bounds.size.height.as_f32(),
        }
    }
}

/// Source location for a rendered element, when debug metadata is available.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevtoolsElementSource {
    /// The source file that constructed the element.
    pub file: String,
    /// The one-based source line that constructed the element.
    pub line: u32,
    /// The one-based source column that constructed the element.
    pub column: u32,
}

impl DevtoolsElementSource {
    fn from_location(location: &'static core::panic::Location<'static>) -> Self {
        Self {
            file: location.file().to_string(),
            line: location.line(),
            column: location.column(),
        }
    }
}

/// Read-only inspection data for one rendered GPUI element.
#[derive(Clone, Debug, PartialEq)]
pub struct DevtoolsElementSnapshot {
    /// Per-frame rendered element occurrence id.
    pub devtools_node_id: usize,
    /// The parent rendered element occurrence id for this frame, if any.
    pub parent_devtools_node_id: Option<usize>,
    /// The GPUI dispatch-tree node id for this rendered frame.
    pub dispatch_node_id: usize,
    /// The parent GPUI dispatch-tree node id for this rendered frame.
    pub parent_dispatch_node_id: Option<usize>,
    /// The Rust type name of the rendered element.
    pub type_name: String,
    /// The stable element id path, when the element supplied one.
    pub global_id: Option<String>,
    /// Debug source information, when available.
    pub source: Option<DevtoolsElementSource>,
    /// The element bounds in window-local pixels.
    pub bounds: DevtoolsBounds,
    /// Component-authored accessibility and DevKit semantics for this element.
    pub semantics: DevtoolsSemantics,
    /// The number of hitboxes registered while prepainting this element subtree.
    pub hitbox_count: usize,
    /// The number of keyboard listeners registered on this element.
    pub key_listener_count: usize,
    /// The number of action listeners registered on this element.
    pub action_listener_count: usize,
    /// Whether this element owns the currently focused dispatch node.
    pub focused: bool,
}

/// Component-authored accessibility and DevKit semantics for a rendered element.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DevtoolsSemantics {
    /// Accessibility role such as `button`, `textbox`, `checkbox`, or `text`.
    pub role: Option<String>,
    /// Accessible name exposed to automation agents.
    pub name: Option<String>,
    /// Text content rendered by this element.
    pub text: Option<String>,
    /// Editable value exposed by this element.
    pub value: Option<String>,
    /// Placeholder text for editable controls.
    pub placeholder: Option<String>,
    /// Whether the element is enabled.
    pub enabled: Option<bool>,
    /// Whether the element is selected.
    pub selected: Option<bool>,
    /// Whether the element is checked.
    pub checked: Option<bool>,
    /// Whether the element is expanded.
    pub expanded: Option<bool>,
    /// Whether the element is required by its owning form.
    pub required: Option<bool>,
    /// Whether the element currently has an invalid value.
    pub invalid: Option<bool>,
    /// Whether the element is read-only.
    pub read_only: Option<bool>,
    /// Semantic orientation such as `horizontal` or `vertical`.
    pub orientation: Option<String>,
    /// Stable authored ids of elements that label this element.
    pub labelled_by: Vec<String>,
    /// Stable authored ids of elements that describe this element.
    pub described_by: Vec<String>,
    /// Stable authored id of the element that contains the validation error.
    pub error_message: Option<String>,
    /// Stable authored ids of controls owned by this element.
    pub controls: Vec<String>,
    /// Pointer operations exposed by this element without changing its label or value semantics.
    pub pointer_actions: Vec<DevtoolsPointerAction>,
    /// Component-authored writable properties for live DevKit mutation.
    pub writable_properties: Vec<DevtoolsWritableProperty>,
}

/// A pointer operation exposed to accessibility and DevKit clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DevtoolsPointerAction {
    /// Activates the element with a primary tap or click.
    Activate,
    /// Invokes the element's long-press affordance.
    LongPress,
    /// Starts a drag interaction.
    Drag,
    /// Changes an element's scale.
    Scale,
}

impl DevtoolsSemantics {
    /// Create empty semantics to populate with builder methods.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the accessibility role.
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }

    /// Set the accessible name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set text content.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Set the editable value.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Set placeholder text.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Set whether the element is enabled.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Set whether the element is selected.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    /// Set whether the element is checked.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Set whether the element is expanded.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// Set whether this element is required by its owning form.
    pub fn required(mut self, required: bool) -> Self {
        self.required = Some(required);
        self
    }

    /// Set whether this element currently has an invalid value.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = Some(invalid);
        self
    }

    /// Set whether this element is read-only.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = Some(read_only);
        self
    }

    /// Set the semantic orientation.
    pub fn orientation(mut self, orientation: impl Into<String>) -> Self {
        self.orientation = Some(orientation.into());
        self
    }

    /// Add the stable authored id of an element that labels this element.
    pub fn labelled_by(mut self, id: impl Into<String>) -> Self {
        push_unique_string(&mut self.labelled_by, id.into());
        self
    }

    /// Add the stable authored id of an element that describes this element.
    pub fn described_by(mut self, id: impl Into<String>) -> Self {
        push_unique_string(&mut self.described_by, id.into());
        self
    }

    /// Set the stable authored id of the validation error element.
    pub fn error_message(mut self, id: impl Into<String>) -> Self {
        self.error_message = Some(id.into());
        self
    }

    /// Add the stable authored id of a control owned by this element.
    pub fn controls(mut self, id: impl Into<String>) -> Self {
        push_unique_string(&mut self.controls, id.into());
        self
    }

    /// Adds a pointer operation without changing the rest of this element's semantics.
    pub fn pointer_action(mut self, action: DevtoolsPointerAction) -> Self {
        if !self.pointer_actions.contains(&action) {
            self.pointer_actions.push(action);
        }
        self
    }

    /// Add a writable property exposed by the component.
    pub fn writable_property(
        mut self,
        name: impl Into<String>,
        value_type: impl Into<String>,
    ) -> Self {
        self.writable_properties.push(DevtoolsWritableProperty {
            name: name.into(),
            value_type: value_type.into(),
        });
        self
    }

    fn merge(&mut self, other: Self) {
        if other.role.is_some() {
            self.role = other.role;
        }
        if other.name.is_some() {
            self.name = other.name;
        }
        if other.text.is_some() {
            self.text = other.text;
        }
        if other.value.is_some() {
            self.value = other.value;
        }
        if other.placeholder.is_some() {
            self.placeholder = other.placeholder;
        }
        if other.enabled.is_some() {
            self.enabled = other.enabled;
        }
        if other.selected.is_some() {
            self.selected = other.selected;
        }
        if other.checked.is_some() {
            self.checked = other.checked;
        }
        if other.expanded.is_some() {
            self.expanded = other.expanded;
        }
        if other.required.is_some() {
            self.required = other.required;
        }
        if other.invalid.is_some() {
            self.invalid = other.invalid;
        }
        if other.read_only.is_some() {
            self.read_only = other.read_only;
        }
        if other.orientation.is_some() {
            self.orientation = other.orientation;
        }
        for id in other.labelled_by {
            push_unique_string(&mut self.labelled_by, id);
        }
        for id in other.described_by {
            push_unique_string(&mut self.described_by, id);
        }
        if other.error_message.is_some() {
            self.error_message = other.error_message;
        }
        for id in other.controls {
            push_unique_string(&mut self.controls, id);
        }
        for action in other.pointer_actions {
            if !self.pointer_actions.contains(&action) {
                self.pointer_actions.push(action);
            }
        }
        for property in other.writable_properties {
            if !self
                .writable_properties
                .iter()
                .any(|existing| existing.name == property.name)
            {
                self.writable_properties.push(property);
            }
        }
    }
}

fn push_unique_string(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

/// A component-authored writable property exposed to DevKit agents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevtoolsWritableProperty {
    /// Property name accepted by the component setter.
    pub name: String,
    /// Human-readable JSON value type, such as `string`, `bool`, or `number`.
    pub value_type: String,
}

type DevtoolsPropertyCallback =
    Rc<dyn Fn(serde_json::Value, &mut Window, &mut App) -> std::result::Result<(), String>>;

#[derive(Clone)]
pub(crate) struct DevtoolsPropertySetter {
    pub(crate) property: DevtoolsWritableProperty,
    pub(crate) callback: DevtoolsPropertyCallback,
}

/// Read-only inspection data for the last rendered GPUI frame.
#[derive(Clone, Debug, PartialEq)]
pub struct DevtoolsFrameSnapshot {
    /// The window-local viewport covered by this frame.
    pub viewport: DevtoolsBounds,
    /// Shader-effect diagnostics for this frame.
    pub effects: DevtoolsEffectSnapshot,
    /// The rendered element tree flattened in prepaint order.
    pub elements: Vec<DevtoolsElementSnapshot>,
}

/// Read-only shader-effect diagnostics for the last rendered GPUI frame.
#[derive(Clone, Debug, PartialEq)]
pub struct DevtoolsEffectSnapshot {
    /// Number of active screen effects in the rendered scene.
    pub active_screen_effect_count: usize,
    /// Number of retained screen-effect snapshots in the rendered scene.
    pub screen_effect_snapshot_count: usize,
    /// Number of element visual-effect primitives in the rendered scene.
    pub visual_effect_count: usize,
    /// Number of registered screen-effect regions in the rendered scene.
    pub screen_effect_region_count: usize,
    /// Number of active region-targeted screen effects whose region is absent.
    pub missing_screen_effect_region_count: usize,
    /// Number of active snapshot transitions whose retained snapshot is absent.
    pub missing_screen_effect_snapshot_count: usize,
    /// Capabilities reported by the renderer backing this window.
    pub capabilities: EffectCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
enum InputModality {
    Mouse,
    Keyboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StickyRelationRecord {
    predecessor: Option<usize>,
    run_root: usize,
    depth: usize,
}

#[derive(Clone, Debug, Default)]
#[doc(hidden)]
pub struct StickyRelationIndex {
    candidate_indices: Vec<usize>,
    records: Vec<StickyRelationRecord>,
}

impl StickyRelationIndex {
    /// Builds the canonical relation index for an ordered sticky-candidate sequence.
    #[doc(hidden)]
    pub fn from_candidates(candidates: impl IntoIterator<Item = (usize, StickyStacking)>) -> Self {
        let candidates = candidates.into_iter().collect::<Vec<_>>();
        let mut records: Vec<StickyRelationRecord> = Vec::with_capacity(candidates.len());
        let mut active_levels = Vec::<Option<usize>>::new();
        let mut keyed_tail: Option<(ElementId, usize)> = None;

        for (_, relation) in &candidates {
            let record_index = records.len();
            let record = match relation {
                StickyStacking::Level(level) => {
                    keyed_tail = None;
                    let level = *level as usize;
                    active_levels.truncate(level);
                    while active_levels.len() < level {
                        active_levels.push(None);
                    }
                    let predecessor = level
                        .checked_sub(1)
                        .and_then(|parent_level| active_levels[parent_level]);
                    let (run_root, depth) = predecessor
                        .map(|predecessor| {
                            let predecessor = records[predecessor];
                            (predecessor.run_root, predecessor.depth + 1)
                        })
                        .unwrap_or((record_index, 0));
                    active_levels.push(Some(record_index));
                    StickyRelationRecord {
                        predecessor,
                        run_root,
                        depth,
                    }
                }
                StickyStacking::StackId(stack_id) => {
                    active_levels.clear();
                    let predecessor = keyed_tail.as_ref().and_then(|(previous_id, previous)| {
                        (previous_id == stack_id).then_some(*previous)
                    });
                    let (run_root, depth) = predecessor
                        .map(|predecessor| {
                            let predecessor = records[predecessor];
                            (predecessor.run_root, predecessor.depth + 1)
                        })
                        .unwrap_or((record_index, 0));
                    keyed_tail = Some((stack_id.clone(), record_index));
                    StickyRelationRecord {
                        predecessor,
                        run_root,
                        depth,
                    }
                }
            };
            records.push(record);
        }

        Self {
            candidate_indices: candidates
                .into_iter()
                .map(|(candidate_index, _)| candidate_index)
                .collect(),
            records,
        }
    }

    pub(crate) fn record(&self, index: usize) -> Option<StickyRelationRecord> {
        self.records.get(index).copied()
    }

    /// Returns the relation-record position at or before the supplied candidate index.
    #[doc(hidden)]
    pub fn source_at_or_before(&self, candidate_index: usize) -> Option<usize> {
        self.candidate_indices
            .partition_point(|candidate| *candidate <= candidate_index)
            .checked_sub(1)
    }

    /// Returns the active relation-record positions for the supplied source.
    #[doc(hidden)]
    pub fn active_chain(&self, source: usize) -> Vec<usize> {
        let mut chain = Vec::with_capacity(
            self.records
                .get(source)
                .map_or(0, |record| record.depth + 1),
        );
        let mut current = Some(source);
        while let Some(index) = current {
            chain.push(index);
            current = self.records[index].predecessor;
        }
        chain.reverse();
        chain
    }

    /// Computes active offsets from candidate insets and extents.
    #[doc(hidden)]
    pub fn active_offsets(
        &self,
        source: usize,
        insets: &[Pixels],
        extents: &[Pixels],
    ) -> Vec<(usize, Pixels)> {
        let mut next_offset = px(0.);
        self.active_chain(source)
            .into_iter()
            .map(|index| {
                let offset = next_offset + insets[index];
                next_offset = offset + extents[index];
                (index, offset)
            })
            .collect()
    }

    #[cfg(test)]
    fn record_count(&self) -> usize {
        self.records.len()
    }

    #[cfg(test)]
    fn active_chain_with_visits(&self, source: usize) -> (Vec<usize>, usize) {
        let chain = self.active_chain(source);
        let visits = chain.len();
        (chain, visits)
    }
}

type StickyLayoutContextId = usize;
type StickyOccurrenceId = usize;

#[derive(Clone, Copy)]
struct StickyLayoutContext {
    id: StickyLayoutContextId,
    overflow: Point<Overflow>,
}

#[derive(Clone)]
struct KeyedStickyCandidate {
    layout_id: Option<LayoutId>,
    stack_id: ElementId,
    inset: Edges<Length>,
    horizontal_context: Option<StickyLayoutContextId>,
    vertical_context: Option<StickyLayoutContextId>,
}

#[derive(Clone, Copy, Default)]
struct KeyedStickyCandidatePlan {
    x: Option<ResolvedStickyAxis>,
    y: Option<ResolvedStickyAxis>,
}

/// Describes the active scroll container used to resolve sticky descendants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollContainerContext {
    /// Bounds of the visible scrollport in window coordinates.
    pub scrollport_bounds: Bounds<Pixels>,
    /// Bounds of the full scrollable content in window coordinates.
    pub content_bounds: Bounds<Pixels>,
    /// Current scroll offset applied to the content.
    pub scroll_offset: Point<Pixels>,
    /// Overflow behavior by axis.
    pub overflow: Point<Overflow>,
}

impl ScrollContainerContext {
    fn is_active_on_axis(&self, axis: ScrollAxis) -> bool {
        match axis {
            ScrollAxis::Horizontal => self.overflow.x == Overflow::Scroll,
            ScrollAxis::Vertical => self.overflow.y == Overflow::Scroll,
        }
    }
}

#[derive(Default)]
struct ScrollContainerContextStack {
    stack: Vec<ScrollContainerContext>,
}

impl ScrollContainerContextStack {
    fn push(&mut self, context: ScrollContainerContext) {
        self.stack.push(context);
    }

    fn pop(&mut self) -> Option<ScrollContainerContext> {
        self.stack.pop()
    }

    fn nearest(&self, axis: ScrollAxis) -> Option<ScrollContainerContext> {
        self.stack
            .iter()
            .rev()
            .find(|context| context.is_active_on_axis(axis))
            .copied()
    }

    fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

/// Resolved layout and visual bounds for positioned elements.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedBounds {
    /// The element's layout bounds before visual fixed/sticky adjustment.
    pub layout_bounds: Bounds<Pixels>,
    /// The element's visual bounds after fixed/sticky adjustment.
    pub visual_bounds: Bounds<Pixels>,
    /// Difference between visual and layout origins.
    pub delta: Point<Pixels>,
}

/// The resolved sticky phase for one axis.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StickyAxisState {
    /// The element is in its normal layout position on this axis.
    #[default]
    Unstuck,
    /// The element is stuck to the axis start edge, such as left or top.
    StuckToStart,
    /// The element is stuck to the axis end edge, such as right or bottom.
    StuckToEnd,
    /// The element reached the sticky containing block's start edge.
    ReleasedAtStart,
    /// The element reached the sticky containing block's end edge.
    ReleasedAtEnd,
}

impl StickyAxisState {
    /// Returns whether this axis is actively pinned to a scrollport edge.
    pub fn is_stuck(self) -> bool {
        matches!(self, Self::StuckToStart | Self::StuckToEnd)
    }

    /// Returns whether this axis has reached its containing block limit.
    pub fn is_released(self) -> bool {
        matches!(self, Self::ReleasedAtStart | Self::ReleasedAtEnd)
    }
}

/// The resolved sticky state for an element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickyState {
    /// The horizontal sticky phase.
    pub x: StickyAxisState,
    /// The vertical sticky phase.
    pub y: StickyAxisState,
    /// The element's layout bounds before sticky adjustment.
    pub layout_bounds: Bounds<Pixels>,
    /// The element's visual bounds after sticky adjustment.
    pub visual_bounds: Bounds<Pixels>,
}

impl StickyState {
    /// Returns whether either axis is actively pinned to a scrollport edge.
    pub fn is_stuck(self) -> bool {
        self.is_stuck_x() || self.is_stuck_y()
    }

    /// Returns whether the horizontal axis is actively pinned.
    pub fn is_stuck_x(self) -> bool {
        self.x.is_stuck()
    }

    /// Returns whether the vertical axis is actively pinned.
    pub fn is_stuck_y(self) -> bool {
        self.y.is_stuck()
    }

    /// Returns whether either axis has reached its containing block limit.
    pub fn is_released(self) -> bool {
        self.x.is_released() || self.y.is_released()
    }

    pub(crate) fn same_phase_as(self, other: Self) -> bool {
        self.x == other.x && self.y == other.y
    }
}

/// Event sent when a sticky element transitions between sticky phases.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickyStateChangedEvent {
    /// The previous sticky state. `None` means this is the first observed state.
    pub previous: Option<StickyState>,
    /// The current sticky state.
    pub current: StickyState,
}

impl PositionedBounds {
    fn unchanged(layout_bounds: Bounds<Pixels>) -> Self {
        Self {
            layout_bounds,
            visual_bounds: layout_bounds,
            delta: Point::default(),
        }
    }

    fn resolve_fixed_for_containing_block(
        style: &Style,
        layout_bounds: Bounds<Pixels>,
        containing_block_bounds: Bounds<Pixels>,
        rem_size: Pixels,
    ) -> Self {
        let mut visual_origin = layout_bounds.origin;

        if let Some(left) = length_to_pixels(
            style.inset.left,
            containing_block_bounds.size.width,
            rem_size,
        ) {
            visual_origin.x = containing_block_bounds.left() + left;
        } else if let Some(right) = length_to_pixels(
            style.inset.right,
            containing_block_bounds.size.width,
            rem_size,
        ) {
            visual_origin.x = containing_block_bounds.right() - right - layout_bounds.size.width;
        }

        if let Some(top) = length_to_pixels(
            style.inset.top,
            containing_block_bounds.size.height,
            rem_size,
        ) {
            visual_origin.y = containing_block_bounds.top() + top;
        } else if let Some(bottom) = length_to_pixels(
            style.inset.bottom,
            containing_block_bounds.size.height,
            rem_size,
        ) {
            visual_origin.y = containing_block_bounds.bottom() - bottom - layout_bounds.size.height;
        }

        let visual_bounds = Bounds::new(visual_origin, layout_bounds.size);
        Self {
            layout_bounds,
            visual_bounds,
            delta: visual_bounds.origin - layout_bounds.origin,
        }
    }

    #[cfg(test)]
    fn resolve_sticky_for_scroll_contexts(
        style: &Style,
        layout_bounds: Bounds<Pixels>,
        horizontal_scroll_context: Option<ScrollContainerContext>,
        vertical_scroll_context: Option<ScrollContainerContext>,
        rem_size: Pixels,
    ) -> Self {
        Self::resolve_sticky_for_scroll_contexts_with_state(
            style,
            layout_bounds,
            horizontal_scroll_context,
            vertical_scroll_context,
            rem_size,
        )
        .0
    }

    fn resolve_sticky_for_scroll_contexts_with_state(
        style: &Style,
        layout_bounds: Bounds<Pixels>,
        horizontal_scroll_context: Option<ScrollContainerContext>,
        vertical_scroll_context: Option<ScrollContainerContext>,
        rem_size: Pixels,
    ) -> (Self, StickyState) {
        let mut visual_origin = layout_bounds.origin;
        let mut sticky_state = StickyState {
            x: StickyAxisState::Unstuck,
            y: StickyAxisState::Unstuck,
            layout_bounds,
            visual_bounds: layout_bounds,
        };

        if let Some(context) = horizontal_scroll_context {
            let resolved = resolve_sticky_axis(
                layout_bounds.origin.x,
                layout_bounds.size.width,
                context.scrollport_bounds.left(),
                context.scrollport_bounds.right(),
                context.content_bounds.left(),
                context.content_bounds.right(),
                style.inset.left,
                style.inset.right,
                context.scrollport_bounds.size.width,
                rem_size,
            );
            visual_origin.x = resolved.origin;
            sticky_state.x = resolved.state;
        }

        if let Some(context) = vertical_scroll_context {
            let resolved = resolve_sticky_axis(
                layout_bounds.origin.y,
                layout_bounds.size.height,
                context.scrollport_bounds.top(),
                context.scrollport_bounds.bottom(),
                context.content_bounds.top(),
                context.content_bounds.bottom(),
                style.inset.top,
                style.inset.bottom,
                context.scrollport_bounds.size.height,
                rem_size,
            );
            visual_origin.y = resolved.origin;
            sticky_state.y = resolved.state;
        }

        let visual_bounds = Bounds::new(visual_origin, layout_bounds.size);
        let positioned_bounds = Self {
            layout_bounds,
            visual_bounds,
            delta: visual_bounds.origin - layout_bounds.origin,
        };
        sticky_state.visual_bounds = visual_bounds;
        (positioned_bounds, sticky_state)
    }

    #[cfg(test)]
    fn resolve_noop(style: &Style, layout_bounds: Bounds<Pixels>) -> Self {
        match style.position {
            Position::Relative | Position::Absolute => Self::unchanged(layout_bounds),
            Position::Fixed | Position::Sticky => {
                panic!("resolve_noop only validates existing no-op positioning modes")
            }
        }
    }
}

#[derive(Clone, Copy)]
struct ResolvedStickyAxis {
    origin: Pixels,
    state: StickyAxisState,
}

fn length_to_pixels(length: Length, base_size: Pixels, rem_size: Pixels) -> Option<Pixels> {
    match length {
        Length::Definite(length) => Some(length.to_pixels(base_size.into(), rem_size)),
        Length::Auto => None,
    }
}

fn resolve_sticky_axis(
    layout_origin: Pixels,
    layout_size: Pixels,
    scrollport_start: Pixels,
    scrollport_end: Pixels,
    content_start: Pixels,
    content_end: Pixels,
    start_inset: Length,
    end_inset: Length,
    inset_base_size: Pixels,
    rem_size: Pixels,
) -> ResolvedStickyAxis {
    let start_inset = length_to_pixels(start_inset, inset_base_size, rem_size);
    let end_inset = length_to_pixels(end_inset, inset_base_size, rem_size);

    if start_inset.is_none() && end_inset.is_none() {
        return ResolvedStickyAxis {
            origin: layout_origin,
            state: StickyAxisState::Unstuck,
        };
    }

    let mut visual_origin = layout_origin;
    let mut state = StickyAxisState::Unstuck;
    if let Some(start_inset) = start_inset {
        let stuck_origin = scrollport_start + start_inset;
        if visual_origin < stuck_origin {
            visual_origin = stuck_origin;
            state = StickyAxisState::StuckToStart;
        }
    }
    if let Some(end_inset) = end_inset {
        let stuck_origin = scrollport_end - end_inset - layout_size;
        if visual_origin > stuck_origin {
            visual_origin = stuck_origin;
            state = StickyAxisState::StuckToEnd;
        }
    }

    let unclamped_origin = visual_origin;
    let min_origin = content_start;
    let max_origin = content_end - layout_size;
    if min_origin <= max_origin {
        visual_origin = visual_origin.clamp(min_origin, max_origin);
    } else {
        visual_origin = min_origin;
    }

    if state.is_stuck() && visual_origin != unclamped_origin {
        state = if visual_origin == min_origin {
            StickyAxisState::ReleasedAtStart
        } else {
            StickyAxisState::ReleasedAtEnd
        };
    }

    ResolvedStickyAxis {
        origin: visual_origin,
        state,
    }
}

#[derive(Clone)]
struct WindowActiveScreenEffect {
    id: ScreenEffectId,
    effect: ScreenEffect,
    started_at: Instant,
}

impl WindowActiveScreenEffect {
    fn elapsed(&self, now: Instant) -> Duration {
        now.duration_since(self.started_at)
    }

    fn is_expired(&self, now: Instant) -> bool {
        self.elapsed(now).as_millis() >= self.effect.duration_ms_value() as u128
    }

    fn snapshot(&self, now: Instant) -> ActiveScreenEffect {
        let elapsed_ms = self.elapsed(now).as_millis().min(u64::MAX as u128) as u64;
        ActiveScreenEffect::new(self.id, self.effect.clone(), elapsed_ms)
    }
}

#[derive(Clone)]
struct WindowScreenEffectSnapshot {
    id: ScreenEffectSnapshotId,
    target: ScreenEffectTarget,
}

impl WindowScreenEffectSnapshot {
    fn snapshot(&self) -> ActiveScreenEffectSnapshot {
        ActiveScreenEffectSnapshot::new(self.id, self.target.clone())
    }
}

/// Identifies one handled-action-owned pointer sequence within a window.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PointerInteractionId(u64);

/// Explains why an active pointer interaction terminated without completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerInteractionCancellationReason {
    /// The interaction's initiating hitbox no longer owns pointer capture.
    CaptureLost,
    /// The initiating hitbox or dispatch target disappeared after a redraw.
    TargetRemoved,
    /// The containing window was removed.
    WindowRemoved,
    /// The active gesture-map profile changed during the pointer sequence.
    ProfileReplaced,
}

/// Describes the lifecycle state of a pointer-interaction owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerInteractionPhase {
    /// The handled action currently owns pointer continuation.
    Active,
    /// The pointer sequence completed normally.
    Completed,
    /// The pointer sequence was cancelled for the enclosed reason.
    Cancelled(PointerInteractionCancellationReason),
}

/// The single handled action that owns continuation for a pointer sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct PointerInteractionOwner {
    id: PointerInteractionId,
    pointer_id: PointerId,
    action_name: &'static str,
    action_invocation_id: u64,
    target: usize,
    hitbox_id: HitboxId,
    button: MouseButton,
    start: Point<Pixels>,
    current: Point<Pixels>,
    gesture_revision: GestureMapVersion,
    phase: PointerInteractionPhase,
}

impl PointerInteractionOwner {
    /// Returns this interaction's window-local identifier.
    pub fn id(&self) -> PointerInteractionId {
        self.id
    }

    /// Returns the pointer sequence that owns this interaction.
    pub fn pointer_id(&self) -> PointerId {
        self.pointer_id
    }

    /// Returns the fully qualified name of the action that claimed the sequence.
    pub fn action_name(&self) -> &'static str {
        self.action_name
    }

    /// Returns the action invocation that produced the handled claim.
    pub fn action_invocation_id(&self) -> u64 {
        self.action_invocation_id
    }

    /// Returns the retained dispatch target selected at pointer-down time.
    pub fn target(&self) -> usize {
        self.target
    }

    /// Returns the retained hitbox selected at pointer-down time.
    pub fn hitbox_id(&self) -> HitboxId {
        self.hitbox_id
    }

    /// Returns the button that began this interaction.
    pub fn button(&self) -> MouseButton {
        self.button
    }

    /// Returns the pointer-down position.
    pub fn start(&self) -> Point<Pixels> {
        self.start
    }

    /// Returns the most recently observed pointer position.
    pub fn current(&self) -> Point<Pixels> {
        self.current
    }

    /// Returns the gesture-map revision retained for this sequence.
    pub fn gesture_revision(&self) -> GestureMapVersion {
        self.gesture_revision
    }

    /// Returns the current or terminal lifecycle phase.
    pub fn phase(&self) -> PointerInteractionPhase {
        self.phase
    }
}

/// Bounded pointer-interaction diagnostics for a window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointerInteractionDiagnostics {
    active: FxHashMap<PointerId, PointerInteractionOwner>,
    last_terminal: FxHashMap<PointerId, PointerInteractionOwner>,
    claim_conflict_count: u64,
}

impl PointerInteractionDiagnostics {
    /// Returns the most recently created active owner, if one exists.
    pub fn active(&self) -> Option<&PointerInteractionOwner> {
        self.active.values().max_by_key(|owner| owner.id().0)
    }

    /// Returns the active owner for one concrete pointer sequence.
    pub fn active_for(&self, pointer_id: PointerId) -> Option<&PointerInteractionOwner> {
        self.active.get(&pointer_id)
    }

    /// Returns the most recently completed or cancelled owner.
    pub fn last_terminal(&self) -> Option<&PointerInteractionOwner> {
        self.last_terminal.values().max_by_key(|owner| owner.id().0)
    }

    /// Returns the latest terminal owner for one concrete pointer sequence.
    pub fn last_terminal_for(&self, pointer_id: PointerId) -> Option<&PointerInteractionOwner> {
        self.last_terminal.get(&pointer_id)
    }

    /// Returns the saturating count of rejected attempts to steal an active sequence.
    pub fn claim_conflict_count(&self) -> u64 {
        self.claim_conflict_count
    }
}

/// Why a pointer-interaction claim was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerInteractionClaimError {
    /// Only a handled action outcome can claim a pointer sequence.
    ActionNotHandled,
    /// Only actions dispatched from mapped pointer input can claim a pointer sequence.
    NonPointerAction,
    /// The action report target does not match the initiating hitbox target.
    TargetMismatch,
    /// Another handled action already owns the pointer sequence.
    AlreadyOwned(PointerInteractionId),
}

struct PendingMappedPointer {
    pointer_id: PointerId,
    hitbox_id: HitboxId,
    target: DispatchNodeId,
    button: MouseButton,
    start: Point<Pixels>,
    gesture_revision: GestureMapVersion,
}

/// Holds the state for a specific window.
pub struct Window {
    pub(crate) handle: AnyWindowHandle,
    pub(crate) invalidator: WindowInvalidator,
    pub(crate) removed: bool,
    pub(crate) platform_window: Box<dyn PlatformWindow>,
    display_id: Option<DisplayId>,
    sprite_atlas: Arc<dyn PlatformAtlas>,
    text_system: Arc<WindowTextSystem>,
    text_rendering_mode: Rc<Cell<TextRenderingMode>>,
    rem_size: Pixels,
    /// The stack of override values for the window's rem size.
    ///
    /// This is used by `with_rem_size` to allow rendering an element tree with
    /// a given rem size.
    rem_size_override_stack: SmallVec<[Pixels; 8]>,
    pub(crate) viewport_size: Size<Pixels>,
    layout_engine: Option<TaffyLayoutEngine>,
    pub(crate) root: Option<AnyView>,
    pub(crate) element_id_stack: SmallVec<[ElementId; 32]>,
    pub(crate) text_style_stack: Vec<TextStyleRefinement>,
    pub(crate) rendered_entity_stack: Vec<EntityId>,
    pub(crate) element_offset_stack: Vec<Point<Pixels>>,
    pub(crate) element_opacity: f32,
    element_transform_stack: Vec<TransformationMatrix>,
    pub(crate) content_mask_stack: Vec<ContentMask<Pixels>>,
    scroll_container_context_stack: ScrollContainerContextStack,
    sticky_layout_context_stack: Vec<StickyLayoutContext>,
    keyed_sticky_candidates: Vec<KeyedStickyCandidate>,
    keyed_sticky_plans: Vec<KeyedStickyCandidatePlan>,
    next_sticky_layout_context_id: StickyLayoutContextId,
    paint_content_mask_stack: Vec<ContentMask<ScaledPixels>>,
    hitbox_behavior_overrides: Vec<HitboxBehavior>,
    pointer_semantics_suppressed: usize,
    pub(crate) requested_autoscroll: Option<Bounds<Pixels>>,
    pub(crate) image_cache_stack: Vec<AnyImageCache>,
    pub(crate) rendered_frame: WindowFrame,
    pub(crate) next_frame: WindowFrame,
    active_screen_effects: VecDeque<WindowActiveScreenEffect>,
    screen_effect_snapshots: VecDeque<WindowScreenEffectSnapshot>,
    next_screen_effect_id: u64,
    next_screen_effect_snapshot_id: u64,
    next_hitbox_id: HitboxId,
    pub(crate) next_tooltip_id: TooltipId,
    pub(crate) tooltip_bounds: Option<TooltipBounds>,
    next_frame_callbacks: Rc<RefCell<Vec<FrameCallback>>>,
    pub(crate) dirty_views: FxHashSet<EntityId>,
    action_dispatch_observers: SubscriberSet<(), AnyActionDispatchObserver>,
    next_action_invocation_id: u64,
    action_invocation_stack: Vec<ActionInvocation>,
    focus_listeners: SubscriberSet<(), AnyWindowFocusListener>,
    pub(crate) focus_lost_listeners: SubscriberSet<(), AnyObserver>,
    default_prevented: bool,
    mouse_position: Point<Pixels>,
    pressed_mouse_button: Option<MouseButton>,
    mouse_hit_test: HitTest,
    modifiers: Modifiers,
    capslock: Capslock,
    scale_factor: f32,
    pub(crate) bounds_observers: SubscriberSet<(), AnyObserver>,
    appearance: WindowAppearance,
    pub(crate) appearance_observers: SubscriberSet<(), AnyObserver>,
    pub(crate) button_layout_observers: SubscriberSet<(), AnyObserver>,
    active: Rc<Cell<bool>>,
    minimized: Rc<Cell<bool>>,
    hovered: Rc<Cell<bool>>,
    _thermal_state_subscription: Subscription,
    pub(crate) needs_present: Rc<Cell<bool>>,
    /// Tracks recent input event timestamps to determine if input is arriving at a high rate.
    /// Used to selectively enable VRR optimization only when input rate exceeds 60fps.
    pub(crate) input_rate_tracker: Rc<RefCell<InputRateTracker>>,
    #[cfg(feature = "input-latency-histogram")]
    input_latency_tracker: InputLatencyTracker,
    last_input_modality: InputModality,
    pub(crate) refreshing: bool,
    pub(crate) activation_observers: SubscriberSet<(), AnyObserver>,
    pub(crate) minimized_observers: SubscriberSet<(), AnyObserver>,
    pub(crate) focus: Option<FocusId>,
    focus_enabled: bool,
    pending_input: Option<PendingInput>,
    pending_modifier: ModifierState,
    pub(crate) pending_input_observers: SubscriberSet<(), AnyObserver>,
    prompt: Option<RenderablePromptHandle>,
    pub(crate) client_inset: Option<Pixels>,
    /// The hitbox that has captured the pointer, if any.
    /// While captured, mouse events route to this hitbox regardless of hit testing.
    captured_hitbox: Option<HitboxId>,
    captured_hitboxes: FxHashMap<PointerId, HitboxId>,
    active_drags: FxHashMap<PointerId, AnyDrag>,
    drag_feedback_composed_by_root: bool,
    next_pointer_interaction_id: u64,
    pointer_interaction_owners: FxHashMap<PointerId, PointerInteractionOwner>,
    pointer_interaction_diagnostics: PointerInteractionDiagnostics,
    pending_mapped_pointers: FxHashMap<PointerId, PendingMappedPointer>,
    gesture_recognizer: GestureRecognizer,
    recognized_gesture_event: Option<PointerEvent>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    inspector: Option<Entity<Inspector>>,
}

#[derive(Clone, Debug, Default)]
struct ModifierState {
    modifiers: Modifiers,
    saw_keystroke: bool,
}

/// Tracks input event timestamps to determine if input is arriving at a high rate.
/// Used for selective VRR (Variable Refresh Rate) optimization.
#[derive(Clone, Debug)]
pub(crate) struct InputRateTracker {
    timestamps: Vec<Instant>,
    window: Duration,
    inputs_per_second: u32,
    sustain_until: Instant,
    sustain_duration: Duration,
}

impl Default for InputRateTracker {
    fn default() -> Self {
        Self {
            timestamps: Vec::new(),
            window: Duration::from_millis(100),
            inputs_per_second: 60,
            sustain_until: Instant::now(),
            sustain_duration: Duration::from_secs(1),
        }
    }
}

impl InputRateTracker {
    pub fn record_input(&mut self) {
        let now = Instant::now();
        self.timestamps.push(now);
        self.prune_old_timestamps(now);

        let min_events = self.inputs_per_second as u128 * self.window.as_millis() / 1000;
        if self.timestamps.len() as u128 >= min_events {
            self.sustain_until = now + self.sustain_duration;
        }
    }

    pub fn is_high_rate(&self) -> bool {
        Instant::now() < self.sustain_until
    }

    fn prune_old_timestamps(&mut self, now: Instant) {
        self.timestamps
            .retain(|&t| now.duration_since(t) <= self.window);
    }
}

/// A point-in-time snapshot of the input-latency histograms for a window,
/// suitable for external formatting.
#[cfg(feature = "input-latency-histogram")]
pub struct InputLatencySnapshot {
    /// Histogram of input-to-frame latency samples, in nanoseconds.
    pub latency_histogram: Histogram<u64>,
    /// Histogram of input events coalesced per rendered frame.
    pub events_per_frame_histogram: Histogram<u64>,
    /// Count of input events that arrived mid-draw and were excluded from
    /// latency recording.
    pub mid_draw_events_dropped: u64,
}

/// Records the time between when the first input event in a frame is dispatched
/// and when the resulting frame is presented, capturing worst-case latency when
/// multiple events are coalesced into a single frame.
#[cfg(feature = "input-latency-histogram")]
struct InputLatencyTracker {
    /// Timestamp of the first unrendered input event in the current frame;
    /// cleared when a frame is presented.
    first_input_at: Option<Instant>,
    /// Count of input events received since the last frame was presented.
    pending_input_count: u64,
    /// Histogram of input-to-frame latency samples, in nanoseconds.
    latency_histogram: Histogram<u64>,
    /// Histogram of input events coalesced per rendered frame.
    events_per_frame_histogram: Histogram<u64>,
    /// Count of input events that arrived mid-draw and were excluded from
    /// latency recording because their effects won't appear until the next frame.
    mid_draw_events_dropped: u64,
}

#[cfg(feature = "input-latency-histogram")]
impl InputLatencyTracker {
    fn new() -> Result<Self> {
        Ok(Self {
            first_input_at: None,
            pending_input_count: 0,
            latency_histogram: Histogram::new(3)
                .map_err(|e| anyhow!("Failed to create input latency histogram: {e}"))?,
            events_per_frame_histogram: Histogram::new(3)
                .map_err(|e| anyhow!("Failed to create events per frame histogram: {e}"))?,
            mid_draw_events_dropped: 0,
        })
    }

    /// Record that an input event was dispatched at the given time.
    /// Only the first event's timestamp per frame is retained (worst-case latency).
    fn record_input(&mut self, dispatch_time: Instant) {
        self.first_input_at.get_or_insert(dispatch_time);
        self.pending_input_count += 1;
    }

    /// Record that an input event arrived during a draw phase and was excluded
    /// from latency tracking.
    fn record_mid_draw_input(&mut self) {
        self.mid_draw_events_dropped += 1;
    }

    /// Record that a frame was presented, flushing pending latency and coalescing samples.
    fn record_frame_presented(&mut self) {
        if let Some(first_input_at) = self.first_input_at.take() {
            let latency_nanos = first_input_at.elapsed().as_nanos() as u64;
            self.latency_histogram.record(latency_nanos).ok();
        }
        if self.pending_input_count > 0 {
            self.events_per_frame_histogram
                .record(self.pending_input_count)
                .ok();
            self.pending_input_count = 0;
        }
    }

    fn snapshot(&self) -> InputLatencySnapshot {
        InputLatencySnapshot {
            latency_histogram: self.latency_histogram.clone(),
            events_per_frame_histogram: self.events_per_frame_histogram.clone(),
            mid_draw_events_dropped: self.mid_draw_events_dropped,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DrawPhase {
    None,
    Prepaint,
    Paint,
    Focus,
}

#[derive(Default)]
struct PendingInput {
    keystrokes: SmallVec<[Keystroke; 1]>,
    keymap_snapshot: Option<Rc<KeymapSnapshot>>,
    focus: Option<FocusId>,
    timer: Option<Task<()>>,
    needs_timeout: bool,
}

pub(crate) struct ElementStateBox {
    pub(crate) inner: Box<dyn Any>,
    #[cfg(debug_assertions)]
    pub(crate) type_name: &'static str,
}

fn default_bounds(display_id: Option<DisplayId>, cx: &mut App) -> WindowBounds {
    // TODO, BUG: if you open a window with the currently active window
    // on the stack, this will erroneously fallback to `None`
    //
    // TODO these should be the initial window bounds not considering maximized/fullscreen
    let active_window_bounds = cx
        .active_window()
        .and_then(|w| w.update(cx, |_, window, _| window.window_bounds()).ok());

    const CASCADE_OFFSET: f32 = 25.0;

    let display = display_id
        .map(|id| cx.find_display(id))
        .unwrap_or_else(|| cx.primary_display());

    let default_placement = || Bounds::new(point(px(0.), px(0.)), DEFAULT_WINDOW_SIZE);

    // Use visible_bounds to exclude taskbar/dock areas
    let display_bounds = display
        .as_ref()
        .map(|d| d.visible_bounds())
        .unwrap_or_else(default_placement);

    let (
        Bounds {
            origin: base_origin,
            size: base_size,
        },
        window_bounds_ctor,
    ): (_, fn(Bounds<Pixels>) -> WindowBounds) = match active_window_bounds {
        Some(bounds) => match bounds {
            WindowBounds::Windowed(bounds) => (bounds, WindowBounds::Windowed),
            WindowBounds::Maximized(bounds) => (bounds, WindowBounds::Maximized),
            WindowBounds::Fullscreen(bounds) => (bounds, WindowBounds::Fullscreen),
        },
        None => (
            display
                .as_ref()
                .map(|d| d.default_bounds())
                .unwrap_or_else(default_placement),
            WindowBounds::Windowed,
        ),
    };

    let cascade_offset = point(px(CASCADE_OFFSET), px(CASCADE_OFFSET));
    let proposed_origin = base_origin + cascade_offset;
    let proposed_bounds = Bounds::new(proposed_origin, base_size);

    let display_right = display_bounds.origin.x + display_bounds.size.width;
    let display_bottom = display_bounds.origin.y + display_bounds.size.height;
    let window_right = proposed_bounds.origin.x + proposed_bounds.size.width;
    let window_bottom = proposed_bounds.origin.y + proposed_bounds.size.height;

    let fits_horizontally = window_right <= display_right;
    let fits_vertically = window_bottom <= display_bottom;

    let final_origin = match (fits_horizontally, fits_vertically) {
        (true, true) => proposed_origin,
        (false, true) => point(display_bounds.origin.x, base_origin.y),
        (true, false) => point(base_origin.x, display_bounds.origin.y),
        (false, false) => display_bounds.origin,
    };
    window_bounds_ctor(Bounds::new(final_origin, base_size))
}

impl Window {
    pub(crate) fn new(
        handle: AnyWindowHandle,
        options: WindowOptions,
        cx: &mut App,
    ) -> Result<Self> {
        let WindowOptions {
            window_bounds,
            titlebar,
            focus,
            show,
            kind,
            is_movable,
            is_resizable,
            is_minimizable,
            display_id,
            window_background,
            app_id,
            window_min_size,
            window_decorations,
            #[cfg_attr(
                not(any(target_os = "linux", target_os = "freebsd")),
                allow(unused_variables)
            )]
            icon,
            #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
            tabbing_identifier,
        } = options;

        let window_bounds = window_bounds.unwrap_or_else(|| default_bounds(display_id, cx));
        let mut platform_window = cx.platform.open_window(
            handle,
            WindowParams {
                bounds: window_bounds.get_bounds(),
                titlebar,
                kind,
                is_movable,
                is_resizable,
                is_minimizable,
                focus,
                show,
                display_id,
                window_min_size,
                icon,
                #[cfg(target_os = "macos")]
                tabbing_identifier,
            },
        )?;

        let tab_bar_visible = platform_window.tab_bar_visible();
        SystemWindowTabController::init_visible(cx, tab_bar_visible);
        if let Some(tabs) = platform_window.tabbed_windows() {
            SystemWindowTabController::add_tab(cx, handle.window_id(), tabs);
        }

        let display_id = platform_window.display().map(|display| display.id());
        let sprite_atlas = platform_window.sprite_atlas();
        let mouse_position = platform_window.mouse_position();
        let modifiers = platform_window.modifiers();
        let capslock = platform_window.capslock();
        let content_size = platform_window.content_size();
        let scale_factor = platform_window.scale_factor();
        let appearance = platform_window.appearance();
        let text_system = Arc::new(WindowTextSystem::new(cx.text_system().clone()));
        let invalidator = WindowInvalidator::new();
        let active = Rc::new(Cell::new(platform_window.is_active()));
        let minimized = Rc::new(Cell::new(false));
        let hovered = Rc::new(Cell::new(platform_window.is_hovered()));
        let needs_present = Rc::new(Cell::new(false));
        let next_frame_callbacks: Rc<RefCell<Vec<FrameCallback>>> = Default::default();
        let input_rate_tracker = Rc::new(RefCell::new(InputRateTracker::default()));
        let last_frame_time = Rc::new(Cell::new(None));
        let thermal_state = Rc::new(Cell::new(cx.thermal_state()));
        let thermal_state_subscription = cx.on_thermal_state_change({
            let thermal_state = Rc::clone(&thermal_state);
            move |cx| thermal_state.set(cx.thermal_state())
        });

        platform_window
            .request_decorations(window_decorations.unwrap_or(WindowDecorations::Server));
        platform_window.set_background_appearance(window_background);

        match window_bounds {
            WindowBounds::Fullscreen(_) => platform_window.toggle_fullscreen(),
            WindowBounds::Maximized(_) => platform_window.zoom(),
            WindowBounds::Windowed(_) => {}
        }

        platform_window.on_close(Box::new({
            let window_id = handle.window_id();
            let mut cx = cx.to_async();
            move || {
                let _ = handle.update(&mut cx, |_, window, _| window.remove_window());
                let _ = cx.update(|cx| {
                    SystemWindowTabController::remove_tab(cx, window_id);
                });
            }
        }));
        platform_window.on_request_frame(Box::new({
            let mut cx = cx.to_async();
            let invalidator = invalidator.clone();
            let active = active.clone();
            let needs_present = needs_present.clone();
            let next_frame_callbacks = next_frame_callbacks.clone();
            let input_rate_tracker = input_rate_tracker.clone();
            move |request_frame_options| {
                // Throttle frame rate based on conditions:
                // - Thermal pressure (Serious/Critical): cap to ~60fps
                // - Inactive window (not focused): cap to ~30fps to save energy
                let min_frame_interval = if !request_frame_options.force_render
                    && !request_frame_options.require_presentation
                    && next_frame_callbacks.borrow().is_empty()
                {
                    None
                } else if !active.get() {
                    Some(Duration::from_micros(33333))
                } else if matches!(
                    thermal_state.get(),
                    ThermalState::Critical | ThermalState::Serious
                ) {
                    Some(Duration::from_micros(16667))
                } else {
                    None
                };

                let now = Instant::now();
                if let Some(min_interval) = min_frame_interval {
                    if let Some(last_frame) = last_frame_time.get()
                        && now.duration_since(last_frame) < min_interval
                    {
                        // Must still complete the frame on platforms that require it.
                        // On Wayland, `surface.frame()` was already called to request the
                        // next frame callback, so we must call `surface.commit()` (via
                        // `complete_frame`) or the compositor won't send another callback.
                        handle
                            .update(&mut cx, |_, window, _| window.complete_frame())
                            .log_err();
                        return;
                    }
                }
                last_frame_time.set(Some(now));

                let next_frame_callbacks = next_frame_callbacks.take();
                if !next_frame_callbacks.is_empty() {
                    handle
                        .update(&mut cx, |_, window, cx| {
                            for callback in next_frame_callbacks {
                                callback(window, cx);
                            }
                        })
                        .log_err();
                }

                // Keep presenting if input was recently arriving at a high rate (>= 60fps).
                // Once high-rate input is detected, we sustain presentation for 1 second
                // to prevent display underclocking during active input.
                let needs_present = request_frame_options.require_presentation
                    || needs_present.get()
                    || (active.get() && input_rate_tracker.borrow_mut().is_high_rate());

                if invalidator.is_dirty() || request_frame_options.force_render {
                    measure("frame duration", || {
                        handle
                            .update(&mut cx, |_, window, cx| {
                                if request_frame_options.force_render {
                                    // Bypass cached view reuse so we don't replay stale
                                    // atlas tile references after a GPU device recovery.
                                    window.refresh();
                                }
                                let arena_clear_needed = window.draw(cx);
                                window.present();
                                arena_clear_needed.clear();
                            })
                            .log_err();
                    })
                } else if needs_present {
                    handle
                        .update(&mut cx, |_, window, _| window.present())
                        .log_err();
                }

                handle
                    .update(&mut cx, |_, window, _| {
                        window.complete_frame();
                    })
                    .log_err();
            }
        }));
        platform_window.on_resize(Box::new({
            let mut cx = cx.to_async();
            move |_, _| {
                handle
                    .update(&mut cx, |_, window, cx| window.bounds_changed(cx))
                    .log_err();
            }
        }));
        platform_window.on_moved(Box::new({
            let mut cx = cx.to_async();
            move || {
                handle
                    .update(&mut cx, |_, window, cx| window.bounds_changed(cx))
                    .log_err();
            }
        }));
        platform_window.on_appearance_changed(Box::new({
            let mut cx = cx.to_async();
            move || {
                handle
                    .update(&mut cx, |_, window, cx| window.appearance_changed(cx))
                    .log_err();
            }
        }));
        platform_window.on_button_layout_changed(Box::new({
            let mut cx = cx.to_async();
            move || {
                handle
                    .update(&mut cx, |_, window, cx| window.button_layout_changed(cx))
                    .log_err();
            }
        }));
        platform_window.on_active_status_change(Box::new({
            let mut cx = cx.to_async();
            move |active| {
                handle
                    .update(&mut cx, |_, window, cx| {
                        window.active.set(active);
                        window.modifiers = window.platform_window.modifiers();
                        window.capslock = window.platform_window.capslock();
                        window
                            .activation_observers
                            .clone()
                            .retain(&(), |callback| callback(window, cx));

                        window.bounds_changed(cx);
                        window.refresh();

                        SystemWindowTabController::update_last_active(cx, window.handle.id);
                    })
                    .log_err();
            }
        }));
        platform_window.on_minimized_status_change(Box::new({
            let mut cx = cx.to_async();
            move |minimized| {
                handle
                    .update(&mut cx, |_, window, cx| {
                        window.minimized.set(minimized);
                        window
                            .minimized_observers
                            .clone()
                            .retain(&(), |callback| callback(window, cx));

                        window.bounds_changed(cx);
                        window.refresh();
                    })
                    .log_err();
            }
        }));
        platform_window.on_hover_status_change(Box::new({
            let mut cx = cx.to_async();
            move |active| {
                handle
                    .update(&mut cx, |_, window, _| {
                        window.hovered.set(active);
                        window.refresh();
                    })
                    .log_err();
            }
        }));
        platform_window.on_input({
            let mut cx = cx.to_async();
            Box::new(move |event| {
                handle
                    .update(&mut cx, |_, window, cx| window.dispatch_event(event, cx))
                    .log_err()
                    .unwrap_or(DispatchEventResult::default())
            })
        });
        platform_window.on_hit_test_window_control({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, window, _cx| {
                        for (area, hitbox) in &window.rendered_frame.window_control_hitboxes {
                            if window.mouse_hit_test.ids.contains(&hitbox.id) {
                                return Some(*area);
                            }
                        }
                        None
                    })
                    .log_err()
                    .unwrap_or(None)
            })
        });
        platform_window.on_move_tab_to_new_window({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, _window, cx| {
                        SystemWindowTabController::move_tab_to_new_window(cx, handle.window_id());
                    })
                    .log_err();
            })
        });
        platform_window.on_merge_all_windows({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, _window, cx| {
                        SystemWindowTabController::merge_all_windows(cx, handle.window_id());
                    })
                    .log_err();
            })
        });
        platform_window.on_select_next_tab({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, _window, cx| {
                        SystemWindowTabController::select_next_tab(cx, handle.window_id());
                    })
                    .log_err();
            })
        });
        platform_window.on_select_previous_tab({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, _window, cx| {
                        SystemWindowTabController::select_previous_tab(cx, handle.window_id())
                    })
                    .log_err();
            })
        });
        platform_window.on_toggle_tab_bar({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, window, cx| {
                        let tab_bar_visible = window.platform_window.tab_bar_visible();
                        SystemWindowTabController::set_visible(cx, tab_bar_visible);
                    })
                    .log_err();
            })
        });

        if let Some(app_id) = app_id {
            platform_window.set_app_id(&app_id);
        }

        platform_window.map_window().unwrap();

        Ok(Window {
            handle,
            invalidator,
            removed: false,
            platform_window,
            display_id,
            sprite_atlas,
            text_system,
            text_rendering_mode: cx.text_rendering_mode.clone(),
            rem_size: px(16.),
            rem_size_override_stack: SmallVec::new(),
            viewport_size: content_size,
            layout_engine: Some(TaffyLayoutEngine::new()),
            root: None,
            element_id_stack: SmallVec::default(),
            text_style_stack: Vec::new(),
            rendered_entity_stack: Vec::new(),
            element_offset_stack: Vec::new(),
            content_mask_stack: Vec::new(),
            scroll_container_context_stack: ScrollContainerContextStack::default(),
            sticky_layout_context_stack: Vec::new(),
            keyed_sticky_candidates: Vec::new(),
            keyed_sticky_plans: Vec::new(),
            next_sticky_layout_context_id: 0,
            element_opacity: 1.0,
            element_transform_stack: Vec::new(),
            paint_content_mask_stack: Vec::new(),
            hitbox_behavior_overrides: Vec::new(),
            pointer_semantics_suppressed: 0,
            requested_autoscroll: None,
            rendered_frame: WindowFrame::new(DispatchTree::new(
                cx.keymap.clone(),
                cx.actions.clone(),
            )),
            next_frame: WindowFrame::new(DispatchTree::new(cx.keymap.clone(), cx.actions.clone())),
            active_screen_effects: VecDeque::new(),
            screen_effect_snapshots: VecDeque::new(),
            next_screen_effect_id: 1,
            next_screen_effect_snapshot_id: 1,
            next_frame_callbacks,
            next_hitbox_id: HitboxId(0),
            next_tooltip_id: TooltipId::default(),
            tooltip_bounds: None,
            dirty_views: FxHashSet::default(),
            action_dispatch_observers: SubscriberSet::new(),
            next_action_invocation_id: 0,
            action_invocation_stack: Vec::new(),
            focus_listeners: SubscriberSet::new(),
            focus_lost_listeners: SubscriberSet::new(),
            default_prevented: true,
            mouse_position,
            pressed_mouse_button: None,
            mouse_hit_test: HitTest::default(),
            modifiers,
            capslock,
            scale_factor,
            bounds_observers: SubscriberSet::new(),
            appearance,
            appearance_observers: SubscriberSet::new(),
            button_layout_observers: SubscriberSet::new(),
            active,
            minimized,
            hovered,
            _thermal_state_subscription: thermal_state_subscription,
            needs_present,
            input_rate_tracker,
            #[cfg(feature = "input-latency-histogram")]
            input_latency_tracker: InputLatencyTracker::new()?,
            last_input_modality: InputModality::Mouse,
            refreshing: false,
            activation_observers: SubscriberSet::new(),
            minimized_observers: SubscriberSet::new(),
            focus: None,
            focus_enabled: true,
            pending_input: None,
            pending_modifier: ModifierState::default(),
            pending_input_observers: SubscriberSet::new(),
            prompt: None,
            client_inset: None,
            image_cache_stack: Vec::new(),
            captured_hitbox: None,
            captured_hitboxes: FxHashMap::default(),
            active_drags: FxHashMap::default(),
            drag_feedback_composed_by_root: false,
            next_pointer_interaction_id: 0,
            pointer_interaction_owners: FxHashMap::default(),
            pointer_interaction_diagnostics: PointerInteractionDiagnostics::default(),
            pending_mapped_pointers: FxHashMap::default(),
            gesture_recognizer: GestureRecognizer::default(),
            recognized_gesture_event: None,
            #[cfg(any(feature = "inspector", debug_assertions))]
            inspector: None,
        })
    }

    pub(crate) fn new_focus_listener(
        &self,
        value: AnyWindowFocusListener,
    ) -> (Subscription, impl FnOnce() + use<>) {
        self.focus_listeners.insert((), value)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[expect(missing_docs)]
pub struct DispatchEventResult {
    pub propagate: bool,
    pub default_prevented: bool,
}

/// Indicates which region of the window is visible. Content falling outside of this mask will not be
/// rendered. Currently, only rectangular content masks are supported, but we give the mask its own type
/// to leave room to support more complex shapes in the future.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct ContentMask<P: Clone + Debug + Default + PartialEq> {
    /// The bounds
    pub bounds: Bounds<P>,
    /// The corner radii of the clipping shape.
    pub corner_radii: Corners<P>,
}

impl<P> ContentMask<P>
where
    P: Clone + Debug + Default + PartialEq,
{
    /// Creates a rectangular content mask.
    pub fn rectangular(bounds: Bounds<P>) -> Self {
        Self {
            bounds,
            corner_radii: Corners::default(),
        }
    }

    /// Creates a content mask with explicit corner radii.
    pub fn rounded(bounds: Bounds<P>, corner_radii: Corners<P>) -> Self {
        Self {
            bounds,
            corner_radii,
        }
    }
}

impl<P> ContentMask<P>
where
    P: Ord
        + std::ops::Add<P, Output = P>
        + std::ops::Sub<Output = P>
        + Clone
        + Debug
        + Default
        + PartialEq,
{
    fn intersected_corner_radius(
        corner: Point<P>,
        a_corner: Point<P>,
        a_radius: P,
        b_corner: Point<P>,
        b_radius: P,
    ) -> P {
        let mut radius = P::default();
        if corner == a_corner {
            radius = radius.max(a_radius);
        }
        if corner == b_corner {
            radius = radius.max(b_radius);
        }
        radius
    }

    /// Intersect the content mask with the given content mask.
    pub fn intersect(&self, other: &Self) -> Self {
        let bounds = self.bounds.intersect(&other.bounds);
        let corner_radii = Corners {
            top_left: Self::intersected_corner_radius(
                bounds.origin.clone(),
                self.bounds.origin.clone(),
                self.corner_radii.top_left.clone(),
                other.bounds.origin.clone(),
                other.corner_radii.top_left.clone(),
            ),
            top_right: Self::intersected_corner_radius(
                bounds.top_right(),
                self.bounds.top_right(),
                self.corner_radii.top_right.clone(),
                other.bounds.top_right(),
                other.corner_radii.top_right.clone(),
            ),
            bottom_right: Self::intersected_corner_radius(
                bounds.bottom_right(),
                self.bounds.bottom_right(),
                self.corner_radii.bottom_right.clone(),
                other.bounds.bottom_right(),
                other.corner_radii.bottom_right.clone(),
            ),
            bottom_left: Self::intersected_corner_radius(
                bounds.bottom_left(),
                self.bounds.bottom_left(),
                self.corner_radii.bottom_left.clone(),
                other.bounds.bottom_left(),
                other.corner_radii.bottom_left.clone(),
            ),
        };
        ContentMask {
            bounds,
            corner_radii,
        }
    }
}

impl ContentMask<Pixels> {
    /// Scale the content mask's pixel units by the given scaling factor.
    pub fn scale(&self, factor: f32) -> ContentMask<ScaledPixels> {
        ContentMask {
            bounds: self.bounds.scale(factor),
            corner_radii: self.corner_radii.scale(factor),
        }
    }
}

impl Window {
    fn mark_view_dirty(&mut self, view_id: EntityId) {
        // Mark ancestor views as dirty. If already in the `dirty_views` set, then all its ancestors
        // should already be dirty.
        for view_id in self
            .rendered_frame
            .dispatch_tree
            .view_path_reversed(view_id)
        {
            if !self.dirty_views.insert(view_id) {
                break;
            }
        }
    }

    /// Registers a callback to be invoked when the window appearance changes.
    pub fn observe_window_appearance(
        &self,
        mut callback: impl FnMut(&mut Window, &mut App) + 'static,
    ) -> Subscription {
        let (subscription, activate) = self.appearance_observers.insert(
            (),
            Box::new(move |window, cx| {
                callback(window, cx);
                true
            }),
        );
        activate();
        subscription
    }

    /// Registers a callback to be invoked when the window button layout changes.
    pub fn observe_button_layout_changed(
        &self,
        mut callback: impl FnMut(&mut Window, &mut App) + 'static,
    ) -> Subscription {
        let (subscription, activate) = self.button_layout_observers.insert(
            (),
            Box::new(move |window, cx| {
                callback(window, cx);
                true
            }),
        );
        activate();
        subscription
    }

    /// Replaces the root entity of the window with a new one.
    pub fn replace_root<E>(
        &mut self,
        cx: &mut App,
        build_view: impl FnOnce(&mut Window, &mut Context<E>) -> E,
    ) -> Entity<E>
    where
        E: 'static + Render,
    {
        let view = cx.new(|cx| build_view(self, cx));
        self.root = Some(view.clone().into());
        self.refresh();
        view
    }

    /// Returns the root entity of the window, if it has one.
    pub fn root<E>(&self) -> Option<Option<Entity<E>>>
    where
        E: 'static + Render,
    {
        self.root
            .as_ref()
            .map(|view| view.clone().downcast::<E>().ok())
    }

    /// Obtain a handle to the window that belongs to this context.
    pub fn window_handle(&self) -> AnyWindowHandle {
        self.handle
    }

    /// Mark the window as dirty, scheduling it to be redrawn on the next frame.
    pub fn refresh(&mut self) {
        if self.invalidator.not_drawing() {
            self.refreshing = true;
            self.invalidator.set_dirty(true);
        }
    }

    /// Close this window.
    pub fn remove_window(&mut self) {
        self.cancel_all_pointer_interactions(PointerInteractionCancellationReason::WindowRemoved);
        self.pending_mapped_pointers.clear();
        self.gesture_recognizer.cancel();
        self.removed = true;
    }

    /// Obtain the currently focused [`FocusHandle`]. If no elements are focused, returns `None`.
    pub fn focused(&self, cx: &App) -> Option<FocusHandle> {
        self.focus
            .and_then(|id| FocusHandle::for_id(id, &cx.focus_handles))
    }

    /// Move focus to the element associated with the given [`FocusHandle`].
    pub fn focus(&mut self, handle: &FocusHandle, cx: &mut App) {
        if !self.focus_enabled || self.focus == Some(handle.id) {
            return;
        }

        self.focus = Some(handle.id);
        self.clear_pending_keystrokes();

        // Avoid re-entrant entity updates by deferring observer notifications to the end of the
        // current effect cycle, and only for this window.
        let window_handle = self.handle;
        cx.defer(move |cx| {
            window_handle
                .update(cx, |_, window, cx| {
                    window.pending_input_changed(cx);
                })
                .ok();
        });

        self.refresh();
    }

    /// Remove focus from all elements within this context's window.
    pub fn blur(&mut self) {
        if !self.focus_enabled {
            return;
        }

        self.focus = None;
        self.refresh();
    }

    /// Blur the window and don't allow anything in it to be focused again.
    pub fn disable_focus(&mut self) {
        self.blur();
        self.focus_enabled = false;
    }

    /// Move focus to next tab stop.
    pub fn focus_next(&mut self, cx: &mut App) {
        if !self.focus_enabled {
            return;
        }

        if let Some(handle) = self.rendered_frame.tab_stops.next(self.focus.as_ref()) {
            self.focus(&handle, cx)
        }
    }

    /// Move focus to previous tab stop.
    pub fn focus_prev(&mut self, cx: &mut App) {
        if !self.focus_enabled {
            return;
        }

        if let Some(handle) = self.rendered_frame.tab_stops.prev(self.focus.as_ref()) {
            self.focus(&handle, cx)
        }
    }

    /// Accessor for the text system.
    pub fn text_system(&self) -> &Arc<WindowTextSystem> {
        &self.text_system
    }

    /// The current text style. Which is composed of all the style refinements provided to `with_text_style`.
    pub fn text_style(&self) -> TextStyle {
        let mut style = TextStyle::default();
        for refinement in &self.text_style_stack {
            style.refine(refinement);
        }
        style
    }

    /// Check if the platform window is maximized.
    ///
    /// On some platforms (namely Windows) this is different than the bounds being the size of the display
    pub fn is_maximized(&self) -> bool {
        self.platform_window.is_maximized()
    }

    /// request a certain window decoration (Wayland)
    pub fn request_decorations(&self, decorations: WindowDecorations) {
        self.platform_window.request_decorations(decorations);
    }

    /// Start a window resize operation (Wayland)
    pub fn start_window_resize(&self, edge: ResizeEdge) {
        self.platform_window.start_window_resize(edge);
    }

    /// Return the `WindowBounds` to indicate that how a window should be opened
    /// after it has been closed
    pub fn window_bounds(&self) -> WindowBounds {
        self.platform_window.window_bounds()
    }

    /// Return the `WindowBounds` excluding insets (Wayland and X11)
    pub fn inner_window_bounds(&self) -> WindowBounds {
        self.platform_window.inner_window_bounds()
    }

    /// Dispatch the given action on the currently focused element.
    pub fn dispatch_action(
        &mut self,
        action: Box<dyn Action>,
        cx: &mut App,
    ) -> ActionDispatchReport {
        self.dispatch_action_from(ActionInvocationSource::Programmatic, action.as_ref(), cx)
    }

    /// Dispatch an action with explicit invocation metadata.
    pub fn dispatch_action_from(
        &mut self,
        source: ActionInvocationSource,
        action: &dyn Action,
        cx: &mut App,
    ) -> ActionDispatchReport {
        let focus_id = self.focused(cx).map(|handle| handle.id);
        let node_id = self.focus_node_id_in_rendered_frame(focus_id);
        self.dispatch_action_on_node_from(node_id, source, action, cx)
    }

    /// Observe the begin and terminal event for every action dispatched in this window.
    pub fn observe_action_dispatch(
        &mut self,
        mut observer: impl FnMut(&ActionDispatchEvent, &mut Window, &mut App) + 'static,
    ) -> Subscription {
        let (subscription, activate) = self.action_dispatch_observers.insert(
            (),
            Box::new(move |event, window, cx| {
                observer(event, window, cx);
                true
            }),
        );
        activate();
        subscription
    }

    pub(crate) fn dispatch_keystroke_observers(
        &mut self,
        event: &dyn Any,
        action: Option<Box<dyn Action>>,
        context_stack: Vec<KeyContext>,
        cx: &mut App,
    ) {
        let Some(key_down_event) = event.downcast_ref::<KeyDownEvent>() else {
            return;
        };

        cx.keystroke_observers.clone().retain(&(), move |callback| {
            (callback)(
                &KeystrokeEvent {
                    keystroke: key_down_event.keystroke.clone(),
                    action: action.as_ref().map(|action| action.boxed_clone()),
                    context_stack: context_stack.clone(),
                },
                self,
                cx,
            )
        });
    }

    pub(crate) fn dispatch_keystroke_interceptors(
        &mut self,
        event: &dyn Any,
        context_stack: Vec<KeyContext>,
        cx: &mut App,
    ) {
        let Some(key_down_event) = event.downcast_ref::<KeyDownEvent>() else {
            return;
        };

        cx.keystroke_interceptors
            .clone()
            .retain(&(), move |callback| {
                (callback)(
                    &KeystrokeEvent {
                        keystroke: key_down_event.keystroke.clone(),
                        action: None,
                        context_stack: context_stack.clone(),
                    },
                    self,
                    cx,
                )
            });
    }

    /// Schedules the given function to be run at the end of the current effect cycle, allowing entities
    /// that are currently on the stack to be returned to the app.
    pub fn defer(&self, cx: &mut App, f: impl FnOnce(&mut Window, &mut App) + 'static) {
        let handle = self.handle;
        cx.defer(move |cx| {
            handle.update(cx, |_, window, cx| f(window, cx)).ok();
        });
    }

    /// Subscribe to events emitted by a entity.
    /// The entity to which you're subscribing must implement the [`EventEmitter`] trait.
    /// The callback will be invoked a handle to the emitting entity, the event, and a window context for the current window.
    pub fn observe<T: 'static>(
        &mut self,
        observed: &Entity<T>,
        cx: &mut App,
        mut on_notify: impl FnMut(Entity<T>, &mut Window, &mut App) + 'static,
    ) -> Subscription {
        let entity_id = observed.entity_id();
        let observed = observed.downgrade();
        let window_handle = self.handle;
        cx.new_observer(
            entity_id,
            Box::new(move |cx| {
                window_handle
                    .update(cx, |_, window, cx| {
                        if let Some(handle) = observed.upgrade() {
                            on_notify(handle, window, cx);
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false)
            }),
        )
    }

    /// Subscribe to events emitted by a entity.
    /// The entity to which you're subscribing must implement the [`EventEmitter`] trait.
    /// The callback will be invoked a handle to the emitting entity, the event, and a window context for the current window.
    pub fn subscribe<Emitter, Evt>(
        &mut self,
        entity: &Entity<Emitter>,
        cx: &mut App,
        mut on_event: impl FnMut(Entity<Emitter>, &Evt, &mut Window, &mut App) + 'static,
    ) -> Subscription
    where
        Emitter: EventEmitter<Evt>,
        Evt: 'static,
    {
        let entity_id = entity.entity_id();
        let handle = entity.downgrade();
        let window_handle = self.handle;
        cx.new_subscription(
            entity_id,
            (
                TypeId::of::<Evt>(),
                Box::new(move |event, cx| {
                    window_handle
                        .update(cx, |_, window, cx| {
                            if let Some(entity) = handle.upgrade() {
                                let event = event.downcast_ref().expect("invalid event type");
                                on_event(entity, event, window, cx);
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false)
                }),
            ),
        )
    }

    /// Register a callback to be invoked when the given `Entity` is released.
    pub fn observe_release<T>(
        &self,
        entity: &Entity<T>,
        cx: &mut App,
        mut on_release: impl FnOnce(&mut T, &mut Window, &mut App) + 'static,
    ) -> Subscription
    where
        T: 'static,
    {
        let entity_id = entity.entity_id();
        let window_handle = self.handle;
        let (subscription, activate) = cx.release_listeners.insert(
            entity_id,
            Box::new(move |entity, cx| {
                let entity = entity.downcast_mut().expect("invalid entity type");
                let _ = window_handle.update(cx, |_, window, cx| on_release(entity, window, cx));
            }),
        );
        activate();
        subscription
    }

    /// Creates an [`AsyncWindowContext`], which has a static lifetime and can be held across
    /// await points in async code.
    pub fn to_async(&self, cx: &App) -> AsyncWindowContext {
        AsyncWindowContext::new_context(cx.to_async(), self.handle)
    }

    /// Schedule the given closure to be run directly after the current frame is rendered.
    pub fn on_next_frame(&self, callback: impl FnOnce(&mut Window, &mut App) + 'static) {
        RefCell::borrow_mut(&self.next_frame_callbacks).push(Box::new(callback));
    }

    /// Schedule a frame to be drawn on the next animation frame.
    ///
    /// This is useful for elements that need to animate continuously, such as a video player or an animated GIF.
    /// It will cause the window to redraw on the next frame, even if no other changes have occurred.
    ///
    /// If called from within a view, it will notify that view on the next frame. Otherwise, it will refresh the entire window.
    pub fn request_animation_frame(&self) {
        let entity = self.current_view();
        self.on_next_frame(move |_, cx| cx.notify(entity));
    }

    /// Start a screen-level shader effect in this window.
    ///
    /// The oldest effects are evicted first when the per-window cap is reached.
    pub fn play_screen_effect(&mut self, effect: impl Into<ScreenEffect>) -> ScreenEffectId {
        self.play_screen_effect_at(effect, Instant::now())
    }

    fn play_screen_effect_at(
        &mut self,
        effect: impl Into<ScreenEffect>,
        started_at: Instant,
    ) -> ScreenEffectId {
        let id = ScreenEffectId::new(self.next_screen_effect_id);
        self.next_screen_effect_id = self.next_screen_effect_id.saturating_add(1);

        self.active_screen_effects
            .push_back(WindowActiveScreenEffect {
                id,
                effect: effect.into(),
                started_at,
            });

        while self.active_screen_effects.len() > MAX_ACTIVE_SCREEN_EFFECTS {
            self.active_screen_effects.pop_front();
        }

        self.invalidator.set_dirty(true);
        id
    }

    /// Retain a renderer-facing snapshot request for a window or region target.
    ///
    /// The renderer may use the most recently rendered scene color for the
    /// requested target when a snapshot transition references this id.
    pub fn capture_screen_effect_snapshot(
        &mut self,
        target: impl Into<ScreenEffectTarget>,
    ) -> ScreenEffectSnapshotId {
        let id = ScreenEffectSnapshotId::new(self.next_screen_effect_snapshot_id);
        self.next_screen_effect_snapshot_id = self.next_screen_effect_snapshot_id.saturating_add(1);

        self.screen_effect_snapshots
            .push_back(WindowScreenEffectSnapshot {
                id,
                target: target.into(),
            });

        while self.screen_effect_snapshots.len() > MAX_SCREEN_EFFECT_SNAPSHOTS {
            self.screen_effect_snapshots.pop_front();
        }

        self.invalidator.set_dirty(true);
        id
    }

    /// Release a retained screen-effect snapshot request.
    pub fn release_screen_effect_snapshot(&mut self, id: ScreenEffectSnapshotId) -> bool {
        let previous_len = self.screen_effect_snapshots.len();
        self.screen_effect_snapshots
            .retain(|snapshot| snapshot.id != id);
        let released = self.screen_effect_snapshots.len() != previous_len;
        if released {
            self.invalidator.set_dirty(true);
        }
        released
    }

    /// Returns the number of retained screen-effect snapshots.
    pub fn screen_effect_snapshot_count(&self) -> usize {
        self.screen_effect_snapshots.len()
    }

    /// Prune expired screen effects and return whether any active effects remain.
    pub fn tick_screen_effects(&mut self) -> bool {
        self.tick_screen_effects_at(Instant::now())
    }

    fn tick_screen_effects_at(&mut self, now: Instant) -> bool {
        let previous_len = self.active_screen_effects.len();
        self.prune_expired_screen_effects(now);
        if self.active_screen_effects.len() != previous_len {
            self.invalidator.set_dirty(true);
        }
        !self.active_screen_effects.is_empty()
    }

    /// Returns the number of currently retained active screen effects.
    pub fn active_screen_effect_count(&self) -> usize {
        self.active_screen_effects.len()
    }

    /// Returns active screen effects as renderer-facing snapshots.
    pub fn active_screen_effects(&self) -> Vec<ActiveScreenEffect> {
        self.active_screen_effect_snapshots(Instant::now())
    }

    fn prune_expired_screen_effects(&mut self, now: Instant) {
        self.active_screen_effects
            .retain(|effect| !effect.is_expired(now));
    }

    fn active_screen_effect_snapshots(&self, now: Instant) -> Vec<ActiveScreenEffect> {
        self.active_screen_effects
            .iter()
            .map(|effect| effect.snapshot(now))
            .collect()
    }

    fn active_screen_effect_snapshot_requests(&self) -> Vec<ActiveScreenEffectSnapshot> {
        self.screen_effect_snapshots
            .iter()
            .map(WindowScreenEffectSnapshot::snapshot)
            .collect()
    }

    /// Spawn the future returned by the given closure on the application thread pool.
    /// The closure is provided a handle to the current window and an `AsyncWindowContext` for
    /// use within your future.
    #[track_caller]
    pub fn spawn<AsyncFn, R>(&self, cx: &App, f: AsyncFn) -> Task<R>
    where
        R: 'static,
        AsyncFn: AsyncFnOnce(&mut AsyncWindowContext) -> R + 'static,
    {
        let handle = self.handle;
        cx.spawn(async move |app| {
            let mut async_window_cx = AsyncWindowContext::new_context(app.clone(), handle);
            f(&mut async_window_cx).await
        })
    }

    /// Spawn the future returned by the given closure on the application thread
    /// pool, with the given priority. The closure is provided a handle to the
    /// current window and an `AsyncWindowContext` for use within your future.
    #[track_caller]
    pub fn spawn_with_priority<AsyncFn, R>(
        &self,
        priority: Priority,
        cx: &App,
        f: AsyncFn,
    ) -> Task<R>
    where
        R: 'static,
        AsyncFn: AsyncFnOnce(&mut AsyncWindowContext) -> R + 'static,
    {
        let handle = self.handle;
        cx.spawn_with_priority(priority, async move |app| {
            let mut async_window_cx = AsyncWindowContext::new_context(app.clone(), handle);
            f(&mut async_window_cx).await
        })
    }

    /// Notify the window that its bounds have changed.
    ///
    /// This updates internal state like `viewport_size` and `scale_factor` from
    /// the platform window, then notifies observers. Normally called automatically
    /// by the platform's resize callback, but exposed publicly for test infrastructure.
    pub fn bounds_changed(&mut self, cx: &mut App) {
        self.scale_factor = self.platform_window.scale_factor();
        self.viewport_size = self.platform_window.content_size();
        self.display_id = self.platform_window.display().map(|display| display.id());

        self.refresh();

        self.bounds_observers
            .clone()
            .retain(&(), |callback| callback(self, cx));
    }

    /// Returns the bounds of the current window in the global coordinate space, which could span across multiple displays.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.platform_window.bounds()
    }

    /// Renders the current frame's scene to a texture and returns the pixel data as an RGBA image.
    /// This does not present the frame to screen - useful for visual testing where we want
    /// to capture what would be rendered without displaying it or requiring the window to be visible.
    pub fn render_to_image(&self) -> anyhow::Result<image::RgbaImage> {
        self.platform_window
            .render_to_image(&self.rendered_frame.scene)
    }

    /// Set the content size of the window.
    pub fn resize(&mut self, size: Size<Pixels>) {
        self.platform_window.resize(size);
    }

    /// Returns whether or not the window is currently fullscreen
    pub fn is_fullscreen(&self) -> bool {
        self.platform_window.is_fullscreen()
    }

    pub(crate) fn appearance_changed(&mut self, cx: &mut App) {
        self.appearance = self.platform_window.appearance();

        self.appearance_observers
            .clone()
            .retain(&(), |callback| callback(self, cx));
    }

    pub(crate) fn button_layout_changed(&mut self, cx: &mut App) {
        self.button_layout_observers
            .clone()
            .retain(&(), |callback| callback(self, cx));
    }

    /// Returns the appearance of the current window.
    pub fn appearance(&self) -> WindowAppearance {
        self.appearance
    }

    /// Returns the size of the drawable area within the window.
    pub fn viewport_size(&self) -> Size<Pixels> {
        self.viewport_size
    }

    /// Returns whether this window is focused by the operating system (receiving key events).
    pub fn is_window_active(&self) -> bool {
        self.active.get()
    }

    /// Returns whether this window is minimized by the operating system.
    pub fn is_window_minimized(&self) -> bool {
        self.minimized.get()
    }

    /// Returns whether this window is considered to be the window
    /// that currently owns the mouse cursor.
    /// On mac, this is equivalent to `is_window_active`.
    pub fn is_window_hovered(&self) -> bool {
        if cfg!(any(
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )) {
            self.hovered.get()
        } else {
            self.is_window_active()
        }
    }

    /// Toggle zoom on the window.
    pub fn zoom_window(&self) {
        self.platform_window.zoom();
    }

    /// Opens the native title bar context menu, useful when implementing client side decorations (Wayland and X11)
    pub fn show_window_menu(&self, position: Point<Pixels>) {
        self.platform_window.show_window_menu(position)
    }

    /// Handle window movement for Linux and macOS.
    /// Tells the compositor to take control of window movement (Wayland and X11)
    ///
    /// Events may not be received during a move operation.
    pub fn start_window_move(&self) {
        self.platform_window.start_window_move()
    }

    /// When using client side decorations, set this to the width of the invisible decorations (Wayland and X11)
    pub fn set_client_inset(&mut self, inset: Pixels) {
        self.client_inset = Some(inset);
        self.platform_window.set_client_inset(inset);
    }

    /// Returns the client_inset value by [`Self::set_client_inset`].
    pub fn client_inset(&self) -> Option<Pixels> {
        self.client_inset
    }

    /// Returns whether the title bar window controls need to be rendered by the application (Wayland and X11)
    pub fn window_decorations(&self) -> Decorations {
        self.platform_window.window_decorations()
    }

    /// Returns which window controls are currently visible (Wayland)
    pub fn window_controls(&self) -> WindowControls {
        self.platform_window.window_controls()
    }

    /// Updates the window's title at the platform level.
    pub fn set_window_title(&mut self, title: &str) {
        self.platform_window.set_title(title);
    }

    /// Sets the application identifier.
    pub fn set_app_id(&mut self, app_id: &str) {
        self.platform_window.set_app_id(app_id);
    }

    /// Sets the window background appearance.
    pub fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        self.platform_window
            .set_background_appearance(background_appearance);
    }

    /// Mark the window as dirty at the platform level.
    pub fn set_window_edited(&mut self, edited: bool) {
        self.platform_window.set_edited(edited);
    }

    /// Set the path of the file this window represents.
    /// On macOS, this sets the window's accessibility document property (AXDocument).
    pub fn set_document_path(&self, path: Option<&std::path::Path>) {
        self.platform_window.set_document_path(path);
    }

    /// Determine the display on which the window is visible.
    pub fn display(&self, cx: &App) -> Option<Rc<dyn PlatformDisplay>> {
        cx.platform
            .displays()
            .into_iter()
            .find(|display| Some(display.id()) == self.display_id)
    }

    /// Show the platform character palette.
    pub fn show_character_palette(&self) {
        self.platform_window.show_character_palette();
    }

    /// The scale factor of the display associated with the window. For example, it could
    /// return 2.0 for a "retina" display, indicating that each logical pixel should actually
    /// be rendered as two pixels on screen.
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// The size of an em for the base font of the application. Adjusting this value allows the
    /// UI to scale, just like zooming a web page.
    pub fn rem_size(&self) -> Pixels {
        self.rem_size_override_stack
            .last()
            .copied()
            .unwrap_or(self.rem_size)
    }

    /// Sets the size of an em for the base font of the application. Adjusting this value allows the
    /// UI to scale, just like zooming a web page.
    pub fn set_rem_size(&mut self, rem_size: impl Into<Pixels>) {
        self.rem_size = rem_size.into();
    }

    /// Acquire a globally unique identifier for the given ElementId.
    /// Only valid for the duration of the provided closure.
    pub fn with_global_id<R>(
        &mut self,
        element_id: ElementId,
        f: impl FnOnce(&GlobalElementId, &mut Self) -> R,
    ) -> R {
        self.with_id(element_id, |this| {
            let global_id = GlobalElementId(Arc::from(&*this.element_id_stack));

            f(&global_id, this)
        })
    }

    /// Calls the provided closure with the element ID pushed on the stack.
    #[inline]
    pub fn with_id<R>(
        &mut self,
        element_id: impl Into<ElementId>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.element_id_stack.push(element_id.into());
        let result = f(self);
        self.element_id_stack.pop();
        result
    }

    /// Executes the provided function with the specified rem size.
    ///
    /// This method must only be called as part of element drawing.
    // This function is called in a highly recursive manner in editor
    // prepainting, make sure its inlined to reduce the stack burden
    #[inline]
    pub fn with_rem_size<F, R>(&mut self, rem_size: Option<impl Into<Pixels>>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.invalidator.debug_assert_paint_or_prepaint();

        if let Some(rem_size) = rem_size {
            self.rem_size_override_stack.push(rem_size.into());
            let result = f(self);
            self.rem_size_override_stack.pop();
            result
        } else {
            f(self)
        }
    }

    /// The line height associated with the current text style.
    pub fn line_height(&self) -> Pixels {
        self.text_style().line_height_in_pixels(self.rem_size())
    }

    /// Rounds a logical value to the nearest device pixel.
    #[inline]
    pub fn pixel_snap(&self, value: Pixels) -> Pixels {
        px(round_to_device_pixel(value.0, self.scale_factor()) / self.scale_factor())
    }

    /// f64 variant of [`Self::pixel_snap`].
    #[inline]
    pub fn pixel_snap_f64(&self, value: f64) -> f64 {
        let scale_factor = f64::from(self.scale_factor());
        round_half_toward_zero_f64(value * scale_factor) / scale_factor
    }

    /// Snaps a bounds' origin and size to the nearest device pixel.
    #[inline]
    pub fn pixel_snap_bounds(&self, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        bounds.map(|c| self.pixel_snap(c))
    }

    /// Snaps a point's coordinates to the nearest device pixel.
    #[inline]
    pub fn pixel_snap_point(&self, position: Point<Pixels>) -> Point<Pixels> {
        position.map(|c| self.pixel_snap(c))
    }

    #[inline]
    fn snap_bounds(&self, bounds: Bounds<Pixels>) -> Bounds<ScaledPixels> {
        let scale_factor = self.scale_factor();
        let left = round_to_device_pixel(bounds.left().0, scale_factor);
        let top = round_to_device_pixel(bounds.top().0, scale_factor);
        let right = round_to_device_pixel(bounds.right().0, scale_factor).max(left);
        let bottom = round_to_device_pixel(bounds.bottom().0, scale_factor).max(top);
        Bounds::from_corners(
            point(ScaledPixels(left), ScaledPixels(top)),
            point(ScaledPixels(right), ScaledPixels(bottom)),
        )
    }

    /// Rounds half-to-zero but clamps any non-zero input up to 1 dp so thin strokes do not disappear.
    #[inline]
    fn snap_stroke(&self, value: Pixels) -> ScaledPixels {
        ScaledPixels(round_stroke_to_device_pixel(value.0, self.scale_factor()))
    }

    #[inline]
    fn snap_border_widths(&self, edges: Edges<Pixels>) -> Edges<ScaledPixels> {
        edges.map(|e| self.snap_stroke(*e))
    }

    /// Floors the near edge and ceils the far edge, producing a strict superset of the raw region.
    #[inline]
    fn cover_bounds(&self, bounds: Bounds<Pixels>) -> Bounds<ScaledPixels> {
        let scale_factor = self.scale_factor();
        let left = floor_to_device_pixel(bounds.left().0, scale_factor);
        let top = floor_to_device_pixel(bounds.top().0, scale_factor);
        let right = ceil_to_device_pixel(bounds.right().0, scale_factor).max(left);
        let bottom = ceil_to_device_pixel(bounds.bottom().0, scale_factor).max(top);
        Bounds::from_corners(
            point(ScaledPixels(left), ScaledPixels(top)),
            point(ScaledPixels(right), ScaledPixels(bottom)),
        )
    }

    #[inline]
    fn snapped_content_mask(&self) -> ContentMask<ScaledPixels> {
        let content_mask = self.content_mask();
        ContentMask {
            bounds: self.cover_bounds(content_mask.bounds),
            corner_radii: content_mask.corner_radii.scale(self.scale_factor()),
        }
    }

    /// Call to prevent the default action of an event. Currently only used to prevent
    /// parent elements from becoming focused on mouse down.
    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    /// Obtain whether default has been prevented for the event currently being dispatched.
    pub fn default_prevented(&self) -> bool {
        self.default_prevented
    }

    /// Determine whether the given action is available along the dispatch path to the currently focused element.
    pub fn is_action_available(&self, action: &dyn Action, cx: &App) -> bool {
        let node_id =
            self.focus_node_id_in_rendered_frame(self.focused(cx).map(|handle| handle.id));
        self.rendered_frame
            .dispatch_tree
            .is_action_available(action, node_id)
    }

    /// Determine whether the given action is available along the dispatch path to the given focus_handle.
    pub fn is_action_available_in(&self, action: &dyn Action, focus_handle: &FocusHandle) -> bool {
        let node_id = self.focus_node_id_in_rendered_frame(Some(focus_handle.id));
        self.rendered_frame
            .dispatch_tree
            .is_action_available(action, node_id)
    }

    /// The position of the mouse relative to the window.
    pub fn mouse_position(&self) -> Point<Pixels> {
        self.mouse_position
    }

    /// The mouse button currently held according to the latest platform event.
    pub fn pressed_mouse_button(&self) -> Option<MouseButton> {
        self.pressed_mouse_button
    }

    /// Returns read-only inspection data for the last rendered frame.
    pub fn devtools_frame_snapshot(&self) -> DevtoolsFrameSnapshot {
        DevtoolsFrameSnapshot {
            viewport: Bounds {
                origin: point(px(0.0), px(0.0)),
                size: self.viewport_size,
            }
            .into(),
            effects: self.devtools_effect_snapshot(),
            elements: self.rendered_frame.devtools_nodes.clone(),
        }
    }

    fn devtools_effect_snapshot(&self) -> DevtoolsEffectSnapshot {
        let scene = &self.rendered_frame.scene;
        let missing_screen_effect_region_count = scene
            .active_screen_effects
            .iter()
            .filter(|effect| match effect.effect().target() {
                ScreenEffectTarget::Region(region_id) => {
                    scene.screen_effect_region(region_id.as_ref()).is_none()
                }
                ScreenEffectTarget::Window | ScreenEffectTarget::Bounds(_) => false,
            })
            .count();
        let missing_screen_effect_snapshot_count = scene
            .active_screen_effects
            .iter()
            .filter(|effect| {
                effect.effect().snapshot_id().is_some_and(|snapshot_id| {
                    !scene
                        .active_screen_effect_snapshots
                        .iter()
                        .any(|snapshot| snapshot.id() == snapshot_id)
                })
            })
            .count();

        DevtoolsEffectSnapshot {
            active_screen_effect_count: scene.active_screen_effects.len(),
            screen_effect_snapshot_count: scene.active_screen_effect_snapshots.len(),
            visual_effect_count: scene.visual_effects.len(),
            screen_effect_region_count: scene.screen_effect_regions.len(),
            missing_screen_effect_region_count,
            missing_screen_effect_snapshot_count,
            capabilities: self.effect_capabilities(),
        }
    }

    pub(crate) fn record_devtools_element(
        &mut self,
        devtools_node_id: usize,
        parent_devtools_node_id: Option<usize>,
        node_id: DispatchNodeId,
        type_name: &'static str,
        global_id: Option<&GlobalElementId>,
        source: Option<&'static core::panic::Location<'static>>,
        bounds: Bounds<Pixels>,
        hitbox_range: Range<usize>,
    ) {
        let parent_dispatch_node_id = self
            .next_frame
            .dispatch_tree
            .parent_node_id(node_id)
            .map(DispatchNodeId::as_usize);
        let dispatch_node = self.next_frame.dispatch_tree.node(node_id);
        self.next_frame
            .devtools_nodes
            .push(DevtoolsElementSnapshot {
                devtools_node_id,
                parent_devtools_node_id,
                dispatch_node_id: node_id.as_usize(),
                parent_dispatch_node_id,
                type_name: type_name.to_string(),
                global_id: global_id.map(ToString::to_string),
                source: source.map(DevtoolsElementSource::from_location),
                bounds: bounds.into(),
                semantics: DevtoolsSemantics::default(),
                hitbox_count: hitbox_range.len(),
                key_listener_count: dispatch_node.key_listeners.len(),
                action_listener_count: dispatch_node.action_listeners.len(),
                focused: dispatch_node.focus_id.is_some()
                    && dispatch_node.focus_id == self.next_frame.focus,
            });
    }

    pub(crate) fn push_devtools_node(&mut self) -> (usize, Option<usize>) {
        let devtools_node_id = self.next_frame.next_devtools_node_id;
        self.next_frame.next_devtools_node_id += 1;
        let parent_devtools_node_id = self.next_frame.devtools_node_stack.last().copied();
        self.next_frame.devtools_node_stack.push(devtools_node_id);
        (devtools_node_id, parent_devtools_node_id)
    }

    pub(crate) fn push_existing_devtools_node(&mut self, devtools_node_id: usize) {
        self.next_frame.devtools_node_stack.push(devtools_node_id);
    }

    pub(crate) fn pop_devtools_node(&mut self, devtools_node_id: usize) {
        debug_assert_eq!(
            self.next_frame.devtools_node_stack.pop(),
            Some(devtools_node_id)
        );
    }

    fn active_devtools_node_id(&self) -> Option<usize> {
        self.next_frame.devtools_node_stack.last().copied()
    }

    /// Attach component-authored DevKit semantics to the element currently being painted.
    pub fn set_devtools_semantics(&mut self, semantics: DevtoolsSemantics) {
        let Some(devtools_node_id) = self.active_devtools_node_id() else {
            return;
        };
        let Some(element) = self
            .next_frame
            .devtools_nodes
            .iter_mut()
            .find(|element| element.devtools_node_id == devtools_node_id)
        else {
            return;
        };
        let mut semantics = semantics;
        if self.pointer_semantics_suppressed != 0 {
            semantics.pointer_actions.clear();
        }
        element.semantics.merge(semantics);
    }

    /// Register a live writable DevKit property on the element currently being painted.
    pub fn on_devtools_property(
        &mut self,
        name: impl Into<String>,
        value_type: impl Into<String>,
        setter: impl Fn(serde_json::Value, &mut Window, &mut App) -> std::result::Result<(), String>
        + 'static,
    ) {
        self.register_devtools_property_setter(DevtoolsPropertySetter {
            property: DevtoolsWritableProperty {
                name: name.into(),
                value_type: value_type.into(),
            },
            callback: Rc::new(setter),
        });
    }

    pub(crate) fn register_devtools_property_setter(&mut self, setter: DevtoolsPropertySetter) {
        let Some(devtools_node_id) = self.active_devtools_node_id() else {
            return;
        };
        let Some(element) = self
            .next_frame
            .devtools_nodes
            .iter_mut()
            .find(|element| element.devtools_node_id == devtools_node_id)
        else {
            return;
        };
        let dispatch_node_id = element.dispatch_node_id;
        if !element
            .semantics
            .writable_properties
            .iter()
            .any(|property| property.name == setter.property.name)
        {
            element
                .semantics
                .writable_properties
                .push(setter.property.clone());
        }
        self.next_frame
            .devtools_property_setters
            .entry(dispatch_node_id)
            .or_default()
            .push(setter);
    }

    /// Mutate a component-authored DevKit property on a rendered dispatch node.
    pub fn set_devtools_property(
        &mut self,
        dispatch_node_id: usize,
        property: &str,
        value: serde_json::Value,
        cx: &mut App,
    ) -> std::result::Result<bool, String> {
        let setter = self
            .rendered_frame
            .devtools_property_setters
            .get(&dispatch_node_id)
            .and_then(|setters| {
                setters
                    .iter()
                    .find(|setter| setter.property.name == property)
                    .cloned()
            });
        let Some(setter) = setter else {
            return Ok(false);
        };
        (setter.callback)(value, self, cx)?;
        Ok(true)
    }

    pub(crate) fn update_devtools_element_after_paint(
        &mut self,
        devtools_node_id: usize,
        node_id: DispatchNodeId,
    ) {
        let dispatch_node = self.next_frame.dispatch_tree.node(node_id);
        let key_listener_count = dispatch_node.key_listeners.len();
        let action_listener_count = dispatch_node.action_listeners.len();
        let focused =
            dispatch_node.focus_id.is_some() && dispatch_node.focus_id == self.next_frame.focus;
        let Some(element) = self
            .next_frame
            .devtools_nodes
            .iter_mut()
            .find(|element| element.devtools_node_id == devtools_node_id)
        else {
            return;
        };
        element.key_listener_count = key_listener_count;
        element.action_listener_count = action_listener_count;
        element.focused = focused;
    }

    /// Captures the pointer for the given hitbox. While captured, all mouse move and mouse up
    /// events will be routed to listeners that check this hitbox's `is_hovered` status,
    /// regardless of actual hit testing. This enables drag operations that continue
    /// even when the pointer moves outside the element's bounds.
    ///
    /// The capture is automatically released on mouse up.
    pub fn capture_pointer(&mut self, hitbox_id: HitboxId) {
        self.capture_pointer_for(PointerId::new(0), hitbox_id);
    }

    fn capture_pointer_for(&mut self, pointer_id: PointerId, hitbox_id: HitboxId) {
        if self
            .pointer_interaction_owners
            .get(&pointer_id)
            .is_some_and(|owner| owner.hitbox_id != hitbox_id)
        {
            self.cancel_pointer_interaction_for(
                pointer_id,
                PointerInteractionCancellationReason::CaptureLost,
            );
        }
        self.captured_hitboxes.insert(pointer_id, hitbox_id);
        self.captured_hitbox = Some(hitbox_id);
    }

    /// Releases any active pointer capture.
    pub fn release_pointer(&mut self) {
        self.cancel_all_pointer_interactions(PointerInteractionCancellationReason::CaptureLost);
        self.captured_hitbox = None;
        self.captured_hitboxes.clear();
        self.pending_mapped_pointers.clear();
        self.gesture_recognizer.cancel();
    }

    /// Returns the active handled-action owner for the current pointer sequence.
    pub fn active_pointer_interaction(&self) -> Option<&PointerInteractionOwner> {
        self.pointer_interaction_diagnostics.active()
    }

    /// Returns the active owner for one concrete pointer sequence.
    pub fn active_pointer_interaction_for(
        &self,
        pointer_id: PointerId,
    ) -> Option<&PointerInteractionOwner> {
        self.pointer_interaction_owners.get(&pointer_id)
    }

    /// Starts an independent drag operation for one pointer sequence.
    pub fn start_drag(&mut self, pointer_id: PointerId, drag: AnyDrag) {
        self.active_drags.insert(pointer_id, drag);
        self.refresh();
    }

    /// Delegates drag feedback composition to the application's root view.
    ///
    /// GPUI continues to provide its built-in feedback rendering until this is enabled so
    /// standalone GPUI applications retain their existing behavior.
    pub fn compose_drag_feedback_in_root(&mut self) {
        self.drag_feedback_composed_by_root = true;
    }

    /// Returns the feedback views and window-relative origins for active pointer drags.
    pub fn drag_feedbacks(&self) -> Vec<(AnyView, Point<Pixels>)> {
        self.active_drags
            .values()
            .map(|drag| (drag.view.clone(), drag.position - drag.cursor_offset))
            .collect()
    }

    /// Returns the active drag for one pointer sequence.
    pub(crate) fn active_drag_for(&self, pointer_id: PointerId) -> Option<&AnyDrag> {
        self.active_drags.get(&pointer_id)
    }

    /// Returns whether this window owns at least one active drag operation.
    pub(crate) fn has_active_drags(&self) -> bool {
        !self.active_drags.is_empty()
    }

    /// Updates an active drag's pointer position.
    pub(crate) fn update_drag_position(&mut self, pointer_id: PointerId, position: Point<Pixels>) {
        if let Some(drag) = self.active_drags.get_mut(&pointer_id) {
            drag.position = match drag.axis {
                Some(crate::DragAxis::Horizontal) => point(position.x, drag.position.y),
                Some(crate::DragAxis::Vertical) => point(drag.position.x, position.y),
                None => position,
            };
            self.refresh();
        }
    }

    /// Removes and returns the active drag for one pointer sequence.
    pub(crate) fn take_active_drag(&mut self, pointer_id: PointerId) -> Option<AnyDrag> {
        let drag = self.active_drags.remove(&pointer_id);
        if drag.is_some() {
            self.refresh();
        }
        drag
    }

    /// Completes one pointer drag and notifies its source after removing feedback.
    pub(crate) fn finish_drag(
        &mut self,
        pointer_id: PointerId,
        was_accepted: bool,
        position: Point<Pixels>,
        cx: &mut App,
    ) {
        if let Some(drag) = self.take_active_drag(pointer_id)
            && let Some(callback) = drag.on_end
        {
            callback(pointer_id, was_accepted, position, self, cx);
        }
    }

    /// Returns the hitbox that has captured the pointer, if any.
    pub fn captured_hitbox(&self) -> Option<HitboxId> {
        self.captured_hitbox
    }

    /// Returns the capture retained by one concrete pointer sequence.
    pub fn captured_hitbox_for(&self, pointer_id: PointerId) -> Option<HitboxId> {
        self.captured_hitboxes.get(&pointer_id).copied()
    }

    /// Returns bounded pointer-owner diagnostics keyed by `PointerId`.
    pub fn pointer_interaction_diagnostics(&self) -> &PointerInteractionDiagnostics {
        &self.pointer_interaction_diagnostics
    }

    /// Returns the direct-pointer sample currently resolving a mapped gesture.
    ///
    /// The value exists only while the corresponding action handler runs. It
    /// lets component-local action handlers create typed callback details
    /// without maintaining a second recognizer or global pointer cache.
    pub fn recognized_gesture_event(&self) -> Option<&PointerEvent> {
        self.recognized_gesture_event.as_ref()
    }

    /// Claims the current pointer sequence from an explicitly handled pointer action.
    pub fn claim_pointer_interaction(
        &mut self,
        report: &ActionDispatchReport,
        hitbox_id: HitboxId,
        button: MouseButton,
        start: Point<Pixels>,
        current: Point<Pixels>,
        gesture_revision: GestureMapVersion,
    ) -> Result<PointerInteractionId, PointerInteractionClaimError> {
        if report.outcome() != ActionDispatchOutcome::Handled {
            return Err(PointerInteractionClaimError::ActionNotHandled);
        }
        if report.invocation().source() != ActionInvocationSource::Pointer {
            return Err(PointerInteractionClaimError::NonPointerAction);
        }
        let Some(target) = report.invocation().target() else {
            return Err(PointerInteractionClaimError::TargetMismatch);
        };
        if self
            .rendered_frame
            .hitbox_dispatch_nodes
            .get(&hitbox_id)
            .is_none_or(|node| node.as_usize() != target)
        {
            return Err(PointerInteractionClaimError::TargetMismatch);
        }
        self.claim_pointer_interaction_for_target(
            PointerId::new(0),
            report,
            DispatchNodeId::from_usize(target),
            hitbox_id,
            button,
            start,
            current,
            gesture_revision,
        )
    }

    fn claim_pointer_interaction_for_target(
        &mut self,
        pointer_id: PointerId,
        report: &ActionDispatchReport,
        target: DispatchNodeId,
        hitbox_id: HitboxId,
        button: MouseButton,
        start: Point<Pixels>,
        current: Point<Pixels>,
        gesture_revision: GestureMapVersion,
    ) -> Result<PointerInteractionId, PointerInteractionClaimError> {
        if report.outcome() != ActionDispatchOutcome::Handled {
            return Err(PointerInteractionClaimError::ActionNotHandled);
        }
        if report.invocation().source() != ActionInvocationSource::Pointer {
            return Err(PointerInteractionClaimError::NonPointerAction);
        }
        let Some(hitbox_target) = self
            .rendered_frame
            .hitbox_dispatch_nodes
            .get(&hitbox_id)
            .copied()
        else {
            return Err(PointerInteractionClaimError::TargetMismatch);
        };
        if !self.dispatch_target_contains(hitbox_target, target) {
            return Err(PointerInteractionClaimError::TargetMismatch);
        }
        if let Some(active) = self.pointer_interaction_owners.get(&pointer_id) {
            let active_id = active.id;
            self.pointer_interaction_diagnostics.claim_conflict_count = self
                .pointer_interaction_diagnostics
                .claim_conflict_count
                .saturating_add(1);
            return Err(PointerInteractionClaimError::AlreadyOwned(active_id));
        }

        let id = PointerInteractionId(self.next_pointer_interaction_id);
        self.next_pointer_interaction_id = self.next_pointer_interaction_id.saturating_add(1);
        let owner = PointerInteractionOwner {
            id,
            pointer_id,
            action_name: report.invocation().action_name(),
            action_invocation_id: report.invocation().id(),
            target: target.as_usize(),
            hitbox_id,
            button,
            start,
            current,
            gesture_revision,
            phase: PointerInteractionPhase::Active,
        };
        self.pointer_interaction_diagnostics
            .active
            .insert(pointer_id, owner.clone());
        self.pointer_interaction_owners.insert(pointer_id, owner);
        self.capture_pointer_for(pointer_id, hitbox_id);
        Ok(id)
    }

    /// Completes the active owner exactly once and releases its capture.
    pub fn complete_pointer_interaction(&mut self) -> Option<PointerInteractionOwner> {
        self.complete_pointer_interaction_for(PointerId::new(0))
    }

    fn complete_pointer_interaction_for(
        &mut self,
        pointer_id: PointerId,
    ) -> Option<PointerInteractionOwner> {
        let mut owner = self.pointer_interaction_owners.remove(&pointer_id)?;
        owner.phase = PointerInteractionPhase::Completed;
        if self.captured_hitboxes.remove(&pointer_id) == Some(owner.hitbox_id) {
            self.captured_hitbox = None;
        }
        if self
            .pointer_interaction_diagnostics
            .active
            .get(&pointer_id)
            .is_some_and(|active| active.id == owner.id)
        {
            self.pointer_interaction_diagnostics
                .active
                .remove(&pointer_id);
        }
        self.pointer_interaction_diagnostics
            .last_terminal
            .insert(pointer_id, owner.clone());
        Some(owner)
    }

    /// Cancels the active owner exactly once and releases its capture.
    pub fn cancel_pointer_interaction(
        &mut self,
        reason: PointerInteractionCancellationReason,
    ) -> Option<PointerInteractionOwner> {
        self.cancel_pointer_interaction_for(PointerId::new(0), reason)
    }

    fn cancel_pointer_interaction_for(
        &mut self,
        pointer_id: PointerId,
        reason: PointerInteractionCancellationReason,
    ) -> Option<PointerInteractionOwner> {
        let mut owner = self.pointer_interaction_owners.remove(&pointer_id)?;
        owner.phase = PointerInteractionPhase::Cancelled(reason);
        if self.captured_hitboxes.remove(&pointer_id) == Some(owner.hitbox_id) {
            self.captured_hitbox = None;
        }
        if self
            .pointer_interaction_diagnostics
            .active
            .get(&pointer_id)
            .is_some_and(|active| active.id == owner.id)
        {
            self.pointer_interaction_diagnostics
                .active
                .remove(&pointer_id);
        }
        self.pointer_interaction_diagnostics
            .last_terminal
            .insert(pointer_id, owner.clone());
        Some(owner)
    }

    fn cancel_all_pointer_interactions(&mut self, reason: PointerInteractionCancellationReason) {
        let pointer_ids = self
            .pointer_interaction_owners
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for pointer_id in pointer_ids {
            self.cancel_pointer_interaction_for(pointer_id, reason);
        }
    }

    fn reconcile_pointer_interaction(&mut self, cx: &App) {
        self.rebind_repainted_pointer_owner();
        let pointer_ids = self
            .pointer_interaction_owners
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for pointer_id in pointer_ids {
            let owner = &self.pointer_interaction_owners[&pointer_id];
            let reason = if owner.gesture_revision != cx.gesture_map.borrow().version() {
                Some(PointerInteractionCancellationReason::ProfileReplaced)
            } else if self.captured_hitboxes.get(&pointer_id) != Some(&owner.hitbox_id) {
                Some(PointerInteractionCancellationReason::CaptureLost)
            } else if self
                .rendered_frame
                .hitbox_dispatch_nodes
                .get(&owner.hitbox_id)
                .is_none_or(|target| {
                    !self
                        .dispatch_target_contains(*target, DispatchNodeId::from_usize(owner.target))
                })
            {
                Some(PointerInteractionCancellationReason::TargetRemoved)
            } else {
                None
            };
            if let Some(reason) = reason {
                self.cancel_pointer_interaction_for(pointer_id, reason);
                self.gesture_recognizer.cancel_pointer(pointer_id);
                self.pending_mapped_pointers.remove(&pointer_id);
            }
        }
    }

    fn hitbox_for_retained_target(
        &self,
        position: Point<Pixels>,
        target: DispatchNodeId,
    ) -> Option<HitboxId> {
        self.rendered_frame
            .hit_test(position)
            .ids
            .into_iter()
            .find(|hitbox_id| self.hitbox_descends_from(*hitbox_id, target))
            .or_else(|| {
                self.rendered_frame.hitbox_dispatch_nodes.iter().find_map(
                    |(hitbox_id, hitbox_target)| {
                        self.dispatch_target_contains(*hitbox_target, target)
                            .then_some(*hitbox_id)
                    },
                )
            })
    }

    fn hitbox_descends_from(&self, hitbox_id: HitboxId, target: DispatchNodeId) -> bool {
        self.rendered_frame
            .hitbox_dispatch_nodes
            .get(&hitbox_id)
            .is_some_and(|hitbox_target| self.dispatch_target_contains(*hitbox_target, target))
    }

    fn dispatch_target_contains(
        &self,
        descendant: DispatchNodeId,
        ancestor: DispatchNodeId,
    ) -> bool {
        descendant.as_usize() < self.rendered_frame.dispatch_tree.len()
            && self
                .rendered_frame
                .dispatch_tree
                .dispatch_path(descendant)
                .contains(&ancestor)
    }

    fn rebind_repainted_pointer_owner(&mut self) {
        let pointer_ids = self
            .pointer_interaction_owners
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for pointer_id in pointer_ids {
            let owner = &self.pointer_interaction_owners[&pointer_id];
            if self.captured_hitboxes.get(&pointer_id) != Some(&owner.hitbox_id)
                || self
                    .hitbox_descends_from(owner.hitbox_id, DispatchNodeId::from_usize(owner.target))
            {
                continue;
            }
            let retained_target = DispatchNodeId::from_usize(owner.target);
            let Some(hitbox_id) = self.hitbox_for_retained_target(owner.start, retained_target)
            else {
                continue;
            };
            if let Some(owner) = self.pointer_interaction_owners.get_mut(&pointer_id) {
                owner.hitbox_id = hitbox_id;
                if self
                    .pointer_interaction_diagnostics
                    .active
                    .get(&pointer_id)
                    .is_some_and(|active| active.id == owner.id)
                {
                    self.pointer_interaction_diagnostics
                        .active
                        .insert(pointer_id, owner.clone());
                }
            }
            self.captured_hitboxes.insert(pointer_id, hitbox_id);
        }
    }

    fn gesture_handler(
        &self,
        snapshot: &GestureMapSnapshot,
        target: DispatchNodeId,
    ) -> Option<DispatchNodeId> {
        let contexts = self.gesture_contexts(target);
        self.rendered_frame
            .dispatch_tree
            .dispatch_path(target)
            .into_iter()
            .rev()
            .find(|node_id| {
                let node = self.rendered_frame.dispatch_tree.node(*node_id);
                snapshot
                    .layers()
                    .flat_map(|layer| layer.rules())
                    .any(|rule| {
                        rule.context_depth(&contexts).is_some()
                            && node.action_listeners.iter().any(|listener| {
                                listener.action_type == rule.action().as_any().type_id()
                            })
                    })
            })
    }

    fn gesture_contexts(&self, target: DispatchNodeId) -> Vec<KeyContext> {
        self.rendered_frame
            .dispatch_tree
            .dispatch_path(target)
            .into_iter()
            .filter_map(|node_id| {
                self.rendered_frame
                    .dispatch_tree
                    .node(node_id)
                    .context
                    .clone()
            })
            .collect()
    }

    fn gesture_target(
        &self,
        pointer_id: PointerId,
        snapshot: &GestureMapSnapshot,
    ) -> Option<(HitboxId, DispatchNodeId)> {
        let mut selected = None;
        for hitbox_id in self
            .captured_hitboxes
            .get(&pointer_id)
            .copied()
            .into_iter()
            .chain(self.mouse_hit_test.ids.iter().copied())
        {
            let Some((target, handler)) = self
                .rendered_frame
                .hitbox_dispatch_nodes
                .get(&hitbox_id)
                .copied()
                .and_then(|target| {
                    self.gesture_handler(snapshot, target)
                        .map(|handler| (target, handler))
                })
            else {
                continue;
            };

            match selected {
                None => selected = Some((hitbox_id, target, handler)),
                Some((_, _, selected_handler))
                    if handler != selected_handler
                        && self.dispatch_target_contains(handler, selected_handler) =>
                {
                    selected = Some((hitbox_id, target, handler));
                }
                Some(_) => {}
            }
        }
        selected.map(|(hitbox_id, target, _)| (hitbox_id, target))
    }

    fn begin_mapped_pointer_gesture(&mut self, event: &PointerEvent, cx: &mut App) {
        if self.pointer_interaction_owners.contains_key(&event.id) {
            return;
        }
        self.pending_mapped_pointers.remove(&event.id);
        self.gesture_recognizer.cancel_pointer(event.id);
        let (snapshot, revision) = {
            let map = cx.gesture_map.borrow();
            (map.snapshot(), map.version())
        };
        let Some((hitbox_id, target)) = self.gesture_target(event.id, &snapshot) else {
            return;
        };
        let contexts = self.gesture_contexts(target);
        let snapshot = snapshot.filtered_for_target(&contexts, |action| {
            self.rendered_frame
                .dispatch_tree
                .is_action_available(action, target)
        });
        self.pending_mapped_pointers.insert(
            event.id,
            PendingMappedPointer {
                pointer_id: event.id,
                hitbox_id,
                target,
                button: event.button,
                start: event.position,
                gesture_revision: revision,
            },
        );
        self.gesture_recognizer
            .pointer_down(event, snapshot, revision, target.as_usize());
        let pointer_id = event.id;
        let Some(delay) = self.gesture_recognizer.long_press_delay(pointer_id) else {
            return;
        };
        self.spawn(cx, async move |cx| {
            cx.background_executor().timer(delay).await;
            cx.update(move |window, cx| {
                let Some(gesture) = window
                    .gesture_recognizer
                    .advance_time_for(pointer_id, Instant::now())
                else {
                    return;
                };
                window.pending_mapped_pointers.remove(&pointer_id);
                window.dispatch_recognized_gesture(gesture, cx);
            })
            .log_err();
        })
        .detach();
    }

    fn recognize_mapped_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut App) {
        let (snapshot, revision) = {
            let map = cx.gesture_map.borrow();
            (map.snapshot(), map.version())
        };
        let Some((_, target)) = self.gesture_target(PointerId::new(0), &snapshot) else {
            return;
        };
        let contexts = self.gesture_contexts(target);
        let snapshot = snapshot.filtered_for_target(&contexts, |action| {
            self.rendered_frame
                .dispatch_tree
                .is_action_available(action, target)
        });
        if let Some(gesture) =
            self.gesture_recognizer
                .scroll_wheel(event, snapshot, revision, target.as_usize())
        {
            self.dispatch_recognized_gesture(gesture, cx);
        }
    }

    fn recognize_mapped_pointer_before_raw(&mut self, event: &dyn Any, cx: &mut App) {
        self.reconcile_pointer_interaction(cx);

        let Some(event) = event.downcast_ref::<PointerEvent>() else {
            return;
        };
        self.captured_hitbox = self.captured_hitboxes.get(&event.id).copied();

        match event.phase {
            crate::PointerPhase::Down => self.begin_mapped_pointer_gesture(event, cx),
            crate::PointerPhase::Move | crate::PointerPhase::Hover => {
                if let Some(owner) = self.pointer_interaction_owners.get_mut(&event.id) {
                    owner.current = event.position;
                    return;
                }

                let Some(gesture) = self.gesture_recognizer.pointer_move(event) else {
                    return;
                };
                let candidate =
                    self.pending_mapped_pointers
                        .remove(&event.id)
                        .and_then(|mut candidate| {
                            if self
                                .rendered_frame
                                .hitbox_dispatch_nodes
                                .get(&candidate.hitbox_id)
                                != Some(&candidate.target)
                            {
                                candidate.hitbox_id = self.hitbox_for_retained_target(
                                    candidate.start,
                                    candidate.target,
                                )?;
                            }
                            Some(candidate)
                        });
                let Some(candidate) = candidate else {
                    self.gesture_recognizer.cancel_pointer(event.id);
                    return;
                };
                let (report, handled_target) =
                    self.dispatch_recognized_gesture_with_handler(gesture, cx);
                if report.outcome() == ActionDispatchOutcome::Handled
                    && report.invocation().target() == Some(candidate.target.as_usize())
                {
                    let _ = handled_target.map_or(
                        Err(PointerInteractionClaimError::TargetMismatch),
                        |handled_target| {
                            self.claim_pointer_interaction_for_target(
                                candidate.pointer_id,
                                &report,
                                handled_target,
                                candidate.hitbox_id,
                                candidate.button,
                                candidate.start,
                                event.position,
                                candidate.gesture_revision,
                            )
                        },
                    );
                }
            }
            crate::PointerPhase::Up | crate::PointerPhase::Cancel => {
                if let Some(owner) = self.pointer_interaction_owners.get_mut(&event.id) {
                    owner.current = event.position;
                } else if let Some(gesture) = self.gesture_recognizer.pointer_up(event) {
                    self.dispatch_recognized_gesture(gesture, cx);
                }
                self.pending_mapped_pointers.remove(&event.id);
                self.gesture_recognizer.cancel_pointer(event.id);
            }
        }
    }

    fn dispatch_recognized_gesture(
        &mut self,
        gesture: RecognizedGesture,
        cx: &mut App,
    ) -> ActionDispatchReport {
        self.dispatch_recognized_gesture_with_handler(gesture, cx).0
    }

    fn dispatch_recognized_gesture_with_handler(
        &mut self,
        gesture: RecognizedGesture,
        cx: &mut App,
    ) -> (ActionDispatchReport, Option<DispatchNodeId>) {
        let previous_event =
            std::mem::replace(&mut self.recognized_gesture_event, gesture.event().cloned());
        let node_id = DispatchNodeId::from_usize(gesture.target());
        let result = if node_id.as_usize() < self.rendered_frame.dispatch_tree.len() {
            self.dispatch_action_on_node_from_with_handler(
                node_id,
                ActionInvocationSource::Pointer,
                gesture.action(),
                cx,
            )
        } else {
            (
                self.dispatch_action_outcome(
                    ActionInvocationSource::Pointer,
                    gesture.action(),
                    Some(gesture.target()),
                    ActionDispatchOutcome::Cancelled,
                    cx,
                ),
                None,
            )
        };
        self.recognized_gesture_event = previous_event;
        result
    }

    /// The current state of the keyboard's modifiers
    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Returns true if the last input event was keyboard-based (key press, tab navigation, etc.)
    /// This is used for focus-visible styling to show focus indicators only for keyboard navigation.
    pub fn last_input_was_keyboard(&self) -> bool {
        self.last_input_modality == InputModality::Keyboard
    }

    /// The current state of the keyboard's capslock
    pub fn capslock(&self) -> Capslock {
        self.capslock
    }

    fn complete_frame(&self) {
        self.platform_window.completed_frame();
    }

    /// Returns whether the window has pending invalidated content that has not
    /// been drawn into `rendered_frame` yet.
    pub fn is_dirty(&self) -> bool {
        self.invalidator.is_dirty()
    }

    /// Produces a new frame and assigns it to `rendered_frame`. To actually show
    /// the contents of the new [`Scene`], use [`Self::present`].
    #[profiling::function]
    pub fn draw(&mut self, cx: &mut App) -> ArenaClearNeeded {
        // Set up the per-App arena for element allocation during this draw.
        // This ensures that multiple test Apps have isolated arenas.
        let _arena_scope = ElementArenaScope::enter(&cx.element_arena);

        self.invalidate_entities();
        cx.entities.clear_accessed();
        debug_assert!(self.rendered_entity_stack.is_empty());
        self.invalidator.set_dirty(false);
        self.requested_autoscroll = None;

        // Restore the previously-used input handler.
        // Place it back into a None slot (left by a previous .take()) so that
        // cached paint_range indices in reuse_paint find the handler at the
        // expected position.
        if let Some(input_handler) = self.platform_window.take_input_handler() {
            if let Some(slot) = self
                .rendered_frame
                .input_handlers
                .iter_mut()
                .rev()
                .find(|h| h.is_none())
            {
                *slot = Some(input_handler);
            } else {
                self.rendered_frame.input_handlers.push(Some(input_handler));
            }
        }
        if !cx.mode.skip_drawing() {
            self.draw_roots(cx);
        }
        self.dirty_views.clear();
        self.next_frame.window_active = self.active.get();

        // Register requested input handler with the platform window.
        // Use .take() instead of .pop() to preserve Vec length, so that cached
        // paint_range indices remain valid for reuse_paint on the next frame.
        // Search backwards to find the last Some entry, since reuse_paint may
        // have copied None slots from the previous frame. (Fixes #50456)
        if let Some(input_handler) = self
            .next_frame
            .input_handlers
            .iter_mut()
            .rev()
            .find_map(|h| h.take())
        {
            self.platform_window.set_input_handler(input_handler);
        }

        self.layout_engine.as_mut().unwrap().clear();
        self.text_system().finish_frame();
        let screen_effect_now = cx.background_executor().now();
        self.prune_expired_screen_effects(screen_effect_now);
        let has_active_screen_effects = !self.active_screen_effects.is_empty();
        self.next_frame.scene.scale_factor = self.scale_factor();
        self.next_frame.scene.active_screen_effects =
            self.active_screen_effect_snapshots(screen_effect_now);
        self.next_frame.scene.active_screen_effect_snapshots =
            self.active_screen_effect_snapshot_requests();
        self.next_frame.finish(&mut self.rendered_frame);

        self.invalidator.set_phase(DrawPhase::Focus);
        let previous_focus_path = self.rendered_frame.focus_path();
        let previous_window_active = self.rendered_frame.window_active;
        mem::swap(&mut self.rendered_frame, &mut self.next_frame);
        self.next_frame.clear();
        if has_active_screen_effects {
            self.invalidator.set_dirty(true);
        }
        let current_focus_path = self.rendered_frame.focus_path();
        let current_window_active = self.rendered_frame.window_active;

        if previous_focus_path != current_focus_path
            || previous_window_active != current_window_active
        {
            if !previous_focus_path.is_empty() && current_focus_path.is_empty() {
                self.focus_lost_listeners
                    .clone()
                    .retain(&(), |listener| listener(self, cx));
            }

            let event = WindowFocusEvent {
                previous_focus_path: if previous_window_active {
                    previous_focus_path
                } else {
                    Default::default()
                },
                current_focus_path: if current_window_active {
                    current_focus_path
                } else {
                    Default::default()
                },
            };
            self.focus_listeners
                .clone()
                .retain(&(), |listener| listener(&event, self, cx));
        }

        debug_assert!(self.rendered_entity_stack.is_empty());
        self.record_entities_accessed(cx);
        self.reset_cursor_style(cx);
        self.refreshing = false;
        self.invalidator.set_phase(DrawPhase::None);
        self.needs_present.set(true);

        ArenaClearNeeded::new(&cx.element_arena)
    }

    fn record_entities_accessed(&mut self, cx: &mut App) {
        let mut entities_ref = cx.entities.accessed_entities.get_mut();
        let mut entities = mem::take(entities_ref.deref_mut());
        let handle = self.handle;
        cx.record_entities_accessed(
            handle,
            // Try moving window invalidator into the Window
            self.invalidator.clone(),
            &entities,
        );
        let mut entities_ref = cx.entities.accessed_entities.get_mut();
        mem::swap(&mut entities, entities_ref.deref_mut());
    }

    fn invalidate_entities(&mut self) {
        let mut views = self.invalidator.take_views();
        for entity in views.drain() {
            self.mark_view_dirty(entity);
        }
        self.invalidator.replace_views(views);
    }

    #[profiling::function]
    fn present(&mut self) {
        self.platform_window.draw(&self.rendered_frame.scene);
        #[cfg(feature = "input-latency-histogram")]
        self.input_latency_tracker.record_frame_presented();
        self.needs_present.set(false);
        profiling::finish_frame!();
    }

    /// Returns a snapshot of the current input-latency histograms.
    #[cfg(feature = "input-latency-histogram")]
    pub fn input_latency_snapshot(&self) -> InputLatencySnapshot {
        self.input_latency_tracker.snapshot()
    }

    fn draw_roots(&mut self, cx: &mut App) {
        // The Windows backend may coalesce WM_MOUSEMOVE during hover and drag to avoid starving
        // redraw delivery. Refreshing from the live platform cursor here keeps hover visuals,
        // overlay scrollbars, and drag affordances anchored to the latest pointer position even
        // when intermediate move messages are dropped.
        if self.is_window_hovered() || self.has_active_drags() || self.captured_hitbox.is_some() {
            self.mouse_position = self.platform_window.mouse_position();
            self.modifiers = self.platform_window.modifiers();
            self.capslock = self.platform_window.capslock();
        }

        self.invalidator.set_phase(DrawPhase::Prepaint);
        self.debug_assert_scroll_container_context_stack_empty();
        self.sticky_layout_context_stack.clear();
        self.keyed_sticky_candidates.clear();
        self.keyed_sticky_plans.clear();
        self.next_sticky_layout_context_id = 0;
        self.tooltip_bounds.take();

        let _inspector_width: Pixels = rems(30.0).to_pixels(self.rem_size());
        let root_size = {
            #[cfg(any(feature = "inspector", debug_assertions))]
            {
                if self.inspector.is_some() {
                    let mut size = self.viewport_size;
                    size.width = (size.width - _inspector_width).max(px(0.0));
                    size
                } else {
                    self.viewport_size
                }
            }
            #[cfg(not(any(feature = "inspector", debug_assertions)))]
            {
                self.viewport_size
            }
        };

        // Layout all root elements.
        let mut root_element = self.root.as_ref().unwrap().clone().into_any();
        root_element.prepaint_as_root(Point::default(), root_size.into(), self, cx);

        #[cfg(any(feature = "inspector", debug_assertions))]
        let inspector_element = self.prepaint_inspector(_inspector_width, cx);

        self.prepaint_deferred_draws(cx);

        let mut prompt_element = None;
        let mut active_drag_elements = Vec::new();
        let mut tooltip_element = None;
        if let Some(prompt) = self.prompt.take() {
            let mut element = prompt.view.any_view().into_any();
            element.prepaint_as_root(Point::default(), root_size.into(), self, cx);
            prompt_element = Some(element);
            self.prompt = Some(prompt);
        } else if !self.active_drags.is_empty() && !self.drag_feedback_composed_by_root {
            let drags = self
                .active_drags
                .values()
                .map(|drag| (drag.view.clone(), drag.position - drag.cursor_offset))
                .collect::<Vec<_>>();
            for (view, offset) in drags {
                let mut element = view.into_any();
                element.prepaint_as_root(offset, AvailableSpace::min_size(), self, cx);
                active_drag_elements.push(element);
            }
        } else {
            tooltip_element = self.prepaint_tooltip(cx);
        }

        self.debug_assert_scroll_container_context_stack_empty();
        self.mouse_hit_test = self.next_frame.hit_test(self.mouse_position);

        // Now actually paint the elements.
        self.invalidator.set_phase(DrawPhase::Paint);
        self.debug_assert_scroll_container_context_stack_empty();
        root_element.paint(self, cx);

        #[cfg(any(feature = "inspector", debug_assertions))]
        self.paint_inspector(inspector_element, cx);

        self.paint_deferred_draws(cx);

        if let Some(mut prompt_element) = prompt_element {
            prompt_element.paint(self, cx);
        } else if !active_drag_elements.is_empty() {
            for mut drag_element in active_drag_elements {
                drag_element.paint(self, cx);
            }
        } else if let Some(mut tooltip_element) = tooltip_element {
            tooltip_element.paint(self, cx);
        }

        #[cfg(any(feature = "inspector", debug_assertions))]
        self.paint_inspector_hitbox(cx);
        self.debug_assert_scroll_container_context_stack_empty();
    }

    fn prepaint_tooltip(&mut self, cx: &mut App) -> Option<AnyElement> {
        // Use indexing instead of iteration to avoid borrowing self for the duration of the loop.
        for tooltip_request_index in (0..self.next_frame.tooltip_requests.len()).rev() {
            let Some(Some(tooltip_request)) = self
                .next_frame
                .tooltip_requests
                .get(tooltip_request_index)
                .cloned()
            else {
                log::error!("Unexpectedly absent TooltipRequest");
                continue;
            };
            let mut element = tooltip_request.tooltip.view.clone().into_any();
            let mouse_position = tooltip_request.tooltip.mouse_position;
            let tooltip_size = element.layout_as_root(AvailableSpace::min_size(), self, cx);

            let mut tooltip_bounds =
                Bounds::new(mouse_position + point(px(1.), px(1.)), tooltip_size);
            let window_bounds = Bounds {
                origin: Point::default(),
                size: self.viewport_size(),
            };

            if tooltip_bounds.right() > window_bounds.right() {
                let new_x = mouse_position.x - tooltip_bounds.size.width - px(1.);
                if new_x >= Pixels::ZERO {
                    tooltip_bounds.origin.x = new_x;
                } else {
                    tooltip_bounds.origin.x = cmp::max(
                        Pixels::ZERO,
                        tooltip_bounds.origin.x - tooltip_bounds.right() - window_bounds.right(),
                    );
                }
            }

            if tooltip_bounds.bottom() > window_bounds.bottom() {
                let new_y = mouse_position.y - tooltip_bounds.size.height - px(1.);
                if new_y >= Pixels::ZERO {
                    tooltip_bounds.origin.y = new_y;
                } else {
                    tooltip_bounds.origin.y = cmp::max(
                        Pixels::ZERO,
                        tooltip_bounds.origin.y - tooltip_bounds.bottom() - window_bounds.bottom(),
                    );
                }
            }

            // It's possible for an element to have an active tooltip while not being painted (e.g.
            // via the `visible_on_hover` method). Since mouse listeners are not active in this
            // case, instead update the tooltip's visibility here.
            let is_visible =
                (tooltip_request.tooltip.check_visible_and_update)(tooltip_bounds, self, cx);
            if !is_visible {
                continue;
            }

            self.with_absolute_element_offset(tooltip_bounds.origin, |window| {
                element.prepaint(window, cx)
            });

            self.tooltip_bounds = Some(TooltipBounds {
                id: tooltip_request.id,
                bounds: tooltip_bounds,
            });
            return Some(element);
        }
        None
    }

    fn prepaint_deferred_draws(&mut self, cx: &mut App) {
        assert_eq!(self.element_id_stack.len(), 0);

        let mut completed_draws = Vec::new();

        // Process deferred draws in multiple rounds to support nesting.
        // Each round processes all current deferred draws, which may produce new ones.
        let mut depth = 0;
        loop {
            // Limit maximum nesting depth to prevent infinite loops.
            assert!(depth < 10, "Exceeded maximum (10) deferred depth");
            depth += 1;
            let deferred_count = self.next_frame.deferred_draws.len();
            if deferred_count == 0 {
                break;
            }

            // Sort by priority for this round
            let traversal_order = self.deferred_draw_traversal_order();
            let mut deferred_draws = mem::take(&mut self.next_frame.deferred_draws);

            for deferred_draw_ix in traversal_order {
                let deferred_draw = &mut deferred_draws[deferred_draw_ix];
                self.element_id_stack
                    .clone_from(&deferred_draw.element_id_stack);
                self.text_style_stack
                    .clone_from(&deferred_draw.text_style_stack);
                self.next_frame
                    .dispatch_tree
                    .set_active_node(deferred_draw.parent_node);

                let prepaint_start = self.prepaint_index();
                if let Some(element) = deferred_draw.element.as_mut() {
                    let content_mask = deferred_draw.content_mask;
                    self.with_rendered_view(deferred_draw.current_view, |window| {
                        window.with_rem_size(Some(deferred_draw.rem_size), |window| {
                            window.with_absolute_element_offset(
                                deferred_draw.absolute_offset,
                                |window| {
                                    window.with_content_mask(content_mask, |window| {
                                        element.prepaint(window, cx);
                                    });
                                },
                            );
                        });
                    })
                } else {
                    self.reuse_prepaint(deferred_draw.prepaint_range.clone());
                }
                let prepaint_end = self.prepaint_index();
                deferred_draw.prepaint_range = prepaint_start..prepaint_end;
            }

            // Save completed draws and continue with newly added ones
            completed_draws.append(&mut deferred_draws);

            self.element_id_stack.clear();
            self.text_style_stack.clear();
        }

        // Restore all completed draws
        self.next_frame.deferred_draws = completed_draws;
    }

    fn paint_deferred_draws(&mut self, cx: &mut App) {
        assert_eq!(self.element_id_stack.len(), 0);

        // Paint all deferred draws in priority order.
        // Since prepaint has already processed nested deferreds, we just paint them all.
        if self.next_frame.deferred_draws.len() == 0 {
            return;
        }

        let traversal_order = self.deferred_draw_traversal_order();
        let mut deferred_draws = mem::take(&mut self.next_frame.deferred_draws);
        for deferred_draw_ix in traversal_order {
            let mut deferred_draw = &mut deferred_draws[deferred_draw_ix];
            self.element_id_stack
                .clone_from(&deferred_draw.element_id_stack);
            self.next_frame
                .dispatch_tree
                .set_active_node(deferred_draw.parent_node);

            let paint_start = self.paint_index();
            let content_mask = deferred_draw.content_mask;
            if let Some(element) = deferred_draw.element.as_mut() {
                self.with_rendered_view(deferred_draw.current_view, |window| {
                    window.with_content_mask(content_mask, |window| {
                        window.with_rem_size(Some(deferred_draw.rem_size), |window| {
                            element.paint(window, cx);
                        });
                    })
                })
            } else {
                self.reuse_paint(deferred_draw.paint_range.clone());
            }
            let paint_end = self.paint_index();
            deferred_draw.paint_range = paint_start..paint_end;
        }
        self.next_frame.deferred_draws = deferred_draws;
        self.element_id_stack.clear();
    }

    fn deferred_draw_traversal_order(&mut self) -> SmallVec<[usize; 8]> {
        let deferred_count = self.next_frame.deferred_draws.len();
        let mut sorted_indices = (0..deferred_count).collect::<SmallVec<[_; 8]>>();
        sorted_indices.sort_by_key(|ix| self.next_frame.deferred_draws[*ix].priority);
        sorted_indices
    }

    pub(crate) fn prepaint_index(&self) -> PrepaintStateIndex {
        PrepaintStateIndex {
            hitboxes_index: self.next_frame.hitboxes.len(),
            tooltips_index: self.next_frame.tooltip_requests.len(),
            deferred_draws_index: self.next_frame.deferred_draws.len(),
            dispatch_tree_index: self.next_frame.dispatch_tree.len(),
            accessed_element_states_index: self.next_frame.accessed_element_states.len(),
            line_layout_index: self.text_system.layout_index(),
        }
    }

    pub(crate) fn reuse_prepaint(&mut self, range: Range<PrepaintStateIndex>) {
        self.next_frame.hitboxes.extend(
            self.rendered_frame.hitboxes[range.start.hitboxes_index..range.end.hitboxes_index]
                .iter()
                .cloned(),
        );
        self.next_frame.tooltip_requests.extend(
            self.rendered_frame.tooltip_requests
                [range.start.tooltips_index..range.end.tooltips_index]
                .iter_mut()
                .map(|request| request.take()),
        );
        self.next_frame.accessed_element_states.extend(
            self.rendered_frame.accessed_element_states[range.start.accessed_element_states_index
                ..range.end.accessed_element_states_index]
                .iter()
                .map(|(id, type_id)| (id.clone(), *type_id)),
        );
        self.text_system
            .reuse_layouts(range.start.line_layout_index..range.end.line_layout_index);

        let reused_subtree = self.next_frame.dispatch_tree.reuse_subtree(
            range.start.dispatch_tree_index..range.end.dispatch_tree_index,
            &mut self.rendered_frame.dispatch_tree,
            self.focus,
        );

        if reused_subtree.contains_focus() {
            self.next_frame.focus = self.focus;
        }

        self.next_frame.deferred_draws.extend(
            self.rendered_frame.deferred_draws
                [range.start.deferred_draws_index..range.end.deferred_draws_index]
                .iter()
                .map(|deferred_draw| DeferredDraw {
                    current_view: deferred_draw.current_view,
                    parent_node: reused_subtree.refresh_node_id(deferred_draw.parent_node),
                    element_id_stack: deferred_draw.element_id_stack.clone(),
                    text_style_stack: deferred_draw.text_style_stack.clone(),
                    content_mask: deferred_draw.content_mask,
                    rem_size: deferred_draw.rem_size,
                    priority: deferred_draw.priority,
                    element: None,
                    absolute_offset: deferred_draw.absolute_offset,
                    prepaint_range: deferred_draw.prepaint_range.clone(),
                    paint_range: deferred_draw.paint_range.clone(),
                }),
        );
    }

    pub(crate) fn paint_index(&self) -> PaintIndex {
        PaintIndex {
            scene_index: self.next_frame.scene.len(),
            mouse_listeners_index: self.next_frame.mouse_listeners.len(),
            input_handlers_index: self.next_frame.input_handlers.len(),
            cursor_styles_index: self.next_frame.cursor_styles.len(),
            accessed_element_states_index: self.next_frame.accessed_element_states.len(),
            tab_handle_index: self.next_frame.tab_stops.paint_index(),
            line_layout_index: self.text_system.layout_index(),
        }
    }

    pub(crate) fn reuse_paint(&mut self, range: Range<PaintIndex>) {
        self.next_frame.cursor_styles.extend(
            self.rendered_frame.cursor_styles
                [range.start.cursor_styles_index..range.end.cursor_styles_index]
                .iter()
                .cloned(),
        );
        self.next_frame.input_handlers.extend(
            self.rendered_frame.input_handlers
                [range.start.input_handlers_index..range.end.input_handlers_index]
                .iter_mut()
                .map(|handler| handler.take()),
        );
        self.next_frame.mouse_listeners.extend(
            self.rendered_frame.mouse_listeners
                [range.start.mouse_listeners_index..range.end.mouse_listeners_index]
                .iter_mut()
                .map(|listener| listener.take()),
        );
        self.next_frame.accessed_element_states.extend(
            self.rendered_frame.accessed_element_states[range.start.accessed_element_states_index
                ..range.end.accessed_element_states_index]
                .iter()
                .map(|(id, type_id)| (id.clone(), *type_id)),
        );
        self.next_frame.tab_stops.replay(
            &self.rendered_frame.tab_stops.insertion_history
                [range.start.tab_handle_index..range.end.tab_handle_index],
        );

        self.text_system
            .reuse_layouts(range.start.line_layout_index..range.end.line_layout_index);
        self.next_frame.scene.replay(
            range.start.scene_index..range.end.scene_index,
            &self.rendered_frame.scene,
        );
    }

    /// Push a text style onto the stack, and call a function with that style active.
    /// Use [`Window::text_style`] to get the current, combined text style. This method
    /// should only be called as part of element drawing.
    pub fn with_text_style<F, R>(&mut self, style: Option<TextStyleRefinement>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.invalidator.debug_assert_paint_or_prepaint();
        if let Some(style) = style {
            self.text_style_stack.push(style);
            let result = f(self);
            self.text_style_stack.pop();
            result
        } else {
            f(self)
        }
    }

    /// Updates the cursor style at the platform level. This method should only be called
    /// during the paint phase of element drawing.
    pub fn set_cursor_style(&mut self, style: CursorStyle, hitbox: &Hitbox) {
        self.invalidator.debug_assert_paint();
        self.next_frame.cursor_styles.push(CursorStyleRequest {
            hitbox_id: Some(hitbox.id),
            style,
        });
    }

    /// Updates the cursor style for the entire window at the platform level. A cursor
    /// style using this method will have precedence over any cursor style set using
    /// `set_cursor_style`. This method should only be called during the paint
    /// phase of element drawing.
    pub fn set_window_cursor_style(&mut self, style: CursorStyle) {
        self.invalidator.debug_assert_paint();
        self.next_frame.cursor_styles.push(CursorStyleRequest {
            hitbox_id: None,
            style,
        })
    }

    /// Sets a tooltip to be rendered for the upcoming frame. This method should only be called
    /// during the paint phase of element drawing.
    pub fn set_tooltip(&mut self, tooltip: AnyTooltip) -> TooltipId {
        self.invalidator.debug_assert_prepaint();
        let id = TooltipId(post_inc(&mut self.next_tooltip_id.0));
        self.next_frame
            .tooltip_requests
            .push(Some(TooltipRequest { id, tooltip }));
        id
    }

    /// Invoke the given function with the given content mask after intersecting it
    /// with the current mask. This method should only be called during element drawing.
    // This function is called in a highly recursive manner in editor
    // prepainting, make sure its inlined to reduce the stack burden
    #[inline]
    pub fn with_content_mask<R>(
        &mut self,
        mask: Option<ContentMask<Pixels>>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint_or_prepaint();
        if let Some(mask) = mask {
            let is_paint = self.is_paint_phase();
            let mask = mask.intersect(&self.content_mask());
            let transformed_mask = is_paint.then(|| {
                self.intersect_paint_content_mask(
                    self.transform_content_mask(mask.scale(self.scale_factor())),
                )
            });
            self.content_mask_stack.push(mask);
            if let Some(transformed_mask) = transformed_mask {
                self.paint_content_mask_stack.push(transformed_mask);
            }
            let result = f(self);
            if transformed_mask.is_some() {
                self.paint_content_mask_stack.pop();
            }
            self.content_mask_stack.pop();
            result
        } else {
            f(self)
        }
    }

    /// Runs `f` with an active scroll container used by sticky descendants.
    pub fn with_scroll_container_context<R>(
        &mut self,
        context: ScrollContainerContext,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint_or_prepaint();
        self.scroll_container_context_stack.push(context);
        let result = f(self);
        self.scroll_container_context_stack.pop();
        result
    }

    pub(crate) fn nearest_scroll_container_context(
        &self,
        axis: ScrollAxis,
    ) -> Option<ScrollContainerContext> {
        self.invalidator.debug_assert_paint_or_prepaint();
        self.scroll_container_context_stack.nearest(axis)
    }

    pub(crate) fn reserve_sticky_layout_context(
        &mut self,
        overflow: Point<Overflow>,
    ) -> Option<StickyLayoutContextId> {
        (overflow.x == Overflow::Scroll || overflow.y == Overflow::Scroll).then(|| {
            let id = self.next_sticky_layout_context_id;
            self.next_sticky_layout_context_id += 1;
            id
        })
    }

    pub(crate) fn with_sticky_layout_context<R>(
        &mut self,
        id: StickyLayoutContextId,
        overflow: Point<Overflow>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.sticky_layout_context_stack
            .push(StickyLayoutContext { id, overflow });
        let result = f(self);
        self.sticky_layout_context_stack.pop();
        result
    }

    pub(crate) fn reserve_keyed_sticky_candidate(
        &mut self,
        style: &Style,
    ) -> Option<StickyOccurrenceId> {
        if style.position != Position::Sticky {
            return None;
        }
        let stack_id = style.sticky_stack_id.clone()?;
        let nearest_context = |axis| {
            self.sticky_layout_context_stack
                .iter()
                .rev()
                .find(|context| match axis {
                    ScrollAxis::Horizontal => context.overflow.x == Overflow::Scroll,
                    ScrollAxis::Vertical => context.overflow.y == Overflow::Scroll,
                })
                .map(|context| context.id)
        };
        let horizontal_context = nearest_context(ScrollAxis::Horizontal);
        let vertical_context = nearest_context(ScrollAxis::Vertical);
        if horizontal_context.is_none() && vertical_context.is_none() {
            return None;
        }

        let occurrence = self.keyed_sticky_candidates.len();
        self.keyed_sticky_candidates.push(KeyedStickyCandidate {
            layout_id: None,
            stack_id,
            inset: style.inset,
            horizontal_context,
            vertical_context,
        });
        self.keyed_sticky_plans
            .push(KeyedStickyCandidatePlan::default());
        Some(occurrence)
    }

    pub(crate) fn complete_keyed_sticky_candidate(
        &mut self,
        occurrence: Option<StickyOccurrenceId>,
        layout_id: LayoutId,
    ) {
        if let Some(candidate) =
            occurrence.and_then(|occurrence| self.keyed_sticky_candidates.get_mut(occurrence))
        {
            candidate.layout_id = Some(layout_id);
        }
    }

    pub(crate) fn prepare_keyed_sticky_context(
        &mut self,
        context_id: StickyLayoutContextId,
        context: ScrollContainerContext,
    ) {
        self.prepare_keyed_sticky_axis(context_id, ScrollAxis::Horizontal, context);
        self.prepare_keyed_sticky_axis(context_id, ScrollAxis::Vertical, context);
    }

    fn prepare_keyed_sticky_axis(
        &mut self,
        context_id: StickyLayoutContextId,
        axis: ScrollAxis,
        context: ScrollContainerContext,
    ) {
        if !context.is_active_on_axis(axis) {
            return;
        }

        let rem_size = self.rem_size();
        let configured = self
            .keyed_sticky_candidates
            .iter()
            .enumerate()
            .filter_map(|(occurrence, candidate)| {
                let owns_context = match axis {
                    ScrollAxis::Horizontal => candidate.horizontal_context == Some(context_id),
                    ScrollAxis::Vertical => candidate.vertical_context == Some(context_id),
                };
                if !owns_context {
                    return None;
                }
                let start_inset = match axis {
                    ScrollAxis::Horizontal => candidate.inset.left,
                    ScrollAxis::Vertical => candidate.inset.top,
                };
                length_to_pixels(
                    start_inset,
                    match axis {
                        ScrollAxis::Horizontal => context.scrollport_bounds.size.width,
                        ScrollAxis::Vertical => context.scrollport_bounds.size.height,
                    },
                    rem_size,
                )?;
                Some((
                    occurrence,
                    candidate.layout_id?,
                    candidate.stack_id.clone(),
                    start_inset,
                ))
            })
            .collect::<Vec<_>>();
        if configured.is_empty() {
            return;
        }

        let mut candidates = Vec::with_capacity(configured.len());
        for (occurrence, layout_id, stack_id, start_inset) in configured {
            let bounds = self.layout_bounds(layout_id);
            candidates.push((occurrence, bounds, stack_id, start_inset));
        }

        let relation = StickyRelationIndex::from_candidates(candidates.iter().enumerate().map(
            |(index, (_, _, stack_id, _))| (index, StickyStacking::StackId(stack_id.clone())),
        ));
        let mut offsets = Vec::with_capacity(candidates.len());
        let mut active_source = None;
        for (index, (_, bounds, _, start_inset)) in candidates.iter().enumerate() {
            let record = relation
                .record(index)
                .expect("relation record exists for every sticky candidate");
            let parent_extent = record.predecessor.map_or(px(0.), |predecessor| {
                let predecessor_bounds = candidates[predecessor].1;
                let predecessor_size = match axis {
                    ScrollAxis::Horizontal => predecessor_bounds.size.width,
                    ScrollAxis::Vertical => predecessor_bounds.size.height,
                };
                offsets[predecessor] + predecessor_size
            });
            let inset = length_to_pixels(
                *start_inset,
                match axis {
                    ScrollAxis::Horizontal => context.scrollport_bounds.size.width,
                    ScrollAxis::Vertical => context.scrollport_bounds.size.height,
                },
                rem_size,
            )
            .expect("configured keyed sticky candidates have a start inset");
            let offset = parent_extent + inset;
            offsets.push(offset);

            let layout_origin = match axis {
                ScrollAxis::Horizontal => bounds.origin.x,
                ScrollAxis::Vertical => bounds.origin.y,
            };
            let scrollport_start = match axis {
                ScrollAxis::Horizontal => context.scrollport_bounds.left(),
                ScrollAxis::Vertical => context.scrollport_bounds.top(),
            };
            if layout_origin <= scrollport_start + offset {
                active_source = Some(index);
            }
        }

        for (occurrence, bounds, _, _) in &candidates {
            let unchanged = ResolvedStickyAxis {
                origin: match axis {
                    ScrollAxis::Horizontal => bounds.origin.x,
                    ScrollAxis::Vertical => bounds.origin.y,
                },
                state: StickyAxisState::Unstuck,
            };
            match axis {
                ScrollAxis::Horizontal => self.keyed_sticky_plans[*occurrence].x = Some(unchanged),
                ScrollAxis::Vertical => self.keyed_sticky_plans[*occurrence].y = Some(unchanged),
            }
        }

        let Some(active_source) = active_source else {
            return;
        };
        let active_source = relation
            .source_at_or_before(active_source)
            .expect("active source is present in the sticky relation");
        let candidate_insets = candidates
            .iter()
            .map(|(_, _, _, inset)| {
                length_to_pixels(
                    *inset,
                    match axis {
                        ScrollAxis::Horizontal => context.scrollport_bounds.size.width,
                        ScrollAxis::Vertical => context.scrollport_bounds.size.height,
                    },
                    rem_size,
                )
                .expect("configured keyed sticky candidates have a start inset")
            })
            .collect::<Vec<_>>();
        let candidate_extents = candidates
            .iter()
            .map(|(_, bounds, _, _)| match axis {
                ScrollAxis::Horizontal => bounds.size.width,
                ScrollAxis::Vertical => bounds.size.height,
            })
            .collect::<Vec<_>>();
        for (index, offset) in
            relation.active_offsets(active_source, &candidate_insets, &candidate_extents)
        {
            let (occurrence, bounds, _, _) = candidates[index];
            let resolved = match axis {
                ScrollAxis::Horizontal => resolve_sticky_axis(
                    bounds.origin.x,
                    bounds.size.width,
                    context.scrollport_bounds.left(),
                    context.scrollport_bounds.right(),
                    context.content_bounds.left(),
                    context.content_bounds.right(),
                    offset.into(),
                    Length::Auto,
                    context.scrollport_bounds.size.width,
                    rem_size,
                ),
                ScrollAxis::Vertical => resolve_sticky_axis(
                    bounds.origin.y,
                    bounds.size.height,
                    context.scrollport_bounds.top(),
                    context.scrollport_bounds.bottom(),
                    context.content_bounds.top(),
                    context.content_bounds.bottom(),
                    offset.into(),
                    Length::Auto,
                    context.scrollport_bounds.size.height,
                    rem_size,
                ),
            };
            match axis {
                ScrollAxis::Horizontal => self.keyed_sticky_plans[occurrence].x = Some(resolved),
                ScrollAxis::Vertical => self.keyed_sticky_plans[occurrence].y = Some(resolved),
            }
        }
    }

    pub(crate) fn resolve_positioned_bounds_and_sticky_state(
        &self,
        style: &Style,
        layout_bounds: Bounds<Pixels>,
        keyed_sticky_occurrence: Option<StickyOccurrenceId>,
    ) -> (PositionedBounds, Option<StickyState>) {
        match style.position {
            Position::Relative | Position::Absolute => {
                (PositionedBounds::unchanged(layout_bounds), None)
            }
            Position::Sticky => {
                let (mut positioned_bounds, mut sticky_state) =
                    PositionedBounds::resolve_sticky_for_scroll_contexts_with_state(
                        style,
                        layout_bounds,
                        self.nearest_scroll_container_context(ScrollAxis::Horizontal),
                        self.nearest_scroll_container_context(ScrollAxis::Vertical),
                        self.rem_size(),
                    );
                if let Some(plan) = keyed_sticky_occurrence
                    .and_then(|occurrence| self.keyed_sticky_plans.get(occurrence))
                {
                    if let Some(resolved) = plan.x {
                        positioned_bounds.visual_bounds.origin.x = resolved.origin;
                        sticky_state.x = resolved.state;
                    }
                    if let Some(resolved) = plan.y {
                        positioned_bounds.visual_bounds.origin.y = resolved.origin;
                        sticky_state.y = resolved.state;
                    }
                    positioned_bounds.delta =
                        positioned_bounds.visual_bounds.origin - layout_bounds.origin;
                    sticky_state.visual_bounds = positioned_bounds.visual_bounds;
                }
                (positioned_bounds, Some(sticky_state))
            }
            Position::Fixed => {
                let containing_block_bounds = self.content_mask().bounds;
                (
                    PositionedBounds::resolve_fixed_for_containing_block(
                        style,
                        layout_bounds,
                        containing_block_bounds,
                        self.rem_size(),
                    ),
                    None,
                )
            }
        }
    }

    /// Updates the global element offset relative to the current offset. This is used to implement
    /// scrolling. This method should only be called during the prepaint phase of element drawing.
    pub fn with_element_offset<R>(
        &mut self,
        offset: Point<Pixels>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_prepaint();

        if offset.is_zero() {
            return f(self);
        };

        let abs_offset = self.element_offset() + offset;
        self.with_absolute_element_offset(abs_offset, f)
    }

    /// Updates the global element offset based on the given offset. This is used to implement
    /// drag handles and other manual painting of elements. This method should only be called during
    /// the prepaint phase of element drawing.
    pub fn with_absolute_element_offset<R>(
        &mut self,
        offset: Point<Pixels>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_prepaint();
        self.element_offset_stack.push(offset);
        let result = f(self);
        self.element_offset_stack.pop();
        result
    }

    pub(crate) fn with_element_opacity<R>(
        &mut self,
        opacity: Option<f32>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint_or_prepaint();

        let Some(opacity) = opacity else {
            return f(self);
        };

        let previous_opacity = self.element_opacity;
        self.element_opacity = previous_opacity * opacity;
        let result = f(self);
        self.element_opacity = previous_opacity;
        result
    }

    /// Applies a cumulative paint-time transform to all painting performed in the given closure.
    ///
    /// This affects painting only and does not change layout or hit testing.
    pub fn with_element_transform<R>(
        &mut self,
        transformation: Option<TransformationMatrix>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();

        let Some(transformation) =
            transformation.filter(|transformation| *transformation != TransformationMatrix::unit())
        else {
            return f(self);
        };

        let current = self.current_element_transform();
        self.element_transform_stack
            .push(current.compose(transformation));
        let result = f(self);
        self.element_transform_stack.pop();
        result
    }

    /// Perform prepaint on child elements in a "retryable" manner, so that any side effects
    /// of prepaints can be discarded before prepainting again. This is used to support autoscroll
    /// where we need to prepaint children to detect the autoscroll bounds, then adjust the
    /// element offset and prepaint again. See [`crate::List`] for an example. This method should only be
    /// called during the prepaint phase of element drawing.
    pub fn transact<T, U>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, U>) -> Result<T, U> {
        self.invalidator.debug_assert_prepaint();
        let index = self.prepaint_index();
        let result = f(self);
        if result.is_err() {
            self.next_frame.hitboxes.truncate(index.hitboxes_index);
            self.next_frame
                .tooltip_requests
                .truncate(index.tooltips_index);
            self.next_frame
                .deferred_draws
                .truncate(index.deferred_draws_index);
            self.next_frame
                .dispatch_tree
                .truncate(index.dispatch_tree_index);
            self.next_frame
                .accessed_element_states
                .truncate(index.accessed_element_states_index);
            self.text_system.truncate_layouts(index.line_layout_index);
        }
        result
    }

    /// When you call this method during [`Element::prepaint`], containing elements will attempt to
    /// scroll to cause the specified bounds to become visible. When they decide to autoscroll, they will call
    /// [`Element::prepaint`] again with a new set of bounds. See [`crate::List`] for an example of an element
    /// that supports this method being called on the elements it contains. This method should only be
    /// called during the prepaint phase of element drawing.
    pub fn request_autoscroll(&mut self, bounds: Bounds<Pixels>) {
        self.invalidator.debug_assert_prepaint();
        self.requested_autoscroll = Some(bounds);
    }

    /// This method can be called from a containing element such as [`crate::List`] to support the autoscroll behavior
    /// described in [`Self::request_autoscroll`].
    pub fn take_autoscroll(&mut self) -> Option<Bounds<Pixels>> {
        self.invalidator.debug_assert_prepaint();
        self.requested_autoscroll.take()
    }

    /// Asynchronously load an asset, if the asset hasn't finished loading this will return None.
    /// Your view will be re-drawn once the asset has finished loading.
    ///
    /// Note that the multiple calls to this method will only result in one `Asset::load` call at a
    /// time.
    pub fn use_asset<A: Asset>(&mut self, source: &A::Source, cx: &mut App) -> Option<A::Output> {
        let (task, is_first) = cx.fetch_asset::<A>(source);
        task.clone().now_or_never().or_else(|| {
            if is_first {
                let entity_id = self.current_view();
                self.spawn(cx, {
                    let task = task.clone();
                    async move |cx| {
                        task.await;

                        cx.on_next_frame(move |_, cx| {
                            cx.notify(entity_id);
                        });
                    }
                })
                .detach();
            }

            None
        })
    }

    /// Asynchronously load an asset, if the asset hasn't finished loading or doesn't exist this will return None.
    /// Your view will not be re-drawn once the asset has finished loading.
    ///
    /// Note that the multiple calls to this method will only result in one `Asset::load` call at a
    /// time.
    pub fn get_asset<A: Asset>(&mut self, source: &A::Source, cx: &mut App) -> Option<A::Output> {
        let (task, _) = cx.fetch_asset::<A>(source);
        task.now_or_never()
    }
    /// Obtain the current element offset. This method should only be called during the
    /// prepaint phase of element drawing.
    pub fn element_offset(&self) -> Point<Pixels> {
        self.invalidator.debug_assert_prepaint();
        self.element_offset_stack
            .last()
            .copied()
            .unwrap_or_default()
    }

    /// Obtain the current element opacity. This method should only be called during the
    /// prepaint phase of element drawing.
    #[inline]
    pub(crate) fn element_opacity(&self) -> f32 {
        self.invalidator.debug_assert_paint_or_prepaint();
        self.element_opacity
    }

    /// Obtain the current content mask. This method should only be called during element drawing.
    pub fn content_mask(&self) -> ContentMask<Pixels> {
        self.invalidator.debug_assert_paint_or_prepaint();
        self.content_mask_stack.last().cloned().unwrap_or_else(|| {
            ContentMask::rectangular(Bounds {
                origin: Point::default(),
                size: self.viewport_size,
            })
        })
    }

    #[inline]
    fn is_paint_phase(&self) -> bool {
        matches!(self.invalidator.inner.borrow().draw_phase, DrawPhase::Paint)
    }

    fn debug_assert_scroll_container_context_stack_empty(&self) {
        debug_assert!(
            self.scroll_container_context_stack.is_empty(),
            "scroll container context stack should be balanced at frame phase boundaries"
        );
    }

    #[inline]
    fn current_element_transform(&self) -> TransformationMatrix {
        self.element_transform_stack
            .last()
            .copied()
            .unwrap_or_default()
    }

    #[inline]
    fn transform_scaled_bounds(&self, bounds: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        self.current_element_transform()
            .apply_to_scaled_bounds(bounds)
    }

    #[inline]
    fn transform_content_mask(
        &self,
        content_mask: ContentMask<ScaledPixels>,
    ) -> ContentMask<ScaledPixels> {
        ContentMask {
            bounds: self.transform_scaled_bounds(content_mask.bounds),
            corner_radii: content_mask.corner_radii,
        }
    }

    #[inline]
    fn paint_content_mask(&self) -> ContentMask<ScaledPixels> {
        self.invalidator.debug_assert_paint();
        self.paint_content_mask_stack
            .last()
            .cloned()
            .unwrap_or_else(|| {
                ContentMask::rectangular(Bounds {
                    origin: Point::default(),
                    size: self.viewport_size.scale(self.scale_factor()),
                })
            })
    }

    #[inline]
    fn intersect_paint_content_mask(
        &self,
        content_mask: ContentMask<ScaledPixels>,
    ) -> ContentMask<ScaledPixels> {
        let current = self.paint_content_mask();
        content_mask.intersect(&current)
    }

    /// Provide elements in the called function with a new namespace in which their identifiers must be unique.
    /// This can be used within a custom element to distinguish multiple sets of child elements.
    pub fn with_element_namespace<R>(
        &mut self,
        element_id: impl Into<ElementId>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.element_id_stack.push(element_id.into());
        let result = f(self);
        self.element_id_stack.pop();
        result
    }

    /// Use a piece of state that exists as long this element is being rendered in consecutive frames.
    pub fn use_keyed_state<S: 'static>(
        &mut self,
        key: impl Into<ElementId>,
        cx: &mut App,
        init: impl FnOnce(&mut Self, &mut Context<S>) -> S,
    ) -> Entity<S> {
        let current_view = self.current_view();
        self.with_global_id(key.into(), |global_id, window| {
            window.with_element_state(global_id, |state: Option<Entity<S>>, window| {
                if let Some(state) = state {
                    (state.clone(), state)
                } else {
                    let new_state = cx.new(|cx| init(window, cx));
                    cx.observe(&new_state, move |_, cx| {
                        cx.notify(current_view);
                    })
                    .detach();
                    (new_state.clone(), new_state)
                }
            })
        })
    }

    /// Use a piece of state that exists as long this element is being rendered in consecutive frames, without needing to specify a key
    ///
    /// NOTE: This method uses the location of the caller to generate an ID for this state.
    ///       If this is not sufficient to identify your state (e.g. you're rendering a list item),
    ///       you can provide a custom ElementID using the `use_keyed_state` method.
    #[track_caller]
    pub fn use_state<S: 'static>(
        &mut self,
        cx: &mut App,
        init: impl FnOnce(&mut Self, &mut Context<S>) -> S,
    ) -> Entity<S> {
        self.use_keyed_state(
            ElementId::CodeLocation(*core::panic::Location::caller()),
            cx,
            init,
        )
    }

    /// Updates or initializes state for an element with the given id that lives across multiple
    /// frames. If an element with this ID existed in the rendered frame, its state will be passed
    /// to the given closure. The state returned by the closure will be stored so it can be referenced
    /// when drawing the next frame. This method should only be called as part of element drawing.
    pub fn with_element_state<S, R>(
        &mut self,
        global_id: &GlobalElementId,
        f: impl FnOnce(Option<S>, &mut Self) -> (R, S),
    ) -> R
    where
        S: 'static,
    {
        self.invalidator.debug_assert_paint_or_prepaint();

        let key = (global_id.clone(), TypeId::of::<S>());
        self.next_frame.accessed_element_states.push(key.clone());

        if let Some(any) = self
            .next_frame
            .element_states
            .remove(&key)
            .or_else(|| self.rendered_frame.element_states.remove(&key))
        {
            let ElementStateBox {
                inner,
                #[cfg(debug_assertions)]
                type_name,
            } = any;
            // Using the extra inner option to avoid needing to reallocate a new box.
            let mut state_box = inner
                .downcast::<Option<S>>()
                .map_err(|_| {
                    #[cfg(debug_assertions)]
                    {
                        anyhow::anyhow!(
                            "invalid element state type for id, requested {:?}, actual: {:?}",
                            std::any::type_name::<S>(),
                            type_name
                        )
                    }

                    #[cfg(not(debug_assertions))]
                    {
                        anyhow::anyhow!(
                            "invalid element state type for id, requested {:?}",
                            std::any::type_name::<S>(),
                        )
                    }
                })
                .unwrap();

            let state = state_box.take().expect(
                "reentrant call to with_element_state for the same state type and element id",
            );
            let (result, state) = f(Some(state), self);
            state_box.replace(state);
            self.next_frame.element_states.insert(
                key,
                ElementStateBox {
                    inner: state_box,
                    #[cfg(debug_assertions)]
                    type_name,
                },
            );
            result
        } else {
            let (result, state) = f(None, self);
            self.next_frame.element_states.insert(
                key,
                ElementStateBox {
                    inner: Box::new(Some(state)),
                    #[cfg(debug_assertions)]
                    type_name: std::any::type_name::<S>(),
                },
            );
            result
        }
    }

    /// A variant of `with_element_state` that allows the element's id to be optional. This is a convenience
    /// method for elements where the element id may or may not be assigned. Prefer using `with_element_state`
    /// when the element is guaranteed to have an id.
    ///
    /// The first option means 'no ID provided'
    /// The second option means 'not yet initialized'
    pub fn with_optional_element_state<S, R>(
        &mut self,
        global_id: Option<&GlobalElementId>,
        f: impl FnOnce(Option<Option<S>>, &mut Self) -> (R, Option<S>),
    ) -> R
    where
        S: 'static,
    {
        self.invalidator.debug_assert_paint_or_prepaint();

        if let Some(global_id) = global_id {
            self.with_element_state(global_id, |state, cx| {
                let (result, state) = f(Some(state), cx);
                let state =
                    state.expect("you must return some state when you pass some element id");
                (result, state)
            })
        } else {
            let (result, state) = f(None, self);
            debug_assert!(
                state.is_none(),
                "you must not return an element state when passing None for the global id"
            );
            result
        }
    }

    /// Executes the given closure within the context of a tab group.
    #[inline]
    pub fn with_tab_group<R>(&mut self, index: Option<isize>, f: impl FnOnce(&mut Self) -> R) -> R {
        if let Some(index) = index {
            self.next_frame.tab_stops.begin_group(index);
            let result = f(self);
            self.next_frame.tab_stops.end_group();
            result
        } else {
            f(self)
        }
    }

    /// Defers the drawing of the given element, scheduling it to be painted on top of the currently-drawn tree
    /// at a later time. The `priority` parameter determines the drawing order relative to other deferred elements,
    /// with higher values being drawn on top.
    ///
    /// When `content_mask` is provided, the deferred element will be clipped to that region during
    /// both prepaint and paint. When `None`, no additional clipping is applied.
    ///
    /// This method should only be called as part of the prepaint phase of element drawing.
    pub fn defer_draw(
        &mut self,
        element: AnyElement,
        absolute_offset: Point<Pixels>,
        priority: usize,
        content_mask: Option<ContentMask<Pixels>>,
    ) {
        self.invalidator.debug_assert_prepaint();
        let parent_node = self.next_frame.dispatch_tree.active_node_id().unwrap();
        self.next_frame.deferred_draws.push(DeferredDraw {
            current_view: self.current_view(),
            parent_node,
            element_id_stack: self.element_id_stack.clone(),
            text_style_stack: self.text_style_stack.clone(),
            content_mask,
            rem_size: self.rem_size(),
            priority,
            element: Some(element),
            absolute_offset,
            prepaint_range: PrepaintStateIndex::default()..PrepaintStateIndex::default(),
            paint_range: PaintIndex::default()..PaintIndex::default(),
        });
    }

    /// Creates a new painting layer for the specified bounds. A "layer" is a batch
    /// of geometry that are non-overlapping and have the same draw order. This is typically used
    /// for performance reasons.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_layer<R>(&mut self, bounds: Bounds<Pixels>, f: impl FnOnce(&mut Self) -> R) -> R {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let content_mask = if transformation == TransformationMatrix::unit() {
            self.snapped_content_mask()
        } else {
            self.paint_content_mask()
        };
        let clipped_bounds = self
            .transform_scaled_bounds(bounds.scale(scale_factor))
            .intersect(&content_mask.bounds);
        if !clipped_bounds.is_empty() {
            self.next_frame.scene.push_layer(clipped_bounds);
        }

        let result = f(self);

        if !clipped_bounds.is_empty() {
            self.next_frame.scene.pop_layer();
        }

        result
    }

    /// Creates a positioned painting layer for the specified bounds.
    ///
    /// Positioned layers paint above normal-flow primitives while retaining the same content mask,
    /// opacity, transform, and child paint behavior as the element's normal paint path. Overlapping
    /// positioned layers are ordered by insertion, so later fixed/sticky elements paint above
    /// earlier fixed/sticky elements at the same visual position.
    pub(crate) fn paint_positioned_layer<R>(
        &mut self,
        bounds: Bounds<Pixels>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let content_mask = if transformation == TransformationMatrix::unit() {
            self.snapped_content_mask()
        } else {
            self.paint_content_mask()
        };
        let clipped_bounds = self
            .transform_scaled_bounds(bounds.scale(scale_factor))
            .intersect(&content_mask.bounds);
        if !clipped_bounds.is_empty() {
            self.next_frame.scene.push_positioned_layer(clipped_bounds);
        }

        let result = f(self);

        if !clipped_bounds.is_empty() {
            self.next_frame.scene.pop_layer();
        }

        result
    }

    /// Creates a top painting layer for chrome that must remain above positioned content.
    ///
    /// This is intended for scrollbars and direct-manipulation affordances whose visual target must
    /// stay above fixed and sticky elements. Ordinary app content should use normal painting or
    /// fixed/sticky positioning instead.
    pub fn paint_top_layer<R>(
        &mut self,
        bounds: Bounds<Pixels>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let content_mask = if transformation == TransformationMatrix::unit() {
            self.snapped_content_mask()
        } else {
            self.paint_content_mask()
        };
        let clipped_bounds = self
            .transform_scaled_bounds(bounds.scale(scale_factor))
            .intersect(&content_mask.bounds);
        if !clipped_bounds.is_empty() {
            self.next_frame.scene.push_top_layer(clipped_bounds);
        }

        let result = f(self);

        if !clipped_bounds.is_empty() {
            self.next_frame.scene.pop_layer();
        }

        result
    }

    /// Paint one or more drop shadows into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_shadows(
        &mut self,
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        shadows: &[BoxShadow],
    ) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let content_mask = if transformation == TransformationMatrix::unit() {
            self.snapped_content_mask()
        } else {
            self.paint_content_mask()
        };
        let opacity = self.element_opacity();
        for shadow in shadows {
            let shadow_bounds = (bounds + shadow.offset).dilate(shadow.spread_radius);
            self.next_frame.scene.insert_primitive(Shadow {
                order: 0,
                blur_radius: shadow.blur_radius.scale(scale_factor),
                bounds: shadow_bounds.scale(scale_factor),
                content_mask,
                corner_radii: corner_radii.scale(scale_factor),
                color: shadow.color.opacity(opacity).paint_color(),
                transformation,
            });
        }
    }

    /// Paint shader-backed element effects into the scene for the next frame.
    ///
    /// Effects are emitted between shadows and quads by `Style::paint`, allowing
    /// renderers to sample already-painted content before the element's own
    /// background, children, and border are drawn.
    pub fn paint_visual_effects(
        &mut self,
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        effects: &[ElementEffect],
    ) {
        self.invalidator.debug_assert_paint();

        if effects.is_empty() {
            return;
        }

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let content_mask = if transformation == TransformationMatrix::unit() {
            self.snapped_content_mask()
        } else {
            self.paint_content_mask()
        };

        self.next_frame
            .scene
            .insert_primitive(VisualEffectPrimitive {
                order: 0,
                bounds: self.snap_bounds(bounds),
                content_mask,
                corner_radii: corner_radii.scale(scale_factor),
                opacity: self.element_opacity(),
                effects: effects.iter().map(ElementEffect::prepare).collect(),
                transformation,
            });
    }

    /// Register a named region that can receive screen-level shader effects.
    ///
    /// The scene stores transformed physical bounds so later screen-effect
    /// target resolution does not need to replay element transforms.
    pub fn paint_screen_effect_region(
        &mut self,
        id: impl Into<ScreenEffectRegionId>,
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
    ) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let scaled_bounds = bounds.scale(scale_factor);
        let bounds = if transformation == TransformationMatrix::unit() {
            self.snap_bounds(bounds)
        } else {
            self.transform_scaled_bounds(scaled_bounds)
        };
        let logical_bounds = bounds.map(|coordinate| px(coordinate.0 / scale_factor));
        let content_mask = if transformation == TransformationMatrix::unit() {
            self.snapped_content_mask()
        } else {
            self.paint_content_mask()
        };

        self.next_frame
            .scene
            .insert_primitive(ScreenEffectRegionPrimitive {
                order: 0,
                id: id.into(),
                bounds,
                logical_bounds,
                content_mask,
                corner_radii: corner_radii.scale(scale_factor),
                transformation: TransformationMatrix::unit(),
            });
    }

    /// Paint one or more quads into the scene for the next frame at the current stacking context.
    /// Quads are colored rectangular regions with an optional background, border, and corner radius.
    /// see [`fill`], [`outline`], and [`quad`] to construct this type.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    ///
    /// Note that the `quad.corner_radii` are allowed to exceed the bounds, creating sharp corners
    /// where the circular arcs meet. This will not display well when combined with dashed borders.
    /// Use `Corners::clamp_radii_for_quad_size` if the radii should fit within the bounds.
    pub fn paint_quad(&mut self, quad: PaintQuad) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let content_mask = if transformation == TransformationMatrix::unit() {
            self.snapped_content_mask()
        } else {
            self.paint_content_mask()
        };
        let opacity = self.element_opacity();
        let snapped_bounds = self.snap_bounds(quad.bounds);
        let snapped_border_widths = self.snap_border_widths(quad.border_widths);
        self.next_frame.scene.insert_primitive(Quad {
            order: 0,
            bounds: snapped_bounds,
            content_mask,
            background: quad.background.opacity(opacity).prepare(),
            border_color: quad.border_color.opacity(opacity).prepare(),
            corner_radii: quad.corner_radii.scale(scale_factor),
            border_widths: snapped_border_widths,
            border_style: quad.border_style,
            transformation,
        });
    }

    /// Paint an experimental HTML island into the scene for the next frame.
    ///
    /// The HTML payload is trusted application markup and is not sanitized here.
    /// Native renderers and web renderers without HTML-in-canvas support
    /// currently skip this primitive; the web DOM registry and texture renderer
    /// consume it in later slices.
    pub fn paint_html_island(
        &mut self,
        id: ElementId,
        bounds: Bounds<Pixels>,
        input_mode: HtmlIslandInputMode,
        content_revision: u64,
        trusted_html: SharedString,
    ) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let scaled_bounds = bounds.scale(scale_factor);
        let bounds = if transformation == TransformationMatrix::unit() {
            self.snap_bounds(bounds)
        } else {
            self.transform_scaled_bounds(scaled_bounds)
        };
        let logical_bounds = bounds.map(|coordinate| px(coordinate.0 / scale_factor));
        let content_mask = if transformation == TransformationMatrix::unit() {
            self.snapped_content_mask()
        } else {
            self.paint_content_mask()
        };

        self.next_frame.scene.insert_primitive(HtmlIslandPrimitive {
            order: 0,
            id,
            bounds,
            logical_bounds,
            content_mask,
            input_mode,
            opacity: self.element_opacity(),
            content_revision,
            trusted_html,
        });
    }

    /// Paint a trusted browser DOM overlay element into the scene for the next frame.
    ///
    /// The HTML payload is trusted application markup and is not sanitized here.
    /// Native renderers ignore this metadata; `gpui_web` consumes it to sync a
    /// real DOM overlay above the canvas.
    pub fn paint_html_element(
        &mut self,
        id: ElementId,
        bounds: Bounds<Pixels>,
        input_mode: HtmlElementInputMode,
        lifetime: HtmlElementLifetime,
        content_fit: HtmlElementContentFit,
        wheel_behavior: HtmlElementWheelBehavior,
        content_revision: u64,
        trusted_html: SharedString,
        extra_styles: SharedString,
    ) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let scaled_bounds = bounds.scale(scale_factor);
        let bounds = if transformation == TransformationMatrix::unit() {
            self.snap_bounds(bounds)
        } else {
            self.transform_scaled_bounds(scaled_bounds)
        };
        let logical_bounds = bounds.map(|coordinate| px(coordinate.0 / scale_factor));
        let content_mask = if transformation == TransformationMatrix::unit() {
            self.snapped_content_mask()
        } else {
            self.paint_content_mask()
        };
        let logical_content_mask = ContentMask {
            bounds: content_mask
                .bounds
                .map(|coordinate| px(coordinate.0 / scale_factor)),
            corner_radii: content_mask
                .corner_radii
                .map(|coordinate| px(coordinate.0 / scale_factor)),
        };

        self.next_frame
            .scene
            .insert_primitive(HtmlElementPrimitive {
                order: 0,
                id,
                bounds,
                logical_bounds,
                logical_content_mask,
                content_mask,
                input_mode,
                lifetime,
                content_fit,
                wheel_behavior,
                opacity: self.element_opacity(),
                content_revision,
                trusted_html,
                extra_styles,
            });
    }

    /// Paint the given `Path` into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_path(&mut self, mut path: Path<Pixels>, color: impl Into<Background>) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let opacity = self.element_opacity();
        let content_mask = self.paint_content_mask();
        let color: Background = color.into();
        path.color = color.opacity(opacity).prepare();
        let path = path.scale(scale_factor);
        let path = if self.current_element_transform() == TransformationMatrix::unit() {
            path.with_content_mask(content_mask)
        } else {
            path.transformed(self.current_element_transform(), content_mask)
        };
        self.next_frame.scene.insert_primitive(path);
    }

    /// Paint an underline into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_underline(
        &mut self,
        origin: Point<Pixels>,
        width: Pixels,
        style: &UnderlineStyle,
    ) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let thickness = self.snap_stroke(style.thickness);
        let height = if style.wavy {
            ScaledPixels(thickness.0 * 3.)
        } else {
            thickness
        };
        let bounds = Bounds {
            origin: origin.map(|c| ScaledPixels(round_to_device_pixel(c.0, scale_factor))),
            size: size(self.snap_stroke(width), height),
        };
        let content_mask = self.paint_content_mask();
        let element_opacity = self.element_opacity();

        self.next_frame.scene.insert_primitive(Underline {
            order: 0,
            pad: 0,
            bounds,
            content_mask,
            color: style
                .color
                .clone()
                .unwrap_or_default()
                .opacity(element_opacity)
                .paint_color(),
            thickness,
            wavy: if style.wavy { 1 } else { 0 },
            transformation: self.current_element_transform(),
        });
    }

    /// Paint a strikethrough into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_strikethrough(
        &mut self,
        origin: Point<Pixels>,
        width: Pixels,
        style: &StrikethroughStyle,
    ) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let height = style.thickness;
        let bounds = Bounds {
            origin: origin.map(|c| ScaledPixels(round_to_device_pixel(c.0, scale_factor))),
            size: size(self.snap_stroke(width), self.snap_stroke(height)),
        };
        let content_mask = self.paint_content_mask();
        let opacity = self.element_opacity();

        self.next_frame.scene.insert_primitive(Underline {
            order: 0,
            pad: 0,
            bounds,
            content_mask,
            thickness: self.snap_stroke(style.thickness),
            color: style
                .color
                .clone()
                .unwrap_or_default()
                .opacity(opacity)
                .paint_color(),
            wavy: 0,
            transformation: self.current_element_transform(),
        });
    }

    /// Paints a monochrome (non-emoji) glyph into the scene for the next frame at the current z-index.
    ///
    /// The y component of the origin is the baseline of the glyph.
    /// You should generally prefer to use the [`ShapedLine::paint`](crate::ShapedLine::paint) or
    /// [`WrappedLine::paint`](crate::WrappedLine::paint) methods in the [`TextSystem`](crate::TextSystem).
    /// This method is only useful if you need to paint a single glyph that has already been shaped.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_glyph(
        &mut self,
        origin: Point<Pixels>,
        font_id: FontId,
        glyph_id: GlyphId,
        font_size: Pixels,
        color: impl Into<Color>,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let color = color.into();

        let element_opacity = self.element_opacity();
        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let glyph_origin = origin.scale(scale_factor);

        let quantized_origin = Point::new(
            round_half_toward_zero(glyph_origin.x.0 * SUBPIXEL_VARIANTS_X as f32)
                / SUBPIXEL_VARIANTS_X as f32,
            round_half_toward_zero(glyph_origin.y.0 * SUBPIXEL_VARIANTS_Y as f32)
                / SUBPIXEL_VARIANTS_Y as f32,
        );
        let subpixel_variant = Point::new(
            (quantized_origin.x.fract() * SUBPIXEL_VARIANTS_X as f32) as u8,
            (quantized_origin.y.fract() * SUBPIXEL_VARIANTS_Y as f32) as u8,
        );
        let integer_origin = quantized_origin.map(|c| ScaledPixels(c.trunc()));
        let subpixel_rendering = transformation == TransformationMatrix::unit()
            && self.should_use_subpixel_rendering(font_id, font_size);
        let dilation = self.text_system().glyph_dilation_for_color(color.clone());
        let params = RenderGlyphParams {
            font_id,
            glyph_id,
            font_size,
            subpixel_variant,
            scale_factor,
            is_emoji: false,
            subpixel_rendering,
            dilation,
        };

        let raster_bounds = self.text_system().raster_bounds(&params)?;
        if !raster_bounds.is_zero() {
            let tile = self
                .sprite_atlas
                .get_or_insert_with(&params.clone().into(), &mut || {
                    let (size, bytes) = self.text_system().rasterize_glyph(&params)?;
                    Ok(Some((size, Cow::Owned(bytes))))
                })?
                .expect("Callback above only errors or returns Some");
            let bounds = Bounds {
                origin: integer_origin + raster_bounds.origin.map(Into::into),
                size: tile.bounds.size.map(Into::into),
            };
            let content_mask = self.paint_content_mask();

            if subpixel_rendering {
                self.next_frame.scene.insert_primitive(SubpixelSprite {
                    order: 0,
                    pad: 0,
                    bounds,
                    content_mask,
                    color: color.opacity(element_opacity).paint_color(),
                    tile,
                    transformation,
                });
            } else {
                self.next_frame.scene.insert_primitive(MonochromeSprite {
                    order: 0,
                    kind: MonochromeSpriteKind::Glyph,
                    bounds,
                    content_mask,
                    color: color.opacity(element_opacity).paint_color(),
                    tile,
                    transformation,
                });
            }
        }
        Ok(())
    }

    /// Paints a monochrome glyph with pre-computed raster bounds.
    ///
    /// This is faster than `paint_glyph` because it skips the per-glyph cache lookup.
    /// Use `ShapedLine::compute_glyph_raster_data` to batch-compute raster bounds during prepaint.
    pub fn paint_glyph_with_raster_bounds(
        &mut self,
        origin: Point<Pixels>,
        _font_id: FontId,
        _glyph_id: GlyphId,
        _font_size: Pixels,
        color: impl Into<Color>,
        raster_bounds: Bounds<DevicePixels>,
        params: &RenderGlyphParams,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let color = color.into();

        let element_opacity = self.element_opacity();
        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let glyph_origin = origin.scale(scale_factor);

        if !raster_bounds.is_zero() {
            let tile = self
                .sprite_atlas
                .get_or_insert_with(&params.clone().into(), &mut || {
                    let (size, bytes) = self.text_system().rasterize_glyph(params)?;
                    Ok(Some((size, Cow::Owned(bytes))))
                })?
                .expect("Callback above only errors or returns Some");
            let bounds = Bounds {
                origin: glyph_origin.map(|px| px.floor()) + raster_bounds.origin.map(Into::into),
                size: tile.bounds.size.map(Into::into),
            };
            let content_mask = self.paint_content_mask();
            self.next_frame.scene.insert_primitive(MonochromeSprite {
                order: 0,
                kind: MonochromeSpriteKind::Glyph,
                bounds,
                content_mask,
                color: color.opacity(element_opacity).paint_color(),
                tile,
                transformation,
            });
        }
        Ok(())
    }

    /// Paints an emoji glyph with pre-computed raster bounds.
    ///
    /// This is faster than `paint_emoji` because it skips the per-glyph cache lookup.
    /// Use `ShapedLine::compute_glyph_raster_data` to batch-compute raster bounds during prepaint.
    pub fn paint_emoji_with_raster_bounds(
        &mut self,
        origin: Point<Pixels>,
        _font_id: FontId,
        _glyph_id: GlyphId,
        _font_size: Pixels,
        raster_bounds: Bounds<DevicePixels>,
        params: &RenderGlyphParams,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let glyph_origin = origin.scale(scale_factor);

        if !raster_bounds.is_zero() {
            let tile = self
                .sprite_atlas
                .get_or_insert_with(&params.clone().into(), &mut || {
                    let (size, bytes) = self.text_system().rasterize_glyph(params)?;
                    Ok(Some((size, Cow::Owned(bytes))))
                })?
                .expect("Callback above only errors or returns Some");

            let bounds = Bounds {
                origin: glyph_origin.map(|px| px.floor()) + raster_bounds.origin.map(Into::into),
                size: tile.bounds.size.map(Into::into),
            };
            let content_mask = self.paint_content_mask();
            let opacity = self.element_opacity();

            self.next_frame.scene.insert_primitive(PolychromeSprite {
                order: 0,
                pad: 0,
                grayscale: false,
                bounds,
                corner_radii: Default::default(),
                content_mask,
                tile,
                opacity,
                transformation,
            });
        }
        Ok(())
    }

    fn should_use_subpixel_rendering(&self, font_id: FontId, font_size: Pixels) -> bool {
        if self.platform_window.background_appearance() != WindowBackgroundAppearance::Opaque {
            return false;
        }

        if !self.platform_window.is_subpixel_rendering_supported() {
            return false;
        }

        let mode = match self.text_rendering_mode.get() {
            TextRenderingMode::PlatformDefault => self
                .text_system()
                .recommended_rendering_mode(font_id, font_size),
            mode => mode,
        };

        mode == TextRenderingMode::Subpixel
    }

    /// Paints an emoji glyph into the scene for the next frame at the current z-index.
    ///
    /// The y component of the origin is the baseline of the glyph.
    /// You should generally prefer to use the [`ShapedLine::paint`](crate::ShapedLine::paint) or
    /// [`WrappedLine::paint`](crate::WrappedLine::paint) methods in the [`TextSystem`](crate::TextSystem).
    /// This method is only useful if you need to paint a single emoji that has already been shaped.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_emoji(
        &mut self,
        origin: Point<Pixels>,
        font_id: FontId,
        glyph_id: GlyphId,
        font_size: Pixels,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let transformation = self.current_element_transform();
        let glyph_origin = origin.scale(scale_factor);
        let integer_origin = glyph_origin.map(|c| ScaledPixels(round_half_toward_zero(c.0)));
        let params = RenderGlyphParams {
            font_id,
            glyph_id,
            font_size,
            subpixel_variant: Default::default(),
            scale_factor,
            is_emoji: true,
            subpixel_rendering: false,
            dilation: 0,
        };

        let raster_bounds = self.text_system().raster_bounds(&params)?;
        if !raster_bounds.is_zero() {
            let tile = self
                .sprite_atlas
                .get_or_insert_with(&params.clone().into(), &mut || {
                    let (size, bytes) = self.text_system().rasterize_glyph(&params)?;
                    Ok(Some((size, Cow::Owned(bytes))))
                })?
                .expect("Callback above only errors or returns Some");

            let bounds = Bounds {
                origin: integer_origin + raster_bounds.origin.map(Into::into),
                size: tile.bounds.size.map(Into::into),
            };
            let content_mask = self.paint_content_mask();
            let opacity = self.element_opacity();

            self.next_frame.scene.insert_primitive(PolychromeSprite {
                order: 0,
                pad: 0,
                grayscale: false,
                bounds,
                corner_radii: Default::default(),
                content_mask,
                tile,
                opacity,
                transformation,
            });
        }
        Ok(())
    }

    /// Paint a monochrome SVG into the scene for the next frame at the current stacking context.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_svg(
        &mut self,
        bounds: Bounds<Pixels>,
        path: SharedString,
        mut data: Option<&[u8]>,
        transformation: TransformationMatrix,
        color: impl Into<Color>,
        cx: &App,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let color = color.into();

        let element_opacity = self.element_opacity();
        let bounds = self.snap_bounds(bounds);

        let params = RenderSvgParams {
            path,
            size: bounds.size.map(|pixels| {
                DevicePixels::from((pixels.0 * SMOOTH_SVG_SCALE_FACTOR).ceil() as i32)
            }),
        };

        let Some(tile) =
            self.sprite_atlas
                .get_or_insert_with(&params.clone().into(), &mut || {
                    let Some((size, bytes)) = cx.svg_renderer.render_alpha_mask(&params, data)?
                    else {
                        return Ok(None);
                    };
                    Ok(Some((size, Cow::Owned(bytes))))
                })?
        else {
            return Ok(());
        };
        let content_mask = self.paint_content_mask();
        let svg_bounds = Bounds {
            origin: bounds.center()
                - Point::new(
                    ScaledPixels(tile.bounds.size.width.0 as f32 / SMOOTH_SVG_SCALE_FACTOR / 2.),
                    ScaledPixels(tile.bounds.size.height.0 as f32 / SMOOTH_SVG_SCALE_FACTOR / 2.),
                ),
            size: tile
                .bounds
                .size
                .map(|value| ScaledPixels(value.0 as f32 / SMOOTH_SVG_SCALE_FACTOR)),
        };
        let final_bounds = svg_bounds
            .map_origin(|value| ScaledPixels(round_half_toward_zero(value.0)))
            .map_size(|size| size.ceil());

        self.next_frame.scene.insert_primitive(MonochromeSprite {
            order: 0,
            kind: MonochromeSpriteKind::ImageMask,
            bounds: final_bounds,
            content_mask,
            color: color.opacity(element_opacity).paint_color(),
            tile,
            transformation: self.current_element_transform().compose(transformation),
        });

        Ok(())
    }

    /// Paint an image into the scene for the next frame at the current z-index.
    /// This method will panic if the frame_index is not valid
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_image(
        &mut self,
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        data: Arc<RenderImage>,
        frame_index: usize,
        grayscale: bool,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let bounds = self.snap_bounds(bounds);
        let params = RenderImageParams {
            image_id: data.id,
            frame_index,
        };

        let tile = self
            .sprite_atlas
            .get_or_insert_with(&params.into(), &mut || {
                Ok(Some((
                    data.size(frame_index),
                    Cow::Borrowed(
                        data.as_bytes(frame_index)
                            .expect("It's the caller's job to pass a valid frame index"),
                    ),
                )))
            })?
            .expect("Callback above only returns Some");
        let content_mask = self.paint_content_mask();
        let corner_radii = corner_radii.scale(self.scale_factor());
        let opacity = self.element_opacity();

        self.next_frame.scene.insert_primitive(PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale,
            bounds,
            content_mask,
            corner_radii,
            tile,
            opacity,
            transformation: self.current_element_transform(),
        });
        Ok(())
    }

    /// Paint a surface into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    #[cfg(target_os = "macos")]
    pub fn paint_surface(&mut self, bounds: Bounds<Pixels>, image_buffer: CVPixelBuffer) {
        use crate::PaintSurface;

        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let bounds = self.transform_scaled_bounds(bounds.scale(scale_factor));
        let content_mask = self.paint_content_mask();
        self.next_frame.scene.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask,
            image_buffer,
        });
    }

    /// Removes an image from the sprite atlas.
    pub fn drop_image(&mut self, data: Arc<RenderImage>) -> Result<()> {
        for frame_index in 0..data.frame_count() {
            let params = RenderImageParams {
                image_id: data.id,
                frame_index,
            };

            self.sprite_atlas.remove(&params.clone().into());
        }

        Ok(())
    }

    /// Removes a rendered SVG from the sprite atlas.
    pub fn drop_svg(&mut self, path: SharedString, size: Size<DevicePixels>) -> Result<()> {
        let params = RenderSvgParams { path, size };
        self.sprite_atlas.remove(&params.into());
        Ok(())
    }

    /// Add a node to the layout tree for the current frame. Takes the `Style` of the element for which
    /// layout is being requested, along with the layout ids of any children. This method is called during
    /// calls to the [`Element::request_layout`] trait method and enables any element to participate in layout.
    ///
    /// This method should only be called as part of the request_layout or prepaint phase of element drawing.
    #[must_use]
    pub fn request_layout(
        &mut self,
        style: Style,
        children: impl IntoIterator<Item = LayoutId>,
        cx: &mut App,
    ) -> LayoutId {
        self.invalidator.debug_assert_prepaint();

        cx.layout_id_buffer.clear();
        cx.layout_id_buffer.extend(children);
        let rem_size = self.rem_size();
        let scale_factor = self.scale_factor();

        self.layout_engine.as_mut().unwrap().request_layout(
            style,
            rem_size,
            scale_factor,
            &cx.layout_id_buffer,
        )
    }

    /// Add a node to the layout tree for the current frame. Instead of taking a `Style` and children,
    /// this variant takes a function that is invoked during layout so you can use arbitrary logic to
    /// determine the element's size. One place this is used internally is when measuring text.
    ///
    /// The given closure is invoked at layout time with the known dimensions and available space and
    /// returns a `Size`.
    ///
    /// This method should only be called as part of the request_layout or prepaint phase of element drawing.
    pub fn request_measured_layout<F>(&mut self, style: Style, measure: F) -> LayoutId
    where
        F: Fn(Size<Option<Pixels>>, Size<AvailableSpace>, &mut Window, &mut App) -> Size<Pixels>
            + 'static,
    {
        self.invalidator.debug_assert_prepaint();

        let rem_size = self.rem_size();
        let scale_factor = self.scale_factor();
        self.layout_engine
            .as_mut()
            .unwrap()
            .request_measured_layout(style, rem_size, scale_factor, measure)
    }

    /// Compute the layout for the given id within the given available space.
    /// This method is called for its side effect, typically by the framework prior to painting.
    /// After calling it, you can request the bounds of the given layout node id or any descendant.
    ///
    /// This method should only be called as part of the prepaint phase of element drawing.
    pub fn compute_layout(
        &mut self,
        layout_id: LayoutId,
        available_space: Size<AvailableSpace>,
        cx: &mut App,
    ) {
        self.invalidator.debug_assert_prepaint();

        let mut layout_engine = self.layout_engine.take().unwrap();
        layout_engine.compute_layout(layout_id, available_space, self, cx);
        self.layout_engine = Some(layout_engine);
    }

    /// Obtain the bounds computed for the given LayoutId relative to the window. This method will usually be invoked by
    /// GPUI itself automatically in order to pass your element its `Bounds` automatically.
    ///
    /// This method should only be called as part of element drawing.
    pub fn layout_bounds(&mut self, layout_id: LayoutId) -> Bounds<Pixels> {
        self.invalidator.debug_assert_prepaint();

        let scale_factor = self.scale_factor();
        let mut bounds = self
            .layout_engine
            .as_mut()
            .unwrap()
            .layout_bounds(layout_id, scale_factor)
            .map(Into::into);
        let snapped_offset = self.pixel_snap_point(self.element_offset());
        bounds.origin += snapped_offset;
        bounds
    }

    /// Applies a hitbox behavior to every hitbox inserted while `f` prepaints
    /// an element subtree.
    pub fn with_subtree_hitbox_behavior<T>(
        &mut self,
        behavior: HitboxBehavior,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.hitbox_behavior_overrides.push(behavior);
        let result = f(self);
        self.hitbox_behavior_overrides.pop();
        result
    }

    /// Removes only pointer operations from semantics authored while `f` paints.
    pub fn with_pointer_semantics_suppressed<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.pointer_semantics_suppressed += 1;
        let result = f(self);
        self.pointer_semantics_suppressed -= 1;
        result
    }

    /// This method should be called during `prepaint`. You can use
    /// the returned [Hitbox] during `paint` or in an event handler
    /// to determine whether the inserted hitbox was the topmost.
    ///
    /// This method should only be called as part of the prepaint phase of element drawing.
    pub fn insert_hitbox(&mut self, bounds: Bounds<Pixels>, behavior: HitboxBehavior) -> Hitbox {
        self.invalidator.debug_assert_prepaint();

        let id = self.next_hitbox_id;
        self.next_hitbox_id = self.next_hitbox_id.next();
        self.insert_hitbox_with_id(id, bounds, behavior)
    }

    /// This method should only be called as part of the prepaint phase of element drawing.
    pub(crate) fn insert_positioned_hitbox(
        &mut self,
        bounds: Bounds<Pixels>,
        behavior: HitboxBehavior,
    ) -> Hitbox {
        self.invalidator.debug_assert_prepaint();

        let id = self.next_hitbox_id.positioned();
        self.next_hitbox_id = self.next_hitbox_id.next();
        self.insert_hitbox_with_id(id, bounds, behavior)
    }

    /// Inserts a hitbox above positioned hitboxes for top-layer chrome such as scrollbars.
    ///
    /// Use this with [`Window::paint_top_layer`] when the interactive area must match a topmost
    /// visual affordance.
    pub fn insert_top_layer_hitbox(
        &mut self,
        bounds: Bounds<Pixels>,
        behavior: HitboxBehavior,
    ) -> Hitbox {
        self.invalidator.debug_assert_prepaint();

        let id = self.next_hitbox_id.top_layer();
        self.next_hitbox_id = self.next_hitbox_id.next();
        self.insert_hitbox_with_id(id, bounds, behavior)
    }

    fn insert_hitbox_with_id(
        &mut self,
        id: HitboxId,
        bounds: Bounds<Pixels>,
        behavior: HitboxBehavior,
    ) -> Hitbox {
        let behavior = self
            .hitbox_behavior_overrides
            .last()
            .copied()
            .unwrap_or(behavior);
        let content_mask = self.content_mask();
        let hitbox = Hitbox {
            id,
            bounds,
            content_mask,
            behavior,
        };
        self.next_frame.hitboxes.push(hitbox.clone());
        if let Some(node_id) = self.next_frame.dispatch_tree.active_node_id() {
            self.next_frame.hitbox_dispatch_nodes.insert(id, node_id);
        }
        hitbox
    }

    /// Set a hitbox which will act as a control area of the platform window.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn insert_window_control_hitbox(&mut self, area: WindowControlArea, hitbox: Hitbox) {
        self.invalidator.debug_assert_paint();
        self.next_frame.window_control_hitboxes.push((area, hitbox));
    }

    /// Sets the key context for the current element. This context will be used to translate
    /// keybindings into actions.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn set_key_context(&mut self, context: KeyContext) {
        self.invalidator.debug_assert_paint();
        self.next_frame.dispatch_tree.set_key_context(context);
    }

    /// Sets the focus handle for the current element. This handle will be used to manage focus state
    /// and keyboard event dispatch for the element.
    ///
    /// This method should only be called as part of the prepaint phase of element drawing.
    pub fn set_focus_handle(&mut self, focus_handle: &FocusHandle, _: &App) {
        self.invalidator.debug_assert_prepaint();
        if focus_handle.is_focused(self) {
            self.next_frame.focus = Some(focus_handle.id);
        }
        self.next_frame.dispatch_tree.set_focus_id(focus_handle.id);
    }

    /// Sets the view id for the current element, which will be used to manage view caching.
    ///
    /// This method should only be called as part of element prepaint. We plan on removing this
    /// method eventually when we solve some issues that require us to construct editor elements
    /// directly instead of always using editors via views.
    pub fn set_view_id(&mut self, view_id: EntityId) {
        self.invalidator.debug_assert_prepaint();
        self.next_frame.dispatch_tree.set_view_id(view_id);
    }

    /// Get the entity ID for the currently rendering view
    pub fn current_view(&self) -> EntityId {
        self.invalidator.debug_assert_paint_or_prepaint();
        self.rendered_entity_stack.last().copied().unwrap()
    }

    #[inline]
    pub(crate) fn with_rendered_view<R>(
        &mut self,
        id: EntityId,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.rendered_entity_stack.push(id);
        let result = f(self);
        self.rendered_entity_stack.pop();
        result
    }

    /// Executes the provided function with the specified image cache.
    pub fn with_image_cache<F, R>(&mut self, image_cache: Option<AnyImageCache>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        if let Some(image_cache) = image_cache {
            self.image_cache_stack.push(image_cache);
            let result = f(self);
            self.image_cache_stack.pop();
            result
        } else {
            f(self)
        }
    }

    /// Sets an input handler, such as [`ElementInputHandler`][element_input_handler], which interfaces with the
    /// platform to receive textual input with proper integration with concerns such
    /// as IME interactions. This handler will be active for the upcoming frame until the following frame is
    /// rendered.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    ///
    /// [element_input_handler]: crate::ElementInputHandler
    pub fn handle_input(
        &mut self,
        focus_handle: &FocusHandle,
        input_handler: impl InputHandler,
        cx: &App,
    ) {
        self.invalidator.debug_assert_paint();

        if focus_handle.is_focused(self) {
            let cx = self.to_async(cx);
            self.next_frame
                .input_handlers
                .push(Some(PlatformInputHandler::new(cx, Box::new(input_handler))));
        }
    }

    /// Register a mouse event listener on the window for the next frame. The type of event
    /// is determined by the first parameter of the given listener. When the next frame is rendered
    /// the listener will be cleared.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn on_pointer_event<Event: InputEvent>(
        &mut self,
        mut listener: impl FnMut(&Event, DispatchPhase, &mut Window, &mut App) + 'static,
    ) {
        self.invalidator.debug_assert_paint();

        self.next_frame.mouse_listeners.push(Some(Box::new(
            move |event: &dyn Any, phase: DispatchPhase, window: &mut Window, cx: &mut App| {
                if let Some(event) = event.downcast_ref() {
                    listener(event, phase, window, cx)
                }
            },
        )));
    }

    /// Register a key event listener on this node for the next frame. The type of event
    /// is determined by the first parameter of the given listener. When the next frame is rendered
    /// the listener will be cleared.
    ///
    /// This is a fairly low-level method, so prefer using event handlers on elements unless you have
    /// a specific need to register a listener yourself.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn on_key_event<Event: KeyEvent>(
        &mut self,
        listener: impl Fn(&Event, DispatchPhase, &mut Window, &mut App) + 'static,
    ) {
        self.invalidator.debug_assert_paint();

        self.next_frame.dispatch_tree.on_key_event(Rc::new(
            move |event: &dyn Any, phase, window: &mut Window, cx: &mut App| {
                if let Some(event) = event.downcast_ref::<Event>() {
                    listener(event, phase, window, cx)
                }
            },
        ));
    }

    /// Register a modifiers changed event listener on the window for the next frame.
    ///
    /// This is a fairly low-level method, so prefer using event handlers on elements unless you have
    /// a specific need to register a global listener.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn on_modifiers_changed(
        &mut self,
        listener: impl Fn(&ModifiersChangedEvent, &mut Window, &mut App) + 'static,
    ) {
        self.invalidator.debug_assert_paint();

        self.next_frame.dispatch_tree.on_modifiers_changed(Rc::new(
            move |event: &ModifiersChangedEvent, window: &mut Window, cx: &mut App| {
                listener(event, window, cx)
            },
        ));
    }

    /// Register a listener to be called when the given focus handle or one of its descendants receives focus.
    /// This does not fire if the given focus handle - or one of its descendants - was previously focused.
    /// Returns a subscription and persists until the subscription is dropped.
    pub fn on_focus_in(
        &mut self,
        handle: &FocusHandle,
        cx: &mut App,
        mut listener: impl FnMut(&mut Window, &mut App) + 'static,
    ) -> Subscription {
        let focus_id = handle.id;
        let (subscription, activate) =
            self.new_focus_listener(Box::new(move |event, window, cx| {
                if event.is_focus_in(focus_id) {
                    listener(window, cx);
                }
                true
            }));
        cx.defer(move |_| activate());
        subscription
    }

    /// Register a listener to be called when the given focus handle or one of its descendants loses focus.
    /// Returns a subscription and persists until the subscription is dropped.
    pub fn on_focus_out(
        &mut self,
        handle: &FocusHandle,
        cx: &mut App,
        mut listener: impl FnMut(FocusOutEvent, &mut Window, &mut App) + 'static,
    ) -> Subscription {
        let focus_id = handle.id;
        let (subscription, activate) =
            self.new_focus_listener(Box::new(move |event, window, cx| {
                if let Some(blurred_id) = event.previous_focus_path.last().copied()
                    && event.is_focus_out(focus_id)
                {
                    let event = FocusOutEvent {
                        blurred: WeakFocusHandle {
                            id: blurred_id,
                            handles: Arc::downgrade(&cx.focus_handles),
                        },
                    };
                    listener(event, window, cx)
                }
                true
            }));
        cx.defer(move |_| activate());
        subscription
    }

    fn reset_cursor_style(&self, cx: &mut App) {
        // Set the cursor only if we're the active window.
        if self.is_window_hovered() {
            let style = self
                .rendered_frame
                .cursor_style(self)
                .unwrap_or(CursorStyle::Arrow);
            cx.platform.set_cursor_style(style);
        }
    }

    /// Dispatch a given keystroke as though the user had typed it.
    /// You can create a keystroke with Keystroke::parse("").
    pub fn dispatch_keystroke(&mut self, keystroke: Keystroke, cx: &mut App) -> bool {
        let keystroke = keystroke.with_simulated_ime();
        let result = self.dispatch_event(
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            }),
            cx,
        );
        if !result.propagate {
            return true;
        }

        if let Some(input) = keystroke.key_char
            && let Some(mut input_handler) = self.platform_window.take_input_handler()
        {
            input_handler.dispatch_input(&input, self, cx);
            self.platform_window.set_input_handler(input_handler);
            return true;
        }

        false
    }

    /// Dispatch a text input payload through the active platform input handler.
    pub fn dispatch_text_input(&mut self, text: &str, cx: &mut App) -> bool {
        let Some(mut input_handler) = self.platform_window.take_input_handler() else {
            return false;
        };

        let accepts_input = input_handler.accepts_text_input(self, cx);
        if accepts_input {
            input_handler.dispatch_input(text, self, cx);
        }
        self.platform_window.set_input_handler(input_handler);
        accepts_input
    }

    /// Reads the full text payload from the active platform input handler.
    pub fn text_input_value(&mut self, cx: &mut App) -> Option<String> {
        let mut input_handler = self.platform_window.take_input_handler()?;
        let mut adjusted_range = None;
        let value = input_handler.read_text_for_range(0..usize::MAX, &mut adjusted_range, self, cx);
        self.platform_window.set_input_handler(input_handler);
        value
    }

    /// Return a key binding string for an action, to display in the UI. Uses the highest precedence
    /// binding for the action (last binding added to the keymap).
    pub fn keystroke_text_for(&self, action: &dyn Action) -> String {
        self.highest_precedence_binding_for_action(action)
            .map(|binding| {
                binding
                    .keystrokes()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| action.name().to_string())
    }

    /// Dispatch a platform input event on the window.
    #[profiling::function]
    pub fn dispatch_event(&mut self, event: PlatformInput, cx: &mut App) -> DispatchEventResult {
        #[cfg(feature = "input-latency-histogram")]
        let dispatch_time = Instant::now();
        let update_count_before = self.invalidator.update_count();
        // Track input modality for focus-visible styling and hover suppression.
        // Hover is suppressed during keyboard modality so that keyboard navigation
        // doesn't show hover highlights on the item under the mouse cursor.
        let old_modality = self.last_input_modality;
        self.last_input_modality = match &event {
            PlatformInput::KeyDown(_) => InputModality::Keyboard,
            PlatformInput::Pointer(_) => InputModality::Mouse,
            _ => self.last_input_modality,
        };
        if self.last_input_modality != old_modality {
            self.refresh();
        }

        // Handlers may set this to false by calling `stop_propagation`.
        cx.propagate_event = true;
        // Handlers may set this to true by calling `prevent_default`.
        self.default_prevented = false;

        let event = match event {
            PlatformInput::Pointer(pointer) => {
                self.mouse_position = pointer.position;
                self.modifiers = pointer.modifiers;
                self.captured_hitbox = self.captured_hitboxes.get(&pointer.id).copied();
                match pointer.phase {
                    crate::PointerPhase::Down => self.pressed_mouse_button = Some(pointer.button),
                    crate::PointerPhase::Up | crate::PointerPhase::Cancel
                        if self.pressed_mouse_button == Some(pointer.button) =>
                    {
                        self.pressed_mouse_button = None;
                    }
                    _ => {}
                }
                PlatformInput::Pointer(pointer)
            }
            PlatformInput::ModifiersChanged(modifiers_changed) => {
                self.modifiers = modifiers_changed.modifiers;
                self.capslock = modifiers_changed.capslock;
                PlatformInput::ModifiersChanged(modifiers_changed)
            }
            PlatformInput::ScrollWheel(scroll_wheel) => {
                self.mouse_position = scroll_wheel.position;
                self.modifiers = scroll_wheel.modifiers;
                PlatformInput::ScrollWheel(scroll_wheel)
            }
            PlatformInput::Pinch(pinch) => {
                self.mouse_position = pinch.position;
                self.modifiers = pinch.modifiers;
                PlatformInput::Pinch(pinch)
            }
            PlatformInput::Rotate(rotate) => {
                self.mouse_position = rotate.position;
                self.modifiers = rotate.modifiers;
                PlatformInput::Rotate(rotate)
            }
            // Translate dragging and dropping of external files from the operating system
            // to internal drag and drop events.
            PlatformInput::FileDrop(file_drop) => match file_drop {
                FileDropEvent::Entered { position, paths } => {
                    self.mouse_position = position;
                    if !self.has_active_drags() {
                        self.start_drag(
                            crate::PointerId::new(0),
                            AnyDrag {
                                value: Arc::new(paths.clone()),
                                view: cx.new(|_| paths).into(),
                                cursor_offset: position,
                                position,
                                axis: None,
                                on_end: None,
                                cursor_style: None,
                            },
                        );
                    }
                    PlatformInput::Pointer(PointerEvent {
                        pressed_button: Some(MouseButton::Left),
                        ..PointerEvent::new(
                            crate::PointerId::new(0),
                            crate::PointerKind::Mouse,
                            crate::PointerPhase::Move,
                            position,
                            Modifiers::default(),
                        )
                    })
                }
                FileDropEvent::Pending { position } => {
                    self.mouse_position = position;
                    PlatformInput::Pointer(PointerEvent {
                        pressed_button: Some(MouseButton::Left),
                        ..PointerEvent::new(
                            crate::PointerId::new(0),
                            crate::PointerKind::Mouse,
                            crate::PointerPhase::Move,
                            position,
                            Modifiers::default(),
                        )
                    })
                }
                FileDropEvent::Submit { position } => {
                    cx.activate(true);
                    self.mouse_position = position;
                    PlatformInput::Pointer(PointerEvent {
                        button: MouseButton::Left,
                        click_count: 1,
                        ..PointerEvent::new(
                            crate::PointerId::new(0),
                            crate::PointerKind::Mouse,
                            crate::PointerPhase::Up,
                            position,
                            Modifiers::default(),
                        )
                    })
                }
                FileDropEvent::Exited => {
                    self.take_active_drag(crate::PointerId::new(0));
                    PlatformInput::FileDrop(FileDropEvent::Exited)
                }
            },
            PlatformInput::KeyDown(_) | PlatformInput::KeyUp(_) => event,
        };

        if let Some(any_input_event) = event.input_event() {
            self.dispatch_input_event(any_input_event, cx);
        } else if let Some(any_key_event) = event.keyboard_event() {
            self.dispatch_key_event(any_key_event, cx);
        }

        if self.invalidator.update_count() > update_count_before {
            self.input_rate_tracker.borrow_mut().record_input();
            #[cfg(feature = "input-latency-histogram")]
            if self.invalidator.not_drawing() {
                self.input_latency_tracker.record_input(dispatch_time);
            } else {
                self.input_latency_tracker.record_mid_draw_input();
            }
        }

        DispatchEventResult {
            propagate: cx.propagate_event,
            default_prevented: self.default_prevented,
        }
    }

    fn dispatch_input_event(&mut self, event: &dyn Any, cx: &mut App) {
        let hit_test = self.rendered_frame.hit_test(self.mouse_position());
        if hit_test != self.mouse_hit_test {
            self.mouse_hit_test = hit_test;
            self.reset_cursor_style(cx);
        }

        #[cfg(any(feature = "inspector", debug_assertions))]
        if self.is_inspector_picking(cx) {
            self.handle_inspector_mouse_event(event, cx);
            // When inspector is picking, all other mouse handling is skipped.
            return;
        }

        self.recognize_mapped_pointer_before_raw(event, cx);
        // A mapped action may stop action propagation, but raw pointer delivery is a separate
        // transient continuation stream and must still reach the initiating hitbox listeners.
        cx.propagate_event = true;

        let mut mouse_listeners = mem::take(&mut self.rendered_frame.mouse_listeners);

        // Capture phase, events bubble from back to front. Handlers for this phase are used for
        // special purposes, such as detecting events outside of a given Bounds.
        for listener in &mut mouse_listeners {
            let listener = listener.as_mut().unwrap();
            listener(event, DispatchPhase::Capture, self, cx);
            if !cx.propagate_event {
                break;
            }
        }

        // Bubble phase, where most normal handlers do their work.
        if cx.propagate_event {
            for listener in mouse_listeners.iter_mut().rev() {
                let listener = listener.as_mut().unwrap();
                listener(event, DispatchPhase::Bubble, self, cx);
                if !cx.propagate_event {
                    break;
                }
            }
        }

        self.rendered_frame.mouse_listeners = mouse_listeners;

        // Wheel action handlers consume state published by the raw wheel listener for the same
        // sample (for example its normalized delta and pointer location). Pointer gestures still
        // recognize before raw delivery so they can establish one continuation owner, but a wheel
        // has no continuation owner and must dispatch only after the current sample is available.
        if let Some(event) = event.downcast_ref::<ScrollWheelEvent>()
            && self.active_pointer_interaction().is_none()
        {
            self.recognize_mapped_wheel(event, cx);
        }

        if let Some(event) = event.downcast_ref::<PointerEvent>() {
            if matches!(
                event.phase,
                crate::PointerPhase::Move | crate::PointerPhase::Hover
            ) {
                self.update_drag_position(event.id, event.position);
            } else if matches!(
                event.phase,
                crate::PointerPhase::Up | crate::PointerPhase::Cancel
            ) {
                self.finish_drag(event.id, false, event.position, cx);
            }
        }

        if let Some(event) = event.downcast_ref::<PointerEvent>()
            && matches!(
                event.phase,
                crate::PointerPhase::Up | crate::PointerPhase::Cancel
            )
        {
            if self
                .pointer_interaction_owners
                .get(&event.id)
                .is_some_and(|owner| owner.button == event.button)
            {
                self.complete_pointer_interaction_for(event.id);
            } else if self.captured_hitboxes.remove(&event.id).is_some() {
                // Preserve the historical auto-release behavior for explicit captures that are
                // not owned by the mapped pointer state machine.
                self.captured_hitbox = None;
            }
        }
    }

    fn dispatch_key_event(&mut self, event: &dyn Any, cx: &mut App) {
        if self.invalidator.is_dirty() {
            self.draw(cx).clear();
        }

        let node_id = self.focus_node_id_in_rendered_frame(self.focus);
        let dispatch_path = self.rendered_frame.dispatch_tree.dispatch_path(node_id);

        let mut keystroke: Option<Keystroke> = None;

        if let Some(event) = event.downcast_ref::<ModifiersChangedEvent>() {
            if event.modifiers.number_of_modifiers() == 0
                && self.pending_modifier.modifiers.number_of_modifiers() == 1
                && !self.pending_modifier.saw_keystroke
            {
                let key = match self.pending_modifier.modifiers {
                    modifiers if modifiers.shift => Some("shift"),
                    modifiers if modifiers.control => Some("control"),
                    modifiers if modifiers.alt => Some("alt"),
                    modifiers if modifiers.platform => Some("platform"),
                    modifiers if modifiers.function => Some("function"),
                    _ => None,
                };
                if let Some(key) = key {
                    keystroke = Some(Keystroke {
                        key: key.to_string(),
                        key_char: None,
                        modifiers: Modifiers::default(),
                    });
                }
            }

            if self.pending_modifier.modifiers.number_of_modifiers() == 0
                && event.modifiers.number_of_modifiers() == 1
            {
                self.pending_modifier.saw_keystroke = false
            }
            self.pending_modifier.modifiers = event.modifiers
        } else if let Some(key_down_event) = event.downcast_ref::<KeyDownEvent>() {
            self.pending_modifier.saw_keystroke = true;
            keystroke = Some(key_down_event.keystroke.clone());
            if key_down_event.keystroke.key_char.is_some()
                && matches!(
                    cx.cursor_hide_mode,
                    CursorHideMode::OnTyping | CursorHideMode::OnTypingAndAction
                )
            {
                cx.platform.hide_cursor_until_mouse_moves();
            }
        }

        let Some(keystroke) = keystroke else {
            self.finish_dispatch_key_event(event, dispatch_path, self.context_stack(), cx);
            return;
        };

        cx.propagate_event = true;
        self.dispatch_keystroke_interceptors(event, self.context_stack(), cx);
        if !cx.propagate_event {
            self.finish_dispatch_key_event(event, dispatch_path, self.context_stack(), cx);
            return;
        }

        let mut currently_pending = self.pending_input.take().unwrap_or_default();
        if currently_pending.focus.is_some() && currently_pending.focus != self.focus {
            currently_pending = PendingInput::default();
        }

        let keymap_snapshot = currently_pending
            .keymap_snapshot
            .take()
            .unwrap_or_else(|| cx.keymap.borrow().snapshot());
        let match_result = self.rendered_frame.dispatch_tree.dispatch_key(
            &keymap_snapshot,
            currently_pending.keystrokes,
            keystroke,
            &dispatch_path,
        );

        if !match_result.to_replay.is_empty() {
            self.replay_pending_input(match_result.to_replay, cx);
            cx.propagate_event = true;
        }

        if !match_result.pending.is_empty() {
            currently_pending.timer.take();
            currently_pending.keystrokes = match_result.pending;
            currently_pending.keymap_snapshot = Some(keymap_snapshot);
            currently_pending.focus = self.focus;

            let text_input_requires_timeout = event
                .downcast_ref::<KeyDownEvent>()
                .filter(|key_down| key_down.keystroke.key_char.is_some())
                .and_then(|_| self.platform_window.take_input_handler())
                .map_or(false, |mut input_handler| {
                    let accepts = input_handler.accepts_text_input(self, cx);
                    self.platform_window.set_input_handler(input_handler);
                    accepts
                });

            currently_pending.needs_timeout |=
                match_result.pending_has_binding || text_input_requires_timeout;

            if currently_pending.needs_timeout {
                currently_pending.timer = Some(self.spawn(cx, async move |cx| {
                    cx.background_executor.timer(Duration::from_secs(1)).await;
                    cx.update(move |window, cx| {
                        let Some(currently_pending) = window
                            .pending_input
                            .take()
                            .filter(|pending| pending.focus == window.focus)
                        else {
                            return;
                        };

                        let node_id = window.focus_node_id_in_rendered_frame(window.focus);
                        let dispatch_path =
                            window.rendered_frame.dispatch_tree.dispatch_path(node_id);

                        let to_replay = window.rendered_frame.dispatch_tree.flush_dispatch(
                            currently_pending
                                .keymap_snapshot
                                .as_ref()
                                .expect("pending input retains its keymap snapshot"),
                            currently_pending.keystrokes,
                            &dispatch_path,
                        );

                        window.pending_input_changed(cx);
                        window.replay_pending_input(to_replay, cx)
                    })
                    .log_err();
                }));
            } else {
                currently_pending.timer = None;
            }
            self.pending_input = Some(currently_pending);
            self.pending_input_changed(cx);
            cx.propagate_event = false;
            return;
        }

        let skip_bindings = event
            .downcast_ref::<KeyDownEvent>()
            .filter(|key_down_event| key_down_event.prefer_character_input)
            .map(|_| {
                self.platform_window
                    .take_input_handler()
                    .map_or(false, |mut input_handler| {
                        let accepts = input_handler.accepts_text_input(self, cx);
                        self.platform_window.set_input_handler(input_handler);
                        // If modifiers are not excessive (e.g. AltGr), and the input handler is accepting text input,
                        // we prefer the text input over bindings.
                        accepts
                    })
            })
            .unwrap_or(false);

        if !skip_bindings {
            for binding in match_result.bindings {
                self.dispatch_keyboard_action_on_node(node_id, binding.action.as_ref(), cx);
                if !cx.propagate_event {
                    self.dispatch_keystroke_observers(
                        event,
                        Some(binding.action),
                        match_result.context_stack,
                        cx,
                    );
                    self.pending_input_changed(cx);
                    return;
                }
            }
        }

        self.finish_dispatch_key_event(event, dispatch_path, match_result.context_stack, cx);
        self.pending_input_changed(cx);
    }

    fn finish_dispatch_key_event(
        &mut self,
        event: &dyn Any,
        dispatch_path: SmallVec<[DispatchNodeId; 32]>,
        context_stack: Vec<KeyContext>,
        cx: &mut App,
    ) {
        self.dispatch_key_down_up_event(event, &dispatch_path, cx);
        if !cx.propagate_event {
            return;
        }

        self.dispatch_modifiers_changed_event(event, &dispatch_path, cx);
        if !cx.propagate_event {
            return;
        }

        self.dispatch_keystroke_observers(event, None, context_stack, cx);
    }

    pub(crate) fn pending_input_changed(&mut self, cx: &mut App) {
        self.pending_input_observers
            .clone()
            .retain(&(), |callback| callback(self, cx));
    }

    fn dispatch_key_down_up_event(
        &mut self,
        event: &dyn Any,
        dispatch_path: &SmallVec<[DispatchNodeId; 32]>,
        cx: &mut App,
    ) {
        // Capture phase
        for node_id in dispatch_path {
            let node = self.rendered_frame.dispatch_tree.node(*node_id);

            for key_listener in node.key_listeners.clone() {
                key_listener(event, DispatchPhase::Capture, self, cx);
                if !cx.propagate_event {
                    return;
                }
            }
        }

        // Bubble phase
        for node_id in dispatch_path.iter().rev() {
            // Handle low level key events
            let node = self.rendered_frame.dispatch_tree.node(*node_id);
            for key_listener in node.key_listeners.clone() {
                key_listener(event, DispatchPhase::Bubble, self, cx);
                if !cx.propagate_event {
                    return;
                }
            }
        }
    }

    fn dispatch_modifiers_changed_event(
        &mut self,
        event: &dyn Any,
        dispatch_path: &SmallVec<[DispatchNodeId; 32]>,
        cx: &mut App,
    ) {
        let Some(event) = event.downcast_ref::<ModifiersChangedEvent>() else {
            return;
        };
        for node_id in dispatch_path.iter().rev() {
            let node = self.rendered_frame.dispatch_tree.node(*node_id);
            for listener in node.modifiers_changed_listeners.clone() {
                listener(event, self, cx);
                if !cx.propagate_event {
                    return;
                }
            }
        }
    }

    /// Determine whether a potential multi-stroke key binding is in progress on this window.
    pub fn has_pending_keystrokes(&self) -> bool {
        self.pending_input.is_some()
    }

    pub(crate) fn clear_pending_keystrokes(&mut self) {
        self.pending_input.take();
    }

    /// Returns the currently pending input keystrokes that might result in a multi-stroke key binding.
    pub fn pending_input_keystrokes(&self) -> Option<&[Keystroke]> {
        self.pending_input
            .as_ref()
            .map(|pending_input| pending_input.keystrokes.as_slice())
    }

    fn replay_pending_input(&mut self, replays: SmallVec<[Replay; 1]>, cx: &mut App) {
        let node_id = self.focus_node_id_in_rendered_frame(self.focus);
        let dispatch_path = self.rendered_frame.dispatch_tree.dispatch_path(node_id);

        'replay: for replay in replays {
            let event = KeyDownEvent {
                keystroke: replay.keystroke.clone(),
                is_held: false,
                prefer_character_input: true,
            };

            cx.propagate_event = true;
            for binding in replay.bindings {
                self.dispatch_keyboard_action_on_node(node_id, binding.action.as_ref(), cx);
                if !cx.propagate_event {
                    self.dispatch_keystroke_observers(
                        &event,
                        Some(binding.action),
                        Vec::default(),
                        cx,
                    );
                    continue 'replay;
                }
            }

            self.dispatch_key_down_up_event(&event, &dispatch_path, cx);
            if !cx.propagate_event {
                continue 'replay;
            }
            if let Some(input) = replay.keystroke.key_char.as_ref().cloned()
                && let Some(mut input_handler) = self.platform_window.take_input_handler()
            {
                input_handler.dispatch_input(&input, self, cx);
                self.platform_window.set_input_handler(input_handler)
            }
        }
    }

    fn focus_node_id_in_rendered_frame(&self, focus_id: Option<FocusId>) -> DispatchNodeId {
        focus_id
            .and_then(|focus_id| {
                self.rendered_frame
                    .dispatch_tree
                    .focusable_node_id(focus_id)
            })
            .unwrap_or_else(|| self.rendered_frame.dispatch_tree.root_node_id())
    }

    fn dispatch_keyboard_action_on_node(
        &mut self,
        node_id: DispatchNodeId,
        action: &dyn Action,
        cx: &mut App,
    ) -> ActionDispatchReport {
        self.dispatch_action_on_node_from(node_id, ActionInvocationSource::Keyboard, action, cx)
    }

    fn dispatch_action_on_node_from(
        &mut self,
        node_id: DispatchNodeId,
        source: ActionInvocationSource,
        action: &dyn Action,
        cx: &mut App,
    ) -> ActionDispatchReport {
        self.dispatch_action_on_node_from_with_handler(node_id, source, action, cx)
            .0
    }

    fn dispatch_action_on_node_from_with_handler(
        &mut self,
        node_id: DispatchNodeId,
        source: ActionInvocationSource,
        action: &dyn Action,
        cx: &mut App,
    ) -> (ActionDispatchReport, Option<DispatchNodeId>) {
        let started_at = Instant::now();
        let invocation = ActionInvocation::new(
            self.next_action_invocation_id,
            action.name(),
            source,
            self.action_invocation_stack
                .last()
                .map(ActionInvocation::id),
            Some(self.handle.window_id().as_u64()),
            Some(node_id.as_usize()),
            Vec::new(),
        );
        self.next_action_invocation_id = self.next_action_invocation_id.saturating_add(1);
        self.action_invocation_stack.push(invocation.clone());
        cx.propagate_event = true;
        self.notify_action_dispatch(ActionDispatchEvent::Began(invocation.clone()), cx);

        let (outcome, handled_count, handled_target) = if cx.propagate_event {
            self.dispatch_action_on_node_inner(node_id, action, cx)
        } else {
            (ActionDispatchOutcome::Cancelled, 0, None)
        };
        let report =
            ActionDispatchReport::new(invocation, outcome, handled_count, started_at.elapsed());
        self.action_invocation_stack.pop();
        self.notify_action_dispatch(ActionDispatchEvent::Ended(report.clone()), cx);

        if outcome == ActionDispatchOutcome::Handled
            && cx.cursor_hide_mode == CursorHideMode::OnTypingAndAction
            && self.last_input_was_keyboard()
        {
            cx.platform.hide_cursor_until_mouse_moves();
        }

        (report, handled_target)
    }

    fn dispatch_action_on_node_inner(
        &mut self,
        node_id: DispatchNodeId,
        action: &dyn Action,
        cx: &mut App,
    ) -> (ActionDispatchOutcome, usize, Option<DispatchNodeId>) {
        let dispatch_path = self.rendered_frame.dispatch_tree.dispatch_path(node_id);
        let mut handled_count = 0;

        // Capture phase for global actions.
        cx.propagate_event = true;
        if let Some(mut global_listeners) = cx
            .global_action_listeners
            .remove(&action.as_any().type_id())
        {
            for listener in &global_listeners {
                listener(action.as_any(), DispatchPhase::Capture, cx);
                if !cx.propagate_event {
                    break;
                }
            }

            global_listeners.extend(
                cx.global_action_listeners
                    .remove(&action.as_any().type_id())
                    .unwrap_or_default(),
            );

            cx.global_action_listeners
                .insert(action.as_any().type_id(), global_listeners);
        }

        if !cx.propagate_event {
            return (ActionDispatchOutcome::Cancelled, handled_count, None);
        }

        // Capture phase for window actions.
        for node_id in &dispatch_path {
            let node = self.rendered_frame.dispatch_tree.node(*node_id);
            for DispatchActionListener {
                action_type,
                listener,
                outcome_listener,
            } in node.action_listeners.clone()
            {
                if outcome_listener.is_some() {
                    continue;
                }
                let any_action = action.as_any();
                if action_type == any_action.type_id() {
                    listener(any_action, DispatchPhase::Capture, self, cx);

                    if !cx.propagate_event {
                        return (ActionDispatchOutcome::Cancelled, handled_count, None);
                    }
                }
            }
        }

        // Bubble phase for window actions.
        for node_id in dispatch_path.iter().rev() {
            let node = self.rendered_frame.dispatch_tree.node(*node_id);
            for DispatchActionListener {
                action_type,
                listener,
                outcome_listener,
            } in node.action_listeners.clone()
            {
                let any_action = action.as_any();
                if action_type == any_action.type_id() {
                    if let Some(outcome_listener) = outcome_listener {
                        match outcome_listener(any_action, self, cx) {
                            ActionDispatchOutcome::Unhandled => {
                                cx.propagate_event = true;
                                continue;
                            }
                            ActionDispatchOutcome::Handled => {
                                handled_count += 1;
                                return (
                                    ActionDispatchOutcome::Handled,
                                    handled_count,
                                    Some(*node_id),
                                );
                            }
                            outcome @ (ActionDispatchOutcome::Failed
                            | ActionDispatchOutcome::Cancelled) => {
                                return (outcome, handled_count, Some(*node_id));
                            }
                        }
                    }
                    cx.propagate_event = false; // Actions stop propagation by default during the bubble phase
                    listener(any_action, DispatchPhase::Bubble, self, cx);
                    handled_count += 1;

                    if !cx.propagate_event {
                        return (
                            ActionDispatchOutcome::Handled,
                            handled_count,
                            Some(*node_id),
                        );
                    }
                }
            }
        }

        // Bubble phase for global actions.
        if let Some(mut global_listeners) = cx
            .global_action_listeners
            .remove(&action.as_any().type_id())
        {
            for listener in global_listeners.iter().rev() {
                cx.propagate_event = false; // Actions stop propagation by default during the bubble phase

                listener(action.as_any(), DispatchPhase::Bubble, cx);
                handled_count += 1;
                if !cx.propagate_event {
                    break;
                }
            }

            global_listeners.extend(
                cx.global_action_listeners
                    .remove(&action.as_any().type_id())
                    .unwrap_or_default(),
            );

            cx.global_action_listeners
                .insert(action.as_any().type_id(), global_listeners);
        }

        if handled_count == 0 {
            (ActionDispatchOutcome::Unhandled, handled_count, None)
        } else {
            (ActionDispatchOutcome::Handled, handled_count, None)
        }
    }

    fn dispatch_unhandled_action(
        &mut self,
        source: ActionInvocationSource,
        action: &dyn Action,
        cx: &mut App,
    ) -> ActionDispatchReport {
        self.dispatch_action_outcome(source, action, None, ActionDispatchOutcome::Unhandled, cx)
    }

    fn dispatch_action_outcome(
        &mut self,
        source: ActionInvocationSource,
        action: &dyn Action,
        target: Option<usize>,
        outcome: ActionDispatchOutcome,
        cx: &mut App,
    ) -> ActionDispatchReport {
        let invocation = ActionInvocation::new(
            self.next_action_invocation_id,
            action.name(),
            source,
            self.action_invocation_stack
                .last()
                .map(ActionInvocation::id),
            Some(self.handle.window_id().as_u64()),
            target,
            Vec::new(),
        );
        self.next_action_invocation_id = self.next_action_invocation_id.saturating_add(1);
        self.notify_action_dispatch(ActionDispatchEvent::Began(invocation.clone()), cx);
        let report = ActionDispatchReport::new(invocation, outcome, 0, Duration::ZERO);
        self.notify_action_dispatch(ActionDispatchEvent::Ended(report.clone()), cx);
        report
    }

    fn notify_action_dispatch(&mut self, event: ActionDispatchEvent, cx: &mut App) {
        let observers = self.action_dispatch_observers.clone();
        observers.retain(&(), |observer| observer(&event, self, cx));
    }

    /// Register the given handler to be invoked whenever the global of the given type
    /// is updated.
    pub fn observe_global<G: Global>(
        &mut self,
        cx: &mut App,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Subscription {
        let window_handle = self.handle;
        let (subscription, activate) = cx.global_observers.insert(
            TypeId::of::<G>(),
            Box::new(move |cx| {
                window_handle
                    .update(cx, |_, window, cx| f(window, cx))
                    .is_ok()
            }),
        );
        cx.defer(move |_| activate());
        subscription
    }

    /// Focus the current window and bring it to the foreground at the platform level.
    pub fn activate_window(&self) {
        self.platform_window.activate();
    }

    /// Minimize the current window at the platform level.
    pub fn minimize_window(&self) {
        self.platform_window.minimize();
    }

    /// Toggle full screen status on the current window at the platform level.
    pub fn toggle_fullscreen(&self) {
        self.platform_window.toggle_fullscreen();
    }

    /// Updates the IME panel position suggestions for languages like japanese, chinese.
    pub fn invalidate_character_coordinates(&self) {
        self.on_next_frame(|window, cx| {
            if let Some(mut input_handler) = window.platform_window.take_input_handler() {
                if let Some(bounds) = input_handler.selected_bounds(window, cx) {
                    window.platform_window.update_ime_position(bounds);
                }
                window.platform_window.set_input_handler(input_handler);
            }
        });
    }

    /// Present a platform dialog.
    /// The provided message will be presented, along with buttons for each answer.
    /// When a button is clicked, the returned Receiver will receive the index of the clicked button.
    pub fn prompt<T>(
        &mut self,
        level: PromptLevel,
        message: &str,
        detail: Option<&str>,
        answers: &[T],
        cx: &mut App,
    ) -> oneshot::Receiver<usize>
    where
        T: Clone + Into<PromptButton>,
    {
        let prompt_builder = cx.prompt_builder.take();
        let Some(prompt_builder) = prompt_builder else {
            unreachable!("Re-entrant window prompting is not supported by GPUI");
        };

        let answers = answers
            .iter()
            .map(|answer| answer.clone().into())
            .collect::<Vec<_>>();

        let receiver = match &prompt_builder {
            PromptBuilder::Default => self
                .platform_window
                .prompt(level, message, detail, &answers)
                .unwrap_or_else(|| {
                    self.build_custom_prompt(&prompt_builder, level, message, detail, &answers, cx)
                }),
            PromptBuilder::Custom(_) => {
                self.build_custom_prompt(&prompt_builder, level, message, detail, &answers, cx)
            }
        };

        cx.prompt_builder = Some(prompt_builder);

        receiver
    }

    fn build_custom_prompt(
        &mut self,
        prompt_builder: &PromptBuilder,
        level: PromptLevel,
        message: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
        cx: &mut App,
    ) -> oneshot::Receiver<usize> {
        let (sender, receiver) = oneshot::channel();
        let handle = PromptHandle::new(sender);
        let handle = (prompt_builder)(level, message, detail, answers, handle, self, cx);
        self.prompt = Some(handle);
        receiver
    }

    /// Returns the current context stack.
    pub fn context_stack(&self) -> Vec<KeyContext> {
        let node_id = self.focus_node_id_in_rendered_frame(self.focus);
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        dispatch_tree
            .dispatch_path(node_id)
            .iter()
            .filter_map(move |&node_id| dispatch_tree.node(node_id).context.clone())
            .collect()
    }

    /// Returns all available actions for the focused element.
    pub fn available_actions(&self, cx: &App) -> Vec<Box<dyn Action>> {
        let node_id = self.focus_node_id_in_rendered_frame(self.focus);
        let mut actions = self.rendered_frame.dispatch_tree.available_actions(node_id);
        for action_type in cx.global_action_listeners.keys() {
            if let Err(ix) = actions.binary_search_by_key(action_type, |a| a.as_any().type_id()) {
                let action = cx.actions.build_action_type(action_type).ok();
                if let Some(action) = action {
                    actions.insert(ix, action);
                }
            }
        }
        actions
    }

    /// Returns key bindings that invoke an action on the currently focused element. Bindings are
    /// returned in the order they were added. For display, the last binding should take precedence.
    pub fn bindings_for_action(&self, action: &dyn Action) -> Vec<KeyBinding> {
        self.rendered_frame
            .dispatch_tree
            .bindings_for_action(action, &self.rendered_frame.dispatch_tree.context_stack)
    }

    /// Returns the highest precedence key binding that invokes an action on the currently focused
    /// element. This is more efficient than getting the last result of `bindings_for_action`.
    pub fn highest_precedence_binding_for_action(&self, action: &dyn Action) -> Option<KeyBinding> {
        self.rendered_frame
            .dispatch_tree
            .highest_precedence_binding_for_action(
                action,
                &self.rendered_frame.dispatch_tree.context_stack,
            )
    }

    /// Returns the key bindings for an action in a context.
    pub fn bindings_for_action_in_context(
        &self,
        action: &dyn Action,
        context: KeyContext,
    ) -> Vec<KeyBinding> {
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        dispatch_tree.bindings_for_action(action, &[context])
    }

    /// Returns the highest precedence key binding for an action in a context. This is more
    /// efficient than getting the last result of `bindings_for_action_in_context`.
    pub fn highest_precedence_binding_for_action_in_context(
        &self,
        action: &dyn Action,
        context: KeyContext,
    ) -> Option<KeyBinding> {
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        dispatch_tree.highest_precedence_binding_for_action(action, &[context])
    }

    /// Returns any bindings that would invoke an action on the given focus handle if it were
    /// focused. Bindings are returned in the order they were added. For display, the last binding
    /// should take precedence.
    pub fn bindings_for_action_in(
        &self,
        action: &dyn Action,
        focus_handle: &FocusHandle,
    ) -> Vec<KeyBinding> {
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        let Some(context_stack) = self.context_stack_for_focus_handle(focus_handle) else {
            return vec![];
        };
        dispatch_tree.bindings_for_action(action, &context_stack)
    }

    /// Returns the highest precedence key binding that would invoke an action on the given focus
    /// handle if it were focused. This is more efficient than getting the last result of
    /// `bindings_for_action_in`.
    pub fn highest_precedence_binding_for_action_in(
        &self,
        action: &dyn Action,
        focus_handle: &FocusHandle,
    ) -> Option<KeyBinding> {
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        let context_stack = self.context_stack_for_focus_handle(focus_handle)?;
        dispatch_tree.highest_precedence_binding_for_action(action, &context_stack)
    }

    /// Find the bindings that can follow the current input sequence for the current context stack.
    pub fn possible_bindings_for_input(&self, input: &[Keystroke]) -> Vec<KeyBinding> {
        self.rendered_frame
            .dispatch_tree
            .possible_next_bindings_for_input(input, &self.context_stack())
    }

    fn context_stack_for_focus_handle(
        &self,
        focus_handle: &FocusHandle,
    ) -> Option<Vec<KeyContext>> {
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        let node_id = dispatch_tree.focusable_node_id(focus_handle.id)?;
        let context_stack: Vec<_> = dispatch_tree
            .dispatch_path(node_id)
            .into_iter()
            .filter_map(|node_id| dispatch_tree.node(node_id).context.clone())
            .collect();
        Some(context_stack)
    }

    /// Returns a generic event listener that invokes the given listener with the view and context associated with the given view handle.
    pub fn listener_for<T: 'static, E>(
        &self,
        view: &Entity<T>,
        f: impl Fn(&mut T, &E, &mut Window, &mut Context<T>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        let view = view.downgrade();
        move |e: &E, window: &mut Window, cx: &mut App| {
            view.update(cx, |view, cx| f(view, e, window, cx)).ok();
        }
    }

    /// Returns a generic handler that invokes the given handler with the view and context associated with the given view handle.
    pub fn handler_for<E: 'static, Callback: Fn(&mut E, &mut Window, &mut Context<E>) + 'static>(
        &self,
        entity: &Entity<E>,
        f: Callback,
    ) -> impl Fn(&mut Window, &mut App) + 'static {
        let entity = entity.downgrade();
        move |window: &mut Window, cx: &mut App| {
            entity.update(cx, |entity, cx| f(entity, window, cx)).ok();
        }
    }

    /// Register a callback that can interrupt the closing of the current window based the returned boolean.
    /// If the callback returns false, the window won't be closed.
    pub fn on_window_should_close(
        &self,
        cx: &App,
        f: impl Fn(&mut Window, &mut App) -> bool + 'static,
    ) {
        let mut cx = self.to_async(cx);
        self.platform_window.on_should_close(Box::new(move || {
            cx.update(|window, cx| f(window, cx)).unwrap_or(true)
        }))
    }

    /// Register an action listener on this node for the next frame. The type of action
    /// is determined by the first parameter of the given listener. When the next frame is rendered
    /// the listener will be cleared.
    ///
    /// This is a fairly low-level method, so prefer using action handlers on elements unless you have
    /// a specific need to register a listener yourself.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn on_action(
        &mut self,
        action_type: TypeId,
        listener: impl Fn(&dyn Any, DispatchPhase, &mut Window, &mut App) + 'static,
    ) {
        self.invalidator.debug_assert_paint();

        self.next_frame
            .dispatch_tree
            .on_action(action_type, Rc::new(listener));
    }

    /// Register an action listener that reports an explicit terminal outcome during the bubble phase.
    pub fn on_action_with_outcome(
        &mut self,
        action_type: TypeId,
        listener: impl Fn(&dyn Any, &mut Window, &mut App) -> ActionDispatchOutcome + 'static,
    ) {
        self.invalidator.debug_assert_paint();
        self.next_frame
            .dispatch_tree
            .on_action_with_outcome(action_type, Rc::new(listener));
    }

    /// Register a capturing action listener on this node for the next frame if the condition is true.
    /// The type of action is determined by the first parameter of the given listener. When the next
    /// frame is rendered the listener will be cleared.
    ///
    /// This is a fairly low-level method, so prefer using action handlers on elements unless you have
    /// a specific need to register a listener yourself.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn on_action_when(
        &mut self,
        condition: bool,
        action_type: TypeId,
        listener: impl Fn(&dyn Any, DispatchPhase, &mut Window, &mut App) + 'static,
    ) {
        self.invalidator.debug_assert_paint();

        if condition {
            self.next_frame
                .dispatch_tree
                .on_action(action_type, Rc::new(listener));
        }
    }

    /// Read information about the GPU backing this window.
    /// Currently returns None on Mac and Windows.
    pub fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.platform_window.gpu_specs()
    }

    /// Read shader-effect capabilities for the renderer backing this window.
    pub fn effect_capabilities(&self) -> EffectCapabilities {
        self.platform_window.effect_capabilities()
    }

    /// Perform titlebar double-click action.
    /// This is macOS specific.
    pub fn titlebar_double_click(&self) {
        self.platform_window.titlebar_double_click();
    }

    /// Gets the window's title at the platform level.
    /// This is macOS specific.
    pub fn window_title(&self) -> String {
        self.platform_window.get_title()
    }

    /// Returns a list of all tabbed windows and their titles.
    /// This is macOS specific.
    pub fn tabbed_windows(&self) -> Option<Vec<SystemWindowTab>> {
        self.platform_window.tabbed_windows()
    }

    /// Returns the tab bar visibility.
    /// This is macOS specific.
    pub fn tab_bar_visible(&self) -> bool {
        self.platform_window.tab_bar_visible()
    }

    /// Merges all open windows into a single tabbed window.
    /// This is macOS specific.
    pub fn merge_all_windows(&self) {
        self.platform_window.merge_all_windows()
    }

    /// Moves the tab to a new containing window.
    /// This is macOS specific.
    pub fn move_tab_to_new_window(&self) {
        self.platform_window.move_tab_to_new_window()
    }

    /// Shows or hides the window tab overview.
    /// This is macOS specific.
    pub fn toggle_window_tab_overview(&self) {
        self.platform_window.toggle_window_tab_overview()
    }

    /// Sets the tabbing identifier for the window.
    /// This is macOS specific.
    pub fn set_tabbing_identifier(&self, tabbing_identifier: Option<String>) {
        self.platform_window
            .set_tabbing_identifier(tabbing_identifier)
    }

    /// Request the OS to play an alert sound. On some platforms this is associated
    /// with the window, for others it's just a simple global function call.
    pub fn play_system_bell(&self) {
        self.platform_window.play_system_bell()
    }

    /// Toggles the inspector mode on this window.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub fn toggle_inspector(&mut self, cx: &mut App) {
        self.inspector = match self.inspector {
            None => Some(cx.new(|_| Inspector::new())),
            Some(_) => None,
        };
        self.refresh();
    }

    /// Returns true if the window is in inspector mode.
    pub fn is_inspector_picking(&self, _cx: &App) -> bool {
        #[cfg(any(feature = "inspector", debug_assertions))]
        {
            if let Some(inspector) = &self.inspector {
                return inspector.read(_cx).is_picking();
            }
        }
        false
    }

    /// Executes the provided function with mutable access to an inspector state.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub fn with_inspector_state<T: 'static, R>(
        &mut self,
        _inspector_id: Option<&crate::InspectorElementId>,
        cx: &mut App,
        f: impl FnOnce(&mut Option<T>, &mut Self) -> R,
    ) -> R {
        if let Some(inspector_id) = _inspector_id
            && let Some(inspector) = &self.inspector
        {
            let inspector = inspector.clone();
            let active_element_id = inspector.read(cx).active_element_id();
            if Some(inspector_id) == active_element_id {
                return inspector.update(cx, |inspector, _cx| {
                    inspector.with_active_element_state(self, f)
                });
            }
        }
        f(&mut None, self)
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) fn build_inspector_element_id(
        &mut self,
        path: crate::InspectorElementPath,
    ) -> crate::InspectorElementId {
        self.invalidator.debug_assert_paint_or_prepaint();
        let path = Rc::new(path);
        let next_instance_id = self
            .next_frame
            .next_inspector_instance_ids
            .entry(path.clone())
            .or_insert(0);
        let instance_id = *next_instance_id;
        *next_instance_id += 1;
        crate::InspectorElementId { path, instance_id }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn prepaint_inspector(&mut self, inspector_width: Pixels, cx: &mut App) -> Option<AnyElement> {
        if let Some(inspector) = self.inspector.take() {
            let mut inspector_element = AnyView::from(inspector.clone()).into_any_element();
            inspector_element.prepaint_as_root(
                point(self.viewport_size.width - inspector_width, px(0.0)),
                size(inspector_width, self.viewport_size.height).into(),
                self,
                cx,
            );
            self.inspector = Some(inspector);
            Some(inspector_element)
        } else {
            None
        }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn paint_inspector(&mut self, mut inspector_element: Option<AnyElement>, cx: &mut App) {
        if let Some(mut inspector_element) = inspector_element {
            inspector_element.paint(self, cx);
        };
    }

    /// Registers a hitbox that can be used for inspector picking mode, allowing users to select and
    /// inspect UI elements by clicking on them.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub fn insert_inspector_hitbox(
        &mut self,
        hitbox_id: HitboxId,
        inspector_id: Option<&crate::InspectorElementId>,
        cx: &App,
    ) {
        self.invalidator.debug_assert_paint_or_prepaint();
        if !self.is_inspector_picking(cx) {
            return;
        }
        if let Some(inspector_id) = inspector_id {
            self.next_frame
                .inspector_hitboxes
                .insert(hitbox_id, inspector_id.clone());
        }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn paint_inspector_hitbox(&mut self, cx: &App) {
        if let Some(inspector) = self.inspector.as_ref() {
            let inspector = inspector.read(cx);
            if let Some((hitbox_id, _)) = self.hovered_inspector_hitbox(inspector, &self.next_frame)
                && let Some(hitbox) = self
                    .next_frame
                    .hitboxes
                    .iter()
                    .find(|hitbox| hitbox.id == hitbox_id)
            {
                self.paint_quad(crate::fill(hitbox.bounds, crate::rgba(0x61afef4d)));
            }
        }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn handle_inspector_mouse_event(&mut self, event: &dyn Any, cx: &mut App) {
        let Some(inspector) = self.inspector.clone() else {
            return;
        };
        if event.downcast_ref::<PointerEvent>().is_some() {
            inspector.update(cx, |inspector, _cx| {
                if let Some((_, inspector_id)) =
                    self.hovered_inspector_hitbox(inspector, &self.rendered_frame)
                {
                    inspector.hover(inspector_id, self);
                }
            });
        } else if event.downcast_ref::<crate::PointerEvent>().is_some() {
            inspector.update(cx, |inspector, _cx| {
                if let Some((_, inspector_id)) =
                    self.hovered_inspector_hitbox(inspector, &self.rendered_frame)
                {
                    inspector.select(inspector_id, self);
                }
            });
        } else if let Some(event) = event.downcast_ref::<crate::ScrollWheelEvent>() {
            // This should be kept in sync with SCROLL_LINES in x11 platform.
            const SCROLL_LINES: f32 = 3.0;
            const SCROLL_PIXELS_PER_LAYER: f32 = 36.0;
            let delta_y = event
                .delta
                .pixel_delta(px(SCROLL_PIXELS_PER_LAYER / SCROLL_LINES))
                .y;
            if let Some(inspector) = self.inspector.clone() {
                inspector.update(cx, |inspector, _cx| {
                    if let Some(depth) = inspector.pick_depth.as_mut() {
                        *depth += f32::from(delta_y) / SCROLL_PIXELS_PER_LAYER;
                        let max_depth = self.mouse_hit_test.ids.len() as f32 - 0.5;
                        if *depth < 0.0 {
                            *depth = 0.0;
                        } else if *depth > max_depth {
                            *depth = max_depth;
                        }
                        if let Some((_, inspector_id)) =
                            self.hovered_inspector_hitbox(inspector, &self.rendered_frame)
                        {
                            inspector.set_active_element_id(inspector_id, self);
                        }
                    }
                });
            }
        }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn hovered_inspector_hitbox(
        &self,
        inspector: &Inspector,
        frame: &WindowFrame,
    ) -> Option<(HitboxId, crate::InspectorElementId)> {
        if let Some(pick_depth) = inspector.pick_depth {
            let depth = (pick_depth as i64).try_into().unwrap_or(0);
            let max_skipped = self.mouse_hit_test.ids.len().saturating_sub(1);
            let skip_count = (depth as usize).min(max_skipped);
            for hitbox_id in self.mouse_hit_test.ids.iter().skip(skip_count) {
                if let Some(inspector_id) = frame.inspector_hitboxes.get(hitbox_id) {
                    return Some((*hitbox_id, inspector_id.clone()));
                }
            }
        }
        None
    }

    /// For testing: set the current modifier keys state.
    /// This does not generate any events.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// For testing: simulate a mouse move event to the given position.
    /// This dispatches the event through the normal event handling path,
    /// which will trigger hover states and tooltips.
    #[cfg(any(test, feature = "test-support"))]
    pub fn simulate_mouse_move(&mut self, position: Point<Pixels>, cx: &mut App) {
        let event =
            PlatformInput::Pointer(PointerEvent::mouse_move(position, None, self.modifiers));
        let _ = self.dispatch_event(event, cx);
    }
}

// #[derive(Clone, Copy, Eq, PartialEq, Hash)]
slotmap::new_key_type! {
    /// A unique identifier for a window.
    pub struct WindowId;
}

impl WindowId {
    /// Converts this window ID to a `u64`.
    pub fn as_u64(&self) -> u64 {
        self.0.as_ffi()
    }
}

impl From<u64> for WindowId {
    fn from(value: u64) -> Self {
        WindowId(slotmap::KeyData::from_ffi(value))
    }
}

/// A handle to a window with a specific root view type.
/// Note that this does not keep the window alive on its own.
#[derive(Deref, DerefMut)]
pub struct WindowHandle<V> {
    #[deref]
    #[deref_mut]
    pub(crate) any_handle: AnyWindowHandle,
    state_type: PhantomData<fn(V) -> V>,
}

impl<V> Debug for WindowHandle<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowHandle")
            .field("any_handle", &self.any_handle.id.as_u64())
            .finish()
    }
}

impl<V: 'static + Render> WindowHandle<V> {
    /// Creates a new handle from a window ID.
    /// This does not check if the root type of the window is `V`.
    pub fn new(id: WindowId) -> Self {
        WindowHandle {
            any_handle: AnyWindowHandle {
                id,
                state_type: TypeId::of::<V>(),
            },
            state_type: PhantomData,
        }
    }

    /// Get the root view out of this window.
    ///
    /// This will fail if the window is closed or if the root view's type does not match `V`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn root<C>(&self, cx: &mut C) -> Result<Entity<V>>
    where
        C: AppContext,
    {
        cx.update_window(self.any_handle, |root_view, _, _| {
            root_view
                .downcast::<V>()
                .map_err(|_| anyhow!("the type of the window's root view has changed"))
        })?
    }

    /// Updates the root view of this window.
    ///
    /// This will fail if the window has been closed or if the root view's type does not match
    pub fn update<C, R>(
        &self,
        cx: &mut C,
        update: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> R,
    ) -> Result<R>
    where
        C: AppContext,
    {
        cx.update_window(self.any_handle, |root_view, window, cx| {
            let view = root_view
                .downcast::<V>()
                .map_err(|_| anyhow!("the type of the window's root view has changed"))?;

            Ok(view.update(cx, |view, cx| update(view, window, cx)))
        })?
    }

    /// Read the root view out of this window.
    ///
    /// This will fail if the window is closed or if the root view's type does not match `V`.
    pub fn read<'a>(&self, cx: &'a App) -> Result<&'a V> {
        let x = cx
            .windows
            .get(self.id)
            .and_then(|window| {
                window
                    .as_deref()
                    .and_then(|window| window.root.clone())
                    .map(|root_view| root_view.downcast::<V>())
            })
            .context("window not found")?
            .map_err(|_| anyhow!("the type of the window's root view has changed"))?;

        Ok(x.read(cx))
    }

    /// Read the root view out of this window, with a callback
    ///
    /// This will fail if the window is closed or if the root view's type does not match `V`.
    pub fn read_with<C, R>(&self, cx: &C, read_with: impl FnOnce(&V, &App) -> R) -> Result<R>
    where
        C: AppContext,
    {
        cx.read_window(self, |root_view, cx| read_with(root_view.read(cx), cx))
    }

    /// Read the root view pointer off of this window.
    ///
    /// This will fail if the window is closed or if the root view's type does not match `V`.
    pub fn entity<C>(&self, cx: &C) -> Result<Entity<V>>
    where
        C: AppContext,
    {
        cx.read_window(self, |root_view, _cx| root_view)
    }

    /// Check if this window is 'active'.
    ///
    /// Will return `None` if the window is closed or currently
    /// borrowed.
    pub fn is_active(&self, cx: &mut App) -> Option<bool> {
        cx.update_window(self.any_handle, |_, window, _| window.is_window_active())
            .ok()
    }
}

impl<V> Copy for WindowHandle<V> {}

impl<V> Clone for WindowHandle<V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<V> PartialEq for WindowHandle<V> {
    fn eq(&self, other: &Self) -> bool {
        self.any_handle == other.any_handle
    }
}

impl<V> Eq for WindowHandle<V> {}

impl<V> Hash for WindowHandle<V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.any_handle.hash(state);
    }
}

impl<V: 'static> From<WindowHandle<V>> for AnyWindowHandle {
    fn from(val: WindowHandle<V>) -> Self {
        val.any_handle
    }
}

/// A handle to a window with any root view type, which can be downcast to a window with a specific root view type.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct AnyWindowHandle {
    pub(crate) id: WindowId,
    state_type: TypeId,
}

impl AnyWindowHandle {
    /// Get the ID of this window.
    pub fn window_id(&self) -> WindowId {
        self.id
    }

    /// Attempt to convert this handle to a window handle with a specific root view type.
    /// If the types do not match, this will return `None`.
    pub fn downcast<T: 'static>(&self) -> Option<WindowHandle<T>> {
        if TypeId::of::<T>() == self.state_type {
            Some(WindowHandle {
                any_handle: *self,
                state_type: PhantomData,
            })
        } else {
            None
        }
    }

    /// Updates the state of the root view of this window.
    ///
    /// This will fail if the window has been closed.
    pub fn update<C, R>(
        self,
        cx: &mut C,
        update: impl FnOnce(AnyView, &mut Window, &mut App) -> R,
    ) -> Result<R>
    where
        C: AppContext,
    {
        cx.update_window(self, update)
    }

    /// Read the state of the root view of this window.
    ///
    /// This will fail if the window has been closed.
    pub fn read<T, C, R>(self, cx: &C, read: impl FnOnce(Entity<T>, &App) -> R) -> Result<R>
    where
        C: AppContext,
        T: 'static,
    {
        let view = self
            .downcast::<T>()
            .context("the type of the window's root view has changed")?;

        cx.read_window(&view, read)
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, HandleError> {
        self.platform_window.window_handle()
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::DisplayHandle<'_>, HandleError> {
        self.platform_window.display_handle()
    }
}

/// An identifier for an [`Element`].
///
/// Can be constructed with a string, a number, or both, as well
/// as other internal representations.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ElementId {
    /// The ID of a View element
    View(EntityId),
    /// An integer ID.
    Integer(u64),
    /// A string based ID.
    Name(SharedString),
    /// A UUID.
    Uuid(Uuid),
    /// An ID that's equated with a focus handle.
    FocusHandle(FocusId),
    /// A combination of a name and an integer.
    NamedInteger(SharedString, u64),
    /// A path.
    Path(Arc<std::path::Path>),
    /// A code location.
    CodeLocation(core::panic::Location<'static>),
    /// A labeled child of an element.
    NamedChild(Arc<ElementId>, SharedString),
    /// A byte array ID (used for text-anchors)
    OpaqueId([u8; 20]),
}

impl ElementId {
    /// Constructs an `ElementId::NamedInteger` from a name and `usize`.
    pub fn named_usize(name: impl Into<SharedString>, integer: usize) -> ElementId {
        Self::NamedInteger(name.into(), integer as u64)
    }
}

impl Display for ElementId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElementId::View(entity_id) => write!(f, "view-{}", entity_id)?,
            ElementId::Integer(ix) => write!(f, "{}", ix)?,
            ElementId::Name(name) => write!(f, "{}", name)?,
            ElementId::FocusHandle(_) => write!(f, "FocusHandle")?,
            ElementId::NamedInteger(s, i) => write!(f, "{}-{}", s, i)?,
            ElementId::Uuid(uuid) => write!(f, "{}", uuid)?,
            ElementId::Path(path) => write!(f, "{}", path.display())?,
            ElementId::CodeLocation(location) => write!(f, "{}", location)?,
            ElementId::NamedChild(id, name) => write!(f, "{}-{}", id, name)?,
            ElementId::OpaqueId(opaque_id) => write!(f, "{:x?}", opaque_id)?,
        }

        Ok(())
    }
}

impl TryInto<SharedString> for ElementId {
    type Error = anyhow::Error;

    fn try_into(self) -> anyhow::Result<SharedString> {
        if let ElementId::Name(name) = self {
            Ok(name)
        } else {
            anyhow::bail!("element id is not string")
        }
    }
}

impl From<usize> for ElementId {
    fn from(id: usize) -> Self {
        ElementId::Integer(id as u64)
    }
}

impl From<i32> for ElementId {
    fn from(id: i32) -> Self {
        Self::Integer(id as u64)
    }
}

impl From<SharedString> for ElementId {
    fn from(name: SharedString) -> Self {
        ElementId::Name(name)
    }
}

impl From<String> for ElementId {
    fn from(name: String) -> Self {
        ElementId::Name(name.into())
    }
}

impl From<Arc<str>> for ElementId {
    fn from(name: Arc<str>) -> Self {
        ElementId::Name(name.into())
    }
}

impl From<Arc<std::path::Path>> for ElementId {
    fn from(path: Arc<std::path::Path>) -> Self {
        ElementId::Path(path)
    }
}

impl From<&'static str> for ElementId {
    fn from(name: &'static str) -> Self {
        ElementId::Name(SharedString::new_static(name))
    }
}

impl<'a> From<&'a FocusHandle> for ElementId {
    fn from(handle: &'a FocusHandle) -> Self {
        ElementId::FocusHandle(handle.id)
    }
}

impl From<(&'static str, EntityId)> for ElementId {
    fn from((name, id): (&'static str, EntityId)) -> Self {
        ElementId::NamedInteger(SharedString::new_static(name), id.as_u64())
    }
}

impl From<(&'static str, usize)> for ElementId {
    fn from((name, id): (&'static str, usize)) -> Self {
        ElementId::NamedInteger(SharedString::new_static(name), id as u64)
    }
}

impl From<(SharedString, usize)> for ElementId {
    fn from((name, id): (SharedString, usize)) -> Self {
        ElementId::NamedInteger(name, id as u64)
    }
}

impl From<(&'static str, u64)> for ElementId {
    fn from((name, id): (&'static str, u64)) -> Self {
        ElementId::NamedInteger(SharedString::new_static(name), id)
    }
}

impl From<Uuid> for ElementId {
    fn from(value: Uuid) -> Self {
        Self::Uuid(value)
    }
}

impl From<(&'static str, u32)> for ElementId {
    fn from((name, id): (&'static str, u32)) -> Self {
        ElementId::NamedInteger(SharedString::new_static(name), u64::from(id))
    }
}

impl<T: Into<SharedString>> From<(ElementId, T)> for ElementId {
    fn from((id, name): (ElementId, T)) -> Self {
        ElementId::NamedChild(Arc::new(id), name.into())
    }
}

impl From<&'static core::panic::Location<'static>> for ElementId {
    fn from(location: &'static core::panic::Location<'static>) -> Self {
        ElementId::CodeLocation(*location)
    }
}

impl From<[u8; 20]> for ElementId {
    fn from(opaque_id: [u8; 20]) -> Self {
        ElementId::OpaqueId(opaque_id)
    }
}

#[cfg(test)]
mod positioning_tests {
    use super::*;

    #[test]
    fn top_layer_hitbox_id_sorts_above_positioned_hitbox_id() {
        let ordinary = HitboxId(7);

        assert!(ordinary.top_layer().0 > ordinary.positioned().0);
    }

    #[test]
    fn cursor_requests_follow_hit_test_layer_order() {
        let ordinary = HitboxId(7);
        let top_layer = ordinary.top_layer();
        let requests = [
            CursorStyleRequest {
                hitbox_id: Some(top_layer),
                style: CursorStyle::Arrow,
            },
            CursorStyleRequest {
                hitbox_id: Some(ordinary),
                style: CursorStyle::IBeam,
            },
        ];
        let hit_test = HitTest {
            ids: smallvec::smallvec![top_layer, ordinary],
            hover_hitbox_count: 2,
        };

        assert_eq!(
            resolve_cursor_style(&requests, &hit_test, None),
            Some(CursorStyle::Arrow)
        );
    }

    fn scroll_context(
        scrollport_bounds: Bounds<Pixels>,
        content_bounds: Bounds<Pixels>,
        overflow: Point<Overflow>,
    ) -> ScrollContainerContext {
        ScrollContainerContext {
            scrollport_bounds,
            content_bounds,
            scroll_offset: Point::default(),
            overflow,
        }
    }

    #[test]
    fn scroll_container_context_stack_returns_nearest_matching_axis() {
        let mut stack = ScrollContainerContextStack::default();
        let outer = scroll_context(
            Bounds::new(point(px(0.), px(0.)), size(px(100.), px(100.))),
            Bounds::new(point(px(0.), px(0.)), size(px(300.), px(100.))),
            point(Overflow::Scroll, Overflow::Visible),
        );
        let inner = scroll_context(
            Bounds::new(point(px(10.), px(10.)), size(px(80.), px(80.))),
            Bounds::new(point(px(10.), px(10.)), size(px(80.), px(240.))),
            point(Overflow::Visible, Overflow::Scroll),
        );

        assert_eq!(None, stack.nearest(ScrollAxis::Horizontal));
        assert_eq!(None, stack.nearest(ScrollAxis::Vertical));

        stack.push(outer);
        assert_eq!(Some(outer), stack.nearest(ScrollAxis::Horizontal));
        assert_eq!(None, stack.nearest(ScrollAxis::Vertical));

        stack.push(inner);
        assert_eq!(Some(outer), stack.nearest(ScrollAxis::Horizontal));
        assert_eq!(Some(inner), stack.nearest(ScrollAxis::Vertical));

        assert_eq!(Some(inner), stack.pop());
        assert_eq!(Some(outer), stack.nearest(ScrollAxis::Horizontal));
        assert_eq!(None, stack.nearest(ScrollAxis::Vertical));

        assert_eq!(Some(outer), stack.pop());
        assert_eq!(None, stack.nearest(ScrollAxis::Horizontal));
    }

    #[test]
    fn scroll_container_context_records_geometry_offset_and_active_axes() {
        let scrollport_bounds = Bounds::new(point(px(1.), px(2.)), size(px(100.), px(120.)));
        let content_bounds = Bounds::new(point(px(1.), px(2.)), size(px(260.), px(120.)));
        let context = ScrollContainerContext {
            scrollport_bounds,
            content_bounds,
            scroll_offset: point(px(-35.), px(0.)),
            overflow: point(Overflow::Scroll, Overflow::Hidden),
        };

        assert_eq!(scrollport_bounds, context.scrollport_bounds);
        assert_eq!(content_bounds, context.content_bounds);
        assert_eq!(point(px(-35.), px(0.)), context.scroll_offset);
        assert!(context.is_active_on_axis(ScrollAxis::Horizontal));
        assert!(!context.is_active_on_axis(ScrollAxis::Vertical));
    }

    #[test]
    fn positioned_bounds_preserve_relative_and_absolute_layout_bounds() {
        let layout_bounds = Bounds::new(point(px(7.), px(11.)), size(px(90.), px(30.)));

        for position in [Position::Relative, Position::Absolute] {
            let style = Style {
                position,
                ..Default::default()
            };
            let positioned = PositionedBounds::resolve_noop(&style, layout_bounds);

            assert_eq!(layout_bounds, positioned.layout_bounds);
            assert_eq!(layout_bounds, positioned.visual_bounds);
            assert_eq!(Point::default(), positioned.delta);
        }
    }

    #[test]
    fn fixed_positioned_bounds_resolve_top_left_against_containing_block() {
        let containing_block_bounds =
            Bounds::new(point(px(20.), px(30.)), size(px(320.), px(240.)));
        let layout_bounds = Bounds::new(point(px(70.), px(80.)), size(px(90.), px(30.)));
        let mut style = Style {
            position: Position::Fixed,
            ..Default::default()
        };
        style.inset.top = px(12.).into();
        style.inset.left = px(24.).into();

        let positioned = PositionedBounds::resolve_fixed_for_containing_block(
            &style,
            layout_bounds,
            containing_block_bounds,
            px(16.),
        );

        let expected = Bounds::new(point(px(44.), px(42.)), layout_bounds.size);
        assert_eq!(layout_bounds, positioned.layout_bounds);
        assert_eq!(expected, positioned.visual_bounds);
        assert_eq!(point(px(-26.), px(-38.)), positioned.delta);
    }

    #[test]
    fn fixed_positioned_bounds_resolve_bottom_right_against_containing_block() {
        let containing_block_bounds =
            Bounds::new(point(px(20.), px(30.)), size(px(320.), px(240.)));
        let layout_bounds = Bounds::new(point(px(70.), px(80.)), size(px(90.), px(30.)));
        let mut style = Style {
            position: Position::Fixed,
            ..Default::default()
        };
        style.inset.right = px(14.).into();
        style.inset.bottom = px(18.).into();

        let positioned = PositionedBounds::resolve_fixed_for_containing_block(
            &style,
            layout_bounds,
            containing_block_bounds,
            px(16.),
        );

        let expected = Bounds::new(point(px(236.), px(222.)), layout_bounds.size);
        assert_eq!(layout_bounds, positioned.layout_bounds);
        assert_eq!(expected, positioned.visual_bounds);
        assert_eq!(point(px(166.), px(142.)), positioned.delta);
    }

    #[test]
    fn sticky_positioned_bounds_resolve_top_stuck_and_unstuck_against_scrollport() {
        let scroll_context = scroll_context(
            Bounds::new(Point::default(), size(px(100.), px(100.))),
            Bounds::new(point(px(0.), px(-60.)), size(px(100.), px(300.))),
            point(Overflow::Hidden, Overflow::Scroll),
        );
        let mut style = Style {
            position: Position::Sticky,
            ..Default::default()
        };
        style.inset.top = px(8.).into();

        let unstuck_bounds = Bounds::new(point(px(12.), px(40.)), size(px(30.), px(20.)));
        let unstuck = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            unstuck_bounds,
            None,
            Some(scroll_context),
            px(16.),
        );

        assert_eq!(unstuck_bounds, unstuck.layout_bounds);
        assert_eq!(unstuck_bounds, unstuck.visual_bounds);
        assert_eq!(Point::default(), unstuck.delta);

        let stuck_bounds = Bounds::new(point(px(12.), px(-20.)), size(px(30.), px(20.)));
        let stuck = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            stuck_bounds,
            None,
            Some(scroll_context),
            px(16.),
        );

        assert_eq!(stuck_bounds, stuck.layout_bounds);
        assert_eq!(
            Bounds::new(point(px(12.), px(8.)), stuck_bounds.size),
            stuck.visual_bounds
        );
        assert_eq!(point(px(0.), px(28.)), stuck.delta);
    }

    #[test]
    fn sticky_positioned_bounds_report_axis_state() {
        let scroll_context = scroll_context(
            Bounds::new(Point::default(), size(px(100.), px(100.))),
            Bounds::new(point(px(0.), px(-60.)), size(px(100.), px(300.))),
            point(Overflow::Hidden, Overflow::Scroll),
        );
        let mut style = Style {
            position: Position::Sticky,
            ..Default::default()
        };
        style.inset.top = px(8.).into();

        let layout_bounds = Bounds::new(point(px(12.), px(-20.)), size(px(30.), px(20.)));
        let (positioned, sticky_state) =
            PositionedBounds::resolve_sticky_for_scroll_contexts_with_state(
                &style,
                layout_bounds,
                None,
                Some(scroll_context),
                px(16.),
            );

        assert_eq!(positioned.visual_bounds, sticky_state.visual_bounds);
        assert_eq!(StickyAxisState::Unstuck, sticky_state.x);
        assert_eq!(StickyAxisState::StuckToStart, sticky_state.y);
        assert!(sticky_state.is_stuck_y());
        assert!(sticky_state.is_stuck());
    }

    #[test]
    fn keyed_sticky_same_run_reserves_each_predecessor_extent() {
        let relation = StickyRelationIndex::from_candidates([
            (0, StickyStacking::StackId("section".into())),
            (1, StickyStacking::StackId("section".into())),
        ]);

        assert_eq!(
            relation.active_offsets(1, &[px(0.), px(0.)], &[px(20.), px(20.)]),
            vec![(0, px(0.)), (1, px(20.))]
        );
    }

    #[test]
    fn keyed_sticky_finds_the_next_contiguous_run_root() {
        let relation = StickyRelationIndex::from_candidates([
            (0, StickyStacking::StackId("a".into())),
            (8, StickyStacking::StackId("a".into())),
            (16, StickyStacking::StackId("b".into())),
            (24, StickyStacking::StackId("b".into())),
            (32, StickyStacking::StackId("a".into())),
        ]);

        assert_eq!(relation.next_run_root(0), Some(2));
        assert_eq!(relation.next_run_root(1), Some(2));
        assert_eq!(relation.next_run_root(2), Some(4));
        assert_eq!(relation.next_run_root(4), None);
    }

    #[test]
    fn keyed_sticky_push_offset_moves_the_outgoing_run_with_the_incoming_root() {
        assert_eq!(keyed_sticky_push_offset(px(40.), px(52.)), px(0.));
        assert_eq!(keyed_sticky_push_offset(px(40.), px(30.)), px(-10.));
        assert_eq!(keyed_sticky_push_offset(px(40.), px(12.)), px(-28.));
        assert_eq!(keyed_sticky_push_offset(px(40.), px(0.)), px(-40.));
    }

    #[test]
    fn keyed_sticky_a_b_a_runs_do_not_resurrect_prior_members() {
        let relation = StickyRelationIndex::from_candidates([
            (0, StickyStacking::StackId("a".into())),
            (1, StickyStacking::StackId("a".into())),
            (2, StickyStacking::StackId("b".into())),
            (3, StickyStacking::StackId("a".into())),
        ]);

        assert_eq!(relation.active_chain(1), vec![0, 1]);
        assert_eq!(relation.active_chain(2), vec![2]);
        assert_eq!(relation.active_chain(3), vec![3]);
        assert_eq!(
            relation.source_at_or_before(3),
            Some(3),
            "candidate lookup selects the current A run"
        );
    }

    #[test]
    fn keyed_sticky_explicit_levels_preserve_parent_replacement() {
        let relation = StickyRelationIndex::from_candidates([
            (0, StickyStacking::Level(0)),
            (1, StickyStacking::Level(1)),
            (2, StickyStacking::Level(2)),
            (3, StickyStacking::Level(1)),
        ]);

        assert_eq!(relation.active_chain(2), vec![0, 1, 2]);
        assert_eq!(relation.active_chain(3), vec![0, 3]);
    }

    #[test]
    fn sticky_relation_10000_uses_linear_records_and_active_depth_visits() {
        let relation = StickyRelationIndex::from_candidates(
            (0..10_000).map(|index| (index, StickyStacking::StackId(ElementId::from("long-run")))),
        );

        assert_eq!(relation.record_count(), 10_000);
        assert_eq!(relation.source_at_or_before(9_999), Some(9_999));
        let (chain, visits) = relation.active_chain_with_visits(9_999);
        assert_eq!(chain.len(), 10_000);
        assert_eq!(visits, chain.len());

        let alternating = StickyRelationIndex::from_candidates(
            (0..10_000).map(|index| (index, StickyStacking::StackId(ElementId::from(index % 2)))),
        );
        let (chain, visits) = alternating.active_chain_with_visits(9_999);
        assert_eq!(chain, vec![9_999]);
        assert_eq!(visits, 1);
    }

    #[test]
    fn sticky_positioned_bounds_resolve_left_stuck_and_unstuck_against_scrollport() {
        let scroll_context = scroll_context(
            Bounds::new(Point::default(), size(px(100.), px(100.))),
            Bounds::new(point(px(-50.), px(0.)), size(px(300.), px(100.))),
            point(Overflow::Scroll, Overflow::Hidden),
        );
        let mut style = Style {
            position: Position::Sticky,
            ..Default::default()
        };
        style.inset.left = px(6.).into();

        let unstuck_bounds = Bounds::new(point(px(30.), px(14.)), size(px(24.), px(18.)));
        let unstuck = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            unstuck_bounds,
            Some(scroll_context),
            None,
            px(16.),
        );

        assert_eq!(unstuck_bounds, unstuck.layout_bounds);
        assert_eq!(unstuck_bounds, unstuck.visual_bounds);
        assert_eq!(Point::default(), unstuck.delta);

        let stuck_bounds = Bounds::new(point(px(-18.), px(14.)), size(px(24.), px(18.)));
        let stuck = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            stuck_bounds,
            Some(scroll_context),
            None,
            px(16.),
        );

        assert_eq!(stuck_bounds, stuck.layout_bounds);
        assert_eq!(
            Bounds::new(point(px(6.), px(14.)), stuck_bounds.size),
            stuck.visual_bounds
        );
        assert_eq!(point(px(24.), px(0.)), stuck.delta);
    }

    #[test]
    fn sticky_positioned_bounds_resolve_bottom_stuck_and_unstuck_against_scrollport() {
        let scroll_context = scroll_context(
            Bounds::new(Point::default(), size(px(100.), px(100.))),
            Bounds::new(Point::default(), size(px(100.), px(300.))),
            point(Overflow::Hidden, Overflow::Scroll),
        );
        let mut style = Style {
            position: Position::Sticky,
            ..Default::default()
        };
        style.inset.bottom = px(8.).into();

        let unstuck_bounds = Bounds::new(point(px(12.), px(40.)), size(px(30.), px(20.)));
        let unstuck = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            unstuck_bounds,
            None,
            Some(scroll_context),
            px(16.),
        );

        assert_eq!(unstuck_bounds, unstuck.visual_bounds);
        assert_eq!(Point::default(), unstuck.delta);

        let stuck_bounds = Bounds::new(point(px(12.), px(120.)), size(px(30.), px(20.)));
        let stuck = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            stuck_bounds,
            None,
            Some(scroll_context),
            px(16.),
        );

        assert_eq!(
            Bounds::new(point(px(12.), px(72.)), stuck_bounds.size),
            stuck.visual_bounds
        );
        assert_eq!(point(px(0.), px(-48.)), stuck.delta);
    }

    #[test]
    fn sticky_positioned_bounds_resolve_right_stuck_and_unstuck_against_scrollport() {
        let scroll_context = scroll_context(
            Bounds::new(Point::default(), size(px(100.), px(100.))),
            Bounds::new(Point::default(), size(px(300.), px(100.))),
            point(Overflow::Scroll, Overflow::Hidden),
        );
        let mut style = Style {
            position: Position::Sticky,
            ..Default::default()
        };
        style.inset.right = px(6.).into();

        let unstuck_bounds = Bounds::new(point(px(30.), px(14.)), size(px(24.), px(18.)));
        let unstuck = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            unstuck_bounds,
            Some(scroll_context),
            None,
            px(16.),
        );

        assert_eq!(unstuck_bounds, unstuck.visual_bounds);
        assert_eq!(Point::default(), unstuck.delta);

        let stuck_bounds = Bounds::new(point(px(120.), px(14.)), size(px(24.), px(18.)));
        let stuck = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            stuck_bounds,
            Some(scroll_context),
            None,
            px(16.),
        );

        assert_eq!(
            Bounds::new(point(px(70.), px(14.)), stuck_bounds.size),
            stuck.visual_bounds
        );
        assert_eq!(point(px(-50.), px(0.)), stuck.delta);
    }

    #[test]
    fn sticky_positioned_bounds_resolve_opposing_vertical_insets() {
        let scroll_context = scroll_context(
            Bounds::new(Point::default(), size(px(100.), px(100.))),
            Bounds::new(Point::default(), size(px(100.), px(300.))),
            point(Overflow::Hidden, Overflow::Scroll),
        );
        let mut style = Style {
            position: Position::Sticky,
            ..Default::default()
        };
        style.inset.top = px(10.).into();
        style.inset.bottom = px(10.).into();

        let top_stuck_bounds = Bounds::new(point(px(0.), px(-20.)), size(px(30.), px(20.)));
        let top_stuck = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            top_stuck_bounds,
            None,
            Some(scroll_context),
            px(16.),
        );

        assert_eq!(
            Bounds::new(point(px(0.), px(10.)), top_stuck_bounds.size),
            top_stuck.visual_bounds
        );

        let bottom_stuck_bounds = Bounds::new(point(px(0.), px(90.)), size(px(30.), px(20.)));
        let bottom_stuck = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            bottom_stuck_bounds,
            None,
            Some(scroll_context),
            px(16.),
        );

        assert_eq!(
            Bounds::new(point(px(0.), px(70.)), bottom_stuck_bounds.size),
            bottom_stuck.visual_bounds
        );
    }

    #[test]
    fn sticky_positioned_bounds_release_at_scroll_content_end() {
        let scroll_context = scroll_context(
            Bounds::new(Point::default(), size(px(100.), px(100.))),
            Bounds::new(point(px(0.), px(-210.)), size(px(100.), px(220.))),
            point(Overflow::Hidden, Overflow::Scroll),
        );
        let mut style = Style {
            position: Position::Sticky,
            ..Default::default()
        };
        style.inset.top = px(0.).into();

        let layout_bounds = Bounds::new(point(px(0.), px(-210.)), size(px(30.), px(20.)));
        let released = PositionedBounds::resolve_sticky_for_scroll_contexts(
            &style,
            layout_bounds,
            None,
            Some(scroll_context),
            px(16.),
        );

        assert_eq!(
            Bounds::new(point(px(0.), px(-10.)), layout_bounds.size),
            released.visual_bounds
        );
        assert_eq!(point(px(0.), px(200.)), released.delta);

        let (_, sticky_state) = PositionedBounds::resolve_sticky_for_scroll_contexts_with_state(
            &style,
            layout_bounds,
            None,
            Some(scroll_context),
            px(16.),
        );
        assert_eq!(StickyAxisState::ReleasedAtEnd, sticky_state.y);
        assert!(!sticky_state.is_stuck_y());
        assert!(sticky_state.is_released());
    }
}

/// A rectangle to be rendered in the window at the given position and size.
/// Passed as an argument [`Window::paint_quad`].
#[derive(Clone)]
pub struct PaintQuad {
    /// The bounds of the quad within the window.
    pub bounds: Bounds<Pixels>,
    /// The radii of the quad's corners.
    pub corner_radii: Corners<Pixels>,
    /// The background color of the quad.
    pub background: Background,
    /// The widths of the quad's borders.
    pub border_widths: Edges<Pixels>,
    /// The paint of the quad's borders.
    pub border_color: Background,
    /// The style of the quad's borders.
    pub border_style: BorderStyle,
}

impl PaintQuad {
    /// Sets the corner radii of the quad.
    pub fn corner_radii(self, corner_radii: impl Into<Corners<Pixels>>) -> Self {
        PaintQuad {
            corner_radii: corner_radii.into(),
            ..self
        }
    }

    /// Sets the border widths of the quad.
    pub fn border_widths(self, border_widths: impl Into<Edges<Pixels>>) -> Self {
        PaintQuad {
            border_widths: border_widths.into(),
            ..self
        }
    }

    /// Sets the border paint of the quad.
    pub fn border_color(self, border_color: impl Into<Background>) -> Self {
        PaintQuad {
            border_color: border_color.into(),
            ..self
        }
    }

    /// Sets the background color of the quad.
    pub fn background(self, background: impl Into<Background>) -> Self {
        PaintQuad {
            background: background.into(),
            ..self
        }
    }
}

/// Creates a quad with the given parameters.
pub fn quad(
    bounds: Bounds<Pixels>,
    corner_radii: impl Into<Corners<Pixels>>,
    background: impl Into<Background>,
    border_widths: impl Into<Edges<Pixels>>,
    border_color: impl Into<Background>,
    border_style: BorderStyle,
) -> PaintQuad {
    PaintQuad {
        bounds,
        corner_radii: corner_radii.into(),
        background: background.into(),
        border_widths: border_widths.into(),
        border_color: border_color.into(),
        border_style,
    }
}

/// Creates a filled quad with the given bounds and background color.
pub fn fill(bounds: impl Into<Bounds<Pixels>>, background: impl Into<Background>) -> PaintQuad {
    PaintQuad {
        bounds: bounds.into(),
        corner_radii: (0.).into(),
        background: background.into(),
        border_widths: (0.).into(),
        border_color: transparent_black().into(),
        border_style: BorderStyle::default(),
    }
}

/// Creates a rectangle outline with the given bounds, border color, and a 1px border width
pub fn outline(
    bounds: impl Into<Bounds<Pixels>>,
    border_color: impl Into<Color>,
    border_style: BorderStyle,
) -> PaintQuad {
    PaintQuad {
        bounds: bounds.into(),
        corner_radii: (0.).into(),
        background: transparent_black().into(),
        border_widths: (1.).into(),
        border_color: border_color.into().into(),
        border_style,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Empty, MAX_ACTIVE_SCREEN_EFFECTS, ScreenEffectSnapshotId, ScreenEffectTarget,
        TestAppContext, point, screen, size,
    };

    #[test]
    fn content_mask_scale_preserves_corner_radii() {
        let bounds = Bounds::new(point(px(1.), px(2.)), size(px(10.), px(20.)));
        let mask = ContentMask {
            bounds,
            corner_radii: Corners::all(px(3.)),
        };

        let scaled = mask.scale(2.);

        assert_eq!(scaled.bounds, bounds.scale(2.));
        assert_eq!(scaled.corner_radii, Corners::all(ScaledPixels(6.)));
    }

    #[test]
    fn rectangular_content_mask_is_zero_radius() {
        let bounds = Bounds::new(point(px(1.), px(2.)), size(px(10.), px(20.)));

        assert_eq!(
            ContentMask::rectangular(bounds),
            ContentMask::rounded(bounds, Corners::default())
        );
    }

    #[test]
    fn content_mask_intersection_preserves_nested_child_radii() {
        let parent =
            ContentMask::rectangular(Bounds::new(point(px(0.), px(0.)), size(px(100.), px(100.))));
        let child_bounds = Bounds::new(point(px(20.), px(20.)), size(px(40.), px(40.)));
        let child = ContentMask::rounded(child_bounds, Corners::all(px(8.)));

        let intersection = child.intersect(&parent);

        assert_eq!(intersection.bounds, child_bounds);
        assert_eq!(intersection.corner_radii, Corners::all(px(8.)));
    }

    #[gpux::test]
    fn play_screen_effect_enqueues_caps_and_marks_window_dirty(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
        cx.run_until_parked();

        let (before_update_count, after_update_count, ids, count) = window
            .update(cx, |_, window, _| {
                let before_update_count = window.invalidator.update_count();

                for ix in 0..(MAX_ACTIVE_SCREEN_EFFECTS + 2) {
                    window.play_screen_effect(
                        screen::ripple(point(px(ix as f32), px(0.))).duration_ms(1_000),
                    );
                }

                (
                    before_update_count,
                    window.invalidator.update_count(),
                    window
                        .active_screen_effects()
                        .iter()
                        .map(|effect| effect.id().as_u64())
                        .collect::<Vec<_>>(),
                    window.active_screen_effect_count(),
                )
            })
            .unwrap();

        assert!(after_update_count > before_update_count);
        assert_eq!(count, MAX_ACTIVE_SCREEN_EFFECTS);
        assert_eq!(ids[0], 3);
        assert_eq!(ids[ids.len() - 1], (MAX_ACTIVE_SCREEN_EFFECTS + 2) as u64);
    }

    #[gpux::test]
    fn capture_screen_effect_snapshot_retains_target_until_released(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
        cx.run_until_parked();

        let snapshot_id = window
            .update(cx, |_, window, _| {
                window.capture_screen_effect_snapshot(ScreenEffectTarget::window())
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();

        let effects = window
            .update(cx, |_, window, _| window.devtools_frame_snapshot().effects)
            .unwrap();
        assert_eq!(effects.screen_effect_snapshot_count, 1);

        let released = window
            .update(cx, |_, window, _| {
                window.release_screen_effect_snapshot(snapshot_id)
            })
            .unwrap();
        assert!(released);
        cx.refresh().unwrap();
        cx.run_until_parked();

        let effects = window
            .update(cx, |_, window, _| window.devtools_frame_snapshot().effects)
            .unwrap();
        assert_eq!(effects.screen_effect_snapshot_count, 0);
    }

    #[gpux::test]
    fn devtools_effect_snapshot_reports_missing_snapshot_transition(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
        cx.run_until_parked();

        let start = cx.executor().now();
        window
            .update(cx, |_, window, _| {
                window.play_screen_effect_at(
                    screen::crossfade(ScreenEffectSnapshotId::new(404)).duration_ms(1_000),
                    start,
                );
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();

        let effects = window
            .update(cx, |_, window, _| window.devtools_frame_snapshot().effects)
            .unwrap();

        assert_eq!(effects.active_screen_effect_count, 1);
        assert_eq!(effects.screen_effect_snapshot_count, 0);
        assert_eq!(effects.missing_screen_effect_snapshot_count, 1);
    }

    #[gpux::test]
    fn screen_effects_expire_and_rendered_scene_snapshot_clears(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
        cx.run_until_parked();

        let start = cx.executor().now();
        window
            .update(cx, |_, window, _| {
                window.play_screen_effect_at(
                    screen::ripple(point(px(0.), px(0.))).duration_ms(32),
                    start,
                );
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();

        let snapshot_count = window
            .update(cx, |_, window, _| {
                window.rendered_frame.scene.active_screen_effects.len()
            })
            .unwrap();
        assert_eq!(snapshot_count, 1);

        cx.executor().advance_clock(Duration::from_millis(33));
        let expired_at = cx.executor().now();

        let has_active = window
            .update(cx, |_, window, _| window.tick_screen_effects_at(expired_at))
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();

        let (active_count, snapshot_count) = window
            .update(cx, |_, window, _| {
                (
                    window.active_screen_effect_count(),
                    window.rendered_frame.scene.active_screen_effects.len(),
                )
            })
            .unwrap();

        assert!(!has_active);
        assert_eq!(active_count, 0);
        assert_eq!(snapshot_count, 0);
    }

    #[gpux::test]
    fn active_screen_effects_keep_window_dirty_only_until_expiry(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
        cx.run_until_parked();

        let start = cx.executor().now();
        window
            .update(cx, |_, window, _| {
                window.play_screen_effect_at(
                    screen::ripple(point(px(0.), px(0.))).duration_ms(64),
                    start,
                );
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();

        let dirty_while_active = window.update(cx, |_, window, _| window.is_dirty()).unwrap();
        assert!(dirty_while_active);

        cx.executor().advance_clock(Duration::from_millis(65));
        let expired_at = cx.executor().now();
        window
            .update(cx, |_, window, _| {
                window.tick_screen_effects_at(expired_at);
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();

        let dirty_after_expiry = window.update(cx, |_, window, _| window.is_dirty()).unwrap();
        assert!(!dirty_after_expiry);
    }

    #[gpux::test]
    fn effect_capabilities_default_to_unsupported(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
        cx.run_until_parked();

        let capabilities = window
            .update(cx, |_, window, _| window.effect_capabilities())
            .unwrap();

        assert_eq!(capabilities, crate::EffectCapabilities::default());
    }

    #[gpux::test]
    fn devtools_effect_snapshot_reports_active_missing_region_and_backend_support(
        cx: &mut TestAppContext,
    ) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
        cx.run_until_parked();

        let start = cx.executor().now();
        window
            .update(cx, |_, window, _| {
                window.play_screen_effect_at(
                    screen::ripple(point(px(0.), px(0.)))
                        .target(ScreenEffectTarget::region("missing-region"))
                        .duration_ms(1_000),
                    start,
                );
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();

        let effects = window
            .update(cx, |_, window, _| window.devtools_frame_snapshot().effects)
            .unwrap();

        assert_eq!(effects.active_screen_effect_count, 1);
        assert_eq!(effects.visual_effect_count, 0);
        assert_eq!(effects.screen_effect_region_count, 0);
        assert_eq!(effects.missing_screen_effect_region_count, 1);
        assert_eq!(effects.capabilities, crate::EffectCapabilities::default());
    }
}
