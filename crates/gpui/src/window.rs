#[cfg(feature = "profiler")]
use crate::DebugFrameOverlayMode;
#[cfg(any(feature = "inspector", debug_assertions))]
use crate::Inspector;
#[cfg(feature = "profiler")]
use crate::profiler;
use crate::{
    Action, ActionDispatchEvent, ActionDispatchOutcome, ActionDispatchReport, ActionInvocation,
    ActionInvocationSource, ActiveScreenEffect, ActiveScreenEffectSnapshot, AnyDrag, AnyElement,
    AnyImageCache, AnyTooltip, AnyView, App, AppContext, Arena, Asset, AsyncWindowContext,
    AvailableSpace, Background, BorderStyle, Bounds, BoxShadow, Capslock, Color, Context, Corners,
    CursorHideMode, CursorStyle, Decorations, DevicePixels, DispatchActionListener, DispatchNodeId,
    DispatchTree, DisplayId, Edges, Effect, EffectCapabilities, ElementEffect, Entity, EntityId,
    EventEmitter, FileDropEvent, FontId, Global, GlobalElementId, GlyphId, GpuSpecs,
    HtmlElementContentFit, HtmlElementInputMode, HtmlElementLifetime, HtmlElementPrimitive,
    HtmlElementWheelBehavior, HtmlIslandInputMode, HtmlIslandPrimitive, InputHandler,
    InputPreference, IsZero, KeyBinding, KeyContext, KeyDownEvent, KeyEvent, KeymapSnapshot,
    Keystroke, KeystrokeEvent, LayoutId, Length, LineLayoutIndex, MAX_ACTIVE_SCREEN_EFFECTS,
    MAX_SCREEN_EFFECT_SNAPSHOTS, Modifiers, ModifiersChangedEvent, MonochromeSprite,
    MonochromeSpriteKind, MouseButton, MouseEvent, MouseMoveEvent, MouseUpEvent, Overflow, Path,
    Pixels, PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow,
    Point, PolychromeSprite, Position, Priority, PromptButton, PromptLevel, Quad, Render,
    RenderGlyphParams, RenderImage, RenderImageParams, RenderSvgParams, Replay, ResizeEdge,
    SMOOTH_SVG_SCALE_FACTOR, ScaledPixels, Scene, ScreenEffect, ScreenEffectId,
    ScreenEffectRegionId, ScreenEffectRegionPrimitive, ScreenEffectSnapshotId, ScreenEffectTarget,
    Shadow, SharedString, Size, StickyStacking, StrikethroughStyle, Style, SubpixelSprite,
    SubscriberSet, Subscription, SystemWindowTab, SystemWindowTabController, TabStopMap,
    TaffyLayoutEngine, Task, TextInputConfiguration, TextInputStateChange, TextRenderingMode,
    TextStyle, TextStyleRefinement, ThermalState, TransformationMatrix, Underline, UnderlineStyle,
    VisualEffectPrimitive, WindowAppearance, WindowBackgroundAppearance, WindowBounds,
    WindowControls, WindowDecorations, WindowOptions, WindowParams, WindowTextSystem,
    WindowVisibility, point, prelude::*, prepare_color_with_opacity, px, rems, size,
    transparent_black,
};

use crate::gestures::{GestureTuning, RecognizedTouchGesture, TouchGestureRecognizer};
use crate::interactive::TouchEvent;
use anyhow::{Context as _, Result, anyhow};
use collections::{FxHashMap, FxHashSet};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use core_video::pixel_buffer::CVPixelBuffer;
use derive_more::{Deref, DerefMut};
use futures::channel::oneshot;
use gpui_util::post_inc;
use gpui_util::{ResultExt, measure};
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
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
    },
    thread::AccessError,
    time::Duration,
};
use uuid::Uuid;

pub(crate) mod a11y;

mod action_target;
mod prompts;

pub use a11y::roles::{aria_role, query_role_name};
pub use a11y::{A11yInspection, A11yRelationQueue, A11ySubtreeBuilder, AccessibilityMode};

use self::a11y::A11y;
use self::a11y::ROOT_NODE_ID;
use crate::util::{
    atomic_incr_if_not_zero, ceil_to_device_pixel, floor_to_device_pixel,
    round_half_toward_zero_f64, round_stroke_to_device_pixel,
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
    #[cfg(feature = "profiler")]
    pub window_id: WindowId,
    pub dirty: bool,
    pub draw_phase: DrawPhase,
    pub dirty_views: FxHashSet<EntityId>,
    pub update_count: usize,
    #[cfg(feature = "profiler")]
    pub frame_dirty: FrameDirtyAccumulator,
    pub platform_waker: Option<Rc<dyn Fn()>>,
}

/// Per-frame invalidation bookkeeping, drained at draw time and emitted to the
/// frame profiler. Tracks when the current frame first became dirty and how
/// many invalidations were coalesced into it, whenever the profiler is
/// compiled in. Retention of the resulting per-frame records is what
/// `profiler::trace_enabled()` controls, not this measurement.
#[cfg(feature = "profiler")]
#[derive(Default)]
struct FrameDirtyAccumulator {
    dirty_at: Option<Instant>,
    invalidations: u64,
}

/// Why GPUI capped a window's frame rate.
#[derive(Clone, Copy)]
enum FrameRateLimit {
    /// The window isn't focused.
    Inactive,
    /// The system is under thermal pressure.
    Thermal,
}

#[derive(Clone)]
pub(crate) struct WindowInvalidator {
    inner: Rc<RefCell<WindowInvalidatorInner>>,
}

impl WindowInvalidator {
    pub fn new(#[allow(unused_variables)] window_id: WindowId) -> Self {
        WindowInvalidator {
            inner: Rc::new(RefCell::new(WindowInvalidatorInner {
                #[cfg(feature = "profiler")]
                window_id,
                dirty: true,
                draw_phase: DrawPhase::None,
                dirty_views: FxHashSet::default(),
                update_count: 0,
                #[cfg(feature = "profiler")]
                frame_dirty: FrameDirtyAccumulator::default(),
                platform_waker: None,
            })),
        }
    }

    pub fn invalidate_view(&self, entity: EntityId, cx: &mut App) -> bool {
        let mut inner = self.inner.borrow_mut();
        inner.update_count += 1;
        inner.dirty_views.insert(entity);
        if inner.draw_phase == DrawPhase::None {
            #[cfg(feature = "profiler")]
            let dirty_at = Self::record_frame_dirty(&mut inner);
            let became_dirty = !inner.dirty;
            inner.dirty = true;
            let waker = became_dirty.then(|| inner.platform_waker.clone()).flatten();
            #[cfg(feature = "profiler")]
            let window_id = inner.window_id;
            drop(inner);
            #[cfg(feature = "profiler")]
            if became_dirty {
                profiler::journal::record_frame_pending(window_id, dirty_at);
            }
            cx.push_effect(Effect::Notify { emitter: entity });
            if let Some(waker) = waker {
                waker();
            }
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
        let became_dirty = dirty && !inner.dirty;
        inner.dirty = dirty;
        if dirty {
            inner.update_count += 1;
        }
        #[cfg(feature = "profiler")]
        let dirty_at = dirty.then(|| Self::record_frame_dirty(&mut inner));
        let waker = became_dirty.then(|| inner.platform_waker.clone()).flatten();
        #[cfg(feature = "profiler")]
        let window_id = inner.window_id;
        drop(inner);
        #[cfg(feature = "profiler")]
        if became_dirty && let Some(dirty_at) = dirty_at {
            profiler::journal::record_frame_pending(window_id, dirty_at);
        }
        if let Some(waker) = waker {
            waker();
        }
    }

    pub fn set_platform_waker(&self, waker: Option<Rc<dyn Fn()>>) {
        let mut inner = self.inner.borrow_mut();
        inner.platform_waker = waker;
        let waker = inner.dirty.then(|| inner.platform_waker.clone()).flatten();
        drop(inner);
        if let Some(waker) = waker {
            waker();
        }
    }

    /// Wakes the platform's frame-request source so a frame request is
    /// delivered even if the platform stops requesting frames for idle
    /// windows. No-op on platforms without a frame waker.
    pub fn wake_platform(&self) {
        let waker = self.inner.borrow().platform_waker.clone();
        if let Some(waker) = waker {
            waker();
        }
    }

    pub fn set_phase(&self, phase: DrawPhase) {
        self.inner.borrow_mut().draw_phase = phase
    }

    pub fn update_count(&self) -> usize {
        self.inner.borrow().update_count
    }

    #[cfg(feature = "profiler")]
    fn record_frame_dirty(inner: &mut WindowInvalidatorInner) -> Instant {
        let dirty_at = *inner.frame_dirty.dirty_at.get_or_insert_with(Instant::now);
        inner.frame_dirty.invalidations += 1;
        dirty_at
    }

    #[cfg(feature = "profiler")]
    fn take_frame_dirty(&self) -> FrameDirtyAccumulator {
        mem::take(&mut self.inner.borrow_mut().frame_dirty)
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

/// Whether a window draw is currently in progress on this thread.
///
/// This holds exactly while an `ElementArenaScope` is active: nested scopes
/// restore the previous (still set) arena pointer, so `CURRENT_ELEMENT_ARENA`
/// is `Some` from the outermost draw's start to its end.
///
/// The `on_request_frame` callback uses this to defer draw requests that
/// arrive re-entrantly while a draw is already on the stack (e.g. via nested
/// message pumping in the Windows window procedure), instead of running a
/// nested draw or panicking on the already-borrowed App.
fn draw_in_progress() -> bool {
    CURRENT_ELEMENT_ARENA.with(|current| current.get().is_some())
}

/// Allocates an element in the current arena. Uses the app-specific arena if one
/// is active (during draw), otherwise falls back to the thread-local ELEMENT_ARENA.
#[inline(always)]
pub(crate) fn with_element_arena<R>(callback: impl FnOnce(&mut Arena) -> R) -> R {
    let mut callback = Some(callback);
    let mut result = None;
    let access = with_element_arena_erased(&mut |arena| {
        result = Some(callback.take().expect("arena callback runs once")(arena));
    });
    drop(callback);
    access.expect("cannot access a Thread Local Storage value during or after destruction");
    result.expect("arena callback produces a result")
}

/// Scope guard that sets CURRENT_ELEMENT_ARENA for the duration of a draw
/// operation and tracks the arena's scope depth, so that a nested draw's
/// `ArenaClearNeeded::clear` is deferred rather than freeing memory the outer
/// draw still references (see `Arena::clear`).
///
/// Call [`ElementArenaScope::exit`] with the same arena that was entered to
/// obtain the [`ArenaClearNeeded`] token the draw now owes; requiring `exit`
/// makes it impossible to request a clear before the scope has ended. The
/// scope's teardown — restoring the thread-local and balancing `begin_scope`
/// with `end_scope` — happens in `Drop`, so the arena's scope depth stays
/// balanced on every path, including when a panic unwinds a draw before `exit`
/// is reached. (If teardown lived only in `exit`, such a panic would leave the
/// scope depth permanently elevated and defer every future clear, leaking
/// memory unboundedly.)
pub(crate) struct ElementArenaScope {
    /// The entered arena: compared against the argument in `exit`, and
    /// dereferenced in `Drop` to end its scope (see the SAFETY note there).
    entered: *const RefCell<Arena>,
    previous: Option<*const RefCell<Arena>>,
    exited: bool,
}

impl ElementArenaScope {
    /// Enter a scope where element allocations use the given arena.
    pub(crate) fn enter(arena: &RefCell<Arena>) -> Self {
        arena.borrow_mut().begin_scope();
        let previous = CURRENT_ELEMENT_ARENA.with(|current| {
            let prev = current.get();
            current.set(Some(arena as *const RefCell<Arena>));
            prev
        });
        Self {
            entered: arena as *const RefCell<Arena>,
            previous,
            exited: false,
        }
    }

    /// End the scope: restores the previously-current arena and ends the
    /// arena's clear-deferral scope. Returns the token for the arena clear the
    /// draw now owes; producing it here makes it impossible to request a clear
    /// before the scope has ended (which would be silently deferred forever).
    ///
    /// Panics if passed a different arena than was entered: ending the scope
    /// of the wrong arena would unbalance two arenas' scope depths, allowing
    /// one of them to clear while a draw still references its memory.
    pub(crate) fn exit(mut self, arena: &RefCell<Arena>) -> ArenaClearNeeded {
        assert!(
            std::ptr::eq(self.entered, arena),
            "ElementArenaScope::exit called with a different arena than was entered"
        );
        self.exited = true;
        // Teardown (restoring the thread-local and ending the arena's
        // clear-deferral scope) runs in `Drop`, which fires both here — `self`
        // is dropped as `exit` returns, before the token reaches the caller —
        // and when a panic unwinds the draw before `exit` is reached.
        ArenaClearNeeded::new(arena)
    }
}

impl Drop for ElementArenaScope {
    fn drop(&mut self) {
        CURRENT_ELEMENT_ARENA.with(|current| {
            current.set(self.previous);
        });
        // SAFETY: `entered` was created from the arena reference passed to
        // `enter`, and the owning App outlives this draw-local scope guard.
        unsafe { &*self.entered }.borrow_mut().end_scope();
        if !self.exited && !std::thread::panicking() {
            debug_assert!(false, "ElementArenaScope dropped without calling exit()");
            log::error!(
                "ElementArenaScope dropped without calling exit(); the arena clear for this draw was never requested"
            );
        }
    }
}

/// Returned when the element arena has been used and so must be cleared before the next draw.
#[must_use]
pub struct ArenaClearNeeded {
    /// Identity of the arena that was drawn into. Only ever compared against
    /// another pointer in `clear`; never dereferenced.
    arena: *const RefCell<Arena>,
}

impl ArenaClearNeeded {
    /// Create a new ArenaClearNeeded token for the App whose arena was drawn
    /// into. Private: the only way to obtain one is [`ElementArenaScope::exit`].
    fn new(arena: &RefCell<Arena>) -> Self {
        Self {
            arena: arena as *const RefCell<Arena>,
        }
    }

    /// Clear the element arena of the App the draw ran against. If an enclosing
    /// draw is still in progress (this draw was nested inside it), the clear is
    /// deferred to the enclosing draw's own `ArenaClearNeeded` so that its live
    /// allocations aren't freed.
    ///
    /// Panics if passed a different App than the draw ran against, since
    /// clearing another App's arena could free memory its draws still
    /// reference.
    pub fn clear(self, cx: &mut App) {
        assert!(
            std::ptr::eq(self.arena, &cx.element_arena),
            "ArenaClearNeeded::clear called with a different App than the draw ran against"
        );
        cx.element_arena.borrow_mut().clear();
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

/// The opacity and transform that painting applies to primitives at some point of the tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ElementPaintContext {
    pub(crate) opacity: f32,
    pub(crate) transformation: TransformationMatrix,
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
        let hovered = self.hovered_unrecorded(window);
        crate::fast::dependencies::note_hover_read(window, self, hovered);
        hovered
    }

    /// [`Self::is_hovered`] without noting the read for the cached view
    /// being drawn.
    pub(crate) fn hovered_unrecorded(self, window: &Window) -> bool {
        // If this hitbox has captured the pointer, it's always considered hovered
        if window.captured_hitbox == Some(self) {
            return true;
        }
        if window.last_input_was_keyboard() {
            return false;
        }
        self.hit_test(window)
    }

    /// Checks if the hitbox with this ID is currently hovered, regardless of the last
    /// input modality used.
    ///
    /// See [`HitboxId::is_hovered`] for more details.
    pub(crate) fn is_hovered_ignoring_last_input(self, window: &Window) -> bool {
        // If this hitbox has captured the pointer, it's always considered hovered
        if window.captured_hitbox == Some(self) {
            return true;
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

    /// The hit-test layer: normal, positioned, then top layer.
    pub(crate) fn layer(self) -> u64 {
        self.0 / POSITIONED_HITBOX_ID_BASE
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

    /// Checks whether this hitbox would be hovered at `position`, regardless of the current input
    /// modality or mouse position.
    pub fn is_hovered_at(&self, position: Point<Pixels>, window: &Window) -> bool {
        let hit_test = window.rendered_frame.hit_test(position);
        hit_test
            .ids
            .iter()
            .take(hit_test.hover_hitbox_count)
            .any(|id| self.id == *id)
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
    /// window.on_mouse_event(move |_: &EveryMouseEventTypeHere, phase, window, cx| {
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
    /// window.on_mouse_event(move |_: &EveryMouseEventTypeExceptScroll, phase, window, cx| {
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

/// The ids of the elements being drawn, root first, with the hash of every
/// prefix alongside: a [`GlobalElementId`] for the current element is then
/// the ids cloned out and the hash on top of the stack, without walking the
/// path — the ids of every ancestor, string names byte by byte — for each
/// element on every frame.
#[derive(Clone, Default)]
pub(crate) struct ElementIdStack {
    ids: SmallVec<[ElementId; 32]>,
    hashes: SmallVec<[u64; 32]>,
}

impl ElementIdStack {
    pub(crate) fn push(&mut self, id: ElementId) {
        self.hashes.push(crate::extend_path_hash(self.hash(), &id));
        self.ids.push(id);
    }

    pub(crate) fn pop(&mut self) -> Option<ElementId> {
        self.hashes.pop();
        self.ids.pop()
    }

    pub(crate) fn clear(&mut self) {
        self.ids.clear();
        self.hashes.clear();
    }

    /// The hash of the whole path on the stack.
    fn hash(&self) -> u64 {
        self.hashes
            .last()
            .copied()
            .unwrap_or(crate::EMPTY_PATH_HASH)
    }

    pub(crate) fn global_id(&self) -> GlobalElementId {
        GlobalElementId::with_hash(&self.ids, self.hash())
    }
}

impl std::ops::Deref for ElementIdStack {
    type Target = [ElementId];

    fn deref(&self) -> &Self::Target {
        &self.ids
    }
}

pub(crate) struct DeferredDraw {
    current_view: EntityId,
    priority: usize,
    parent_node: DispatchNodeId,
    element_id_stack: ElementIdStack,
    text_style_stack: Vec<TextStyleRefinement>,
    content_mask: Option<ContentMask<Pixels>>,
    rem_size: Pixels,
    element: Option<AnyElement>,
    absolute_offset: Point<Pixels>,
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
    /// The retained views it was deferred from, whose part it is.
    fast_enclosing: crate::fast::retained::EnclosingRetained,
}

pub(crate) struct WindowFrame {
    pub(crate) focus: Option<FocusId>,
    pub(crate) window_active: bool,
    pub(crate) element_states: FxHashMap<(GlobalElementId, TypeId), ElementStateBox>,
    pub(crate) accessed_element_states: Vec<(GlobalElementId, TypeId)>,
    pub(crate) pending_element_state_transfers: Vec<(GlobalElementId, TypeId)>,
    pub(crate) mouse_listeners: Vec<Option<AnyMouseListener>>,
    /// The event type of each mouse listener, for the fast-mode frame description.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fast_mouse_listener_events: Vec<&'static str>,
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
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) debug_bounds_records: Vec<(String, Bounds<Pixels>)>,
    /// Visual bounds that elements reported for their own dispatch node while prepainting,
    /// when they paint somewhere other than their layout bounds (fixed and sticky divs).
    visual_bounds: Vec<(DispatchNodeId, Bounds<Pixels>)>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) next_inspector_instance_ids: FxHashMap<Rc<crate::InspectorElementPath>, usize>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) inspector_hitboxes: FxHashMap<HitboxId, crate::InspectorElementId>,
    pub(crate) tab_stops: TabStopMap,
    /// The retained views drawn in the frame.
    pub(crate) fast_retained: crate::fast::retained::RetainedSubtrees,
    /// Where painting switched composition surface.
    pub(crate) fast_composition_starts: crate::fast::composition::SurfaceStarts,
    /// The native surfaces placed.
    pub(crate) fast_native_surfaces: crate::fast::composition::NativeSurfacePlacements,
}

#[derive(Clone, Default, Debug)]
pub(crate) struct PrepaintStateIndex {
    pub(crate) hitboxes_index: usize,
    pub(crate) tooltips_index: usize,
    pub(crate) deferred_draws_index: usize,
    pub(crate) dispatch_tree_index: usize,
    /// Where the frame's accessibility events stand (see `window::a11y::A11yEvent`).
    pub(crate) a11y_events_index: usize,
    pub(crate) accessed_element_states_index: usize,
    pub(crate) pending_element_state_transfers_index: usize,
    pub(crate) line_layout_index: LineLayoutIndex,
    pub(crate) fast_native_surfaces_index: usize,
}

#[derive(Clone, Default, Debug)]
pub(crate) struct PaintIndex {
    pub(crate) scene_index: usize,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) debug_bounds_index: usize,
    pub(crate) mouse_listeners_index: usize,
    pub(crate) input_handlers_index: usize,
    pub(crate) cursor_styles_index: usize,
    pub(crate) accessed_element_states_index: usize,
    pub(crate) tab_handle_index: usize,
    pub(crate) line_layout_index: LineLayoutIndex,
    /// Where the accessibility action handlers painted from here start.
    pub(crate) a11y_listeners_index: usize,
    /// Where the window-control hitboxes painted from here start: a view
    /// drawn from last frame paints them again with the rest.
    pub(crate) fast_window_control_hitboxes_index: usize,
}

impl WindowFrame {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn record_debug_bounds(&mut self, selector: String, bounds: Bounds<Pixels>) {
        self.debug_bounds.insert(selector.clone(), bounds);
        self.debug_bounds_records.push((selector, bounds));
    }

    pub(crate) fn new(dispatch_tree: DispatchTree) -> Self {
        WindowFrame {
            focus: None,
            window_active: false,
            element_states: FxHashMap::default(),
            accessed_element_states: Vec::new(),
            pending_element_state_transfers: Vec::new(),
            mouse_listeners: Vec::new(),
            #[cfg(any(test, feature = "test-support"))]
            fast_mouse_listener_events: Vec::new(),
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
            #[cfg(any(test, feature = "test-support"))]
            debug_bounds_records: Vec::new(),

            visual_bounds: Vec::new(),

            #[cfg(any(feature = "inspector", debug_assertions))]
            next_inspector_instance_ids: FxHashMap::default(),

            #[cfg(any(feature = "inspector", debug_assertions))]
            inspector_hitboxes: FxHashMap::default(),
            tab_stops: TabStopMap::default(),
            fast_retained: Default::default(),
            fast_composition_starts: Default::default(),
            fast_native_surfaces: Default::default(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.element_states.clear();
        self.accessed_element_states.clear();
        self.pending_element_state_transfers.clear();
        self.mouse_listeners.clear();
        #[cfg(any(test, feature = "test-support"))]
        self.fast_mouse_listener_events.clear();
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
            self.debug_bounds_records.clear();
        }

        self.visual_bounds.clear();

        #[cfg(any(feature = "inspector", debug_assertions))]
        {
            self.next_inspector_instance_ids.clear();
            self.inspector_hitboxes.clear();
        }
        self.fast_retained.clear();
        self.fast_composition_starts.clear();
        self.fast_native_surfaces.clear();
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
            .rev()
            .filter(|hitbox| {
                hitbox.behavior != HitboxBehavior::Ignore
                    && hitbox
                        .bounds
                        .intersect(&hitbox.content_mask.bounds)
                        .contains(&position)
            })
            .collect::<SmallVec<[_; 16]>>();
        // Within a layer, later hitboxes are on top. Ids order only the layers:
        // cached views replay their hitboxes with the previous frame's ids, which
        // can be smaller than those of siblings inserted before them this frame.
        hitboxes.sort_by_key(|hitbox| std::cmp::Reverse(hitbox.id.layer()));

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
        for element_state_key in &self.pending_element_state_transfers {
            if let Some((element_state_key, element_state)) =
                prev_frame.element_states.remove_entry(element_state_key)
            {
                self.element_states.insert(element_state_key, element_state);
            }
        }

        self.scene.finish();
    }
}

/// Shader-effect diagnostics for the last rendered frame of a window, from
/// [`Window::effect_diagnostics`].
#[derive(Clone, Debug, PartialEq)]
pub struct EffectDiagnostics {
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
    Touch,
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

    /// Returns the next candidate that replaces part of the active chain.
    ///
    /// The second tuple field is the number of active ancestors retained by that replacement.
    #[doc(hidden)]
    pub fn next_replacement(&self, source: usize) -> Option<(usize, usize)> {
        let current = self.records.get(source)?;
        let next_index = source + 1;
        let next = self.records.get(next_index)?;
        if next.run_root != current.run_root {
            Some((next_index, 0))
        } else {
            (next.depth <= current.depth).then_some((next_index, next.depth))
        }
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

/// Returns the common displacement that lets an incoming sticky replacement push outgoing
/// candidates away.
///
/// Both inputs are relative to the scrollport's start edge. The result is zero until the
/// incoming run reaches the active run's trailing edge, then follows the incoming run toward
/// the start edge.
#[doc(hidden)]
pub fn sticky_push_offset(active_stack_end: Pixels, incoming_start: Pixels) -> Pixels {
    (incoming_start - active_stack_end).min(px(0.))
}

pub(crate) type StickyLayoutContextId = usize;

type StickyOccurrenceId = usize;

#[derive(Clone, Copy)]
pub(crate) struct StickyLayoutContext {
    id: StickyLayoutContextId,
    overflow: Point<Overflow>,
}

#[derive(Clone)]
pub(crate) struct StickyCandidate {
    layout_id: Option<LayoutId>,
    stack_id: Option<ElementId>,
    inset: Edges<Length>,
    pub(crate) horizontal_context: Option<StickyLayoutContextId>,
    pub(crate) vertical_context: Option<StickyLayoutContextId>,
}

#[derive(Clone, Copy, Default)]
struct StickyCandidatePlan {
    x: Option<ResolvedStickyAxis>,
    y: Option<ResolvedStickyAxis>,
    layer_order: Option<StickyLayerOrder>,
}

#[derive(Clone, Copy)]
struct StickyLayerOrder {
    group: usize,
    rank: u32,
}

fn reorder_sticky_layer_groups(order: &mut [usize], layer_orders: &[Option<StickyLayerOrder>]) {
    let mut groups: FxHashMap<usize, SmallVec<[(usize, usize, u32); 4]>> = FxHashMap::default();
    for (slot, child_index) in order.iter().copied().enumerate() {
        let Some(layer_order) = layer_orders.get(child_index).copied().flatten() else {
            continue;
        };
        groups
            .entry(layer_order.group)
            .or_default()
            .push((slot, child_index, layer_order.rank));
    }

    for mut members in groups.into_values() {
        if members.len() < 2 {
            continue;
        }
        let slots = members
            .iter()
            .map(|(slot, _, _)| *slot)
            .collect::<SmallVec<[_; 4]>>();
        members.sort_by_key(|(_, _, rank)| *rank);
        for (slot, (_, child_index, _)) in slots.into_iter().zip(members) {
            order[slot] = child_index;
        }
    }
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
pub(crate) struct ScrollContainerContextStack {
    pub(crate) stack: Vec<ScrollContainerContext>,
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

/// Holds the state for a specific window.
pub struct Window {
    pub(crate) handle: AnyWindowHandle,
    pub(crate) invalidator: WindowInvalidator,
    pub(crate) removed: bool,
    pub(crate) platform_window: Box<dyn PlatformWindow>,
    display_id: Option<DisplayId>,
    is_resizable: bool,
    is_minimizable: bool,
    pub(crate) sprite_atlas: Arc<dyn PlatformAtlas>,
    pub(crate) text_system: Arc<WindowTextSystem>,
    pub(crate) text_rendering_mode: Rc<Cell<TextRenderingMode>>,
    rem_size: Pixels,
    /// The stack of override values for the window's rem size.
    ///
    /// This is used by `with_rem_size` to allow rendering an element tree with
    /// a given rem size.
    pub(crate) rem_size_override_stack: SmallVec<[Pixels; 8]>,
    pub(crate) viewport_size: Size<Pixels>,
    pub(crate) layout_engine: Option<TaffyLayoutEngine>,
    pub(crate) fast_mode: crate::fast::mode::WindowFastMode,
    pub(crate) fast_hover_reads: crate::fast::dependencies::HoverReads,
    pub(crate) fast_layout: crate::fast::layout_key::WindowLayout,
    pub(crate) fast_ambient_reads: crate::fast::dependencies::AmbientReads,
    pub(crate) fast_retained_state: crate::fast::retained::RetainedState,
    pub(crate) fast_composition: crate::fast::composition::WindowCompositionHandle,
    #[cfg(feature = "diagnostics")]
    pub(crate) fast_verify: crate::fast::verify::retention::RetentionVerify,
    pub(crate) fast_glyph_bounds: crate::fast::glyphs::GlyphBoundsCache,
    pub(crate) fast_layers: crate::fast::layers::WindowLayers,
    /// The measured leaves requested this frame under view retention, with
    /// the view each was requested in and whether its style fixes its size
    /// in every parent (see `fast::layout::measured_size_is_fixed`).
    pub(crate) fast_measured_owners: Vec<(LayoutId, EntityId, bool)>,
    /// The views whose layout last frame held a measured leaf other than text
    /// whose size came, or could come, from its measurement: built on the
    /// next frame, not reused, since the leaf keeps no measurement closure
    /// across frames.
    pub(crate) fast_measuring_views: FxHashSet<EntityId>,
    #[cfg(any(
        test,
        feature = "test-support",
        feature = "bench-support",
        feature = "diagnostics"
    ))]
    pub(crate) fast_stats: crate::fast::stats::WindowFastStats,
    #[cfg(feature = "diagnostics")]
    pub(crate) fast_frame_work: crate::fast::stats::FrameWork,
    pub(crate) root: Option<AnyView>,
    pub(crate) element_id_stack: ElementIdStack,
    pub(crate) text_style_stack: Vec<TextStyleRefinement>,
    retained_styles: Vec<Option<Style>>,
    pub(crate) rendered_entity_stack: Vec<EntityId>,
    pub(crate) element_offset_stack: Vec<Point<Pixels>>,
    pub(crate) element_opacity: f32,
    pub(crate) element_transform_stack: Vec<TransformationMatrix>,
    pub(crate) content_mask_stack: Vec<ContentMask<Pixels>>,
    pub(crate) scroll_container_context_stack: ScrollContainerContextStack,
    pub(crate) sticky_layout_context_stack: Vec<StickyLayoutContext>,
    pub(crate) sticky_candidates: Vec<StickyCandidate>,
    sticky_plans: Vec<StickyCandidatePlan>,
    pub(crate) next_sticky_layout_context_id: StickyLayoutContextId,
    pub(crate) paint_content_mask_stack: Vec<ContentMask<ScaledPixels>>,
    pub(crate) hitbox_behavior_overrides: Vec<HitboxBehavior>,
    pub(crate) requested_autoscroll: Option<Bounds<Pixels>>,
    /// The [`TextInputConfiguration`] most recently forwarded to the platform
    /// window, so that only actual changes are forwarded (reconfiguring a live
    /// input session can restart the IME connection).
    last_text_input_configuration: Option<TextInputConfiguration>,
    focused_text_input_active: bool,
    pub(crate) image_cache_stack: Vec<AnyImageCache>,
    pub(crate) rendered_frame: WindowFrame,
    pub(crate) next_frame: WindowFrame,
    active_screen_effects: VecDeque<WindowActiveScreenEffect>,
    screen_effect_snapshots: VecDeque<WindowScreenEffectSnapshot>,
    next_screen_effect_id: u64,
    next_screen_effect_snapshot_id: u64,
    next_hitbox_id: HitboxId,
    /// How many fixed or sticky subtrees are prepainting. They paint in a
    /// positioned layer, so their hitboxes hit-test in that layer too.
    pub(crate) positioned_hitbox_depth: usize,
    pub(crate) next_tooltip_id: TooltipId,
    pub(crate) tooltip_bounds: Option<TooltipBounds>,
    pub(crate) next_frame_callbacks: Rc<RefCell<Vec<FrameCallback>>>,
    pub(crate) dirty_views: FxHashSet<EntityId>,
    action_dispatch_observers: SubscriberSet<(), AnyActionDispatchObserver>,
    next_action_invocation_id: u64,
    action_invocation_stack: Vec<ActionInvocation>,
    focus_listeners: SubscriberSet<(), AnyWindowFocusListener>,
    pub(crate) focus_lost_listeners: SubscriberSet<(), AnyObserver>,
    focus_lost_path: SmallVec<[FocusId; 8]>,
    default_prevented: bool,
    pub(crate) mouse_position: Point<Pixels>,
    pub(crate) pressed_mouse_button: Option<MouseButton>,
    mouse_hit_test: HitTest,
    pub(crate) modifiers: Modifiers,
    pub(crate) capslock: Capslock,
    scale_factor: f32,
    pub(crate) bounds_observers: SubscriberSet<(), AnyObserver>,
    appearance: WindowAppearance,
    pub(crate) appearance_observers: SubscriberSet<(), AnyObserver>,
    pub(crate) button_layout_observers: SubscriberSet<(), AnyObserver>,
    active: Rc<Cell<bool>>,
    minimized: Rc<Cell<bool>>,
    visibility: WindowVisibility,
    pub(crate) visibility_observers:
        SubscriberSet<(), Box<dyn FnMut(WindowVisibility, &mut Window, &mut App) -> bool>>,
    hovered: Rc<Cell<bool>>,
    _thermal_state_subscription: Subscription,
    pub(crate) needs_present: Rc<Cell<bool>>,
    /// Tracks recent input event timestamps to determine if input is arriving at a high rate.
    /// Used to selectively enable VRR optimization only when input rate exceeds 60fps.
    pub(crate) input_rate_tracker: Rc<RefCell<InputRateTracker>>,
    #[cfg(feature = "profiler")]
    window_profiler: profiler::WindowProfiler,
    last_input_modality: InputModality,
    touch_gestures: TouchGestureRecognizer,
    touch_prediction_enabled: bool,
    long_press_timer: Option<Task<()>>,
    long_press_capture: Option<EntityId>,
    pub(crate) refreshing: bool,
    /// How many frames the window drew; see [`Window::frames_drawn`].
    frames_drawn: u64,
    pub(crate) activation_observers: SubscriberSet<(), AnyObserver>,
    pub(crate) minimized_observers: SubscriberSet<(), AnyObserver>,
    pub(crate) focus: Option<FocusId>,
    focus_enabled: bool,
    /// Incremented every time focus moves. Used to invalidate a
    /// pending keyboard activation state when focus changes.
    pub(crate) focus_generation: u64,
    pending_input: Option<PendingInput>,
    pending_modifier: ModifierState,
    pub(crate) pending_input_observers: SubscriberSet<(), AnyObserver>,
    prompt: Option<RenderablePromptHandle>,
    pub(crate) client_inset: Option<Pixels>,
    /// The hitbox that has captured the pointer, if any.
    /// While captured, mouse events route to this hitbox regardless of hit testing.
    pub(crate) captured_hitbox: Option<HitboxId>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    inspector: Option<Entity<Inspector>>,
    #[cfg(feature = "profiler")]
    debug_frame_overlay: crate::debug_overlay::DebugFrameOverlay,
    pub(crate) a11y: A11y,
    /// The inspection test queries hold for the window's life
    /// (`VisualTestContext::settled_semantic_snapshot`).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) test_inspection: Option<A11yInspection>,
}

#[derive(Clone, Debug, Default)]
struct ModifierState {
    modifiers: Modifiers,
    saw_other_input: bool,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DrawPhase {
    None,
    Prepaint,
    Paint,
    Focus,
}

pub(crate) const PENDING_INPUT_TIMEOUT: Duration = Duration::from_secs(1);

/// Pending input for a potential multi-stroke key binding.
pub struct PendingInputStatus<'a> {
    keystrokes: &'a [Keystroke],
    timeout: Option<PendingInputTimeoutStatus>,
}

impl<'a> PendingInputStatus<'a> {
    /// Returns the keystrokes entered so far.
    pub fn keystrokes(&self) -> &'a [Keystroke] {
        self.keystrokes
    }

    /// Returns the timeout state for flushing this input, if it needs a timeout.
    pub fn timeout(&self) -> Option<PendingInputTimeoutStatus> {
        self.timeout
    }
}

/// The timeout state for pending input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingInputTimeoutStatus {
    duration: Duration,
    remaining: Duration,
    started_at: Option<Instant>,
    paused: bool,
}

impl PendingInputTimeoutStatus {
    /// Returns the full timeout duration.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Returns the duration remaining before pending input is flushed.
    pub fn remaining(&self, cx: &App) -> Duration {
        self.started_at
            .map(|started_at| {
                self.remaining
                    .saturating_sub(cx.background_executor().now() - started_at)
            })
            .unwrap_or(self.remaining)
    }

    /// Returns whether the timeout is paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }
}

#[derive(Debug)]
struct PendingInputTimeout {
    duration: Duration,
    remaining: Duration,
    state: PendingInputTimeoutState,
}

#[derive(Debug)]
enum PendingInputTimeoutState {
    Running { started_at: Instant, task: Task<()> },
    Paused { pause: PendingInputTimeoutPause },
}

#[derive(Debug)]
struct PendingInputTimeoutPause {
    owner_id: EntityId,
    _release_subscription: Subscription,
}

impl PendingInputTimeout {
    fn is_paused(&self) -> bool {
        matches!(&self.state, PendingInputTimeoutState::Paused { .. })
    }

    fn pause(&mut self, pause: PendingInputTimeoutPause, now: Instant) -> bool {
        match std::mem::replace(&mut self.state, PendingInputTimeoutState::Paused { pause }) {
            PendingInputTimeoutState::Running { started_at, task } => {
                self.remaining = self.remaining.saturating_sub(now - started_at);
                drop(task);
                true
            }
            previous_state @ PendingInputTimeoutState::Paused { .. } => {
                self.state = previous_state;
                false
            }
        }
    }

    fn pause_owner_id(&self) -> Option<EntityId> {
        match &self.state {
            PendingInputTimeoutState::Running { .. } => None,
            PendingInputTimeoutState::Paused { pause } => Some(pause.owner_id),
        }
    }

    fn resume(&mut self, owner_id: EntityId, started_at: Instant, task: Task<()>) -> bool {
        match std::mem::replace(
            &mut self.state,
            PendingInputTimeoutState::Running { started_at, task },
        ) {
            PendingInputTimeoutState::Paused { pause } if pause.owner_id == owner_id => true,
            previous_state => {
                self.state = previous_state;
                false
            }
        }
    }

    fn reset_duration(&mut self, duration: Duration) {
        self.duration = duration;
        self.remaining = duration;
    }

    fn status(&self) -> PendingInputTimeoutStatus {
        let (started_at, paused) = match &self.state {
            PendingInputTimeoutState::Running { started_at, .. } => (Some(*started_at), false),
            PendingInputTimeoutState::Paused { .. } => (None, true),
        };
        PendingInputTimeoutStatus {
            duration: self.duration,
            remaining: self.remaining,
            started_at,
            paused,
        }
    }
}

#[derive(Default)]
struct PendingInput {
    keystrokes: SmallVec<[Keystroke; 1]>,
    keymap_snapshot: Option<Rc<KeymapSnapshot>>,
    focus: Option<FocusId>,
    timeout: Option<PendingInputTimeout>,
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
            app_owns_titlebar_drag,
            inactive_frame_interval,
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

        let initial_window_title = titlebar
            .as_ref()
            .and_then(|titlebar| titlebar.title.clone());

        let window_bounds = window_bounds.unwrap_or_else(|| default_bounds(display_id, cx));
        let mut platform_window = cx.platform.open_window(
            handle,
            WindowParams {
                bounds: window_bounds.get_bounds(),
                titlebar,
                kind,
                is_movable,
                app_owns_titlebar_drag,
                is_resizable,
                is_minimizable,
                focus,
                show,
                display_id,
                window_min_size,
                app_id: app_id.clone(),
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
        let invalidator = WindowInvalidator::new(handle.window_id());
        let active = Rc::new(Cell::new(platform_window.is_active()));
        let minimized = Rc::new(Cell::new(false));
        let visibility = platform_window.visibility();
        #[cfg(feature = "profiler")]
        profiler::journal::record_window_visibility(handle.window_id(), visibility);
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

        let accessibility_force_disabled = cx.accessibility_force_disabled;
        let a11y_active_flag = Arc::new(AtomicBool::new(false));

        // Every target sets up the adapter callbacks: a platform without a screen-reader adapter
        // keeps `a11y_init`'s no-op and never turns the platform source on, while inspection and
        // `AccessibilityMode::Always` build the tree on every target.
        if !accessibility_force_disabled {
            let mut initial_root_node = accesskit::Node::new(accesskit::Role::Window);
            if let Some(title) = &initial_window_title {
                initial_root_node.set_label(title.to_string());
            }
            let initial_tree = accesskit::TreeUpdate {
                nodes: vec![(ROOT_NODE_ID, initial_root_node)],
                tree: Some(a11y::tree_info()),
                tree_id: accesskit::TreeId::ROOT,
                focus: ROOT_NODE_ID,
            };
            let (activation_sender, activation_receiver) = async_channel::unbounded::<()>();
            let (deactivation_sender, deactivation_receiver) = async_channel::unbounded::<()>();
            let (action_sender, action_receiver) =
                async_channel::unbounded::<accesskit::ActionRequest>();

            platform_window.a11y_init(crate::A11yCallbacks {
                activation: {
                    let active_flag = a11y_active_flag.clone();
                    Box::new(move || {
                        log::info!("Accessibility activated");
                        active_flag.store(true, SeqCst);
                        activation_sender.try_send(()).log_err();
                        Some(initial_tree.clone())
                    })
                },
                action: Box::new(move |request| {
                    action_sender.try_send(request).log_err();
                }),
                deactivation: {
                    let active_flag = a11y_active_flag.clone();
                    Box::new(move || {
                        log::info!("Accessibility deactivated");
                        active_flag.store(false, SeqCst);
                        deactivation_sender.try_send(()).log_err();
                    })
                },
            });

            // A11y can be activated at any time, and so we cannot compute a
            // correct `TreeUpdate` on-demand. When this happens, we return a
            // default empty `TreeUpdate`.
            //
            // So we force a new frame, which will then send a correct `TreeUpdate`.
            let mut async_cx = cx.to_async();
            cx.foreground_executor()
                .spawn(async move {
                    while activation_receiver.recv().await.is_ok() {
                        // Whole-window: Accessibility activation rebuilds the whole tree.
                        handle
                            .update(&mut async_cx, |_, window, _| window.refresh())
                            .log_err();
                    }
                })
                .detach();

            let mut async_cx = cx.to_async();
            cx.foreground_executor()
                .spawn(async move {
                    while deactivation_receiver.recv().await.is_ok() {
                        // Whole-window: Accessibility deactivation rebuilds the whole tree.
                        handle
                            .update(&mut async_cx, |_, window, _| window.refresh())
                            .log_err();
                    }
                })
                .detach();

            let mut async_cx = cx.to_async();
            cx.foreground_executor()
                .spawn(async move {
                    while let Ok(request) = action_receiver.recv().await {
                        handle
                            .update(&mut async_cx, |_, window, cx| {
                                window.handle_a11y_action(request, cx);
                            })
                            .log_err();
                    }
                })
                .detach();
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
            let thermal_state = Rc::clone(&thermal_state);
            let mut deferred_force_render = false;
            move |request_frame_options| {
                #[cfg(feature = "profiler")]
                let (_foreground_turn, window_id) = {
                    let foreground_turn = profiler::journal::foreground_turn();
                    let window_id = handle.window_id();
                    profiler::journal::record_platform_signal(profiler::journal::PlatformSignal {
                        window_id,
                        signal_at: request_frame_options.signal_at,
                        handled_at: Instant::now(),
                        source: request_frame_options.signal_source,
                    });
                    (foreground_turn, window_id)
                };
                // This must be checked before accessing App: if this request
                // arrived re-entrantly while a draw is on this thread's stack
                // (e.g. via a nested message pump in the Windows window
                // procedure), drawing would nest draws, and even touching the
                // App would panic on its already-mutable borrow. Skip instead;
                // the platform leaves the window invalidated (or re-invalidates
                // it), so a fresh request arrives once the in-progress draw
                // unwinds. Remember force_render so the deferred frame still
                // bypasses the view cache.
                //
                // Returning here skips `complete_frame`, which on Wayland would
                // stall the window's frame callbacks (no `surface.commit()`) —
                // but calling it would hit the App borrow panic above, and this
                // branch is unreachable there in practice: only Windows pumps
                // platform events (and thus requests frames) mid-draw.
                if draw_in_progress() {
                    log::debug!("deferring re-entrant window draw request");
                    deferred_force_render |= request_frame_options.force_render;
                    #[cfg(feature = "profiler")]
                    profiler::journal::record_reentrant_frame_skipped(window_id, Instant::now());
                    return;
                }
                // Take the deferred flag first: `||` short-circuits, and leaving
                // the flag set when this request already forces a render would
                // force a second, redundant render on the next frame.
                let force_render =
                    mem::take(&mut deferred_force_render) || request_frame_options.force_render;

                // Upstream reads the thermal state through an App update whose effect flush
                // requests a follow-up frame while this window has frame demand. gpux reads
                // the cached thermal state instead, so idle frames never enter App; remember
                // the demand and request the same follow-up after the frame.
                let had_frame_demand = invalidator.is_dirty()
                    || needs_present.get()
                    || !next_frame_callbacks.borrow().is_empty();

                // Throttle frame rate based on conditions:
                // - Thermal pressure (Serious/Critical): cap to ~60fps
                // - Inactive window (not focused): cap to ~30fps to save energy
                let min_frame_interval = if request_frame_options.require_presentation
                    || (!request_frame_options.force_render
                        && next_frame_callbacks.borrow().is_empty())
                {
                    None
                } else if !active.get() && !input_rate_tracker.borrow_mut().is_high_rate() {
                    inactive_frame_interval.map(|interval| (interval, FrameRateLimit::Inactive))
                } else if matches!(
                    thermal_state.get(),
                    ThermalState::Critical | ThermalState::Serious
                ) {
                    Some((Duration::from_micros(16667), FrameRateLimit::Thermal))
                } else {
                    None
                };

                // The executor clock is real time in production; tests step it with
                // `advance_clock`, so frame throttling stays deterministic there.
                let now = cx.background_executor().now();
                #[cfg_attr(not(feature = "profiler"), expect(unused_variables))]
                if let Some((min_interval, limit)) = min_frame_interval {
                    if let Some(last_frame) = last_frame_time.get()
                        && now.duration_since(last_frame) < min_interval
                    {
                        // Don't lose a pending forced render to throttling.
                        deferred_force_render |= force_render;
                        // Deferred by throttling: ask demand-driven platforms to retry.
                        handle
                            .update(&mut cx, |_, window, _| {
                                window.platform_window.schedule_frame();
                            })
                            .log_err();
                        // The demand that entered this branch (a deferred forced
                        // render or pending next-frame callbacks) is still
                        // unserved; platforms that stop requesting frames for
                        // idle windows need a wakeup to deliver the retry.
                        invalidator.wake_platform();
                        #[cfg(feature = "profiler")]
                        profiler::journal::record_frame_skipped(
                            window_id,
                            Instant::now(),
                            match limit {
                                FrameRateLimit::Inactive => {
                                    profiler::journal::FrameSkipReason::InactiveFrameRateLimit
                                }
                                FrameRateLimit::Thermal => {
                                    profiler::journal::FrameSkipReason::ThermalFrameRateLimit
                                }
                            },
                        );
                        return;
                    }
                }
                last_frame_time.set(Some(now));

                let pending_next_frame_callbacks = next_frame_callbacks.take();
                if !pending_next_frame_callbacks.is_empty() {
                    handle
                        .update(&mut cx, |_, window, cx| {
                            for callback in pending_next_frame_callbacks {
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
                    || input_rate_tracker.borrow_mut().is_high_rate();

                let should_draw = invalidator.is_dirty() || force_render;
                if should_draw {
                    measure("frame duration", || {
                        handle
                            .update(&mut cx, |_, window, cx| {
                                if force_render {
                                    // Bypass cached view reuse so we don't replay stale
                                    // atlas tile references after a GPU device recovery.
                                    window.refresh();
                                }
                                let arena_clear_needed = window.draw(cx);
                                window.present();
                                arena_clear_needed.clear(cx);
                            })
                            .log_err();
                    })
                } else if needs_present {
                    handle
                        .update(&mut cx, |_, window, _| window.present())
                        .log_err();
                }

                if had_frame_demand
                    || invalidator.is_dirty()
                    || !next_frame_callbacks.borrow().is_empty()
                {
                    handle
                        .update(&mut cx, |_, window, _| {
                            window.platform_window.schedule_frame();
                        })
                        .log_err();
                }

                // Platforms that stop requesting frames for idle windows only
                // deliver another request after a wakeup. If demand remains
                // after this frame (the window was re-invalidated mid-draw, or
                // animations scheduled next-frame callbacks), re-arm the frame
                // source explicitly.
                if invalidator.is_dirty() || !next_frame_callbacks.borrow().is_empty() {
                    invalidator.wake_platform();
                }
                #[cfg(feature = "profiler")]
                if !should_draw {
                    profiler::journal::record_frame_skipped(
                        window_id,
                        Instant::now(),
                        profiler::journal::FrameSkipReason::NoRenderNeeded,
                    );
                }
            }
        }));
        invalidator.set_platform_waker(platform_window.frame_waker());
        platform_window.on_visual_viewport_changed(Box::new({
            let mut cx = cx.to_async();
            move || {
                // Whole-window: The visual viewport moves every view.
                handle
                    .update(&mut cx, |_, window, _| window.refresh())
                    .log_err();
            }
        }));
        platform_window.on_insets_changed(Box::new({
            let mut cx = cx.to_async();
            move |_| {
                // Whole-window: Safe-area insets can change any view's layout.
                handle
                    .update(&mut cx, |_, window, _| window.refresh())
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
            let cx = cx.to_async();
            let foreground_executor = cx.foreground_executor().clone();
            move || {
                let mut cx = cx.clone();
                // Defer the update because changing the AppKit appearance may
                // synchronously invoke this callback while App is already borrowed.
                foreground_executor
                    .spawn(async move {
                        handle
                            .update(&mut cx, |_, window, cx| window.appearance_changed(cx))
                            .log_err();
                    })
                    .detach();
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
                        let ambient = crate::fast::dependencies::AmbientInput::of(window);
                        window.modifiers = window.platform_window.modifiers();
                        window.capslock = window.platform_window.capslock();
                        ambient.stamp_changes(window, cx);
                        window
                            .activation_observers
                            .clone()
                            .retain(&(), |callback| callback(window, cx));

                        window.bounds_changed(cx);
                        // Whole-window: Window activation restyles every view that reads it.
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
                        // Whole-window: Leaving or entering the minimized state redraws everything shown.
                        window.refresh();
                    })
                    .log_err();
            }
        }));
        platform_window.on_visibility_change(Box::new({
            let mut cx = cx.to_async();
            move |_| {
                handle
                    .update(&mut cx, |_, window, cx| {
                        window.refresh_visibility(cx);
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
                        // Whole-window: The window's hover status gates every hover style.
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

        let fast_mode = crate::fast::mode::WindowFastMode::for_window(platform_window.as_ref());
        Ok(Window {
            handle,
            invalidator,
            removed: false,
            platform_window,
            display_id,
            is_resizable,
            is_minimizable,
            sprite_atlas,
            text_system,
            text_rendering_mode: cx.text_rendering_mode.clone(),
            rem_size: px(16.),
            rem_size_override_stack: SmallVec::new(),
            viewport_size: content_size,
            layout_engine: Some(TaffyLayoutEngine::new()),
            fast_mode,
            fast_hover_reads: crate::fast::dependencies::HoverReads::default(),
            fast_layout: crate::fast::layout_key::WindowLayout::default(),
            fast_ambient_reads: cx.fast_ambient_reads(),
            fast_retained_state: Default::default(),
            fast_composition: Default::default(),
            #[cfg(feature = "diagnostics")]
            fast_verify: Default::default(),
            fast_glyph_bounds: crate::fast::glyphs::GlyphBoundsCache::default(),
            fast_layers: crate::fast::layers::WindowLayers::default(),
            fast_measured_owners: Vec::new(),
            fast_measuring_views: FxHashSet::default(),
            #[cfg(any(
                test,
                feature = "test-support",
                feature = "bench-support",
                feature = "diagnostics"
            ))]
            fast_stats: crate::fast::stats::WindowFastStats::default(),
            #[cfg(feature = "diagnostics")]
            fast_frame_work: Default::default(),
            root: None,
            element_id_stack: ElementIdStack::default(),
            text_style_stack: Vec::new(),
            retained_styles: Vec::new(),
            rendered_entity_stack: Vec::new(),
            element_offset_stack: Vec::new(),
            content_mask_stack: Vec::new(),
            scroll_container_context_stack: ScrollContainerContextStack::default(),
            sticky_layout_context_stack: Vec::new(),
            sticky_candidates: Vec::new(),
            sticky_plans: Vec::new(),
            next_sticky_layout_context_id: 0,
            element_opacity: 1.0,
            element_transform_stack: Vec::new(),
            paint_content_mask_stack: Vec::new(),
            hitbox_behavior_overrides: Vec::new(),
            requested_autoscroll: None,
            last_text_input_configuration: None,
            focused_text_input_active: false,
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
            positioned_hitbox_depth: 0,
            next_tooltip_id: TooltipId::default(),
            tooltip_bounds: None,
            dirty_views: FxHashSet::default(),
            action_dispatch_observers: SubscriberSet::new(),
            next_action_invocation_id: 0,
            action_invocation_stack: Vec::new(),
            focus_listeners: SubscriberSet::new(),
            focus_lost_listeners: SubscriberSet::new(),
            focus_lost_path: SmallVec::new(),
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
            visibility,
            visibility_observers: SubscriberSet::new(),
            hovered,
            _thermal_state_subscription: thermal_state_subscription,
            needs_present,
            input_rate_tracker,
            #[cfg(feature = "profiler")]
            window_profiler: profiler::WindowProfiler::new(handle.window_id())?,
            last_input_modality: InputModality::Mouse,
            touch_gestures: TouchGestureRecognizer::new(
                cx.platform
                    .gestures()
                    .map_or_else(GestureTuning::default, |gestures| gestures.tuning()),
            ),
            touch_prediction_enabled: true,
            long_press_timer: None,
            long_press_capture: None,
            refreshing: false,
            frames_drawn: 0,
            activation_observers: SubscriberSet::new(),
            minimized_observers: SubscriberSet::new(),
            focus: None,
            focus_enabled: true,
            focus_generation: 0,
            pending_input: None,
            pending_modifier: ModifierState::default(),
            pending_input_observers: SubscriberSet::new(),
            prompt: None,
            client_inset: None,
            image_cache_stack: Vec::new(),
            captured_hitbox: None,
            #[cfg(any(feature = "inspector", debug_assertions))]
            inspector: None,
            #[cfg(feature = "profiler")]
            debug_frame_overlay: crate::debug_overlay::DebugFrameOverlay::new(),
            #[cfg(any(test, feature = "test-support"))]
            test_inspection: None,
            a11y: A11y::new(
                a11y_active_flag,
                cx.accessibility_mode.clone(),
                accessibility_force_disabled,
                initial_window_title,
            ),
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

    pub(crate) fn refresh_visibility(&mut self, cx: &mut App) {
        let visibility = self.platform_window.visibility();
        if self.visibility != visibility {
            self.visibility = visibility;
            #[cfg(feature = "profiler")]
            profiler::journal::record_window_visibility(self.handle.window_id(), visibility);
            self.visibility_observers
                .clone()
                .retain(&(), |callback| callback(visibility, self, cx));
        }
        #[cfg(feature = "profiler")]
        if self.invalidator.is_dirty() || self.needs_present.get() {
            profiler::journal::record_frame_pending(self.handle.window_id(), Instant::now());
        }
    }

    /// Whether the platform is presenting this window's frames (see
    /// [`WindowVisibility`]).
    pub fn visibility(&self) -> WindowVisibility {
        self.visibility
    }

    /// Whether frames drawn for this window will be shown.
    ///
    /// This is not the window's shown/hidden state: a shown window that is
    /// fully behind another window, minimized, or on a sleeping display is not
    /// visible here.
    pub fn is_visible(&self) -> bool {
        self.visibility.is_visible()
    }

    /// Registers a callback to be invoked when the window's visibility changes.
    pub fn observe_window_visibility(
        &self,
        mut callback: impl FnMut(WindowVisibility, &mut Window, &mut App) + 'static,
    ) -> Subscription {
        let (subscription, activate) = self.visibility_observers.insert(
            (),
            Box::new(move |visibility, window, cx| {
                callback(visibility, window, cx);
                true
            }),
        );
        activate();
        subscription
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
        // Whole-window: A new root replaces every view.
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
        self.removed = true;
    }

    /// Obtain the currently focused [`FocusHandle`]. If no elements are focused, returns `None`.
    pub fn focused(&self, cx: &App) -> Option<FocusHandle> {
        self.focus
            .and_then(|id| FocusHandle::for_id(id, &cx.focus_handles))
    }

    /// While focus-lost listeners are being dispatched, returns the closest ancestor of the
    /// previously focused element that can still receive focus, making it a suitable target
    /// for focus restoration. Returns `None` at all other times, or when no such ancestor exists.
    pub fn focus_lost_restore_target(&self, cx: &App) -> Option<FocusHandle> {
        let (_leaf, ancestors) = self.focus_lost_path.split_last()?;
        ancestors.iter().rev().find_map(|id| {
            self.rendered_frame.dispatch_tree.focusable_node_id(*id)?;
            FocusHandle::for_id(*id, &cx.focus_handles)
        })
    }

    /// Move focus to the element associated with the given [`FocusHandle`].
    pub fn focus(&mut self, handle: &FocusHandle, cx: &mut App) {
        if !self.focus_enabled || self.focus == Some(handle.id) {
            return;
        }

        self.focus = Some(handle.id);
        self.focus_generation = self.focus_generation.wrapping_add(1);
        self.clear_pending_keystrokes(cx);

        // Whole-window: Focus can restyle any view (focus, in-focus and focus-visible styles).
        self.refresh();
    }

    /// Remove focus from all elements within this context's window.
    pub fn blur(&mut self, cx: &mut App) {
        self.clear_pending_keystrokes(cx);

        if !self.focus_enabled {
            return;
        }

        if self.focus.is_some() {
            self.focus_generation = self.focus_generation.wrapping_add(1);
        }
        self.focus = None;
        // Whole-window: Focus can restyle any view (focus, in-focus and focus-visible styles).
        self.refresh();
    }

    /// Blur the window and don't allow anything in it to be focused again.
    pub fn disable_focus(&mut self, cx: &mut App) {
        self.blur(cx);
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

    /// Set the exclusive zone for a layer-shell surface: how much screen space it
    /// reserves so other surfaces avoid occluding it (e.g. a panel reserving space).
    /// Positive values reserve that distance from the anchored edge, 0 lets the
    /// surface be moved out of others' exclusive zones, and -1 ignores reserved
    /// space and may extend under other surfaces. (Wayland layer-shell windows only)
    pub fn set_exclusive_zone(&self, zone: Pixels) {
        self.platform_window.set_exclusive_zone(zone);
    }

    /// Set which anchored edge a layer-shell surface's exclusive zone applies to.
    /// This is only needed to disambiguate a corner-anchored surface; otherwise the
    /// edge is deduced from the anchor. The edge must be a single edge the surface
    /// is anchored to, or it is ignored. (Wayland layer-shell windows only)
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub fn set_exclusive_edge(&self, edge: crate::layer_shell::Anchor) {
        self.platform_window.set_exclusive_edge(edge);
    }

    /// Start an interactive window resize operation if this window is resizable.
    pub fn start_window_resize(&self, edge: ResizeEdge) {
        if self.is_resizable {
            self.platform_window.start_window_resize(edge);
        }
    }

    /// Linux (wayland) only: Set the window's input region, the area that receives pointer
    /// and touch input. Events outside it pass through to whatever is below the window.
    ///
    /// - `Some(rects)` restricts input to the union of `rects`, in window coordinates.
    /// - `Some(&[])` is an empty region, so the window receives no pointer or touch input.
    /// - `None` resets the region to the default, so the whole window receives input again.
    pub fn set_input_region(&self, region: Option<&[Bounds<Pixels>]>) {
        self.platform_window.set_input_region(region);
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

    /// Encode the window's native restorable state into an opaque blob.
    /// Returns `None` on platforms without native state restoration or if encoding fails.
    pub fn native_window_state(&self) -> Option<Vec<u8>> {
        self.platform_window.native_window_state()
    }

    /// Restore the window's native state from a blob previously produced
    /// by [`Window::native_window_state`]. A no-op on platforms without native state restoration.
    pub fn restore_native_window_state(&self, state: &[u8]) {
        self.platform_window.restore_native_window_state(state);
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
        keystroke: &Keystroke,
        input_preference: InputPreference,
        action: Option<&dyn Action>,
        context_stack: Vec<KeyContext>,
        cx: &mut App,
    ) {
        cx.keystroke_observers.clone().retain(&(), move |callback| {
            (callback)(
                &KeystrokeEvent {
                    keystroke: keystroke.clone(),
                    input_preference,
                    action: action.map(|action| action.boxed_clone()),
                    context_stack: context_stack.clone(),
                },
                self,
                cx,
            )
        });
    }

    pub(crate) fn dispatch_keystroke_interceptors(
        &mut self,
        keystroke: &Keystroke,
        input_preference: InputPreference,
        context_stack: Vec<KeyContext>,
        cx: &mut App,
    ) {
        cx.keystroke_interceptors
            .clone()
            .retain(&(), move |callback| {
                (callback)(
                    &KeystrokeEvent {
                        keystroke: keystroke.clone(),
                        input_preference,
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
        self.platform_window.schedule_frame();
        // Next-frame callbacks create frame demand without dirtying the
        // window, so the platform's frame source must be woken explicitly.
        self.invalidator.wake_platform();
    }

    /// Schedule a frame to be drawn on the next animation frame.
    ///
    /// This is useful for elements that need to animate continuously, such as a video player or an animated GIF.
    /// It will cause the window to redraw on the next frame, even if no other changes have occurred.
    ///
    /// If called from within a view, it will notify that view on the next frame. Otherwise, it will refresh the entire window.
    ///
    /// Callers driving purely decorative animations (spinners, pulses, and the
    /// like) should prefer [`AnimationExt::with_animation`](crate::AnimationExt::with_animation),
    /// which automatically respects [`App::reduce_motion`]. When using this
    /// method directly for decorative motion, check [`App::reduce_motion`]
    /// and skip the frame request when it is set.
    pub fn request_animation_frame(&self) {
        let entity = self.current_view();
        crate::fast::layers::invalidate::note_animation_frame(self, entity);
        self.on_next_frame(move |_, cx| cx.notify(entity));
    }

    /// Runs all callbacks scheduled via [`Self::on_next_frame`], returning how many ran.
    ///
    /// Tests have no platform frame loop, so this simulates the delivery of the
    /// next frame.
    #[cfg(any(test, feature = "test-support"))]
    pub fn simulate_next_frame(&mut self, cx: &mut App) -> usize {
        let callbacks = self.next_frame_callbacks.take();
        let count = callbacks.len();
        for callback in callbacks {
            callback(self, cx);
        }
        count
    }

    /// Start a screen-level shader effect in this window.
    ///
    /// The oldest effects are evicted first when the per-window cap is reached.
    pub fn play_screen_effect(&mut self, effect: impl Into<ScreenEffect>) -> ScreenEffectId {
        self.play_screen_effect_at(effect, Instant::now())
    }

    /// Cancel one active screen effect by id.
    ///
    /// Returns whether the effect was still active and removed. The caller
    /// remains responsible for releasing any snapshot referenced by the
    /// canceled effect.
    pub fn cancel_screen_effect(&mut self, id: ScreenEffectId) -> bool {
        let previous_len = self.active_screen_effects.len();
        self.active_screen_effects.retain(|effect| effect.id != id);
        let canceled = self.active_screen_effects.len() != previous_len;
        if canceled {
            self.invalidator.set_dirty(true);
        }
        canceled
    }

    pub(crate) fn play_screen_effect_at(
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

        // Whole-window: A resize lays out every view.
        self.refresh();

        self.bounds_observers
            .clone()
            .retain(&(), |callback| callback(self, cx));
    }

    /// Returns the bounds of the current window in the global coordinate space, which could span across multiple displays.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.platform_window.bounds()
    }

    /// How many frames the window has drawn since it opened.
    pub fn frames_drawn(&self) -> u64 {
        self.frames_drawn
    }

    /// Renders the current frame's scene to a texture and returns the pixel data as an RGBA image.
    /// This does not present the frame to screen - useful for visual testing where we want
    /// to capture what would be rendered without displaying it or requiring the window to be visible.
    pub fn render_to_image(&self) -> anyhow::Result<image::RgbaImage> {
        self.platform_window
            .render_to_image(&self.rendered_frame.scene)
    }

    /// Returns the quads in the most recently rendered frame's scene, so tests can assert on
    /// painted output without rasterizing the frame. Quad bounds are in scaled pixels and are
    /// not clipped; each quad carries the content mask it will be clipped to when drawn. Quads
    /// whose bounds don't intersect their content mask are culled at paint time and won't appear.
    #[cfg(any(test, feature = "test-support"))]
    pub fn painted_quads(&self) -> Vec<Quad> {
        self.rendered_frame.scene.quads.clone()
    }

    /// Returns the underlines in the most recently rendered frame's scene.
    #[cfg(any(test, feature = "test-support"))]
    pub fn painted_underlines(&self) -> Vec<Underline> {
        self.rendered_frame.scene.underlines.clone()
    }

    /// Set the content size of the window.
    pub fn resize(&mut self, size: Size<Pixels>) {
        self.platform_window.resize(size);
    }

    /// Returns whether or not the window is currently fullscreen
    pub fn is_fullscreen(&self) -> bool {
        self.platform_window.is_fullscreen()
    }

    /// Returns whether the window is currently in simple (borderless) fullscreen,
    /// where it covers the entire screen including the menu bar and notch area.
    /// Always `false` on platforms other than macOS.
    pub fn is_simple_fullscreen(&self) -> bool {
        self.platform_window.is_simple_fullscreen()
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

    /// Returns the platform's visible viewport in window-local logical pixels.
    ///
    /// Unlike `viewport_size`, this can shrink or move when the keyboard opens.
    /// During drawing this is a consistent frame snapshot. Outside drawing it
    /// reflects the latest platform sample, not a synchronous geometry query.
    pub fn visual_viewport_bounds(&self) -> Bounds<Pixels> {
        self.platform_window.visual_viewport_bounds()
    }

    /// Returns a conservative rectangle avoiding platform-known obscured content.
    ///
    /// Intersects the visual viewport with the full layout area inset by system
    /// safe areas and keyboard occlusion. Unknown overlays cannot be excluded.
    pub fn fully_visible_bounds(&self) -> Bounds<Pixels> {
        let insets = self.platform_window.insets().effective();
        let viewport = self.viewport_size();
        let left = insets.left.max(Pixels::ZERO).min(viewport.width);
        let top = insets.top.max(Pixels::ZERO).min(viewport.height);
        let right = (viewport.width - insets.right.max(Pixels::ZERO)).max(left);
        let bottom = (viewport.height - insets.bottom.max(Pixels::ZERO)).max(top);
        let safe_bounds = Bounds::from_corners(point(left, top), point(right, bottom));
        let mut visible = safe_bounds.intersect(&self.visual_viewport_bounds());
        visible.size.width = visible.size.width.max(Pixels::ZERO);
        visible.size.height = visible.size.height.max(Pixels::ZERO);
        visible
    }

    /// Requests the virtual keyboard for the currently focused text input.
    ///
    /// Call from a user gesture on platforms that require one. The platform may
    /// decline the request; this does not change focus or the layout viewport.
    pub fn request_virtual_keyboard(&self) {
        self.platform_window.show_soft_keyboard();
    }

    /// Requests dismissal of the virtual keyboard without changing GPUI focus.
    pub fn dismiss_virtual_keyboard(&self) {
        self.platform_window.hide_soft_keyboard();
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

    /// Returns whether this window is resizable.
    pub fn is_resizable(&self) -> bool {
        self.is_resizable
    }

    /// Returns whether this window is minimizable.
    pub fn is_minimizable(&self) -> bool {
        self.is_minimizable
    }

    /// Returns the controls supported by the platform.
    pub fn window_controls(&self) -> WindowControls {
        self.platform_window.window_controls()
    }

    /// Updates the window's title at the platform level.
    pub fn set_window_title(&mut self, title: &str) {
        self.platform_window.set_title(title);
        self.a11y.set_window_title(title.to_string());
    }

    /// Sets the position of the macOS traffic light buttons.
    #[cfg(target_os = "macos")]
    pub fn set_traffic_light_position(&self, position: Point<Pixels>) {
        self.platform_window.set_traffic_light_position(position);
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

    /// Overrides the display scale factor for tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_scale_factor(&mut self, scale_factor: f32) {
        self.scale_factor = scale_factor;
        // Whole-window: A scale change re-rasterizes every view.
        self.refresh();
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
            let global_id = this.element_id_stack.global_id();

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
        px(self.round_edge_to_device_pixel(value.0) / self.scale_factor())
    }

    /// f64 variant of [`Self::pixel_snap`].
    #[inline]
    pub fn pixel_snap_f64(&self, value: f64) -> f64 {
        let scale_factor = f64::from(self.scale_factor());
        let value = value * scale_factor;
        if self.fast_mode.snapping() {
            crate::util::round_half_down_f64(value) / scale_factor
        } else {
            round_half_toward_zero_f64(value) / scale_factor
        }
    }

    /// `logical` in device pixels, rounded as an edge: ties down while scroll
    /// layers snap, toward zero otherwise (see [`crate::util::round_edge`]).
    #[inline]
    fn round_edge_to_device_pixel(&self, logical: f32) -> f32 {
        crate::util::round_edge(logical * self.scale_factor(), self.fast_mode.snapping())
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

    /// Returns the snapped device-space bounds used to paint an underline.
    pub fn underline_bounds(
        &self,
        origin: Point<Pixels>,
        width: Pixels,
        style: &UnderlineStyle,
    ) -> Bounds<ScaledPixels> {
        let thickness = self.snap_stroke(style.thickness);
        let height = if style.wavy {
            ScaledPixels(thickness.0 * 3.)
        } else {
            thickness
        };
        Bounds {
            origin: origin.map(|c| ScaledPixels(self.round_edge_to_device_pixel(c.0))),
            size: size(self.snap_stroke(width), height),
        }
    }

    /// Paints an underline excluding absolute device-space horizontal spans.
    pub fn paint_underline_with_exclusions(
        &mut self,
        origin: Point<Pixels>,
        width: Pixels,
        style: &UnderlineStyle,
        exclusions: &[Range<ScaledPixels>],
    ) {
        self.invalidator.debug_assert_paint();
        let underline = self.underline(origin, width, style);
        if exclusions.is_empty() {
            self.next_frame.scene.insert_primitive(underline);
            return;
        }

        let bounds = underline.bounds.intersect(&underline.content_mask.bounds);
        if bounds.is_empty() {
            return;
        }

        let mut exclusions = exclusions
            .iter()
            .filter_map(|span| {
                let start = span.start.max(bounds.left());
                let end = span.end.min(bounds.right());
                (start < end).then_some(start..end)
            })
            .collect::<SmallVec<[_; 4]>>();
        if exclusions.is_empty() {
            self.next_frame.scene.insert_primitive(underline);
            return;
        }
        exclusions.sort_unstable_by_key(|span| span.start);

        let mut paint_span = |start, end| {
            self.next_frame.scene.insert_primitive(Underline {
                content_mask: ContentMask::rectangular(Bounds::from_corners(
                    point(start, bounds.top()),
                    point(end, bounds.bottom()),
                )),
                ..underline
            });
        };
        let mut start = bounds.left();
        for exclusion in exclusions {
            if start < exclusion.start {
                paint_span(start, exclusion.start);
            }
            start = start.max(exclusion.end);
        }
        if start < bounds.right() {
            paint_span(start, bounds.right());
        }
    }

    fn underline(&self, origin: Point<Pixels>, width: Pixels, style: &UnderlineStyle) -> Underline {
        let element_opacity = self.element_opacity();
        Underline {
            order: 0,
            pad: 0,
            bounds: self.underline_bounds(origin, width, style),
            content_mask: self.snapped_paint_content_mask(),
            color: style
                .color
                .as_ref()
                .map(|color| prepare_color_with_opacity(color, element_opacity))
                .unwrap_or_default(),
            thickness: self.snap_stroke(style.thickness),
            wavy: if style.wavy { 1 } else { 0 },
            transformation: self.current_element_transform(),
        }
    }

    #[inline]
    fn snap_bounds(&self, bounds: Bounds<Pixels>) -> Bounds<ScaledPixels> {
        let left = self.round_edge_to_device_pixel(bounds.left().0);
        let top = self.round_edge_to_device_pixel(bounds.top().0);
        let right = self.round_edge_to_device_pixel(bounds.right().0).max(left);
        let bottom = self.round_edge_to_device_pixel(bounds.bottom().0).max(top);
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
    pub(crate) fn cover_bounds(&self, bounds: Bounds<Pixels>) -> Bounds<ScaledPixels> {
        if self.fast_mode.snapping() {
            return crate::fast::layers::paint::cover_bounds(bounds, self.scale_factor());
        }
        Self::cover_bounds_at_scale(bounds, self.scale_factor())
    }

    #[inline]
    fn cover_bounds_at_scale(bounds: Bounds<Pixels>, scale_factor: f32) -> Bounds<ScaledPixels> {
        if bounds.is_empty() {
            return bounds.scale(scale_factor);
        }

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
    pub(crate) fn snapped_content_mask(&self) -> ContentMask<ScaledPixels> {
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
        self.fast_ambient_reads
            .note::<crate::fast::dependencies::ambient::Pointer>();
        self.mouse_position
    }

    /// The mouse button currently held according to the latest platform event.
    pub fn pressed_mouse_button(&self) -> Option<MouseButton> {
        self.fast_ambient_reads
            .note::<crate::fast::dependencies::ambient::Button>();
        self.pressed_mouse_button
    }

    /// The shader-effect counts of the last rendered frame: the active screen effects, their
    /// snapshots and regions, the element visual effects, the region-targeted effects whose
    /// region is absent and the snapshot transitions whose snapshot is absent, with the
    /// renderer's effect capabilities.
    pub fn effect_diagnostics(&self) -> EffectDiagnostics {
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

        EffectDiagnostics {
            active_screen_effect_count: scene.active_screen_effects.len(),
            screen_effect_snapshot_count: scene.active_screen_effect_snapshots.len(),
            visual_effect_count: scene.visual_effects.len(),
            screen_effect_region_count: scene.screen_effect_regions.len(),
            missing_screen_effect_region_count,
            missing_screen_effect_snapshot_count,
            capabilities: self.effect_capabilities(),
        }
    }

    /// Reports where the element being prepainted paints and hit-tests when that differs from
    /// its layout bounds (fixed and sticky divs). Its accessibility node then uses the visual
    /// bounds.
    pub(crate) fn report_visual_bounds(&mut self, bounds: Bounds<Pixels>) {
        if let Some(node_id) = self.next_frame.dispatch_tree.active_node_id() {
            self.next_frame.visual_bounds.push((node_id, bounds));
        }
    }

    /// Takes the visual bounds the element of dispatch node `node_id` reported while
    /// prepainting.
    pub(crate) fn take_visual_bounds(&mut self, node_id: DispatchNodeId) -> Option<Bounds<Pixels>> {
        let index = self
            .next_frame
            .visual_bounds
            .iter()
            .position(|(id, _)| *id == node_id)?;
        Some(self.next_frame.visual_bounds.swap_remove(index).1)
    }

    /// Captures the pointer for the given hitbox. While captured, all mouse move and mouse up
    /// events will be routed to listeners that check this hitbox's `is_hovered` status,
    /// regardless of actual hit testing. This enables drag operations that continue
    /// even when the pointer moves outside the element's bounds.
    ///
    /// The capture is automatically released on mouse up.
    pub fn capture_pointer(&mut self, hitbox_id: HitboxId) {
        self.captured_hitbox = Some(hitbox_id);
    }

    /// Releases any active pointer capture.
    pub fn release_pointer(&mut self) {
        self.captured_hitbox = None;
    }

    /// Returns the hitbox that has captured the pointer, if any.
    pub fn captured_hitbox(&self) -> Option<HitboxId> {
        self.captured_hitbox
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

    /// Captures the current long press for the given entity.
    ///
    /// The capture is released when the gesture ends or is cancelled, or when
    /// a replacement touch begins. A listener must also call
    /// [`Self::prevent_default`] on the started event to claim the gesture.
    pub fn capture_long_press<T: 'static>(&mut self, entity: &Entity<T>) {
        self.long_press_capture = Some(entity.entity_id());
    }

    /// Returns whether the given entity has captured the current long press.
    pub fn has_long_press_capture<T: 'static>(&self, entity: &Entity<T>) -> bool {
        self.long_press_capture == Some(entity.entity_id())
    }

    /// The current state of the keyboard's modifiers
    pub fn modifiers(&self) -> Modifiers {
        self.fast_ambient_reads
            .note::<crate::fast::dependencies::ambient::Keys>();
        self.modifiers
    }

    /// Returns true if the last input event was keyboard-based (key press, tab navigation, etc.)
    /// This is used for focus-visible styling to show focus indicators only for keyboard navigation.
    pub fn last_input_was_keyboard(&self) -> bool {
        self.last_input_modality == InputModality::Keyboard
    }

    pub(crate) fn last_input_was_touch(&self) -> bool {
        self.last_input_modality == InputModality::Touch
    }

    /// The current state of the keyboard's capslock
    pub fn capslock(&self) -> Capslock {
        self.fast_ambient_reads
            .note::<crate::fast::dependencies::ambient::Keys>();
        self.capslock
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
        self.frames_drawn += 1;
        #[cfg(feature = "diagnostics")]
        self.fast_frame_work.begin_draw();
        // Drain every draw in profiler builds so a previous frame's
        // first-invalidation timestamp can't be attributed to this one.
        #[cfg(feature = "profiler")]
        let frame_dirty = self.invalidator.take_frame_dirty();
        #[cfg(feature = "profiler")]
        self.window_profiler.begin_draw();

        // Set up the per-App arena for element allocation during this draw.
        // This ensures that multiple test Apps have isolated arenas.
        let arena_scope = ElementArenaScope::enter(&cx.element_arena);
        #[cfg(feature = "diagnostics")]
        crate::reset_global_element_id_path_diagnostics();
        #[cfg(feature = "diagnostics")]
        crate::reset_style_phase_diagnostics();
        self.retained_styles.clear();

        if self.platform_window.prepare_frame() {
            // Whole-window: The platform asked for a full frame (surface lost or reconfigured).
            self.refresh();
        }
        self.refresh_for_text_policy();
        self.invalidate_entities();
        // A view whose layout held a measured leaf other than text whose size
        // came from its measurement is built, as if notified: kept, the leaf
        // would have no measurement closure and be sized empty.
        let mut built_for_measured_leaves = 0;
        if self.fast_mode.view_retention() {
            for view in mem::take(&mut self.fast_measuring_views) {
                if !self.dirty_views.contains(&view) {
                    self.mark_view_dirty(view);
                    built_for_measured_leaves += 1;
                }
            }
        } else {
            self.fast_measuring_views.clear();
        }
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
        #[cfg(feature = "diagnostics")]
        let fast_shadow = crate::fast::verify::retention::draw_shadow(self, cx);
        #[cfg(any(
            test,
            feature = "test-support",
            feature = "bench-support",
            feature = "diagnostics"
        ))]
        crate::fast::stats::begin_frame(self);
        crate::fast::fast_stat!(
            self.fast_stats.frame,
            views_built_for_measured_leaves += built_for_measured_leaves
        );
        // Counted only where fast-mode counters are compiled.
        let _ = built_for_measured_leaves;
        crate::fast::layout::begin_frame(self);
        crate::fast::composition::begin_frame(self);
        crate::fast::retained::begin_frame(self, cx);
        if !cx.mode.skip_drawing() {
            self.draw_roots(cx);
            #[cfg(feature = "profiler")]
            {
                let viewport_size = self.viewport_size;
                let scale_factor = self.scale_factor();
                self.debug_frame_overlay.paint(
                    &mut self.next_frame.scene,
                    viewport_size,
                    scale_factor,
                );
            }
        }
        #[cfg(feature = "diagnostics")]
        self.next_frame
            .scene
            .set_global_element_id_path_diagnostics(crate::global_element_id_path_diagnostics());
        #[cfg(feature = "diagnostics")]
        {
            let text_measurement_diagnostics = crate::take_text_measurement_path_diagnostics();
            self.layout_engine
                .as_mut()
                .unwrap()
                .set_text_measurement_path_diagnostics([
                    text_measurement_diagnostics[0],
                    text_measurement_diagnostics[1],
                ]);
            self.next_frame
                .scene
                .set_text_measurement_miss_diagnostics([
                    text_measurement_diagnostics[2],
                    text_measurement_diagnostics[3],
                    text_measurement_diagnostics[4],
                    text_measurement_diagnostics[5],
                ]);
            self.next_frame
                .scene
                .set_line_layout_cache_diagnostics(crate::take_line_layout_cache_diagnostics());
            self.next_frame
                .scene
                .set_text_shape_diagnostics(crate::take_text_shape_diagnostics());
            self.next_frame
                .scene
                .set_style_phase_diagnostics(crate::style_phase_diagnostics());
            self.next_frame
                .scene
                .set_taffy_layout_diagnostics(self.layout_engine.as_ref().unwrap().diagnostics());
            self.next_frame.scene.set_taffy_node_request_diagnostics(
                self.layout_engine.as_ref().unwrap().request_diagnostics(),
            );
        }
        crate::fast::retained::finish_frame(self);
        self.dirty_views.clear();
        self.next_frame.window_active = self.active.get();

        // Register requested input handler with the platform window.
        // Use .take() instead of .pop() to preserve Vec length, so that cached
        // paint_range indices remain valid for reuse_paint on the next frame.
        // Search backwards to find the last Some entry, since reuse_paint may
        // have copied None slots from the previous frame. (Fixes #50456)
        let focused_text_input_active = if let Some(mut input_handler) = self
            .next_frame
            .input_handlers
            .iter_mut()
            .rev()
            .find_map(|h| h.take())
        {
            let accepts_text_input = input_handler.accepts_text_input(self, cx);
            self.platform_window.set_input_handler(input_handler);
            accepts_text_input
        } else {
            false
        };
        self.apply_text_input_configuration(cx);
        if focused_text_input_active != self.focused_text_input_active {
            self.focused_text_input_active = focused_text_input_active;
            self.platform_window
                .text_input_state_changed(if focused_text_input_active {
                    TextInputStateChange::FocusGained
                } else {
                    TextInputStateChange::FocusLost
                });
        }

        {
            // The views holding a measured leaf other than text whose size
            // came from its measurement, or whose style leaves its size to a
            // parent that may change while the view is kept, are built on the
            // next frame (see `draw`).
            let retention = &self.layout_engine.as_ref().unwrap().retention;
            self.fast_measuring_views = mem::take(&mut self.fast_measured_owners)
                .into_iter()
                .filter(|(id, _, fixed)| {
                    !crate::fast::layout::is_text_leaf(retention, *id)
                        && (!fixed || retention.measured_leaves.contains(id))
                })
                .map(|(_, view, _)| view)
                .collect();
        }
        self.layout_engine.as_mut().unwrap().clear();
        #[cfg(any(
            test,
            feature = "test-support",
            feature = "bench-support",
            feature = "diagnostics"
        ))]
        crate::fast::stats::finish_frame(self);
        self.text_system().finish_frame();
        self.fast_glyph_bounds.finish_frame();
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
        self.a11y.swap_frames();
        #[cfg(feature = "diagnostics")]
        crate::fast::verify::retention::check(self, fast_shadow);
        if has_active_screen_effects {
            self.invalidator.set_dirty(true);
        }
        let current_focus_path = self.rendered_frame.focus_path();
        let current_window_active = self.rendered_frame.window_active;
        let mut focus_before_listeners = self.focus;

        if previous_focus_path != current_focus_path
            || previous_window_active != current_window_active
        {
            if !previous_focus_path.is_empty() && current_focus_path.is_empty() {
                self.focus_lost_path = previous_focus_path.clone();
                self.focus_lost_listeners
                    .clone()
                    .retain(&(), |listener| listener(self, cx));
                self.focus_lost_path = SmallVec::new();
                // The focus-lost fallback (e.g. a workspace refocusing itself) may target
                // an element that isn't part of the element tree, in which case scheduling
                // a redraw below would dispatch focus-lost again, looping forever. Only
                // track focus movement caused by the focus listeners.
                focus_before_listeners = self.focus;
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
        // Focus listeners may move focus (e.g. a dock forwarding focus to its active
        // panel). `Window::focus` suppresses `refresh` while a draw is in progress, so
        // schedule another frame here to render the new focus state and dispatch the
        // resulting focus events.
        if self.focus != focus_before_listeners {
            self.refresh();
        }
        // An inspection guard started or dropped during the draw asked for a full frame.
        if self.a11y.take_refresh_after_frame() {
            self.refresh();
        }
        self.needs_present.set(true);
        #[cfg(feature = "diagnostics")]
        self.fast_frame_work.end_draw();

        #[cfg(feature = "profiler")]
        {
            let draw_duration = self
                .window_profiler
                .end_draw(frame_dirty.dirty_at, frame_dirty.invalidations);
            self.debug_frame_overlay.record_frame(draw_duration);
        }

        // Exit the scope to obtain the arena-clear token this draw owes; the
        // scope's teardown itself happens in `ElementArenaScope::drop`.
        arena_scope.exit(&cx.element_arena)
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
        crate::fast::retained::note_notified(self, &views);
        for entity in views.drain() {
            self.mark_view_dirty(entity);
        }
        self.invalidator.replace_views(views);
    }

    #[profiling::function]
    pub(crate) fn present(&mut self) {
        #[cfg(feature = "profiler")]
        let _foreground_turn = profiler::journal::foreground_turn();
        #[cfg(feature = "profiler")]
        let present_start = Instant::now();
        #[cfg(feature = "diagnostics")]
        let work_start = Instant::now();
        crate::fast::composition::present(self);
        #[cfg(feature = "diagnostics")]
        self.fast_frame_work.end_present(work_start);
        #[cfg(feature = "diagnostics")]
        self.platform_window.publish_fast_stats(
            &self.fast_stats.frame.counters(),
            &self.fast_stats.run.counters(),
        );
        #[cfg(feature = "profiler")]
        self.window_profiler.record_present(
            present_start,
            Instant::now(),
            self.active.get(),
            !self.next_frame_callbacks.borrow().is_empty(),
        );
        self.needs_present.set(false);
        profiling::finish_frame!();
    }

    /// Presents the most recently drawn frame if it hasn't been presented yet.
    #[cfg(all(test, feature = "profiler"))]
    pub fn present_if_needed(&mut self) {
        if self.needs_present.get() {
            self.present();
        }
    }

    /// Returns a snapshot of the current input-latency histograms.
    #[cfg(feature = "profiler")]
    pub fn input_latency_snapshot(&self) -> profiler::InputLatencySnapshot {
        self.window_profiler.input_latency_snapshot()
    }

    /// Returns a snapshot of the current frame-duration histograms.
    #[cfg(feature = "profiler")]
    pub fn frame_duration_snapshot(&self) -> profiler::FrameDurationSnapshot {
        self.window_profiler.frame_duration_snapshot()
    }

    /// Returns the current mode of the debug frame overlay.
    #[cfg(feature = "profiler")]
    pub fn debug_frame_overlay_mode(&self) -> DebugFrameOverlayMode {
        self.debug_frame_overlay.mode()
    }

    /// Sets the mode of the debug frame overlay and schedules a redraw.
    #[cfg(feature = "profiler")]
    pub fn set_debug_frame_overlay_mode(&mut self, mode: DebugFrameOverlayMode) {
        self.debug_frame_overlay.set_mode(mode);
        // Whole-window: The profiler overlay covers the whole window.
        self.refresh();
    }

    /// Advances the debug frame overlay through its hidden, frame-time-only,
    /// and detailed modes.
    #[cfg(feature = "profiler")]
    pub fn cycle_debug_frame_overlay_mode(&mut self) {
        self.set_debug_frame_overlay_mode(self.debug_frame_overlay.mode().next());
    }

    /// Clears the debug frame overlay's frame-time statistics, except for the
    /// total frame count, and schedules a redraw.
    #[cfg(feature = "profiler")]
    pub fn reset_debug_frame_overlay_stats(&mut self) {
        self.debug_frame_overlay.reset_stats();
        // Whole-window: The profiler overlay covers the whole window.
        self.refresh();
    }

    pub(crate) fn draw_roots(&mut self, cx: &mut App) {
        // The Windows backend may coalesce WM_MOUSEMOVE during hover and drag to avoid starving
        // redraw delivery. Refreshing from the live platform cursor here keeps hover visuals,
        // overlay scrollbars, and drag affordances anchored to the latest pointer position even
        // when intermediate move messages are dropped.
        if self.is_window_hovered() || cx.active_drag.is_some() || self.captured_hitbox.is_some() {
            let ambient = crate::fast::dependencies::AmbientInput::of(self);
            self.mouse_position = self.platform_window.mouse_position();
            self.modifiers = self.platform_window.modifiers();
            self.capslock = self.platform_window.capslock();
            ambient.stamp_changes(self, cx);
        }

        self.invalidator.set_phase(DrawPhase::Prepaint);
        self.debug_assert_scroll_container_context_stack_empty();
        self.sticky_layout_context_stack.clear();
        self.sticky_candidates.clear();
        self.sticky_plans.clear();
        self.next_sticky_layout_context_id = 0;
        self.tooltip_bounds.take();

        if self.a11y.sync_active_flag() {
            // Whole-window: the accessibility tree starts this frame, and content drawn from
            // last frame recorded none to carry.
            self.refreshing = true;
        }
        if self.a11y.is_active() {
            self.a11y.begin_frame();
        } else {
            self.a11y.clear_frame();
        }

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

        // Layout all root elements. Like the root element on the web, which
        // stretches to fill the viewport unless explicitly sized, window roots
        // fill the window when their size is `auto`.
        let scale_factor = self.scale_factor();
        let mut root_element = self.root.as_ref().unwrap().clone().into_any_element();
        let root_layout_id = root_element.request_layout(self, cx);
        self.layout_engine
            .as_mut()
            .unwrap()
            .stretch_auto_size_to_fill(root_layout_id, root_size, scale_factor);
        root_element.prepaint_as_root(Point::default(), root_size.into(), self, cx);

        #[cfg(any(feature = "inspector", debug_assertions))]
        let inspector_element = self.prepaint_inspector(_inspector_width, cx);

        self.prepaint_deferred_draws(cx);

        let mut prompt_element = None;
        let mut active_drag_element = None;
        let mut tooltip_element = None;
        if let Some(prompt) = self.prompt.take() {
            let mut element = prompt.view.any_view().into_any_element();
            let prompt_layout_id = element.request_layout(self, cx);
            self.layout_engine
                .as_mut()
                .unwrap()
                .stretch_auto_size_to_fill(prompt_layout_id, root_size, scale_factor);
            element.prepaint_as_root(Point::default(), root_size.into(), self, cx);
            prompt_element = Some(element);
            self.prompt = Some(prompt);
        } else if let Some(active_drag) = cx.active_drag.take() {
            let mut element = active_drag.view.clone().into_any_element();
            let offset = self.mouse_position() - active_drag.cursor_offset;
            element.prepaint_as_root(offset, AvailableSpace::min_size(), self, cx);
            active_drag_element = Some(element);
            cx.active_drag = Some(active_drag);
        } else {
            tooltip_element = self.prepaint_tooltip(cx);
        }

        self.debug_assert_scroll_container_context_stack_empty();
        self.mouse_hit_test = self.next_frame.hit_test(self.mouse_position);

        // Now actually paint the elements.
        self.invalidator.set_phase(DrawPhase::Paint);
        self.debug_assert_scroll_container_context_stack_empty();
        crate::fast::composition::begin_base(self);
        root_element.paint(self, cx);
        crate::fast::composition::begin_overlay(self);

        #[cfg(any(feature = "inspector", debug_assertions))]
        self.paint_inspector(inspector_element, cx);

        self.paint_deferred_draws(cx);

        if let Some(mut prompt_element) = prompt_element {
            prompt_element.paint(self, cx);
        } else if let Some(mut drag_element) = active_drag_element {
            drag_element.paint(self, cx);
        } else if let Some(mut tooltip_element) = tooltip_element {
            tooltip_element.paint(self, cx);
        }

        #[cfg(any(feature = "inspector", debug_assertions))]
        self.paint_inspector_hitbox(cx);

        // A source may have been switched halfway through the frame: the tree was
        // built for the sources sampled at its start, and it reaches the platform
        // adapter only when the platform source covered the frame's start and end.
        let a11y_active_start_of_frame = self.a11y.is_active();
        let should_send_a11y_update = self.a11y.platform_covers_frame_end();

        if a11y_active_start_of_frame {
            // Harvest frame metadata for the debug dump while the live window
            // and frame are still in scope.
            let frame_info = crate::window::a11y::debug::FrameDebugInfo {
                viewport_size: self.viewport_size,
                scale_factor: self.scale_factor,
                tab_stop_count: self.next_frame.tab_stops.tab_stop_count(),
            };
            // clear the builder state regardless
            let tree_update = self.a11y.end_frame(frame_info);

            if should_send_a11y_update {
                log::debug!(
                    "Sending a11y tree update: {} nodes",
                    tree_update.nodes.len()
                );
                self.platform_window.a11y_tree_update(tree_update);
            }
        } else {
            // No source asked for this frame's tree; a later read must not see an old one.
            self.a11y.forget_last_tree();
        }
        if let Some(root_only) = self
            .a11y
            .end_platform_frame(a11y_active_start_of_frame && should_send_a11y_update)
        {
            log::debug!("Accessibility is off: sending the adapter only the window's root");
            self.platform_window.a11y_tree_update(root_only);
        }
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
            let mut element = tooltip_request.tooltip.view.clone().into_any_element();
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

        // Process deferred draws in multiple rounds to support nesting.
        // Keep draws in place so cached prepaint ranges index the same vector
        // that `reuse_prepaint` slices on the next frame.
        let mut round_start = 0;
        let mut depth = 0;
        loop {
            let round_end = self.next_frame.deferred_draws.len();
            if round_start == round_end {
                break;
            }
            assert!(depth < 10, "Exceeded maximum (10) deferred depth");
            depth += 1;

            let mut traversal_order = (round_start..round_end).collect::<SmallVec<[usize; 8]>>();
            traversal_order.sort_by_key(|ix| self.next_frame.deferred_draws[*ix].priority);

            for deferred_draw_ix in traversal_order {
                let (
                    element,
                    parent_node,
                    current_view,
                    rem_size,
                    absolute_offset,
                    content_mask,
                    prepaint_range,
                ) = {
                    let deferred_draw = &mut self.next_frame.deferred_draws[deferred_draw_ix];
                    self.element_id_stack
                        .clone_from(&deferred_draw.element_id_stack);
                    self.text_style_stack
                        .clone_from(&deferred_draw.text_style_stack);
                    (
                        deferred_draw.element.take(),
                        deferred_draw.parent_node,
                        deferred_draw.current_view,
                        deferred_draw.rem_size,
                        deferred_draw.absolute_offset,
                        deferred_draw.content_mask,
                        deferred_draw.prepaint_range.clone(),
                    )
                };
                self.next_frame.dispatch_tree.set_active_node(parent_node);

                let prepaint_start = self.prepaint_index();
                let enclosing = self.next_frame.deferred_draws[deferred_draw_ix]
                    .fast_enclosing
                    .clone();
                let recording = crate::fast::retained::begin_deferred(self, &enclosing, None, cx);
                if let Some(mut element) = element {
                    self.with_rendered_view(current_view, |window| {
                        window.with_rem_size(Some(rem_size), |window| {
                            window.with_absolute_element_offset(absolute_offset, |window| {
                                window.with_content_mask(content_mask, |window| {
                                    element.prepaint(window, cx);
                                });
                            });
                        });
                    });
                    self.next_frame.deferred_draws[deferred_draw_ix].element = Some(element);
                } else {
                    self.reuse_prepaint(prepaint_range);
                }
                crate::fast::retained::finish_deferred(self, recording, cx);
                let prepaint_end = self.prepaint_index();
                self.next_frame.deferred_draws[deferred_draw_ix].prepaint_range =
                    prepaint_start..prepaint_end;
            }

            self.element_id_stack.clear();
            self.text_style_stack.clear();
            round_start = round_end;
        }
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
                let recording = crate::fast::retained::begin_deferred(
                    self,
                    &deferred_draw.fast_enclosing,
                    Some(deferred_draw.prepaint_range.clone()),
                    cx,
                );
                self.with_rendered_view(deferred_draw.current_view, |window| {
                    window.with_content_mask(content_mask, |window| {
                        window.with_rem_size(Some(deferred_draw.rem_size), |window| {
                            element.paint(window, cx);
                        });
                    })
                });
                crate::fast::retained::finish_deferred(self, recording, cx);
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
            a11y_events_index: self.a11y.nodes.events.len(),
            accessed_element_states_index: self.next_frame.accessed_element_states.len(),
            pending_element_state_transfers_index: self
                .next_frame
                .pending_element_state_transfers
                .len(),
            line_layout_index: self.text_system.layout_index(),
            fast_native_surfaces_index: self.next_frame.fast_native_surfaces.len(),
        }
    }

    pub(crate) fn reuse_prepaint(&mut self, range: Range<PrepaintStateIndex>) {
        crate::fast::composition::reuse_placements(self, &range);
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
        self.next_frame.pending_element_state_transfers.extend(
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

        // Hitboxes retain their identities across cached prepaint, but their
        // dispatch nodes move with the reused subtree in the new frame.
        for hitbox in
            &self.rendered_frame.hitboxes[range.start.hitboxes_index..range.end.hitboxes_index]
        {
            if let Some(node_id) = self.rendered_frame.hitbox_dispatch_nodes.get(&hitbox.id) {
                self.next_frame
                    .hitbox_dispatch_nodes
                    .insert(hitbox.id, reused_subtree.refresh_node_id(*node_id));
            }
        }

        if reused_subtree.contains_focus() {
            self.next_frame.focus = self.focus;
        }

        // The accessibility tree of the reused content, built again from what building it
        // recorded, under the node it sits in now.
        let scale_factor = self.scale_factor();
        self.a11y.replay(
            range.start.a11y_events_index..range.end.a11y_events_index,
            self.focus,
            scale_factor,
        );

        let fast_enclosing = crate::fast::retained::enclosing_retained(self);
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
                    // Copied with the views it was deferred from, it is a
                    // part of the views around them that are being drawn.
                    fast_enclosing: fast_enclosing.clone(),
                }),
        );
    }

    pub(crate) fn paint_index(&self) -> PaintIndex {
        PaintIndex {
            scene_index: self.next_frame.scene.len(),
            #[cfg(any(test, feature = "test-support"))]
            debug_bounds_index: self.next_frame.debug_bounds_records.len(),
            mouse_listeners_index: self.next_frame.mouse_listeners.len(),
            input_handlers_index: self.next_frame.input_handlers.len(),
            cursor_styles_index: self.next_frame.cursor_styles.len(),
            accessed_element_states_index: self.next_frame.accessed_element_states.len(),
            tab_handle_index: self.next_frame.tab_stops.paint_index(),
            line_layout_index: self.text_system.layout_index(),
            a11y_listeners_index: self.a11y.listeners.len(),
            fast_window_control_hitboxes_index: self.next_frame.window_control_hitboxes.len(),
        }
    }

    pub(crate) fn reuse_paint(&mut self, range: Range<PaintIndex>) {
        crate::fast::composition::reuse_starts(self, &range);
        self.a11y
            .reuse_listeners(range.start.a11y_listeners_index..range.end.a11y_listeners_index);
        self.next_frame.window_control_hitboxes.extend(
            self.rendered_frame.window_control_hitboxes[range
                .start
                .fast_window_control_hitboxes_index
                ..range.end.fast_window_control_hitboxes_index]
                .iter()
                .cloned(),
        );
        // Cached elements still exist in the frame even when their paint methods don't run.
        #[cfg(any(test, feature = "test-support"))]
        for (selector, bounds) in &self.rendered_frame.debug_bounds_records
            [range.start.debug_bounds_index..range.end.debug_bounds_index]
        {
            self.next_frame
                .record_debug_bounds(selector.clone(), *bounds);
        }
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
        #[cfg(any(test, feature = "test-support"))]
        self.next_frame
            .fast_mouse_listener_events
            .extend_from_slice(
                &self.rendered_frame.fast_mouse_listener_events
                    [range.start.mouse_listeners_index..range.end.mouse_listeners_index],
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
        self.next_frame.pending_element_state_transfers.extend(
            self.rendered_frame.accessed_element_states[range.start.accessed_element_states_index
                ..range.end.accessed_element_states_index]
                .iter()
                .map(|(id, type_id)| (id.clone(), *type_id)),
        );
        self.next_frame.tab_stops.replay(
            &self.rendered_frame.tab_stops.insertion_history
                [range.start.tab_handle_index..range.end.tab_handle_index],
        );

        crate::fast::fast_stat!(
            self.fast_stats.frame,
            primitives_copied += self
                .rendered_frame
                .scene
                .primitive_count_in(range.start.scene_index..range.end.scene_index)
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

    pub(crate) fn retain_draw_style(&mut self, style: Style) -> usize {
        self.invalidator.debug_assert_prepaint();
        let index = self.retained_styles.len();
        self.retained_styles.push(Some(style));
        index
    }

    pub(crate) fn take_draw_style(&mut self, index: usize) -> Option<Style> {
        self.invalidator.debug_assert_paint_or_prepaint();
        self.retained_styles.get_mut(index)?.take()
    }

    pub(crate) fn replace_draw_style(&mut self, index: usize, style: Style) {
        self.invalidator.debug_assert_prepaint();
        let slot = self
            .retained_styles
            .get_mut(index)
            .expect("retained style index must remain valid for the current draw");
        debug_assert!(slot.is_none());
        *slot = Some(style);
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
        self.with_content_mask_telling(mask, true, f)
    }

    /// [`Self::with_content_mask`] without telling the scene the clip: for a
    /// mask the scene was told of with the clip around it (see
    /// `crate::fast::border_clips`).
    #[inline]
    pub(crate) fn with_untold_content_mask<R>(
        &mut self,
        mask: Option<ContentMask<Pixels>>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.with_content_mask_telling(mask, false, f)
    }

    #[inline]
    fn with_content_mask_telling<R>(
        &mut self,
        mask: Option<ContentMask<Pixels>>,
        tell: bool,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint_or_prepaint();
        if let Some(mask) = mask {
            let is_paint = self.is_paint_phase();
            if tell {
                crate::fast::edge_shadows::push_clip(self, is_paint, &mask);
            }
            let mask = mask.intersect(&self.content_mask());
            let transformed_mask = is_paint.then(|| {
                let mask = crate::fast::layers::paint::transformed_clip(self, mask);
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
                if tell {
                    crate::fast::edge_shadows::pop_clip(self);
                }
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
        crate::fast::edge_shadows::scroll(self, self.is_paint_phase(), true);
        let result = f(self);
        crate::fast::edge_shadows::scroll(self, self.is_paint_phase(), false);
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

    pub(crate) fn reserve_sticky_candidate(&mut self, style: &Style) -> Option<StickyOccurrenceId> {
        if style.position != Position::Sticky {
            return None;
        }
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

        let occurrence = self.sticky_candidates.len();
        self.sticky_candidates.push(StickyCandidate {
            layout_id: None,
            stack_id: style.sticky_stack_id.clone(),
            inset: style.inset,
            horizontal_context,
            vertical_context,
        });
        self.sticky_plans.push(StickyCandidatePlan::default());
        Some(occurrence)
    }

    pub(crate) fn complete_sticky_candidate(
        &mut self,
        occurrence: Option<StickyOccurrenceId>,
        layout_id: LayoutId,
    ) {
        if let Some(candidate) =
            occurrence.and_then(|occurrence| self.sticky_candidates.get_mut(occurrence))
        {
            candidate.layout_id = Some(layout_id);
        }
    }

    pub(crate) fn prepare_sticky_context(
        &mut self,
        context_id: StickyLayoutContextId,
        context: ScrollContainerContext,
    ) {
        self.prepare_sticky_axis(context_id, ScrollAxis::Horizontal, context);
        self.prepare_sticky_axis(context_id, ScrollAxis::Vertical, context);
    }

    pub(crate) fn reorder_sticky_children(
        &self,
        child_layout_ids: &[LayoutId],
        order: &mut [usize],
    ) {
        let layer_orders = child_layout_ids
            .iter()
            .map(|layout_id| {
                self.sticky_candidates
                    .iter()
                    .enumerate()
                    .find_map(|(occurrence, candidate)| {
                        (candidate.layout_id == Some(*layout_id))
                            .then(|| self.sticky_plans[occurrence].layer_order)
                            .flatten()
                    })
            })
            .collect::<SmallVec<[_; 8]>>();
        reorder_sticky_layer_groups(order, &layer_orders);
    }

    fn prepare_sticky_axis(
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
            .sticky_candidates
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

        let mut unkeyed_insets = candidates
            .iter()
            .filter_map(|(_, _, stack_id, start_inset)| {
                stack_id.is_none().then(|| {
                    length_to_pixels(
                        *start_inset,
                        match axis {
                            ScrollAxis::Horizontal => context.scrollport_bounds.size.width,
                            ScrollAxis::Vertical => context.scrollport_bounds.size.height,
                        },
                        rem_size,
                    )
                    .expect("configured sticky candidates have a start inset")
                })
            })
            .collect::<Vec<_>>();
        unkeyed_insets.sort_unstable();
        unkeyed_insets.dedup();
        let relation = StickyRelationIndex::from_candidates(candidates.iter().enumerate().map(
            |(index, (_, _, stack_id, start_inset))| {
                let stacking = stack_id.clone().map_or_else(
                    || {
                        let inset = length_to_pixels(
                            *start_inset,
                            match axis {
                                ScrollAxis::Horizontal => context.scrollport_bounds.size.width,
                                ScrollAxis::Vertical => context.scrollport_bounds.size.height,
                            },
                            rem_size,
                        )
                        .expect("configured sticky candidates have a start inset");
                        let level = unkeyed_insets
                            .binary_search(&inset)
                            .expect("unkeyed sticky inset was collected")
                            .min(u16::MAX as usize) as u16;
                        StickyStacking::Level(level)
                    },
                    StickyStacking::StackId,
                );
                (index, stacking)
            },
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
            .expect("configured sticky candidates have a start inset");
            let offset = if candidates[index].2.is_some() {
                parent_extent + inset
            } else {
                inset
            };
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
                ScrollAxis::Horizontal => self.sticky_plans[*occurrence].x = Some(unchanged),
                ScrollAxis::Vertical => self.sticky_plans[*occurrence].y = Some(unchanged),
            }
        }

        let Some(active_source) = active_source else {
            return;
        };
        let active_source = relation
            .source_at_or_before(active_source)
            .expect("active source is present in the sticky relation");
        let candidate_extents = candidates
            .iter()
            .map(|(_, bounds, _, _)| match axis {
                ScrollAxis::Horizontal => bounds.size.width,
                ScrollAxis::Vertical => bounds.size.height,
            })
            .collect::<Vec<_>>();
        let active_offsets = relation
            .active_chain(active_source)
            .into_iter()
            .map(|index| (index, offsets[index]))
            .collect::<Vec<_>>();
        let active_stack_end = active_offsets
            .last()
            .map(|(index, offset)| *offset + candidate_extents[*index])
            .unwrap_or(px(0.));
        let replacement = relation.next_replacement(active_source);
        let push_offset = replacement
            .map(|(next_candidate, _)| {
                let next_bounds = candidates[next_candidate].1;
                let incoming_start = match axis {
                    ScrollAxis::Horizontal => {
                        next_bounds.origin.x - context.scrollport_bounds.left()
                    }
                    ScrollAxis::Vertical => next_bounds.origin.y - context.scrollport_bounds.top(),
                };
                sticky_push_offset(active_stack_end, incoming_start)
            })
            .unwrap_or(px(0.));
        let retained_depth = replacement.map_or(0, |(_, retained_depth)| retained_depth);
        let layer_count = active_offsets.len() as u32;
        let layer_group = active_offsets
            .first()
            .map(|(index, _)| candidates[*index].0);
        for (depth, (index, offset)) in active_offsets.iter().copied().enumerate() {
            let (occurrence, bounds, _, _) = candidates[index];
            let offset = offset
                + (depth >= retained_depth)
                    .then_some(push_offset)
                    .unwrap_or(px(0.));
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
                ScrollAxis::Horizontal => self.sticky_plans[occurrence].x = Some(resolved),
                ScrollAxis::Vertical => self.sticky_plans[occurrence].y = Some(resolved),
            }
            if let Some(group) = layer_group {
                self.sticky_plans[occurrence].layer_order = Some(StickyLayerOrder {
                    group,
                    rank: layer_count - depth as u32 - 1,
                });
            }
        }
    }

    pub(crate) fn resolve_positioned_bounds_and_sticky_state(
        &self,
        style: &Style,
        layout_bounds: Bounds<Pixels>,
        sticky_occurrence: Option<StickyOccurrenceId>,
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
                if let Some(plan) =
                    sticky_occurrence.and_then(|occurrence| self.sticky_plans.get(occurrence))
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
                let containing_block_bounds =
                    crate::fast::layers::paint::unlayered_content_mask(self).bounds;
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
    /// This affects painting only and does not change layout or hit testing. An element that
    /// paints its children under a transform should also prepaint them under it: cached views
    /// compare the transform while prepainting, and one that is only applied while painting makes
    /// a cached view below it render again instead of replaying.
    pub fn with_element_transform<R>(
        &mut self,
        transformation: Option<TransformationMatrix>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint_or_prepaint();

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

    /// Applies an element's opacity and transform while its children prepaint, as painting
    /// applies them, so cached views below can tell whether either changed.
    pub(crate) fn with_element_paint_context<R>(
        &mut self,
        opacity: Option<f32>,
        transformation: Option<TransformationMatrix>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.with_element_opacity(opacity, |window| {
            window.with_element_transform(transformation, f)
        })
    }

    /// Perform prepaint on child elements in a "retryable" manner, so that any side effects
    /// of prepaints can be discarded before prepainting again. This is used to support autoscroll
    /// where we need to prepaint children to detect the autoscroll bounds, then adjust the
    /// element offset and prepaint again. See [`crate::List`] for an example. This method should only be
    /// called during the prepaint phase of element drawing.
    pub fn transact<T, U>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, U>) -> Result<T, U> {
        self.invalidator.debug_assert_prepaint();
        let index = self.prepaint_index();
        let fast_records = self.next_frame.fast_retained.len();
        let a11y = self.a11y.checkpoint();
        let result = f(self);
        if result.is_err() {
            self.a11y.rollback(a11y);
            self.next_frame.fast_retained.truncate(fast_records);
            self.next_frame
                .fast_native_surfaces
                .truncate(index.fast_native_surfaces_index);
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
            self.next_frame
                .pending_element_state_transfers
                .truncate(index.pending_element_state_transfers_index);
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
        cx.asset_entry::<A>(source).use_by(self.current_view())
    }

    /// Asynchronously load an asset, if the asset hasn't finished loading or doesn't exist this will return None.
    /// Your view will not be re-drawn once the asset has finished loading.
    ///
    /// Note that the multiple calls to this method will only result in one `Asset::load` call at a
    /// time.
    pub fn get_asset<A: Asset>(&mut self, source: &A::Source, cx: &mut App) -> Option<A::Output> {
        cx.fetch_asset::<A>(source)
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

    /// The opacity and transform that primitives painted now would carry.
    pub(crate) fn element_paint_context(&self) -> ElementPaintContext {
        self.invalidator.debug_assert_paint_or_prepaint();
        ElementPaintContext {
            opacity: self.element_opacity,
            transformation: self.current_element_transform(),
        }
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
    pub(crate) fn is_paint_phase(&self) -> bool {
        matches!(self.invalidator.inner.borrow().draw_phase, DrawPhase::Paint)
    }

    fn debug_assert_scroll_container_context_stack_empty(&self) {
        debug_assert!(
            self.scroll_container_context_stack.is_empty(),
            "scroll container context stack should be balanced at frame phase boundaries"
        );
    }

    #[inline]
    pub(crate) fn current_element_transform(&self) -> TransformationMatrix {
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

    /// The paint content mask grown outward to whole device pixels, so device-aligned
    /// strokes such as underlines never clip at a fractional pixel.
    fn snapped_paint_content_mask(&self) -> ContentMask<ScaledPixels> {
        let content_mask = self.paint_content_mask();
        if content_mask.bounds.is_empty() {
            return content_mask;
        }
        if self.fast_mode.snapping() {
            return crate::fast::layers::paint::cover_mask(content_mask);
        }
        ContentMask {
            bounds: Bounds::from_corners(
                point(
                    ScaledPixels(content_mask.bounds.left().0.floor()),
                    ScaledPixels(content_mask.bounds.top().0.floor()),
                ),
                point(
                    ScaledPixels(content_mask.bounds.right().0.ceil()),
                    ScaledPixels(content_mask.bounds.bottom().0.ceil()),
                ),
            ),
            corner_radii: content_mask.corner_radii,
        }
    }

    #[inline]
    pub(crate) fn paint_content_mask(&self) -> ContentMask<ScaledPixels> {
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
                    Self::observe_keyed_state(&new_state, current_view, cx);
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
    #[inline(always)]
    pub fn with_element_state<S, R>(
        &mut self,
        global_id: &GlobalElementId,
        f: impl FnOnce(Option<S>, &mut Self) -> (R, S),
    ) -> R
    where
        S: 'static,
    {
        self.invalidator.debug_assert_paint_or_prepaint();

        let (key, state) = self.take_element_state(global_id, TypeId::of::<S>());

        if let Some(any) = state {
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
            self.insert_element_state(
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
            self.insert_element_state(
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
            fast_enclosing: crate::fast::retained::enclosing_retained(self),
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

    /// Paint the non-inset shadows into the scene for the next frame.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_drop_shadows(
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
        let element_bounds = self.cover_bounds(bounds);
        let element_corner_radii = corner_radii.scale(scale_factor);
        for shadow in shadows {
            if shadow.inset {
                continue;
            }
            let shadow_bounds = (bounds + shadow.offset).dilate(shadow.spread_radius);
            self.next_frame.scene.insert_primitive(Shadow {
                order: 0,
                blur_radius: shadow.blur_radius.scale(scale_factor),
                bounds: crate::fast::layers::paint::shadow_bounds(
                    self,
                    shadow_bounds.scale(scale_factor),
                ),
                content_mask,
                corner_radii: corner_radii.scale(scale_factor),
                color: prepare_color_with_opacity(&shadow.color, opacity),
                element_bounds,
                element_corner_radii,
                inset: 0,
                pad: 0,
                transformation,
            });
        }
    }

    /// Paint the inset shadows into the scene for the next frame.
    pub fn paint_inset_shadows(
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
        let element_bounds = self.cover_bounds(bounds);
        let element_corner_radii = corner_radii.scale(scale_factor);
        for shadow in shadows {
            if !shadow.inset {
                continue;
            }
            let hole = (bounds + shadow.offset).dilate(-shadow.spread_radius);
            let zero = Pixels::ZERO;
            let hole_corner_radii = Corners {
                top_left: (corner_radii.top_left - shadow.spread_radius).max(zero),
                top_right: (corner_radii.top_right - shadow.spread_radius).max(zero),
                bottom_right: (corner_radii.bottom_right - shadow.spread_radius).max(zero),
                bottom_left: (corner_radii.bottom_left - shadow.spread_radius).max(zero),
            };
            self.next_frame.scene.insert_primitive(Shadow {
                order: 0,
                blur_radius: shadow.blur_radius.scale(scale_factor),
                bounds: self.cover_bounds(hole),
                content_mask,
                corner_radii: hole_corner_radii.scale(scale_factor),
                color: prepare_color_with_opacity(&shadow.color, opacity),
                element_bounds,
                element_corner_radii,
                inset: 1,
                pad: 0,
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
                effects: effects
                    .iter()
                    .map(|effect| effect.prepare().scale(scale_factor))
                    .collect(),
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
            background: quad.background.prepare_with_opacity(opacity),
            border_color: quad.border_color.prepare_with_opacity(opacity),
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
    ///
    /// In the web screen-reader mirror, the element whose
    /// [`accessibility_id`](crate::StatefulInteractiveElement::accessibility_id) equals `id`'s
    /// text owns the overlay (`aria-owns`), so screen readers read the overlay's DOM where that
    /// element is in the tree; give the element painting the overlay that id.
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
        path.color = color.prepare_with_opacity(opacity);
        let path = path.scale(scale_factor);
        let path = if self.current_element_transform() == TransformationMatrix::unit() {
            path.with_content_mask(content_mask)
        } else {
            path.transformed(self.current_element_transform(), content_mask)
        };
        let path = crate::fast::layers::paint::snapped_path(self, path);
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
        let underline = self.underline(origin, width, style);
        self.next_frame.scene.insert_primitive(underline);
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

        let height = style.thickness;
        let bounds = Bounds {
            origin: origin.map(|c| ScaledPixels(self.round_edge_to_device_pixel(c.0))),
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
                .as_ref()
                .map(|color| prepare_color_with_opacity(color, opacity))
                .unwrap_or_default(),
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

        let (integer_origin, subpixel_variant) =
            crate::fast::glyphs::glyph_pixel_origin(self.fast_mode.snapping(), glyph_origin);
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
                .get_or_insert_with(params.clone().into(), &mut || {
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
                    color: prepare_color_with_opacity(&color, element_opacity),
                    tile,
                    transformation,
                });
            } else {
                self.next_frame.scene.insert_primitive(MonochromeSprite {
                    order: 0,
                    kind: MonochromeSpriteKind::Glyph,
                    bounds,
                    content_mask,
                    color: prepare_color_with_opacity(&color, element_opacity),
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
                .get_or_insert_with(params.clone().into(), &mut || {
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
                color: prepare_color_with_opacity(&color, element_opacity),
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
                .get_or_insert_with(params.clone().into(), &mut || {
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

    pub(crate) fn should_use_subpixel_rendering(&self, font_id: FontId, font_size: Pixels) -> bool {
        if crate::fast::composition::paints_over_native_content(self) {
            return false;
        }
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
        let integer_origin =
            crate::fast::glyphs::emoji_origin(self.fast_mode.snapping(), glyph_origin);
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
                .get_or_insert_with(params.clone().into(), &mut || {
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
                .get_or_insert_with(params.clone().into(), &mut || {
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
            .map_origin(|value| {
                ScaledPixels(crate::util::round_edge(value.0, self.fast_mode.snapping()))
            })
            .map_size(|size| size.ceil());

        self.next_frame.scene.insert_primitive(MonochromeSprite {
            order: 0,
            kind: MonochromeSpriteKind::ImageMask,
            bounds: final_bounds,
            content_mask,
            color: prepare_color_with_opacity(&color, element_opacity),
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
        image_bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        data: Arc<RenderImage>,
        frame_index: usize,
        grayscale: bool,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let visible_bounds = bounds.intersect(&image_bounds);
        if visible_bounds.size.width <= Pixels::ZERO
            || visible_bounds.size.height <= Pixels::ZERO
            || image_bounds.size.width <= Pixels::ZERO
            || image_bounds.size.height <= Pixels::ZERO
        {
            return Ok(());
        }

        let params = RenderImageParams {
            image_id: data.id,
            frame_index,
        };

        let tile = self
            .sprite_atlas
            .get_or_insert_with(params.into(), &mut || {
                Ok(Some((
                    data.size(frame_index),
                    Cow::Borrowed(
                        data.as_bytes(frame_index)
                            .expect("It's the caller's job to pass a valid frame index"),
                    ),
                )))
            })?
            .expect("Callback above only returns Some");

        let tile = if visible_bounds == image_bounds {
            tile
        } else {
            let x_offset_ratio =
                (visible_bounds.origin.x - image_bounds.origin.x) / image_bounds.size.width;
            let y_offset_ratio =
                (visible_bounds.origin.y - image_bounds.origin.y) / image_bounds.size.height;
            let width_ratio = visible_bounds.size.width / image_bounds.size.width;
            let height_ratio = visible_bounds.size.height / image_bounds.size.height;

            let tile_origin_x = tile.bounds.origin.x.0;
            let tile_origin_y = tile.bounds.origin.y.0;
            let tile_width = tile.bounds.size.width.0;
            let tile_height = tile.bounds.size.height.0;
            let sub_origin_x = tile_origin_x + (x_offset_ratio * tile_width as f32).round() as i32;
            let sub_origin_y = tile_origin_y + (y_offset_ratio * tile_height as f32).round() as i32;
            let sub_width = (width_ratio * tile_width as f32).round() as i32;
            let sub_height = (height_ratio * tile_height as f32).round() as i32;
            let max_x = tile_origin_x + tile_width;
            let max_y = tile_origin_y + tile_height;
            let clamped_origin_x = sub_origin_x.clamp(tile_origin_x, max_x);
            let clamped_origin_y = sub_origin_y.clamp(tile_origin_y, max_y);

            crate::AtlasTile {
                bounds: Bounds {
                    origin: point(
                        DevicePixels(clamped_origin_x),
                        DevicePixels(clamped_origin_y),
                    ),
                    size: size(
                        DevicePixels(sub_width.min(max_x - clamped_origin_x).max(0)),
                        DevicePixels(sub_height.min(max_y - clamped_origin_y).max(0)),
                    ),
                },
                ..tile
            }
        };

        let bounds = self.snap_bounds(visible_bounds);
        let content_mask = self.paint_content_mask();
        let corner_radii = corner_radii
            .clamp_radii_for_quad_size(visible_bounds.size)
            .scale(self.scale_factor());
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
    #[cfg(any(target_os = "macos", target_os = "ios"))]
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
        crate::fast::layers::atlas_entry_removed(self);

        Ok(())
    }

    /// Removes a rendered SVG from the sprite atlas.
    pub fn drop_svg(&mut self, path: SharedString, size: Size<DevicePixels>) -> Result<()> {
        let params = RenderSvgParams { path, size };
        self.sprite_atlas.remove(&params.into());
        crate::fast::layers::atlas_entry_removed(self);
        Ok(())
    }

    /// Returns whether every frame of an image is present in the sprite atlas.
    #[cfg(any(test, feature = "test-support"))]
    pub fn has_image_atlas_entry(&self, data: &RenderImage) -> bool {
        data.frame_count() > 0
            && (0..data.frame_count()).all(|frame_index| {
                self.sprite_atlas.contains(
                    &RenderImageParams {
                        image_id: data.id,
                        frame_index,
                    }
                    .into(),
                )
            })
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
        let key = crate::fast::layout_key::layout_key(self);

        self.layout_engine.as_mut().unwrap().request_layout_ref(
            key,
            &style,
            rem_size,
            scale_factor,
            &cx.layout_id_buffer,
        )
    }

    pub(crate) fn request_layout_ref(
        &mut self,
        style: &Style,
        children: impl IntoIterator<Item = LayoutId>,
        cx: &mut App,
    ) -> LayoutId {
        self.invalidator.debug_assert_prepaint();

        cx.layout_id_buffer.clear();
        cx.layout_id_buffer.extend(children);
        let rem_size = self.rem_size();
        let scale_factor = self.scale_factor();
        let key = crate::fast::layout_key::layout_key(self);

        self.layout_engine.as_mut().unwrap().request_layout_ref(
            key,
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
    #[cfg_attr(feature = "diagnostics", track_caller)]
    pub fn request_measured_layout<F>(&mut self, style: Style, measure: F) -> LayoutId
    where
        F: Fn(Size<Option<Pixels>>, Size<AvailableSpace>, &mut Window, &mut App) -> Size<Pixels>
            + 'static,
    {
        #[cfg(feature = "diagnostics")]
        let kind = crate::taffy::MeasuredLayoutKind::from_source_file(
            std::panic::Location::caller().file(),
        );
        #[cfg(not(feature = "diagnostics"))]
        let kind = crate::taffy::MeasuredLayoutKind::Custom;
        self.invalidator.debug_assert_prepaint();

        let rem_size = self.rem_size();
        let scale_factor = self.scale_factor();
        let key = crate::fast::layout_key::layout_key(self);
        let id = self
            .layout_engine
            .as_mut()
            .unwrap()
            .request_measured_layout_ref(key, &style, rem_size, scale_factor, kind, measure);
        self.note_measured_owner(id, &style);
        id
    }

    /// Notes the view a measured leaf is requested in, for that view to be
    /// built on the next frame unless the leaf is text or sized without its
    /// measurement.
    fn note_measured_owner(&mut self, id: LayoutId, style: &Style) {
        if self.fast_mode.view_retention()
            && let Some(view) = self.rendered_entity_stack.last().copied()
        {
            let fixed = crate::fast::layout::measured_size_is_fixed(style);
            self.fast_measured_owners.push((id, view, fixed));
        }
    }

    #[cfg_attr(feature = "diagnostics", track_caller)]
    pub(crate) fn request_measured_layout_ref<F>(&mut self, style: &Style, measure: F) -> LayoutId
    where
        F: Fn(Size<Option<Pixels>>, Size<AvailableSpace>, &mut Window, &mut App) -> Size<Pixels>
            + 'static,
    {
        #[cfg(feature = "diagnostics")]
        let kind = crate::taffy::MeasuredLayoutKind::from_source_file(
            std::panic::Location::caller().file(),
        );
        #[cfg(not(feature = "diagnostics"))]
        let kind = crate::taffy::MeasuredLayoutKind::Custom;
        self.invalidator.debug_assert_prepaint();

        let rem_size = self.rem_size();
        let scale_factor = self.scale_factor();
        let key = crate::fast::layout_key::layout_key(self);
        let id = self
            .layout_engine
            .as_mut()
            .unwrap()
            .request_measured_layout_ref(key, style, rem_size, scale_factor, kind, measure);
        self.note_measured_owner(id, style);
        id
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

    /// This method should be called during `prepaint`. You can use
    /// the returned [Hitbox] during `paint` or in an event handler
    /// to determine whether the inserted hitbox was the topmost.
    ///
    /// This method should only be called as part of the prepaint phase of element drawing.
    pub fn insert_hitbox(&mut self, bounds: Bounds<Pixels>, behavior: HitboxBehavior) -> Hitbox {
        self.invalidator.debug_assert_prepaint();

        let id = if self.positioned_hitbox_depth > 0 {
            self.next_hitbox_id.positioned()
        } else {
            self.next_hitbox_id
        };
        self.next_hitbox_id = self.next_hitbox_id.next();
        self.insert_hitbox_with_id(id, bounds, behavior)
    }

    /// Prepaints a fixed or sticky subtree. The subtree paints in a positioned
    /// layer, so every hitbox it inserts hit-tests in that layer as well.
    pub(crate) fn with_positioned_hitboxes<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.positioned_hitbox_depth += 1;
        let result = f(self);
        self.positioned_hitbox_depth -= 1;
        result
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
        let content_mask = crate::fast::layers::input::hitbox_mask(self);
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

    /// Forwards the focused input handler's [`TextInputConfiguration`] to the
    /// platform window when it differs from the last forwarded value. With no
    /// input handler the default configuration applies, so a field's
    /// preferences don't outlive its focus.
    fn apply_text_input_configuration(&mut self, cx: &mut App) {
        let configuration = match self.platform_window.take_input_handler() {
            Some(mut input_handler) => {
                let configuration = input_handler.text_input_configuration(self, cx);
                self.platform_window.set_input_handler(input_handler);
                configuration
            }
            None => TextInputConfiguration::default(),
        };
        if self.last_text_input_configuration.as_ref() != Some(&configuration) {
            self.platform_window
                .set_text_input_configuration(configuration.clone());
            self.last_text_input_configuration = Some(configuration);
        }
    }

    /// Register a mouse event listener on the window for the next frame. The type of event
    /// is determined by the first parameter of the given listener. When the next frame is rendered
    /// the listener will be cleared.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn on_mouse_event<Event: MouseEvent>(
        &mut self,
        mut listener: impl FnMut(&Event, DispatchPhase, &mut Window, &mut App) + 'static,
    ) {
        self.invalidator.debug_assert_paint();

        #[cfg(any(test, feature = "test-support"))]
        self.next_frame
            .fast_mouse_listener_events
            .push(std::any::type_name::<Event>());
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

    /// Dispatch a mouse, keyboard, or touch event on the window.
    #[profiling::function]
    pub fn dispatch_event(&mut self, event: PlatformInput, cx: &mut App) -> DispatchEventResult {
        let ambient = crate::fast::dependencies::AmbientInput::of(self);
        let result = self.dispatch_event_and_track_input(event, cx);
        crate::fast::layers::input::after_dispatch(self);
        ambient.stamp_changes(self, cx);
        result
    }

    fn dispatch_event_and_track_input(
        &mut self,
        event: PlatformInput,
        cx: &mut App,
    ) -> DispatchEventResult {
        #[cfg(feature = "profiler")]
        self.window_profiler.begin_input(event.kind_name());
        let update_count_before = self.invalidator.update_count();
        // Track input modality for focus-visible styling and hover suppression.
        // Hover is suppressed during keyboard modality so that keyboard navigation
        // doesn't show hover highlights on the item under the mouse cursor.
        let old_modality = self.last_input_modality;
        self.last_input_modality = match &event {
            PlatformInput::KeyDown(_) => InputModality::Keyboard,
            PlatformInput::MouseMove(_) | PlatformInput::MouseDown(_) => InputModality::Mouse,
            PlatformInput::Touch(_) => InputModality::Touch,
            _ => self.last_input_modality,
        };
        if self.last_input_modality != old_modality {
            // Whole-window: The input modality gates every hover style.
            self.refresh();
        }

        // Handlers may set this to false by calling `stop_propagation`.
        cx.propagate_event = true;
        // Handlers may set this to true by calling `prevent_default`.
        self.default_prevented = false;

        let event = match event {
            // Track the mouse position with our own state, since accessing the platform
            // API for the mouse position can only occur on the main thread.
            PlatformInput::MouseMove(mouse_move) => {
                self.mouse_position = mouse_move.position;
                self.modifiers = mouse_move.modifiers;
                PlatformInput::MouseMove(mouse_move)
            }
            PlatformInput::MouseDown(mouse_down) => {
                self.mouse_position = mouse_down.position;
                self.modifiers = mouse_down.modifiers;
                self.pressed_mouse_button = Some(mouse_down.button);
                PlatformInput::MouseDown(mouse_down)
            }
            PlatformInput::MouseUp(mouse_up) => {
                self.mouse_position = mouse_up.position;
                self.modifiers = mouse_up.modifiers;
                if self.pressed_mouse_button == Some(mouse_up.button) {
                    self.pressed_mouse_button = None;
                }
                PlatformInput::MouseUp(mouse_up)
            }
            PlatformInput::MousePressure(mouse_pressure) => {
                PlatformInput::MousePressure(mouse_pressure)
            }
            PlatformInput::MouseExited(mouse_exited) => {
                self.modifiers = mouse_exited.modifiers;
                PlatformInput::MouseExited(mouse_exited)
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
            // Translate dragging and dropping of external files from the operating system
            // to internal drag and drop events.
            PlatformInput::FileDrop(file_drop) => match file_drop {
                FileDropEvent::Entered { position, paths } => {
                    self.mouse_position = position;
                    let source_window = self.handle.window_id();
                    if !cx.restore_platform_drag(source_window) && cx.active_drag.is_none() {
                        cx.active_drag = Some(AnyDrag {
                            value: Arc::new(paths.clone()),
                            view: cx.new(|_| paths).into(),
                            cursor_offset: position,
                            cursor_style: None,
                            external_payload_source: None,
                        });
                    }
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Modifiers::default(),
                    })
                }
                FileDropEvent::Pending { position } => {
                    self.mouse_position = position;
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Modifiers::default(),
                    })
                }
                FileDropEvent::Submit { position } => {
                    cx.activate(true);
                    self.mouse_position = position;
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    })
                }
                FileDropEvent::Exited => {
                    if !cx.hand_restored_drag_to_platform(self.handle.window_id()) {
                        cx.active_drag.take();
                    }
                    // Whole-window: The drag preview is window-level chrome drawn over every view.
                    self.refresh();
                    PlatformInput::FileDrop(FileDropEvent::Exited)
                }
                FileDropEvent::Ended => {
                    cx.end_platform_drag(self.handle.window_id());
                    // Whole-window: The drag preview is window-level chrome drawn over every view.
                    self.refresh();
                    PlatformInput::FileDrop(FileDropEvent::Ended)
                }
            },
            PlatformInput::Touch(touch) => PlatformInput::Touch(touch),
            PlatformInput::LongPress(long_press) => {
                self.mouse_position = if long_press.phase == crate::TouchPhase::Started {
                    long_press.start_position
                } else {
                    long_press.position
                };
                if long_press.phase == crate::TouchPhase::Started {
                    self.long_press_capture = None;
                }
                PlatformInput::LongPress(long_press)
            }
            PlatformInput::TouchDrag(touch_drag) => {
                self.mouse_position = touch_drag.start_position;
                PlatformInput::TouchDrag(touch_drag)
            }
            PlatformInput::KeyDown(_) | PlatformInput::KeyUp(_) => event,
        };

        crate::fast::layers::input::before_dispatch(self, cx, &event);

        if let Some(any_mouse_event) = event.mouse_event() {
            self.dispatch_mouse_event(any_mouse_event, cx);
        } else if let Some(any_key_event) = event.keyboard_event() {
            self.dispatch_key_event(any_key_event, cx);
        } else if let Some(touch_event) = event.touch_event() {
            self.dispatch_touch_event(touch_event, cx);
        }
        if let PlatformInput::LongPress(long_press) = &event {
            match long_press.phase {
                crate::TouchPhase::Started if !self.default_prevented => {
                    self.long_press_capture = None;
                }
                crate::TouchPhase::Ended | crate::TouchPhase::Cancelled => {
                    self.long_press_capture = None;
                }
                crate::TouchPhase::Started | crate::TouchPhase::Moved => {}
            }
        }

        // Must run after the move is dispatched: the platform owns the gesture afterwards, so this
        // is the last chance for drag listeners to see the pointer leave and reset their state.
        self.promote_external_drag_to_platform(&event, cx);

        let caused_invalidation = self.invalidator.update_count() > update_count_before;
        if caused_invalidation {
            self.input_rate_tracker.borrow_mut().record_input();
        }
        #[cfg(feature = "profiler")]
        self.window_profiler.end_input(caused_invalidation);

        DispatchEventResult {
            propagate: cx.propagate_event,
            default_prevented: self.default_prevented,
        }
    }

    fn promote_external_drag_to_platform(&mut self, event: &PlatformInput, cx: &mut App) {
        let PlatformInput::MouseMove(mouse_move) = event else {
            return;
        };
        if mouse_move.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if Bounds::new(Point::default(), self.viewport_size).contains(&mouse_move.position) {
            return;
        }
        if !self.platform_window.can_start_external_drag() {
            return;
        }
        let Some(payload_source) = cx
            .active_drag
            .as_mut()
            .and_then(|drag| drag.external_payload_source.take())
        else {
            return;
        };
        let Some(payload) = payload_source(self, cx) else {
            return;
        };
        if self.platform_window.start_external_drag(&payload)
            && cx.hand_active_drag_to_platform(self.handle.window_id())
        {
            // Whole-window: The drag preview is window-level chrome drawn over every view.
            self.refresh();
        }
    }

    /// Whether recognized touch pans may use the platform's predicted touch
    /// positions ([`TouchEvent::predicted_position`]) to compensate for input
    /// latency. Defaults to true.
    pub fn touch_prediction_enabled(&self) -> bool {
        self.touch_prediction_enabled
    }

    /// Sets whether recognized touch pans may use the platform's predicted
    /// touch positions. Disabling drops [`TouchEvent::predicted_position`]
    /// before gesture recognition, so pans track only raw touch positions.
    pub fn set_touch_prediction_enabled(&mut self, enabled: bool) {
        self.touch_prediction_enabled = enabled;
    }

    /// Runs the portable gesture recognizer over a raw touch event and
    /// dispatches whatever it resolves (scroll steps, synthesized taps)
    /// through the ordinary mouse-event path.
    fn dispatch_touch_event(&mut self, event: &TouchEvent, cx: &mut App) {
        let mut event = event.clone();
        if !self.touch_prediction_enabled {
            event.predicted_position = None;
        }
        let now = cx.background_executor().now();
        let recognized_gestures = self.touch_gestures.handle_event_at(&event, now);
        if event.phase == crate::TouchPhase::Started
            && let Some(touch_drag) = self.touch_gestures.offer_touch_drag(event.id)
        {
            self.dispatch_recognized_touch_gesture(touch_drag, cx);
        }
        if event.phase == crate::TouchPhase::Started
            && self.touch_gestures.pending_long_press(now).is_some()
        {
            self.long_press_capture = None;
        }
        let mut tapped = false;
        for gesture in recognized_gestures {
            tapped |= matches!(gesture, RecognizedTouchGesture::Tap { .. });
            self.dispatch_recognized_touch_gesture(gesture, cx);
        }
        if event.phase == crate::TouchPhase::Started {
            self.schedule_long_press_timer(cx);
        } else if self.touch_gestures.pending_long_press(now).is_none() {
            self.long_press_timer.take();
        }
        // The platform's touch-release handler may inspect the input handler
        // as soon as this dispatch returns (the web platform decides virtual
        // keyboard visibility there, inside the user gesture). Input handlers
        // are registered during draw, so draw now to make them reflect any
        // focus change the tap just caused.
        if tapped && self.invalidator.is_dirty() {
            self.draw(cx).clear(cx);
        }
        if self.touch_gestures.has_momentum() {
            self.schedule_touch_momentum_tick();
        }
    }

    fn dispatch_recognized_touch_gesture(&mut self, gesture: RecognizedTouchGesture, cx: &mut App) {
        crate::fast::layers::input::before_touch_gesture(self, cx, &gesture);
        match gesture {
            RecognizedTouchGesture::Scroll(scroll_wheel) => {
                self.mouse_position = scroll_wheel.position;
                cx.propagate_event = true;
                self.dispatch_mouse_event(&scroll_wheel, cx);
            }
            RecognizedTouchGesture::Tap { down, up } => {
                self.mouse_position = up.position;
                cx.propagate_event = true;
                self.dispatch_mouse_event(&down, cx);
                cx.propagate_event = true;
                self.dispatch_mouse_event(&up, cx);
            }
            RecognizedTouchGesture::TouchDrag(touch_drag) => {
                self.mouse_position = touch_drag.start_position;
                cx.propagate_event = true;
                self.default_prevented = false;
                let started = touch_drag.phase == crate::TouchPhase::Started;
                self.dispatch_mouse_event(&touch_drag, cx);
                if started {
                    self.touch_gestures
                        .resolve_touch_drag(self.default_prevented);
                }
            }
            RecognizedTouchGesture::LongPress(long_press) => {
                self.mouse_position = if long_press.phase == crate::TouchPhase::Started {
                    long_press.start_position
                } else {
                    long_press.position
                };
                cx.propagate_event = true;
                self.default_prevented = false;
                let started = long_press.phase == crate::TouchPhase::Started;
                let ended = matches!(
                    long_press.phase,
                    crate::TouchPhase::Ended | crate::TouchPhase::Cancelled
                );
                self.dispatch_mouse_event(&long_press, cx);
                if started {
                    let claimed = self.default_prevented;
                    self.touch_gestures.resolve_long_press(claimed);
                    if !claimed {
                        self.long_press_capture = None;
                    }
                }
                if ended {
                    self.long_press_capture = None;
                }
            }
        }
        crate::fast::layers::input::after_dispatch(self);
    }

    fn schedule_long_press_timer(&mut self, cx: &mut App) {
        self.long_press_timer.take();
        let now = cx.background_executor().now();
        let Some((touch_id, duration)) = self.touch_gestures.pending_long_press(now) else {
            return;
        };
        self.long_press_timer = Some(self.spawn(cx, async move |cx| {
            cx.background_executor.timer(duration).await;
            cx.update(move |window, cx| {
                window.long_press_timer.take();
                if let Some(gesture) = window.touch_gestures.offer_long_press(touch_id) {
                    window.dispatch_recognized_touch_gesture(gesture, cx);
                }
            })
            .log_err();
        }));
    }

    fn schedule_touch_momentum_tick(&mut self) {
        self.on_next_frame(|window, cx| {
            let now = cx.background_executor().now();
            if let Some(gesture) = window.touch_gestures.tick_momentum_at(now) {
                window.dispatch_recognized_touch_gesture(gesture, cx);
            }
            if window.touch_gestures.has_momentum() {
                window.schedule_touch_momentum_tick();
            }
        });
    }

    fn dispatch_mouse_event(&mut self, event: &dyn Any, cx: &mut App) {
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

        if cx.has_active_drag() {
            if event.is::<MouseMoveEvent>() {
                // If this was a mouse move event, redraw the window so that the
                // active drag can follow the mouse cursor.
                self.refresh();
            } else if event.is::<MouseUpEvent>() {
                // If this was a mouse up event, cancel the active drag and redraw
                // the window.
                cx.active_drag = None;
                self.refresh();
            }
        }

        // Auto-release pointer capture on mouse up
        if event.is::<MouseUpEvent>() && self.captured_hitbox.is_some() {
            self.captured_hitbox = None;
        }
    }

    // If modifiers are not excessive (e.g. AltGr), and the input handler is accepting text input,
    // we prefer the text input over bindings.
    fn input_preference(&mut self, event: &dyn Any, cx: &mut App) -> InputPreference {
        let prefer_character_input = event
            .downcast_ref::<KeyDownEvent>()
            .is_some_and(|key_down_event| key_down_event.prefer_character_input);
        if !prefer_character_input {
            return InputPreference::KeyBindings;
        }
        let Some(mut input_handler) = self.platform_window.take_input_handler() else {
            return InputPreference::KeyBindings;
        };
        let accepts_text_input = input_handler.accepts_text_input(self, cx);
        self.platform_window.set_input_handler(input_handler);
        if accepts_text_input {
            InputPreference::CharacterInput
        } else {
            InputPreference::KeyBindings
        }
    }

    fn dispatch_key_event(&mut self, event: &dyn Any, cx: &mut App) {
        if self.invalidator.is_dirty() {
            self.draw(cx).clear(cx);
        }

        let node_id = self.focus_node_id_in_rendered_frame(self.focus);
        let dispatch_path = self.rendered_frame.dispatch_tree.dispatch_path(node_id);

        let mut keystroke: Option<Keystroke> = None;

        if let Some(event) = event.downcast_ref::<ModifiersChangedEvent>() {
            if event.modifiers.number_of_modifiers() == 0
                && self.pending_modifier.modifiers.number_of_modifiers() == 1
                && !self.pending_modifier.saw_other_input
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
                self.pending_modifier.saw_other_input = false
            } else if event.modifiers.number_of_modifiers() > 1 {
                self.pending_modifier.saw_other_input = true
            }
            self.pending_modifier.modifiers = event.modifiers
        } else if let Some(key_down_event) = event.downcast_ref::<KeyDownEvent>() {
            self.pending_modifier.saw_other_input = true;
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
            self.finish_dispatch_key_event(
                event,
                None,
                InputPreference::KeyBindings,
                dispatch_path,
                self.context_stack(),
                cx,
            );
            return;
        };

        let input_preference = self.input_preference(event, cx);

        cx.propagate_event = true;
        self.dispatch_keystroke_interceptors(
            &keystroke,
            input_preference,
            self.context_stack(),
            cx,
        );
        if !cx.propagate_event {
            self.finish_dispatch_key_event(
                event,
                Some(&keystroke),
                input_preference,
                dispatch_path,
                self.context_stack(),
                cx,
            );
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
            keystroke.clone(),
            &dispatch_path,
        );

        if !match_result.to_replay.is_empty() {
            self.replay_pending_input(match_result.to_replay, cx);
            cx.propagate_event = true;
        }

        if !match_result.pending.is_empty() {
            let previous_timeout = currently_pending.timeout.take();
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

            let needs_timeout = previous_timeout.is_some()
                || match_result.pending_has_binding
                || text_input_requires_timeout;
            currently_pending.timeout = if needs_timeout {
                match previous_timeout {
                    Some(mut timeout) if timeout.is_paused() => {
                        timeout.reset_duration(PENDING_INPUT_TIMEOUT);
                        Some(timeout)
                    }
                    previous_timeout => {
                        drop(previous_timeout);
                        Some(self.new_pending_input_timeout(PENDING_INPUT_TIMEOUT, cx))
                    }
                }
            } else {
                None
            };
            self.pending_input = Some(currently_pending);
            self.pending_input_changed(cx);
            cx.propagate_event = false;
            return;
        }

        // Interceptors or replayed actions may have changed whether the input handler accepts text.
        let input_preference = self.input_preference(event, cx);
        if input_preference == InputPreference::KeyBindings {
            for binding in match_result.bindings {
                self.dispatch_keyboard_action_on_node(node_id, binding.action.as_ref(), cx);
                if !cx.propagate_event {
                    self.dispatch_keystroke_observers(
                        &keystroke,
                        input_preference,
                        Some(binding.action.as_ref()),
                        match_result.context_stack,
                        cx,
                    );
                    self.pending_input_changed(cx);
                    return;
                }
            }
        }

        self.finish_dispatch_key_event(
            event,
            Some(&keystroke),
            input_preference,
            dispatch_path,
            match_result.context_stack,
            cx,
        );
        self.pending_input_changed(cx);
    }

    fn new_pending_input_timeout(&self, duration: Duration, cx: &App) -> PendingInputTimeout {
        let (started_at, task) = self.start_pending_input_timeout(duration, cx);
        PendingInputTimeout {
            duration,
            remaining: duration,
            state: PendingInputTimeoutState::Running { started_at, task },
        }
    }

    fn start_pending_input_timeout(&self, remaining: Duration, cx: &App) -> (Instant, Task<()>) {
        let started_at = cx.background_executor().now();
        let task = self.spawn(cx, async move |cx| {
            cx.background_executor.timer(remaining).await;
            cx.update(move |window, cx| {
                let Some(currently_pending) = window
                    .pending_input
                    .take()
                    .filter(|pending| pending.focus == window.focus)
                else {
                    return;
                };

                let node_id = window.focus_node_id_in_rendered_frame(window.focus);
                let dispatch_path = window.rendered_frame.dispatch_tree.dispatch_path(node_id);

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
        });
        (started_at, task)
    }

    fn finish_dispatch_key_event(
        &mut self,
        event: &dyn Any,
        recognized_keystroke: Option<&Keystroke>,
        input_preference: InputPreference,
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

        if let Some(keystroke) = recognized_keystroke {
            self.dispatch_keystroke_observers(keystroke, input_preference, None, context_stack, cx);
        }
    }

    pub(crate) fn pending_input_changed(&mut self, cx: &mut App) {
        self.pending_input_observers
            .clone()
            .retain(&(), |callback| callback(self, cx));
    }

    fn defer_pending_input_changed(&self, cx: &mut App) {
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
        self.pending_input().is_some()
    }

    #[cfg(test)]
    pub(crate) fn pending_input_is_none(&self) -> bool {
        self.pending_input.is_none()
    }

    pub(crate) fn clear_pending_keystrokes(&mut self, cx: &mut App) {
        if self.pending_input.take().is_some() {
            self.defer_pending_input_changed(cx);
        }
    }

    /// Returns pending input that can still complete a multi-stroke key binding. Input left over
    /// from a previous focus can never complete one.
    pub fn pending_input(&self) -> Option<PendingInputStatus<'_>> {
        self.pending_input
            .as_ref()
            .filter(|pending_input| pending_input.focus == self.focus)
            .map(|pending_input| PendingInputStatus {
                keystrokes: pending_input.keystrokes.as_slice(),
                timeout: pending_input
                    .timeout
                    .as_ref()
                    .map(PendingInputTimeout::status),
            })
    }

    /// Pauses or resumes the current pending input timeout on behalf of `owner`.
    ///
    /// A paused timeout resumes automatically if `owner` is released. Returns whether the timeout
    /// state changed. A timeout paused by one owner cannot be resumed by another.
    pub fn set_pending_input_timeout_paused<T: 'static>(
        &mut self,
        owner: &Entity<T>,
        paused: bool,
        cx: &mut App,
    ) -> bool {
        let owner_id = owner.entity_id();
        if !paused {
            return self.resume_pending_input_timeout(owner_id, cx);
        }

        let timeout = self
            .pending_input
            .as_ref()
            .filter(|pending_input| pending_input.focus == self.focus)
            .and_then(|pending_input| pending_input.timeout.as_ref());
        let Some(timeout) = timeout else {
            return false;
        };
        if timeout.is_paused() {
            return false;
        }

        let release_subscription = self.observe_release(owner, cx, move |_, window, cx| {
            window.resume_pending_input_timeout(owner_id, cx);
        });
        let now = cx.background_executor().now();
        let changed = self
            .pending_input
            .as_mut()
            .filter(|pending_input| pending_input.focus == self.focus)
            .and_then(|pending_input| pending_input.timeout.as_mut())
            .is_some_and(|timeout| {
                timeout.pause(
                    PendingInputTimeoutPause {
                        owner_id,
                        _release_subscription: release_subscription,
                    },
                    now,
                )
            });

        if changed {
            self.defer_pending_input_changed(cx);
        }
        changed
    }

    fn resume_pending_input_timeout(&mut self, owner_id: EntityId, cx: &mut App) -> bool {
        let Some(remaining) = self
            .pending_input
            .as_ref()
            .and_then(|pending_input| pending_input.timeout.as_ref())
            .filter(|timeout| timeout.pause_owner_id() == Some(owner_id))
            .map(|timeout| timeout.remaining)
        else {
            return false;
        };

        let (started_at, task) = self.start_pending_input_timeout(remaining, cx);
        let changed = self
            .pending_input
            .as_mut()
            .and_then(|pending_input| pending_input.timeout.as_mut())
            .is_some_and(|timeout| timeout.resume(owner_id, started_at, task));

        if changed {
            self.defer_pending_input_changed(cx);
        }
        changed
    }

    /// Returns the currently pending input keystrokes that might result in a multi-stroke key binding.
    pub fn pending_input_keystrokes(&self) -> Option<&[Keystroke]> {
        self.pending_input()
            .map(|pending_input| pending_input.keystrokes())
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
                        &replay.keystroke,
                        InputPreference::KeyBindings,
                        Some(binding.action.as_ref()),
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
                #[cfg(feature = "profiler")]
                self.window_profiler.begin_action_handler(action, cx);
                listener(action.as_any(), DispatchPhase::Capture, cx);
                #[cfg(feature = "profiler")]
                self.window_profiler.end_action_handler();
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
                    #[cfg(feature = "profiler")]
                    self.window_profiler.begin_action_handler(action, cx);
                    listener(any_action, DispatchPhase::Capture, self, cx);
                    #[cfg(feature = "profiler")]
                    self.window_profiler.end_action_handler();

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
                        #[cfg(feature = "profiler")]
                        self.window_profiler.begin_action_handler(action, cx);
                        let outcome = outcome_listener(any_action, self, cx);
                        #[cfg(feature = "profiler")]
                        self.window_profiler.end_action_handler();
                        match outcome {
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
                    #[cfg(feature = "profiler")]
                    self.window_profiler.begin_action_handler(action, cx);
                    listener(any_action, DispatchPhase::Bubble, self, cx);
                    #[cfg(feature = "profiler")]
                    self.window_profiler.end_action_handler();
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

                #[cfg(feature = "profiler")]
                self.window_profiler.begin_action_handler(action, cx);
                listener(action.as_any(), DispatchPhase::Bubble, cx);
                #[cfg(feature = "profiler")]
                self.window_profiler.end_action_handler();
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

    /// Requests that the operating system draw attention to this window.
    pub fn request_attention(&self) {
        self.platform_window.request_attention();
    }

    /// Minimize the current window at the platform level.
    pub fn minimize_window(&self) {
        self.platform_window.minimize();
    }

    /// Toggle full screen status on the current window at the platform level.
    pub fn toggle_fullscreen(&self) {
        self.platform_window.toggle_fullscreen();
    }

    /// Toggle simple (borderless) fullscreen, where the window covers the entire
    /// screen including the menu bar and, on notched displays, the area around the
    /// notch. Unlike [`Window::toggle_fullscreen`], this does not move the window
    /// into its own Mission Control space. Only has an effect on macOS.
    pub fn toggle_simple_fullscreen(&self) {
        self.platform_window.toggle_simple_fullscreen();
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

    /// Returns whether a prompt rendered by GPUI is currently active in this window.
    ///
    /// This is only true for prompts rendered in the window (see
    /// [`App::set_prompt_builder`]), not for platform-native prompt dialogs.
    pub fn has_active_prompt(&self) -> bool {
        self.prompt.is_some()
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
        self.platform_window
            .titlebar_double_click(self.is_resizable, self.is_minimizable);
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

    /// Returns whether accessibility support is enabled for this window.
    ///
    /// This is false when the app was created with [`crate::Application::new_inaccessible`].
    /// Unlike [`Self::is_a11y_active`], this does not depend on whether assistive
    /// technology is currently connected, so it can be used to gate subscriptions
    /// that are only needed for accessibility.
    pub fn is_a11y_enabled(&self) -> bool {
        self.a11y.is_enabled()
    }

    /// Returns whether accessibility features are active for this frame,
    /// i.e. whether assistive technology (such as a screen reader) is
    /// connected and an accessibility tree is being built.
    ///
    /// Use this to skip computing data during rendering that is only
    /// observable through the accessibility tree. When accessibility is
    /// activated, a redraw is forced, so gated work is recomputed before the
    /// next tree update is sent to the platform.
    ///
    /// See the [accessibility guide](crate::_accessibility) for an overview.
    pub fn is_a11y_active(&self) -> bool {
        self.a11y.is_active()
    }

    /// Starts the inspection source for this window: while at least one returned guard lives,
    /// the window builds its accessibility tree on every frame, whether or not assistive
    /// technology is connected and whatever the [`AccessibilityMode`]. Tests and the devtools
    /// read it through [`Self::a11y_tree`]. Inspection never reaches the platform's adapter.
    ///
    /// The first guard forces a full redraw, so the next frame carries a complete tree;
    /// dropping the last guard requests a frame so the tree stops. In an application created
    /// with [`crate::Application::new_inaccessible`] the guard turns nothing on and reports
    /// [`A11yInspection::accessibility_disabled`].
    pub fn begin_a11y_inspection(&mut self) -> A11yInspection {
        let was_active = self.a11y.any_source_on();
        let (guard, first) = self.a11y.begin_inspection(&self.invalidator);
        if first {
            if !was_active {
                self.a11y.forget_last_tree();
            }
            // A full redraw, so content fast mode would reuse is built into the tree too.
            if self.invalidator.not_drawing() {
                self.refresh();
            }
        }
        guard
    }

    /// The last frame's complete accessibility tree while an activation source (the platform's
    /// assistive technology, inspection or [`AccessibilityMode::Always`]) is on, `None`
    /// otherwise. Start inspection with [`Self::begin_a11y_inspection`] and draw a frame to
    /// read a tree without assistive technology.
    pub fn a11y_tree(&self) -> Option<&accesskit::TreeUpdate> {
        self.a11y.last_tree()
    }

    /// While inspecting, the generic container (an element with an id and no role) that holds the
    /// window's focus, when the focused element is one: containers never take the tree's focus,
    /// which screen readers see.
    pub fn a11y_inspection_focus(&self) -> Option<accesskit::NodeId> {
        self.a11y.container_focus()
    }

    /// While inspecting, the global element id the node `node` of [`Self::a11y_tree`] belongs to:
    /// its element's, or for a text leaf or synthetic child its owner's. `None` for the root,
    /// for nodes of frames built without inspection, and while no source is on.
    pub fn a11y_element_id(&self, node: accesskit::NodeId) -> Option<&GlobalElementId> {
        self.a11y.element_id(node)
    }

    /// While inspecting, every node of [`Self::a11y_tree`] with the global element id it belongs
    /// to (see [`Self::a11y_element_id`]).
    pub fn a11y_element_ids(
        &self,
    ) -> impl Iterator<Item = (accesskit::NodeId, &GlobalElementId)> + '_ {
        self.a11y.element_ids()
    }

    /// Debug representation of the last frame's accessibility information.
    pub fn debug_a11y_tree_json(&self) -> Option<String> {
        self.a11y.debug_tree_json()
    }

    /// The id of the accessibility node of the element at `path` being drawn: the one the
    /// path derives ([`GlobalElementId::accesskit_node_id`]), or, for an element that shares
    /// its path with one drawn before it in the frame, the node id the tree gave it instead.
    pub fn a11y_node_id(&self, path: &GlobalElementId) -> accesskit::NodeId {
        let derived = path.accesskit_node_id();
        match self.a11y.element_nodes.last() {
            Some(&(top, id)) if top == derived => id,
            _ => derived,
        }
    }

    /// Register a listener for an accessibility action on a specific node.
    /// The listener will be called when a screen reader requests the given
    /// action on the node identified by `node_id`.
    ///
    /// See the [accessibility guide](crate::_accessibility) for an overview.
    pub fn on_a11y_action(
        &mut self,
        node_id: accesskit::NodeId,
        action: accesskit::Action,
        listener: impl FnMut(Option<&accesskit::ActionData>, &mut Window, &mut App) + 'static,
    ) {
        self.a11y.add_listener(
            node_id,
            crate::window::a11y::A11yListenerAction::Action(action),
            Box::new(listener),
        );
    }

    /// Registers the handler of the custom accessibility action `id` on a node; see
    /// [`crate::StatefulInteractiveElement::on_a11y_custom_action`].
    pub(crate) fn on_a11y_custom_action(
        &mut self,
        node_id: accesskit::NodeId,
        id: i32,
        listener: crate::window::a11y::A11yActionListener,
    ) {
        self.a11y.add_listener(
            node_id,
            crate::window::a11y::A11yListenerAction::Custom(id),
            listener,
        );
    }

    /// Performs an AccessKit action request in process, through the same path a platform
    /// adapter's requests take, and returns whether a handler ran. Tests and the devtools act
    /// on nodes of [`Self::a11y_tree`] with it; a request for a node or action nothing handles
    /// does nothing and returns `false`.
    pub fn perform_a11y_action(&mut self, request: accesskit::ActionRequest, cx: &mut App) -> bool {
        self.handle_a11y_action(request, cx)
    }

    pub(crate) fn handle_a11y_action(
        &mut self,
        request: accesskit::ActionRequest,
        cx: &mut App,
    ) -> bool {
        use crate::window::a11y::A11yListenerAction;
        let wanted = if request.action == accesskit::Action::CustomAction {
            let Some(accesskit::ActionData::CustomAction(id)) = request.data else {
                return false;
            };
            A11yListenerAction::Custom(id)
        } else {
            A11yListenerAction::Action(request.action)
        };
        // Take each listener out while it runs so it can borrow the window mutably, then
        // put it back where it was.
        let matching: Vec<usize> = self
            .a11y
            .rendered_listeners
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                // A handler of a node the tree does not hold (a list row held off screen)
                // answers nothing.
                entry.shown
                    && entry.node == request.target_node
                    && entry.action == wanted
                    && self.a11y.nodes.has_node(entry.node)
            })
            .map(|(index, _)| index)
            .collect();
        let mut matched = false;
        for index in matching {
            let Some(mut listener) = self
                .a11y
                .rendered_listeners
                .get_mut(index)
                .and_then(|entry| entry.listener.take())
            else {
                continue;
            };
            listener(request.data.as_ref(), self, cx);
            matched = true;
            if let Some(entry) = self.a11y.rendered_listeners.get_mut(index)
                && entry.listener.is_none()
            {
                entry.listener = Some(listener);
            }
        }
        if matched {
            return true;
        }
        if request.action == accesskit::Action::CustomAction {
            return false;
        }

        // Fall back to built-in action handling.
        match request.action {
            accesskit::Action::Click => {
                // Only a node that advertises Click (its element has a click listener) is
                // clicked, at its center, as a pointer would.
                let clickable = self.a11y.last_tree().is_some_and(|tree| {
                    tree.nodes.iter().any(|(id, node)| {
                        *id == request.target_node && node.supports_action(accesskit::Action::Click)
                    })
                });
                let Some(bounds) = self
                    .a11y
                    .node_bounds
                    .get(&request.target_node)
                    .copied()
                    .filter(|_| clickable)
                else {
                    return false;
                };
                let center = bounds.center();
                let mouse_down = PlatformInput::MouseDown(crate::MouseDownEvent {
                    button: MouseButton::Left,
                    position: center,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                });
                let mouse_up = PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: center,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                });
                self.dispatch_event(mouse_down, cx);
                self.dispatch_event(mouse_up, cx);
                true
            }
            accesskit::Action::Focus => {
                if let Some(focus_id) = self.a11y.focus_ids.get(&request.target_node).copied()
                    && let Some(handle) = FocusHandle::for_id(focus_id, &cx.focus_handles)
                {
                    self.focus(&handle, cx);
                    true
                } else {
                    false
                }
            }
            accesskit::Action::Blur => {
                if self.focus.is_none() {
                    return false;
                }
                self.blur(cx);
                true
            }
            accesskit::Action::ScrollIntoView => {
                self.scroll_a11y_node_into_view(request.target_node, cx)
            }
            _ => {
                log::debug!(
                    "Unhandled a11y action: {:?} on {:?}",
                    request.action,
                    request.target_node
                );
                false
            }
        }
    }

    /// Moves the scroll containers around a node of the last frame, innermost first, until
    /// the node's bounds are visible in each, through each container's ordinary programmatic
    /// scroll path. Returns whether the node sits in a scroll container.
    fn scroll_a11y_node_into_view(&mut self, node: accesskit::NodeId, cx: &mut App) -> bool {
        use crate::window::a11y::A11yScroll;
        let (Some(mut bounds), Some(&innermost)) = (
            self.a11y.node_bounds.get(&node).copied(),
            self.a11y.node_scroll_targets.get(&node),
        ) else {
            return false;
        };
        let mut next = Some(innermost);
        let mut moved_views = SmallVec::<[EntityId; 2]>::new();
        while let Some(index) = next {
            let Some(target) = self.a11y.scroll_targets.get(index as usize).cloned() else {
                break;
            };
            let port = target.scrollport;
            // How far the content must move so the node's bounds fall inside the scrollport,
            // preferring its leading edge when it is larger than the port.
            let delta = |start: Pixels, end: Pixels, port_start: Pixels, port_end: Pixels| {
                if end - start > port_end - port_start || start < port_start {
                    port_start - start
                } else if end > port_end {
                    port_end - end
                } else {
                    px(0.)
                }
            };
            let wanted = target.offset
                + point(
                    delta(bounds.left(), bounds.right(), port.left(), port.right()),
                    delta(bounds.top(), bounds.bottom(), port.top(), port.bottom()),
                );
            // Clamped as a wheel's scroll is, and only on the axes the container scrolls.
            let offset = point(
                wanted.x.clamp(-target.max.x, px(0.)),
                wanted.y.clamp(-target.max.y, px(0.)),
            );
            if offset != target.offset {
                match &target.scroll {
                    A11yScroll::Handle { handle, view } => {
                        handle.scroll_to(offset, crate::ScrollBehavior::Instant);
                        moved_views.push(*view);
                    }
                    A11yScroll::Offset {
                        offset: cell,
                        id,
                        view,
                    } => {
                        // As the container's wheel listener moves it: fast mode tells the
                        // scroll apart, and the view that painted it draws it again.
                        *cell.borrow_mut() = offset;
                        if let Some(id) = id {
                            crate::fast::layers::invalidate::note_scrolled_by_id(self, id);
                        }
                        moved_views.push(*view);
                    }
                }
                bounds.origin += offset - target.offset;
            }
            next = target.parent;
        }
        for view in moved_views {
            cx.notify(view);
        }
        true
    }

    /// Toggles the inspector mode on this window.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub fn toggle_inspector(&mut self, cx: &mut App) {
        self.inspector = match self.inspector {
            None => Some(cx.new(|_| Inspector::new())),
            Some(_) => {
                self.rendered_frame.next_inspector_instance_ids = FxHashMap::default();
                self.rendered_frame.inspector_hitboxes = FxHashMap::default();
                self.next_frame.next_inspector_instance_ids = FxHashMap::default();
                self.next_frame.inspector_hitboxes = FxHashMap::default();
                None
            }
        };
        // Whole-window: Toggling the inspector draws without caches.
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
        inspector_id: Option<&crate::InspectorElementId>,
        cx: &mut App,
        f: impl FnOnce(&mut Option<T>, &mut Self) -> R,
    ) -> Option<R> {
        let inspector_id = inspector_id?;
        let inspector = self.inspector.as_ref()?;
        if inspector.read(cx).active_element_id() != Some(inspector_id) {
            return None;
        }
        let inspector = inspector.clone();
        Some(inspector.update(cx, |inspector, _cx| {
            inspector.with_active_element_state(self, f)
        }))
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) fn inspector_enabled(&self) -> bool {
        self.inspector.is_some()
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
        if event.downcast_ref::<MouseMoveEvent>().is_some() {
            inspector.update(cx, |inspector, _cx| {
                if let Some((_, inspector_id)) =
                    self.hovered_inspector_hitbox(inspector, &self.rendered_frame)
                {
                    inspector.hover(inspector_id, self);
                }
            });
        } else if event.downcast_ref::<crate::MouseDownEvent>().is_some() {
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
        let event = PlatformInput::MouseMove(MouseMoveEvent {
            position,
            modifiers: self.modifiers,
            pressed_button: None,
        });
        let _ = self.dispatch_event(event, cx);
    }

    #[inline(never)]
    fn take_element_state(
        &mut self,
        global_id: &GlobalElementId,
        state_type: TypeId,
    ) -> ((GlobalElementId, TypeId), Option<ElementStateBox>) {
        let key = (global_id.clone(), state_type);
        self.next_frame.accessed_element_states.push(key.clone());
        let state = self
            .next_frame
            .element_states
            .remove(&key)
            .or_else(|| self.rendered_frame.element_states.remove(&key));
        (key, state)
    }

    #[inline(never)]
    fn insert_element_state(
        &mut self,
        key: (GlobalElementId, TypeId),
        state: ElementStateBox,
    ) -> Option<ElementStateBox> {
        self.next_frame.element_states.insert(key, state)
    }

    #[inline(never)]
    fn observe_keyed_state<S: 'static>(state: &Entity<S>, current_view: EntityId, cx: &mut App) {
        cx.observe(state, move |_, cx| {
            cx.notify(current_view);
        })
        .detach();
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
                root_entity_type_name: std::any::type_name::<V>(),
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
    root_entity_type_name: &'static str,
}

impl AnyWindowHandle {
    /// Get the ID of this window.
    pub fn window_id(&self) -> WindowId {
        self.id
    }

    /// Returns the name of the window's declared root entity type.
    pub fn root_entity_type_name(&self) -> &'static str {
        self.root_entity_type_name
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
    /// A pair of integer IDs.
    IntegerPair(u64, u64),
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
            ElementId::IntegerPair(left, right) => write!(f, "{}-{}", left, right)?,
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

impl From<u64> for ElementId {
    fn from(id: u64) -> Self {
        ElementId::Integer(id)
    }
}

impl From<usize> for ElementId {
    fn from(id: usize) -> Self {
        ElementId::Integer(id as u64)
    }
}

impl From<(usize, usize)> for ElementId {
    fn from((left, right): (usize, usize)) -> Self {
        ElementId::IntegerPair(left as u64, right as u64)
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
    fn sticky_relation_finds_the_next_replacement_and_retained_depth() {
        let relation = StickyRelationIndex::from_candidates([
            (0, StickyStacking::StackId("a".into())),
            (8, StickyStacking::StackId("a".into())),
            (16, StickyStacking::StackId("b".into())),
            (24, StickyStacking::StackId("b".into())),
            (32, StickyStacking::StackId("a".into())),
        ]);

        assert_eq!(relation.next_replacement(0), None);
        assert_eq!(relation.next_replacement(1), Some((2, 0)));
        assert_eq!(relation.next_replacement(2), None);
        assert_eq!(relation.next_replacement(3), Some((4, 0)));
        assert_eq!(relation.next_replacement(4), None);

        let levels = StickyRelationIndex::from_candidates([
            (0, StickyStacking::Level(0)),
            (8, StickyStacking::Level(1)),
            (16, StickyStacking::Level(2)),
            (24, StickyStacking::Level(1)),
            (32, StickyStacking::Level(0)),
        ]);
        assert_eq!(levels.next_replacement(0), None);
        assert_eq!(levels.next_replacement(1), None);
        assert_eq!(levels.next_replacement(2), Some((3, 1)));
        assert_eq!(levels.next_replacement(3), Some((4, 0)));
    }

    #[test]
    fn sticky_push_offset_moves_outgoing_candidates_with_the_incoming_replacement() {
        assert_eq!(sticky_push_offset(px(40.), px(52.)), px(0.));
        assert_eq!(sticky_push_offset(px(40.), px(30.)), px(-10.));
        assert_eq!(sticky_push_offset(px(40.), px(12.)), px(-28.));
        assert_eq!(sticky_push_offset(px(40.), px(0.)), px(-40.));
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

#[inline(never)]
fn with_element_arena_erased(callback: &mut dyn FnMut(&mut Arena)) -> Result<(), AccessError> {
    if let Some(arena_pointer) = CURRENT_ELEMENT_ARENA.try_with(Cell::get)? {
        // SAFETY: The pointer is valid for the duration of the draw operation
        // that set it, and we're being called during that same draw.
        let arena_cell = unsafe { &*arena_pointer };
        callback(&mut arena_cell.borrow_mut());
        Ok(())
    } else {
        ELEMENT_ARENA.try_with(|arena| callback(&mut arena.borrow_mut()))
    }
}

#[cfg(test)]
mod gpux_tests {
    use super::*;
    use crate::{
        Empty, MAX_ACTIVE_SCREEN_EFFECTS, ScreenEffectSnapshotId, ScreenEffectTarget,
        TestAppContext, point, screen, size,
    };

    struct CachedInspectionPanel {
        name: &'static str,
        focus: FocusHandle,
        renders: Rc<Cell<usize>>,
        written: Rc<Cell<u64>>,
    }

    impl Render for CachedInspectionPanel {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let clicked = self.written.clone();
            crate::div().id(self.name).size_full().child(
                crate::div()
                    .id("control")
                    .role(accesskit::Role::Button)
                    .aria_label(self.name)
                    .track_focus(&self.focus)
                    .size_full()
                    .on_key_down(|_, _, _| {})
                    .on_a11y_action(accesskit::Action::Click, move |_, _, _| {
                        clicked.set(clicked.get() + 1);
                    }),
            )
        }
    }

    struct CachedInspectionRoot {
        panels: [Entity<CachedInspectionPanel>; 2],
        reverse: bool,
        prefix: usize,
    }

    impl Render for CachedInspectionRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let order = if self.reverse { [1, 0] } else { [0, 1] };
            crate::div()
                .id("inspection-root")
                .relative()
                .size_full()
                .children((0..self.prefix).map(|_| crate::div().absolute()))
                .children(order.into_iter().map(|index| {
                    self.panels[index]
                        .clone()
                        .cached(crate::StyleRefinement::default().absolute().size_full())
                }))
        }
    }

    #[gpux::test]
    fn sprint_8_task_1_cached_accessibility_survives_clean_frames_and_late_activation(
        cx: &mut TestAppContext,
    ) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, cx| CachedInspectionRoot {
            panels: ["first", "second"].map(|name| {
                cx.new(|cx| CachedInspectionPanel {
                    name,
                    focus: cx.focus_handle().tab_stop(true),
                    renders: Rc::new(Cell::new(0)),
                    written: Rc::new(Cell::new(0)),
                })
            }),
            reverse: false,
            prefix: 0,
        });
        cx.run_until_parked();
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.run_until_parked();
        window
            .update(cx, |root, _, cx| {
                assert_eq!(root.panels[0].read(cx).renders.get(), 1);
            })
            .unwrap();

        assert!(cx.activate_accessibility(window.into()).is_some());
        cx.run_until_parked();
        for iteration in 0..3 {
            let previous_renders = window
                .update(cx, |root, _, cx| {
                    let renders = root.panels[0].read(cx).renders.get();
                    // Do not refresh the window or notify the cached child:
                    // only the parent changes, leaving a genuine cache hit.
                    root.prefix += 1;
                    cx.notify();
                    renders
                })
                .unwrap();
            cx.run_until_parked();
            let target_node = window
                .update(cx, |root, window, cx| {
                    assert_eq!(root.panels[0].read(cx).renders.get(), previous_renders);
                    let tree: serde_json::Value =
                        serde_json::from_str(&window.debug_a11y_tree_json().unwrap()).unwrap();
                    let node = tree["nodes"]
                        .as_object()
                        .unwrap()
                        .values()
                        .find(|node| {
                            node["aria"]["role"] == "Button" && node["aria"]["label"] == "first"
                        })
                        .expect("cached control remains in the native accessibility tree");
                    let id =
                        accesskit::NodeId(node["accesskit_id"].as_str().unwrap().parse().unwrap());
                    assert_eq!(window.a11y.focus_ids[&id], root.panels[0].read(cx).focus.id);
                    assert_eq!(window.a11y.node_bounds[&id].size, size(px(100.), px(100.)));
                    id
                })
                .unwrap();
            cx.dispatch_accessibility_action(
                window.into(),
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target_tree: accesskit::TreeId::ROOT,
                    target_node,
                    data: None,
                },
            );
            window
                .update(cx, |root, _, cx| {
                    assert_eq!(root.panels[0].read(cx).written.get(), iteration + 1);
                })
                .unwrap();
        }
        cx.deactivate_accessibility(window.into());
    }

    #[gpux::test]
    fn sprint_8_task_1_native_invalid_and_disabled_states_follow_changes(cx: &mut TestAppContext) {
        struct StateProbe(bool);
        impl Render for StateProbe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                crate::div()
                    .id("state-probe")
                    .role(accesskit::Role::TextInput)
                    .aria_label("State probe")
                    .aria_invalid(self.0)
                    .aria_disabled(self.0)
                    .size_full()
            }
        }
        // Explicit false must also clear previously authored AccessKit values.
        let mut node = accesskit::Node::new(accesskit::Role::TextInput);
        for value in [true, false] {
            crate::div()
                .id("state-probe")
                .aria_invalid(value)
                .aria_disabled(value)
                .write_a11y_info(&mut node);
            assert_eq!(node.invalid(), value.then_some(accesskit::Invalid::True));
            assert_eq!(node.is_disabled(), value);
        }
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| StateProbe(false));
        assert!(cx.activate_accessibility(window.into()).is_some());
        cx.run_until_parked();
        for value in [false, true, false] {
            window
                .update(cx, |root, _, cx| {
                    root.0 = value;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |_, window, _| {
                    let tree: serde_json::Value =
                        serde_json::from_str(&window.debug_a11y_tree_json().unwrap()).unwrap();
                    let node = tree["nodes"]
                        .as_object()
                        .unwrap()
                        .values()
                        .find(|node| node["aria"]["label"] == "State probe")
                        .unwrap();
                    assert_eq!(node["aria"]["disabled"], value);
                    assert_eq!(
                        node["aria"]["invalid"],
                        value
                            .then_some("True")
                            .map_or(serde_json::Value::Null, Into::into)
                    );
                })
                .unwrap();
        }
        cx.deactivate_accessibility(window.into());
    }

    #[test]
    fn sticky_layer_groups_put_shallower_parents_after_deeper_children() {
        let mut order = [0, 1, 2, 3, 4];
        let layers = [
            Some(StickyLayerOrder { group: 7, rank: 2 }),
            Some(StickyLayerOrder { group: 7, rank: 1 }),
            None,
            Some(StickyLayerOrder { group: 7, rank: 0 }),
            None,
        ];

        reorder_sticky_layer_groups(&mut order, &layers);

        assert_eq!(order, [3, 1, 2, 0, 4]);
    }

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
    fn covering_empty_bounds_at_fractional_scale_does_not_create_pixel_area() {
        let bounds = Bounds::new(point(px(0.), px(69.)), size(px(100.), px(0.)));

        let covered = Window::cover_bounds_at_scale(bounds, 1.75);

        assert!(covered.is_empty());
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
    fn cancel_screen_effect_removes_only_the_requested_active_effect(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
        cx.run_until_parked();

        let (first, second) = window
            .update(cx, |_, window, _| {
                (
                    window.play_screen_effect(
                        screen::ripple(point(px(0.), px(0.))).duration_ms(1_000),
                    ),
                    window.play_screen_effect(
                        screen::ripple(point(px(10.), px(0.))).duration_ms(1_000),
                    ),
                )
            })
            .unwrap();

        let (canceled, duplicate, remaining) = window
            .update(cx, |_, window, _| {
                let canceled = window.cancel_screen_effect(first);
                let duplicate = window.cancel_screen_effect(first);
                let remaining = window
                    .active_screen_effects()
                    .into_iter()
                    .map(|effect| effect.id())
                    .collect::<Vec<_>>();
                (canceled, duplicate, remaining)
            })
            .unwrap();

        assert!(canceled);
        assert!(!duplicate);
        assert_eq!(remaining, vec![second]);
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
            .update(cx, |_, window, _| window.effect_diagnostics())
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
            .update(cx, |_, window, _| window.effect_diagnostics())
            .unwrap();
        assert_eq!(effects.screen_effect_snapshot_count, 0);
    }

    #[gpux::test]
    fn effect_diagnostics_report_missing_snapshot_transition(cx: &mut TestAppContext) {
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
            .update(cx, |_, window, _| window.effect_diagnostics())
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
    fn effect_diagnostics_report_active_missing_region_and_backend_support(
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
            .update(cx, |_, window, _| window.effect_diagnostics())
            .unwrap();

        assert_eq!(effects.active_screen_effect_count, 1);
        assert_eq!(effects.visual_effect_count, 0);
        assert_eq!(effects.screen_effect_region_count, 0);
        assert_eq!(effects.missing_screen_effect_region_count, 1);
        assert_eq!(effects.capabilities, crate::EffectCapabilities::default());
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        path::PathBuf,
        rc::Rc,
        time::Duration,
    };

    use crate::{
        AnyWindowHandle, AppContext as _, Bounds, ContentMask, Context, DispatchPhase,
        DragMoveEvent, Empty, ExternalDragPayload, ExternalPaths, FileDragPaths, FileDropEvent,
        FocusHandle, InputEvent as _, InteractiveElement as _, IntoElement, KeyDownEvent,
        Keystroke, LongPressEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement,
        Pixels, PlatformInput, Point, Render, RequestFrameOptions, ScaledPixels,
        StatefulInteractiveElement as _, Styled, TestAppContext, TouchDragEvent, TouchEvent,
        TouchId, TouchPhase, Underline, UnderlineStyle, Window, WindowAppearance, WindowOptions,
        canvas, div, hsla, point, px, size,
    };

    #[cfg(feature = "profiler")]
    #[gpux::test]
    fn test_frame_requests_and_skips_are_journaled(cx: &mut TestAppContext) {
        use crate::FrameRequestSource;
        use crate::profiler::journal::{
            self, ForegroundJournalEntry, FrameSkipReason, IntervalBoundary,
        };
        use scheduler::Instant;

        cx.app.borrow_mut().mode = crate::app::GpuiMode::Production;
        let window = cx.add_window(|_, _| EmptyView);
        let platform_window = cx.test_window(window.into());
        platform_window.simulate_frame_request(RequestFrameOptions::default());
        let (journal, _guard) = journal::install_test_foreground_journal(64, 4);
        let mut collector = journal.collector();

        for (signal_at, source) in [
            (Some(Instant::now()), FrameRequestSource::NativeCallback),
            (Some(Instant::now()), FrameRequestSource::LocalSchedule),
            (None, FrameRequestSource::NativeCallback),
        ] {
            platform_window.simulate_frame_request(RequestFrameOptions {
                signal_at,
                signal_source: source,
                require_presentation: true,
                ..Default::default()
            });
            let entries = collector.collect_unseen().entries;
            let signal_index = entries
                .iter()
                .position(|entry| {
                    matches!(
                        entry,
                        ForegroundJournalEntry::PlatformSignal(signal)
                            if signal.window_id == window.window_id()
                                && signal.signal_at == signal_at
                                && signal.source == source
                                && signal_at.is_none_or(|at| signal.handled_at >= at)
                    )
                })
                .expect("frame request metadata");
            let skip_index = entries
                .iter()
                .position(|entry| {
                    matches!(
                        entry,
                        ForegroundJournalEntry::Boundary(IntervalBoundary::FrameSkipped(skipped))
                            if skipped.window_id == window.window_id()
                                && skipped.reason == FrameSkipReason::NoRenderNeeded
                    )
                })
                .expect("completed no-render boundary");
            assert!(signal_index < skip_index);
        }
    }

    #[cfg(feature = "profiler")]
    #[gpux::test]
    fn test_frame_demand_survives_throttling(cx: &mut TestAppContext) {
        use crate::profiler::journal::{
            self, ForegroundJournalEntry, FrameSkipReason, IntervalBoundary,
        };
        use scheduler::Instant;

        // Test mode draws during effect flushing, bypassing platform frame throttling.
        cx.app.borrow_mut().mode = crate::app::GpuiMode::Production;
        let (journal, _guard) = journal::install_test_foreground_journal(256, 4);
        let mut collector = journal.collector();
        for initially_dirty in [true, false] {
            // Keep the throttle closed without depending on short wall-clock deadlines.
            let window = cx
                .app
                .borrow_mut()
                .open_window(
                    WindowOptions {
                        inactive_frame_interval: Some(Duration::from_secs(3600)),
                        ..Default::default()
                    },
                    |_, cx| cx.new(|_| EmptyView),
                )
                .expect("open inactive test window");
            let platform_window = cx.test_window(window.into());
            platform_window.simulate_active_status_change(false);
            platform_window.simulate_frame_request(RequestFrameOptions::default());
            let callback_ran = Rc::new(Cell::new(false));
            let draws_before = window
                .update(cx, |_, window, _| {
                    let draws_before = window
                        .frame_duration_snapshot()
                        .draw_duration_histogram
                        .len();
                    if initially_dirty {
                        window.refresh();
                    }
                    window.on_next_frame({
                        let callback_ran = callback_ran.clone();
                        move |window, _| {
                            callback_ran.set(true);
                            window.refresh();
                        }
                    });
                    draws_before
                })
                .expect("schedule throttled frame");
            // Skips only seal retained work, so give the throttled skip some.
            let input_at = Instant::now();
            journal::record_input(journal::InputTiming {
                kind: "test",
                start: input_at,
                end: input_at,
                caused_invalidation: false,
            });
            collector.collect_unseen();
            platform_window.simulate_frame_request(RequestFrameOptions {
                signal_at: Some(Instant::now()),
                ..Default::default()
            });
            window
                .update(cx, |_, window, _| {
                    assert_eq!(
                        window
                            .frame_duration_snapshot()
                            .draw_duration_histogram
                            .len(),
                        draws_before,
                    );
                })
                .expect("inspect deferred frame");
            assert!(!callback_ran.get());
            assert!(collector.collect_unseen().entries.iter().any(|entry| {
                matches!(
                    entry,
                    ForegroundJournalEntry::Boundary(IntervalBoundary::FrameSkipped(skipped))
                        if skipped.window_id == window.window_id()
                            && skipped.reason == FrameSkipReason::InactiveFrameRateLimit
                )
            }));
            platform_window.simulate_frame_request(RequestFrameOptions {
                signal_at: Some(Instant::now()),
                require_presentation: true,
                ..Default::default()
            });
            window
                .update(cx, |_, window, _| {
                    assert_eq!(
                        window
                            .frame_duration_snapshot()
                            .draw_duration_histogram
                            .len(),
                        draws_before + 1,
                    );
                })
                .expect("inspect presented frame");
            assert!(callback_ran.get());
        }
    }

    /// Visibility transitions reach observers exactly once each, with the new
    /// state already stored on the window, and never wake the platform for a
    /// frame: the platform requests one itself when it resumes presenting.
    #[gpux::test]
    fn test_window_visibility(cx: &mut TestAppContext) {
        use crate::WindowVisibility;

        let window = cx.add_window(|_, _| EmptyView);
        let observed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = window
            .update(cx, {
                let observed = observed.clone();
                move |_, window, _| {
                    assert_eq!(window.visibility(), WindowVisibility::Visible);
                    assert!(window.is_visible());
                    window.observe_window_visibility(move |visibility, window, _| {
                        assert_eq!(window.visibility(), visibility);
                        observed.borrow_mut().push(visibility);
                    })
                }
            })
            .unwrap();
        let test_window = cx.test_window(window.into());
        let frame_wake_count = test_window.frame_wake_count();

        test_window.simulate_visibility_change(WindowVisibility::Hidden);
        assert_eq!(*observed.borrow(), [WindowVisibility::Hidden]);
        window
            .update(cx, |_, window, _| assert!(!window.is_visible()))
            .unwrap();

        // Platforms may report the same state again; observers only see changes.
        test_window.simulate_visibility_change(WindowVisibility::Hidden);
        assert_eq!(observed.borrow().len(), 1);

        test_window.simulate_visibility_change(WindowVisibility::Visible);
        assert_eq!(
            *observed.borrow(),
            [WindowVisibility::Hidden, WindowVisibility::Visible]
        );
        window
            .update(cx, |_, window, _| assert!(window.is_visible()))
            .unwrap();
        assert_eq!(test_window.frame_wake_count(), frame_wake_count);
    }

    #[gpux::test]
    fn test_fully_visible_bounds_preserve_layout_viewport(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| EmptyView);
        let mut platform_window = cx.test_window(window.into());
        platform_window.simulate_resize(size(px(400.), px(800.)));
        window
            .update(cx, |_, window, _| {
                assert_eq!(
                    window.visual_viewport_bounds(),
                    Bounds::new(Point::default(), size(px(400.), px(800.)))
                );
                assert_eq!(
                    window.fully_visible_bounds(),
                    window.visual_viewport_bounds()
                );
            })
            .unwrap();

        platform_window.simulate_frame_request(RequestFrameOptions::default());
        let wakes = platform_window.frame_wake_count();
        let visual_bounds = Bounds::new(point(px(10.), px(40.)), size(px(380.), px(460.)));
        platform_window.simulate_visual_viewport_change(visual_bounds);
        assert!(platform_window.frame_wake_count() > wakes);
        platform_window.simulate_frame_request(RequestFrameOptions::default());
        let wakes = platform_window.frame_wake_count();
        platform_window.simulate_insets_change(crate::WindowInsets {
            safe_area: crate::Edges {
                top: px(60.),
                right: px(20.),
                bottom: px(30.),
                left: px(-10.),
            },
            ime: crate::Edges {
                bottom: px(350.),
                ..Default::default()
            },
        });
        assert!(platform_window.frame_wake_count() > wakes);
        window
            .update(cx, |_, window, _| {
                assert_eq!(window.viewport_size(), size(px(400.), px(800.)));
                assert_eq!(window.visual_viewport_bounds(), visual_bounds);
                assert_eq!(
                    window.fully_visible_bounds(),
                    Bounds::new(point(px(10.), px(60.)), size(px(370.), px(390.)))
                );
                window.request_virtual_keyboard();
                window.dismiss_virtual_keyboard();
                assert_eq!(window.viewport_size(), size(px(400.), px(800.)));
            })
            .unwrap();
        assert_eq!(platform_window.virtual_keyboard_requests(), 1);
        assert_eq!(platform_window.virtual_keyboard_dismissals(), 1);

        platform_window.simulate_insets_change(crate::WindowInsets {
            safe_area: crate::Edges {
                top: px(900.),
                left: px(500.),
                ..Default::default()
            },
            ..Default::default()
        });
        window
            .update(cx, |_, window, _| {
                assert_eq!(
                    window.fully_visible_bounds().size,
                    size(Pixels::ZERO, Pixels::ZERO)
                );
            })
            .unwrap();
    }

    struct EmptyView;

    impl Render for EmptyView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    struct OpensWindowOnPaint {
        opened: Rc<Cell<bool>>,
    }

    impl Render for OpensWindowOnPaint {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let opened = self.opened.clone();
            div()
                .size_full()
                .child(canvas(
                    |_, _, _| {},
                    move |_, _, _window, cx| {
                        if !opened.replace(true) {
                            cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| EmptyView))
                                .unwrap();
                        }
                    },
                ))
                // Siblings painted after the canvas: their elements were
                // allocated in the arena before the nested draw, so they detect
                // a mid-draw arena clear when painted afterwards.
                .child(div().child("after"))
        }
    }

    /// Opening a window synchronously draws it and requests an element arena
    /// clear. When that happens from within another window's draw (here: from
    /// an element's paint), the clear must be deferred until the outer draw
    /// finishes, or the outer draw's arena-allocated elements would be freed
    /// out from under it.
    #[test]
    fn test_window_opened_during_draw_defers_arena_clear() {
        let mut cx = TestAppContext::single();

        let opened = Rc::new(Cell::new(false));
        // add_window draws once, which runs the nested open_window mid-draw.
        let window = cx.add_window({
            let opened = opened.clone();
            move |_, _| OpensWindowOnPaint { opened }
        });

        assert!(opened.get());
        assert_eq!(cx.windows().len(), 2);

        // The deferred clear must actually run once the outer draw unwinds:
        // subsequent draws of both windows work against a fresh arena.
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
    }

    #[test]
    fn test_scale_factor_change_preserves_bounds_and_survives_resize() {
        let mut cx = TestAppContext::single();
        let window = cx.add_window(|_, _| EmptyView);
        let handle: AnyWindowHandle = window.into();
        let window_state = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, _| {
                (
                    window.scale_factor(),
                    window.bounds(),
                    window.viewport_size(),
                )
            })
            .unwrap()
        };

        let (scale_factor, mut expected_bounds, _) = window_state(&mut cx);
        assert_eq!(scale_factor, 2.0);

        for (scale_factor, resized_size) in [
            (1.0, size(px(800.), px(600.))),
            (1.25, size(px(640.), px(480.))),
            (2.0, size(px(1024.), px(768.))),
        ] {
            cx.simulate_window_scale_factor_change(handle, scale_factor);
            assert_eq!(
                window_state(&mut cx),
                (scale_factor, expected_bounds, expected_bounds.size)
            );

            cx.simulate_window_resize(handle, resized_size);
            expected_bounds.size = resized_size;
            assert_eq!(
                window_state(&mut cx),
                (scale_factor, expected_bounds, resized_size)
            );
        }
    }

    /// Platforms that stop requesting frames for idle windows (currently web)
    /// rely on the frame waker firing whenever frame demand arises; a demand
    /// source that skips the waker shows up there as a window that silently
    /// stops repainting until unrelated activity wakes it.
    #[gpux::test]
    fn test_frame_waker_fires_on_frame_demand(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| EmptyView);
        let test_window = cx.test_window(window.into());

        // Windows start dirty, and that can predate waker installation;
        // installing the waker must deliver the pending wake or the first
        // frame would never be requested.
        assert!(
            test_window.frame_wake_count() >= 1,
            "opening a window must wake the frame source for the initial frame"
        );

        // Serve outstanding demand (present the frame drawn by `add_window`).
        test_window.simulate_frame_request(RequestFrameOptions::default());

        // An idle window must not wake on clean frames or plain updates, or
        // the frame source could never stop.
        let baseline = test_window.frame_wake_count();
        test_window.simulate_frame_request(RequestFrameOptions::default());
        window.update(cx, |_, _, _| {}).unwrap();
        assert_eq!(
            test_window.frame_wake_count(),
            baseline,
            "clean frames and non-notifying updates must not wake the frame source"
        );

        // Notifying a view in an idle window is the core demand signal.
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        assert!(
            test_window.frame_wake_count() > baseline,
            "notifying a view in an idle window must wake the frame source"
        );

        // Serving that demand returns to idle without further wakes.
        test_window.simulate_frame_request(RequestFrameOptions::default());
        let baseline = test_window.frame_wake_count();
        test_window.simulate_frame_request(RequestFrameOptions::default());
        assert_eq!(
            test_window.frame_wake_count(),
            baseline,
            "serving demand must return the window to idle"
        );

        // Next-frame callbacks create demand without dirtying the window.
        window
            .update(cx, |_, window, _| window.on_next_frame(|_, _| {}))
            .unwrap();
        assert!(
            test_window.frame_wake_count() > baseline,
            "scheduling a next-frame callback in an idle window must wake the frame source"
        );
    }

    /// A frame request that arrives while next-frame callbacks are pending
    /// must never strand them: either the frame runs them, or (when the
    /// inactive-window frame-rate throttle defers the frame) the waker fires
    /// so another request is delivered.
    #[gpux::test]
    fn test_pending_next_frame_callbacks_are_not_stranded(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| EmptyView);
        let test_window = cx.test_window(window.into());
        // Establish a recent last-frame time so the inactive-window throttle
        // can engage on the next request.
        test_window.simulate_frame_request(RequestFrameOptions::default());

        let callback_ran = Rc::new(Cell::new(false));
        window
            .update(cx, {
                let callback_ran = callback_ran.clone();
                move |_, window, _| {
                    window.on_next_frame(move |_, _| callback_ran.set(true));
                }
            })
            .unwrap();

        let baseline = test_window.frame_wake_count();
        test_window.simulate_frame_request(RequestFrameOptions::default());
        // The test window is inactive, so this request throttles to ~30fps
        // when it lands within the throttle interval of the previous frame
        // (the common case here, but timing-dependent): the callback is
        // deferred and the waker must re-arm the frame source. On a slow run
        // the request instead lands outside the interval and runs the
        // callback directly.
        assert!(
            test_window.frame_wake_count() > baseline || callback_ran.get(),
            "a frame request with pending next-frame callbacks must either run them or re-arm the frame source"
        );
    }

    #[gpux::test]
    fn test_window_reports_no_raw_handle_instead_of_panicking(cx: &mut TestAppContext) {
        use raw_window_handle::{HandleError, HasDisplayHandle as _, HasWindowHandle as _};

        let window = cx.add_window(|_, _| EmptyView);
        window
            .update(cx, |_, window, _| {
                assert!(matches!(
                    window.window_handle(),
                    Err(HandleError::NotSupported)
                ));
                assert!(matches!(
                    window.display_handle(),
                    Err(HandleError::NotSupported)
                ));
            })
            .unwrap();
    }

    #[gpux::test]
    fn test_appearance_change_runs_after_app_update(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| EmptyView);
        let observed_appearance = Rc::new(Cell::new(None));
        let _subscription = window
            .update(cx, {
                let observed_appearance = observed_appearance.clone();
                move |_, window, _| {
                    window.observe_window_appearance(move |window, _| {
                        observed_appearance.set(Some(window.appearance()));
                    })
                }
            })
            .unwrap();
        let test_window = cx.test_window(window.into());

        cx.update(|_| {
            test_window.simulate_appearance_change(WindowAppearance::Dark);
            assert_eq!(observed_appearance.get(), None);
        });
        cx.run_until_parked();

        assert_eq!(observed_appearance.get(), Some(WindowAppearance::Dark));
    }

    #[gpux::test]
    fn queued_frame_callback_wakes_a_parked_render_loop(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| Empty);
        let test_window = cx.test_window(window.into());

        assert!(test_window.simulate_scheduled_frame());
        assert!(test_window.simulate_scheduled_frame());
        assert!(!test_window.frame_scheduled());

        cx.update_window(window.into(), |_, window, _| {
            window.active.set(true);
            window.on_next_frame(|_, _| {});
        })
        .unwrap();
        assert!(
            test_window.frame_scheduled(),
            "queuing work on a parked window must wake the render loop"
        );

        assert!(test_window.simulate_scheduled_frame());
        assert!(
            test_window.frame_scheduled(),
            "presenting the frame must await one compositor callback"
        );
        assert!(test_window.simulate_scheduled_frame());
        assert!(!test_window.frame_scheduled());
    }

    #[gpux::test]
    fn pending_presentation_wakes_a_parked_render_loop(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| Empty);
        let test_window = cx.test_window(window.into());

        assert!(test_window.simulate_scheduled_frame());
        assert!(test_window.simulate_scheduled_frame());
        assert!(!test_window.frame_scheduled());

        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();

        assert!(
            test_window.frame_scheduled(),
            "a rendered scene awaiting presentation must wake the render loop"
        );
    }

    #[gpux::test]
    fn callback_queued_during_a_frame_requests_a_follow_up(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| Empty);
        let test_window = cx.test_window(window.into());

        let callback_ran = Rc::new(Cell::new(false));
        cx.update_window(window.into(), |_, window, _| {
            // Inactive windows are frame-rate throttled, which would defer the
            // ticks this test drives manually.
            window.active.set(true);
            let callback_ran = callback_ran.clone();
            window.on_next_frame(move |window, _| {
                window.on_next_frame(move |_, _| callback_ran.set(true));
            });
        })
        .unwrap();

        assert!(test_window.simulate_scheduled_frame());
        assert!(!callback_ran.get());
        assert!(
            test_window.frame_scheduled(),
            "a callback queued mid-frame must schedule a follow-up before the loop parks"
        );

        assert!(test_window.simulate_scheduled_frame());
        assert!(callback_ran.get());
    }

    struct RootView {
        explicit_size: bool,
        child_bounds: Rc<Cell<Bounds<Pixels>>>,
    }

    impl Render for RootView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let child_bounds = self.child_bounds.clone();
            let root = div().flex().flex_col().child(
                canvas(
                    move |bounds, _, _| child_bounds.set(bounds),
                    |_, _, _, _| {},
                )
                .size_full(),
            );
            if self.explicit_size {
                root.w(px(300.)).h(px(200.))
            } else {
                root
            }
        }
    }

    #[test]
    fn auto_sized_window_root_fills_the_window() {
        let mut cx = TestAppContext::single();
        let child_bounds = Rc::new(Cell::new(Bounds::default()));
        let window = cx.add_window({
            let child_bounds = child_bounds.clone();
            move |_, _| RootView {
                explicit_size: false,
                child_bounds,
            }
        });

        let viewport_size = cx
            .update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear(cx);
                window.viewport_size()
            })
            .unwrap();

        assert_eq!(child_bounds.get().size, viewport_size);
    }

    #[test]
    fn explicitly_sized_window_root_keeps_its_size() {
        let mut cx = TestAppContext::single();
        let child_bounds = Rc::new(Cell::new(Bounds::default()));
        let window = cx.add_window({
            let child_bounds = child_bounds.clone();
            move |_, _| RootView {
                explicit_size: true,
                child_bounds,
            }
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
        })
        .unwrap();

        assert_eq!(child_bounds.get().size, size(px(300.), px(200.)));
    }

    struct FileDragView {
        path: PathBuf,
        observed_drag_moves: Rc<RefCell<Vec<Point<Pixels>>>>,
        observed_drops: Rc<RefCell<Vec<PathBuf>>>,
    }

    struct FileDropExitView(Rc<Cell<usize>>);

    impl Render for FileDropExitView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().on_file_drop_exit({
                let observed_file_drop_exit = self.0.clone();
                move |_, _, _| observed_file_drop_exit.set(observed_file_drop_exit.get() + 1)
            })
        }
    }

    impl Render for FileDragView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("file-drag")
                .size_full()
                .on_drag(self.path.clone(), |_, _, _, cx| cx.new(|_| Empty))
                .external_drag_payload(|path: &PathBuf, _, _| {
                    Some(ExternalDragPayload::Files(FileDragPaths::new([(
                        path.clone(),
                        true,
                    )])))
                })
                .on_drag_move({
                    let observed_drag_moves = self.observed_drag_moves.clone();
                    move |event: &DragMoveEvent<PathBuf>, _, _| {
                        observed_drag_moves.borrow_mut().push(event.event.position);
                    }
                })
                .on_drop({
                    let observed_drops = self.observed_drops.clone();
                    move |path: &PathBuf, _, _| observed_drops.borrow_mut().push(path.clone())
                })
        }
    }

    #[gpux::test]
    fn file_drag_is_promoted_once_and_restored_in_source_window(cx: &mut TestAppContext) {
        struct Drag {
            window: AnyWindowHandle,
            observed_drag_moves: Rc<RefCell<Vec<Point<Pixels>>>>,
            observed_drops: Rc<RefCell<Vec<PathBuf>>>,
        }

        fn start_drag(cx: &mut TestAppContext, path: PathBuf, platform_result: bool) -> Drag {
            let observed_drag_moves = Rc::new(RefCell::new(Vec::new()));
            let observed_drops = Rc::new(RefCell::new(Vec::new()));
            let window: AnyWindowHandle = cx
                .add_window({
                    let observed_drag_moves = observed_drag_moves.clone();
                    let observed_drops = observed_drops.clone();
                    move |_, _| FileDragView {
                        path,
                        observed_drag_moves,
                        observed_drops,
                    }
                })
                .into();
            cx.test_window(window)
                .set_start_external_drag_result(platform_result);

            let update_result = cx.update_window(window, |_, window, cx| {
                window.draw(cx).clear(cx);
                window.dispatch_event(
                    MouseDownEvent {
                        position: point(px(10.), px(10.)),
                        button: MouseButton::Left,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    cx,
                );
                window.dispatch_event(
                    MouseMoveEvent {
                        position: point(px(20.), px(20.)),
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Default::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
                assert!(cx.active_drag.is_some());
            });
            assert!(
                update_result.is_ok(),
                "failed to start drag: {update_result:?}"
            );

            assert!(cx.test_window(window).external_drag_files().is_empty());
            Drag {
                window,
                observed_drag_moves,
                observed_drops,
            }
        }

        let successful_path = PathBuf::from("/tmp/successful-drag");
        let successful = start_drag(cx, successful_path.clone(), true);
        let outside_position = point(px(-1.), px(20.));
        let update_result = cx.update_window(successful.window, |_, window, cx| {
            window.dispatch_event(
                MouseMoveEvent {
                    position: outside_position,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Default::default(),
                }
                .to_platform_input(),
                cx,
            );
            assert!(cx.active_drag.is_none());
        });
        assert!(
            update_result.is_ok(),
            "failed to promote drag: {update_result:?}"
        );
        assert_eq!(
            cx.test_window(successful.window).external_drag_files(),
            [(successful_path.clone(), true)]
        );
        // Views must still see the move that leaves the window, otherwise they never learn to tear
        // down the drag state they built up while the pointer was inside.
        assert_eq!(
            successful.observed_drag_moves.borrow().last(),
            Some(&outside_position)
        );

        let first_destination_exit_count = Rc::new(Cell::new(0));
        let first_destination: AnyWindowHandle = cx
            .add_window({
                let first_destination_exit_count = first_destination_exit_count.clone();
                move |_, _| FileDropExitView(first_destination_exit_count)
            })
            .into();
        let second_destination_exit_count = Rc::new(Cell::new(0));
        let second_destination: AnyWindowHandle = cx
            .add_window({
                let second_destination_exit_count = second_destination_exit_count.clone();
                move |_, _| FileDropExitView(second_destination_exit_count)
            })
            .into();
        let reentry_position = point(px(30.), px(30.));
        let external_paths = || ExternalPaths([successful_path.clone()].into_iter().collect());
        let update_result = cx.update_window(first_destination, |_, window, cx| {
            window.draw(cx).clear(cx);
            window.dispatch_event(
                FileDropEvent::Entered {
                    position: reentry_position,
                    paths: external_paths(),
                }
                .to_platform_input(),
                cx,
            );
            assert!(
                cx.active_drag
                    .as_ref()
                    .is_some_and(|drag| drag.value.downcast_ref::<ExternalPaths>().is_some())
            );
            window.dispatch_event(FileDropEvent::Exited.to_platform_input(), cx);
            assert!(cx.active_drag.is_none());
            assert_eq!(first_destination_exit_count.get(), 1);
            assert_eq!(second_destination_exit_count.get(), 0);
        });
        assert!(
            update_result.is_ok(),
            "failed to handle drag in first destination window: {update_result:?}"
        );

        let update_result = cx.update_window(second_destination, |_, window, cx| {
            window.draw(cx).clear(cx);
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: Keystroke::parse("down").expect("valid keystroke"),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
            window.dispatch_event(
                FileDropEvent::Entered {
                    position: reentry_position,
                    paths: external_paths(),
                }
                .to_platform_input(),
                cx,
            );
            assert!(
                cx.active_drag
                    .as_ref()
                    .is_some_and(|drag| drag.value.downcast_ref::<ExternalPaths>().is_some())
            );
            assert_eq!(first_destination_exit_count.get(), 1);
            assert_eq!(second_destination_exit_count.get(), 0);

            window.dispatch_event(FileDropEvent::Exited.to_platform_input(), cx);
            assert!(cx.active_drag.is_none());
            assert_eq!(first_destination_exit_count.get(), 1);
            assert_eq!(second_destination_exit_count.get(), 1);
        });
        assert!(
            update_result.is_ok(),
            "failed to handle drag in second destination window: {update_result:?}"
        );

        let update_result = cx.update_window(successful.window, |_, window, cx| {
            window.dispatch_event(
                FileDropEvent::Entered {
                    position: reentry_position,
                    paths: external_paths(),
                }
                .to_platform_input(),
                cx,
            );
            assert!(
                cx.active_drag
                    .as_ref()
                    .is_some_and(|drag| drag.value.downcast_ref::<PathBuf>().is_some())
            );
            assert_eq!(
                successful.observed_drag_moves.borrow().last(),
                Some(&reentry_position)
            );

            window.dispatch_event(FileDropEvent::Exited.to_platform_input(), cx);
            assert!(cx.active_drag.is_none());

            window.dispatch_event(
                FileDropEvent::Entered {
                    position: reentry_position,
                    paths: external_paths(),
                }
                .to_platform_input(),
                cx,
            );
            assert!(
                cx.active_drag
                    .as_ref()
                    .is_some_and(|drag| drag.value.downcast_ref::<PathBuf>().is_some())
            );

            window.dispatch_event(
                FileDropEvent::Submit {
                    position: reentry_position,
                }
                .to_platform_input(),
                cx,
            );
            assert_eq!(
                successful.observed_drops.borrow().as_slice(),
                std::slice::from_ref(&successful_path)
            );
            assert!(cx.active_drag.is_none());

            window.dispatch_event(FileDropEvent::Exited.to_platform_input(), cx);
            assert!(cx.active_drag.is_none());
            window.dispatch_event(FileDropEvent::Ended.to_platform_input(), cx);
            assert!(cx.active_drag.is_none());

            window.dispatch_event(
                FileDropEvent::Entered {
                    position: reentry_position,
                    paths: external_paths(),
                }
                .to_platform_input(),
                cx,
            );
            assert!(
                cx.active_drag
                    .as_ref()
                    .is_some_and(|drag| drag.value.downcast_ref::<ExternalPaths>().is_some())
            );
            window.dispatch_event(FileDropEvent::Exited.to_platform_input(), cx);
        });
        assert!(
            update_result.is_ok(),
            "failed to restore drag in source window: {update_result:?}"
        );

        let cancelled_path = PathBuf::from("/tmp/cancelled-drag");
        let cancelled = start_drag(cx, cancelled_path.clone(), true);
        let update_result = cx.update_window(cancelled.window, |_, window, cx| {
            window.dispatch_event(
                MouseMoveEvent {
                    position: outside_position,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Default::default(),
                }
                .to_platform_input(),
                cx,
            );
            assert!(cx.active_drag.is_none());

            window.dispatch_event(
                FileDropEvent::Entered {
                    position: reentry_position,
                    paths: ExternalPaths([cancelled_path].into_iter().collect()),
                }
                .to_platform_input(),
                cx,
            );
            assert!(
                cx.active_drag
                    .as_ref()
                    .is_some_and(|drag| drag.value.downcast_ref::<PathBuf>().is_some())
            );
            assert!(cx.stop_active_drag(window));
            assert!(cx.active_drag.is_none());
        });
        assert!(
            update_result.is_ok(),
            "failed to cancel restored drag: {update_result:?}"
        );
        assert!(!cx.update(|cx| cx.end_platform_drag(cancelled.window.window_id())));

        let removed_path = PathBuf::from("/tmp/removed-window-drag");
        let removed = start_drag(cx, removed_path, true);
        let removed_window_id = removed.window.window_id();
        let update_result = cx.update_window(removed.window, |_, window, cx| {
            window.dispatch_event(
                MouseMoveEvent {
                    position: outside_position,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Default::default(),
                }
                .to_platform_input(),
                cx,
            );
            assert!(cx.active_drag.is_none());
            window.remove_window();
        });
        assert!(
            update_result.is_ok(),
            "failed to remove drag source window: {update_result:?}"
        );
        assert!(!cx.update(|cx| cx.end_platform_drag(removed_window_id)));

        let failed_path = PathBuf::from("/tmp/failed-drag");
        let failed = start_drag(cx, failed_path.clone(), false);
        let update_result = cx.update_window(failed.window, |_, window, cx| {
            for x_position in [-1., -2.] {
                window.dispatch_event(
                    MouseMoveEvent {
                        position: point(px(x_position), px(20.)),
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Default::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
            }
            assert!(cx.active_drag.is_some());
        });
        assert!(
            update_result.is_ok(),
            "failed to retain drag after platform failure: {update_result:?}"
        );
        assert_eq!(
            cx.test_window(failed.window).external_drag_files(),
            [(failed_path, true)]
        );
    }

    struct FocusForwarder {
        a: FocusHandle,
        b: FocusHandle,
    }

    impl Render for FocusForwarder {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(div().w(px(50.)).h(px(50.)).track_focus(&self.a))
                .child(div().w(px(50.)).h(px(50.)).track_focus(&self.b))
        }
    }

    /// When a focus listener moves focus again (e.g. a dock forwarding focus to its
    /// active panel), the resulting focus events must be dispatched without waiting
    /// for an unrelated redraw of the window.
    #[gpux::test]
    fn test_focus_moved_by_focus_listener_is_dispatched(cx: &mut TestAppContext) {
        let b_focus_count = Rc::new(Cell::new(0));
        let window = cx.add_window({
            let b_focus_count = b_focus_count.clone();
            move |window, cx| {
                let a = cx.focus_handle();
                let b = cx.focus_handle();
                cx.on_focus(&a, window, |this: &mut FocusForwarder, window, cx| {
                    let b = this.b.clone();
                    window.focus(&b, cx);
                })
                .detach();
                cx.on_focus(&b, window, move |_, _, _| {
                    b_focus_count.set(b_focus_count.get() + 1);
                })
                .detach();
                FocusForwarder { a, b }
            }
        });

        window
            .update(cx, |_, window, _| window.activate_window())
            .unwrap();
        cx.executor().run_until_parked();

        window
            .update(cx, |this, window, cx| {
                let a = this.a.clone();
                window.focus(&a, cx);
            })
            .unwrap();
        cx.executor().run_until_parked();

        window
            .update(cx, |this, window, _| {
                assert!(this.b.is_focused(window));
            })
            .unwrap();
        assert_eq!(b_focus_count.get(), 1);
    }

    #[gpux::test]
    fn claimed_touch_drag_receives_movement_and_release(cx: &mut TestAppContext) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_window({
            let events = events.clone();
            move |_, _| TouchDragListener { events }
        });
        let touch = TouchId(1);

        dispatch_touch(window, cx, touch, TouchPhase::Started, 10.);
        dispatch_touch(window, cx, touch, TouchPhase::Moved, 30.);
        dispatch_touch(window, cx, touch, TouchPhase::Ended, 40.);

        assert_eq!(
            events.borrow().as_slice(),
            [
                (TouchPhase::Started, px(10.)),
                (TouchPhase::Moved, px(30.)),
                (TouchPhase::Ended, px(40.)),
            ]
        );
    }

    struct TouchDragListener {
        events: Rc<RefCell<Vec<(TouchPhase, Pixels)>>>,
    }

    impl Render for TouchDragListener {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let events = self.events.clone();
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    window.on_mouse_event(move |event: &TouchDragEvent, phase, window, _cx| {
                        if phase != DispatchPhase::Bubble {
                            return;
                        }
                        events.borrow_mut().push((event.phase, event.position.x));
                        if event.phase == TouchPhase::Started {
                            window.prevent_default();
                        }
                    });
                },
            )
        }
    }

    #[gpux::test]
    fn long_press_is_claimed_only_when_started_prevents_default(cx: &mut TestAppContext) {
        for response in [
            LongPressResponse::PreventDefault,
            LongPressResponse::StopPropagation,
            LongPressResponse::None,
        ] {
            let phases = Rc::new(RefCell::new(Vec::new()));
            let window = cx.add_window({
                let phases = phases.clone();
                move |_, _| LongPressListener { phases, response }
            });
            dispatch_touch(window, cx, TouchId(1), TouchPhase::Started, 0.);
            cx.executor().advance_clock(Duration::from_millis(501));
            cx.executor().run_until_parked();
            window
                .update(cx, |_, window, _| {
                    assert_eq!(
                        window.long_press_capture.is_some(),
                        response == LongPressResponse::PreventDefault
                    );
                })
                .unwrap();
            dispatch_touch(window, cx, TouchId(1), TouchPhase::Moved, 2.);
            dispatch_touch(window, cx, TouchId(1), TouchPhase::Ended, 2.);
            window
                .update(cx, |_, window, _| {
                    assert!(window.long_press_capture.is_none());
                })
                .unwrap();

            let phases = phases.borrow();
            if response == LongPressResponse::PreventDefault {
                assert_eq!(
                    phases.as_slice(),
                    [TouchPhase::Started, TouchPhase::Moved, TouchPhase::Ended]
                );
            } else {
                assert_eq!(phases.as_slice(), [TouchPhase::Started]);
            }
        }
    }

    #[gpux::test]
    fn stale_default_prevention_does_not_claim_long_press(cx: &mut TestAppContext) {
        let phases = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_window({
            let phases = phases.clone();
            move |_, _| LongPressListener {
                phases,
                response: LongPressResponse::None,
            }
        });
        window
            .update(cx, |_, window, _| {
                window.prevent_default();
            })
            .unwrap();

        dispatch_touch(window, cx, TouchId(1), TouchPhase::Started, 0.);
        cx.executor().advance_clock(Duration::from_millis(501));
        cx.executor().run_until_parked();
        dispatch_touch(window, cx, TouchId(1), TouchPhase::Moved, 2.);

        assert_eq!(phases.borrow().as_slice(), [TouchPhase::Started]);
    }

    #[gpux::test]
    fn resolved_touch_cancels_scheduled_long_press(cx: &mut TestAppContext) {
        for (phase, position) in [
            (TouchPhase::Ended, 0.),
            (TouchPhase::Cancelled, 0.),
            (TouchPhase::Moved, 20.),
        ] {
            let phases = Rc::new(RefCell::new(Vec::new()));
            let window = cx.add_window({
                let phases = phases.clone();
                move |_, _| LongPressListener {
                    phases,
                    response: LongPressResponse::PreventDefault,
                }
            });
            dispatch_touch(window, cx, TouchId(1), TouchPhase::Started, 0.);
            dispatch_touch(window, cx, TouchId(1), phase, position);
            cx.executor().advance_clock(Duration::from_millis(501));
            cx.executor().run_until_parked();

            assert!(phases.borrow().is_empty(), "{phase:?} allowed long press");
        }
    }

    #[gpux::test]
    fn stale_long_press_timer_cannot_affect_replacement_touch(cx: &mut TestAppContext) {
        let phases = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_window({
            let phases = phases.clone();
            move |_, _| LongPressListener {
                phases,
                response: LongPressResponse::PreventDefault,
            }
        });
        let first_touch = TouchId(1);
        dispatch_touch(window, cx, first_touch, TouchPhase::Started, 0.);
        cx.executor().advance_clock(Duration::from_millis(250));
        dispatch_touch(window, cx, first_touch, TouchPhase::Cancelled, 0.);
        dispatch_touch(window, cx, TouchId(2), TouchPhase::Started, 10.);

        cx.executor().advance_clock(Duration::from_millis(251));
        cx.executor().run_until_parked();
        assert!(phases.borrow().is_empty());

        cx.executor().advance_clock(Duration::from_millis(250));
        cx.executor().run_until_parked();
        assert_eq!(phases.borrow().as_slice(), [TouchPhase::Started]);
    }

    #[gpux::test]
    fn test_underline_exclusions_preserve_device_geometry(cx: &mut TestAppContext) {
        test_underline_paint_at_scales(cx, |window| {
            let (numerator, denominator) = match window.scale_factor() {
                1.0 => (1, 1),
                1.25 => (5, 4),
                1.5 => (3, 2),
                2.0 => (2, 1),
                3.0 => (3, 1),
                scale => panic!("unexpected scale {scale}"),
            };
            let snap = |quarters: i32| {
                let scaled = quarters * numerator;
                let divisor = 4 * denominator;
                let rounded =
                    scaled.abs() / divisor + i32::from(2 * (scaled.abs() % divisor) > divisor);
                ScaledPixels((scaled.signum() * rounded) as f32)
            };
            // Edges round their ties down while scroll layers snap, as they
            // do by default; widths round them toward zero.
            let ties_down = window.fast_mode.snapping();
            let edge = |quarters: i32| {
                if ties_down {
                    // The ceiling of (scaled - divisor / 2) / divisor.
                    let scaled = quarters * numerator;
                    let divisor = 4 * denominator;
                    ScaledPixels(-(divisor - 2 * scaled).div_euclid(2 * divisor) as f32)
                } else {
                    snap(quarters)
                }
            };
            let stroke = |quarters| {
                if quarters == 0 {
                    ScaledPixels(0.)
                } else {
                    snap(quarters).max(ScaledPixels(1.))
                }
            };

            for origin in [-101, -3, -2, -1, 0, 1, 2, 3, 101] {
                for width in [0, 1, 3, 84] {
                    for thickness in [0, 1, 4, 7] {
                        for wavy in [false, true] {
                            let style = UnderlineStyle {
                                thickness: px(thickness as f32 / 4.),
                                color: Some(hsla(0.25, 0.5, 0.75, 0.5).into()),
                                wavy,
                            };
                            let bounds = Bounds::new(
                                point(edge(origin), edge(-origin)),
                                size(
                                    stroke(width),
                                    stroke(thickness) * if wavy { 3. } else { 1. },
                                ),
                            );
                            let origin = point(px(origin as f32 / 4.), px(-origin as f32 / 4.));
                            let width = px(width as f32 / 4.);
                            assert_eq!(window.underline_bounds(origin, width, &style), bounds);
                            let original = paint_test_underlines(window, |window| {
                                window.paint_underline(origin, width, &style);
                            })
                            .to_vec();
                            assert_eq!(original.len(), usize::from(!bounds.is_empty()));
                            if let Some(underline) = original.first() {
                                assert_eq!(underline.bounds, bounds);
                                assert_eq!(underline.thickness, stroke(thickness));
                                assert_eq!(underline.wavy, u32::from(wavy));
                                assert_eq!(
                                    underline.color,
                                    crate::Color::from(hsla(0.25, 0.5, 0.75, 0.5)).paint_color()
                                );
                            }

                            let underlines = paint_test_underlines(window, |window| {
                                window.paint_underline_with_exclusions(origin, width, &style, &[]);
                            });
                            assert_eq!(underlines.len(), original.len());
                            for (actual, original) in underlines.iter().zip(&original) {
                                assert_same_underline_geometry(actual, original);
                                assert_eq!(actual.content_mask, original.content_mask);
                            }

                            let left = bounds.left().0 as i32;
                            let right = bounds.right().0 as i32;
                            let exclusions = [
                                (15, 25),
                                (3, 7),
                                (5, 10),
                                (10, 12),
                                (0, 0),
                                (4, 2),
                                (-50, -1),
                                (70, 100),
                            ]
                            .map(|(start, end)| {
                                ScaledPixels((left + start) as f32)
                                    ..ScaledPixels((left + end) as f32)
                            });
                            let underlines = paint_test_underlines(window, |window| {
                                window.paint_underline_with_exclusions(
                                    origin,
                                    width,
                                    &style,
                                    &exclusions,
                                );
                            });
                            let expected = (left..right)
                                .filter(|column| {
                                    !bounds.is_empty()
                                        && !exclusions.iter().any(|span| {
                                            span.contains(&ScaledPixels(*column as f32))
                                        })
                                })
                                .collect::<Vec<_>>();
                            assert_eq!(underline_device_columns(underlines), expected);
                            for underline in underlines {
                                assert_same_underline_geometry(underline, &original[0]);
                            }
                        }
                    }
                }
            }
        });
    }

    #[gpux::test]
    fn test_underline_exclusions_preserve_helvetica_warning_edge(cx: &mut TestAppContext) {
        test_underline_paint_at_scales(cx, |window| {
            if window.scale_factor() != 2.0 {
                return;
            }
            let style = UnderlineStyle {
                thickness: px(1.),
                color: None,
                wavy: true,
            };
            let origin = point(px(2.888_183_8), px(15.357_917));
            let width = px(23.848_145) - origin.x;
            let original = paint_test_underlines(window, |window| {
                window.paint_underline(origin, width, &style);
            })[0];
            assert_eq!(original.bounds.left(), ScaledPixels(6.));
            assert_eq!(original.bounds.right(), ScaledPixels(48.));
            let point_bounds = window.underline_bounds(origin, px(10.829_102), &style);
            assert_eq!(point_bounds.left(), ScaledPixels(6.));
            assert_eq!(point_bounds.right(), ScaledPixels(28.));

            let underlines = paint_test_underlines(window, |window| {
                window.paint_underline_with_exclusions(
                    origin,
                    width,
                    &style,
                    &[point_bounds.left()..point_bounds.right()],
                );
            });
            assert_eq!(underlines.len(), 1);
            assert_same_underline_geometry(&underlines[0], &original);
            assert_eq!(
                underline_device_columns(underlines),
                (28..48).collect::<Vec<_>>()
            );
        });
    }

    #[gpux::test]
    fn test_underline_exclusions_preserve_content_mask_and_opacity(cx: &mut TestAppContext) {
        test_underline_paint_at_scales(cx, |window| {
            let scale = window.scale_factor();
            let original_mask = window.content_mask();
            let original_opacity = window.element_opacity();
            let mask = ContentMask {
                bounds: Bounds::from_corners(point(px(4.), px(10.)), point(px(16.), px(12.))),
                corner_radii: Default::default(),
            };
            let style = UnderlineStyle {
                thickness: px(2.),
                color: Some(hsla(0.25, 0.5, 0.75, 0.5).into()),
                wavy: true,
            };
            let origin = point(px(-1.25), px(9.25));
            let width = px(24.);
            window.with_content_mask(Some(mask), |window| {
                window.with_element_opacity(Some(0.5), |window| {
                    window.with_element_opacity(Some(0.5), |window| {
                        let original = paint_test_underlines(window, |window| {
                            window.paint_underline(origin, width, &style);
                        })[0];
                        assert_eq!(
                            original.color,
                            crate::Color::from(hsla(0.25, 0.5, 0.75, 0.125)).paint_color()
                        );
                        let underlines = paint_test_underlines(window, |window| {
                            window.paint_underline_with_exclusions(
                                origin,
                                width,
                                &style,
                                &[ScaledPixels(6. * scale)..ScaledPixels(8. * scale)],
                            );
                        });
                        assert_eq!(underlines.len(), 2);
                        let expected = [(4. * scale, 6. * scale), (8. * scale, 16. * scale)].map(
                            |(left, right)| {
                                Bounds::from_corners(
                                    point(ScaledPixels(left), ScaledPixels((10. * scale).floor())),
                                    point(ScaledPixels(right), ScaledPixels((12. * scale).ceil())),
                                )
                            },
                        );
                        assert_eq!(
                            underlines
                                .iter()
                                .map(|underline| underline.content_mask.bounds)
                                .collect::<Vec<_>>(),
                            expected
                        );
                        for underline in underlines {
                            assert_same_underline_geometry(underline, &original);
                        }
                    });
                });
            });
            assert_eq!(window.content_mask(), original_mask);
            assert_eq!(window.element_opacity(), original_opacity);
        });
    }

    #[gpux::test]
    fn test_underline_exclusions_adversarial_permutations(cx: &mut TestAppContext) {
        test_underline_paint_at_scales(cx, |window| {
            let style = UnderlineStyle {
                thickness: px(1.),
                color: None,
                wavy: true,
            };
            let origin = point(px(-4.), px(-1.));
            let width = px(8.);
            let left = (-4. * window.scale_factor()) as i32;
            let right = (4. * window.scale_factor()) as i32;
            let endpoints = [-16, -8, -1, 0, 1, 8, 16];
            for first_start in endpoints {
                for first_end in endpoints {
                    for second_start in endpoints {
                        for second_end in endpoints {
                            let exclusions = [first_start..first_end, second_start..second_end];
                            let expected = (left..right)
                                .filter(|column| {
                                    !exclusions.iter().any(|span| span.contains(column))
                                })
                                .collect::<Vec<_>>();
                            let exclusions = exclusions.map(|span| {
                                ScaledPixels(span.start as f32)..ScaledPixels(span.end as f32)
                            });
                            let underlines = paint_test_underlines(window, |window| {
                                window.paint_underline_with_exclusions(
                                    origin,
                                    width,
                                    &style,
                                    &exclusions,
                                );
                            });
                            assert_eq!(
                                underline_device_columns(underlines),
                                expected,
                                "{exclusions:?}"
                            );
                        }
                    }
                }
            }
        });
    }

    #[derive(Clone, Copy, PartialEq)]
    enum LongPressResponse {
        PreventDefault,
        StopPropagation,
        None,
    }

    struct LongPressListener {
        phases: Rc<RefCell<Vec<TouchPhase>>>,
        response: LongPressResponse,
    }

    impl Render for LongPressListener {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let entity = cx.entity();
            let phases = self.phases.clone();
            let response = self.response;
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    window.on_mouse_event(move |event: &LongPressEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble {
                            return;
                        }
                        phases.borrow_mut().push(event.phase);
                        match response {
                            LongPressResponse::PreventDefault => {
                                window.capture_long_press(&entity);
                                window.prevent_default();
                            }
                            LongPressResponse::StopPropagation => cx.stop_propagation(),
                            LongPressResponse::None => {}
                        }
                    });
                },
            )
        }
    }

    struct UnderlineTestView(Rc<dyn Fn(&mut Window)>);

    impl Render for UnderlineTestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let paint = self.0.clone();
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let mask = ContentMask {
                        bounds: Bounds::from_corners(
                            point(px(-1000.), px(-1000.)),
                            point(px(1000.), px(1000.)),
                        ),
                        corner_radii: Default::default(),
                    };
                    // gpux paints against the scaled paint mask stack as well.
                    let paint_mask = mask.scale(window.scale_factor());
                    window.content_mask_stack.push(mask);
                    window.paint_content_mask_stack.push(paint_mask);
                    paint(window);
                    window.paint_content_mask_stack.pop();
                    window.content_mask_stack.pop();
                },
            )
            .size_full()
        }
    }

    fn test_underline_paint_at_scales(
        cx: &mut TestAppContext,
        paint: impl Fn(&mut Window) + 'static,
    ) {
        let window = cx.add_window(move |_, _| UnderlineTestView(Rc::new(paint)));
        for scale in [1., 1.25, 1.5, 2., 3.] {
            cx.simulate_window_scale_factor_change(window.into(), scale);
            cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
        }
    }

    fn paint_test_underlines(window: &mut Window, paint: impl FnOnce(&mut Window)) -> &[Underline] {
        window.next_frame.scene.clear();
        paint(window);
        &window.next_frame.scene.underlines
    }

    fn assert_same_underline_geometry(actual: &Underline, expected: &Underline) {
        assert_eq!(actual.bounds, expected.bounds);
        assert_eq!(actual.thickness, expected.thickness);
        assert_eq!(actual.wavy, expected.wavy);
        assert_eq!(actual.color, expected.color);
        assert_eq!(actual.pad, expected.pad);
    }

    fn underline_device_columns(underlines: &[Underline]) -> Vec<i32> {
        underlines
            .iter()
            .flat_map(|underline| {
                let bounds = underline.bounds.intersect(&underline.content_mask.bounds);
                bounds.left().0 as i32..bounds.right().0 as i32
            })
            .collect()
    }

    fn dispatch_touch<T: 'static>(
        window: crate::WindowHandle<T>,
        cx: &mut TestAppContext,
        id: TouchId,
        phase: TouchPhase,
        x: f32,
    ) {
        window
            .update(cx, |_, window, cx| {
                window.dispatch_event(
                    TouchEvent {
                        id,
                        phase,
                        position: point(px(x), px(0.)),
                        predicted_position: None,
                        force: None,
                        timestamp: None,
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
    }
}

#[cfg(all(test, any(feature = "inspector", debug_assertions)))]
mod inspector_tests {
    use super::*;
    use crate::{
        DivInspectorState, InspectorElementId, MouseDownEvent, StyleRefinement, TestAppContext, div,
    };

    #[gpux::test]
    fn inspector_only_tracks_its_open_window(cx: &mut TestAppContext) {
        let windows = [
            cx.add_window(|_, cx| InspectorTestRoot {
                child: cx.new(|_| InspectorTestView::default()),
            }),
            cx.add_window(|_, cx| InspectorTestRoot {
                child: cx.new(|_| InspectorTestView::default()),
            }),
        ];
        for window in windows {
            assert_closed_inspector(window.into(), cx);
        }
        for _ in 0..2 {
            cx.update_window(windows[0].into(), |root, window, cx| {
                let root = root.downcast::<InspectorTestRoot>().expect("test root");
                let child_widths = root.read(cx).child.read(cx).child_widths.clone();
                window.toggle_inspector(cx);
                window.draw(cx).clear(cx);
                assert_eq!(child_widths.borrow().as_slice(), &[px(10.); 3]);
                let path = window
                    .rendered_frame
                    .next_inspector_instance_ids
                    .iter()
                    .find_map(|(path, count)| (*count == 3).then(|| path.clone()))
                    .expect("anonymous siblings share an inspector path");
                let selected_id = InspectorElementId {
                    path,
                    instance_id: 1,
                };
                let position = window
                    .rendered_frame
                    .hitboxes
                    .iter()
                    .find(|hitbox| {
                        window.rendered_frame.inspector_hitboxes.get(&hitbox.id)
                            == Some(&selected_id)
                    })
                    .expect("middle sibling is pickable")
                    .bounds
                    .center();
                window.simulate_mouse_move(position, cx);
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        position,
                        button: MouseButton::Left,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        position,
                        button: MouseButton::Left,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }),
                    cx,
                );
                assert!(!window.is_inspector_picking(cx));
                assert_eq!(
                    window
                        .inspector
                        .as_ref()
                        .expect("open inspector")
                        .read(cx)
                        .active_element_id(),
                    Some(&selected_id)
                );
                window.draw(cx).clear(cx);
                window.with_inspector_state::<DivInspectorState, _>(
                    Some(&selected_id),
                    cx,
                    |state, _| {
                        let state = state.as_mut().expect("selected div has style state");
                        assert_eq!(
                            state.base_style.size.width,
                            Some(crate::Length::from(px(10.)))
                        );
                        state.base_style.size.width = Some(crate::Length::from(px(25.)));
                    },
                );
                window.refresh();
                window.draw(cx).clear(cx);
                assert_eq!(
                    child_widths.borrow().as_slice(),
                    &[px(10.), px(25.), px(10.)]
                );
            })
            .expect("pick and edit a cached child");
            assert_closed_inspector(windows[1].into(), cx);
            windows[0]
                .update(cx, |_, window, cx| window.toggle_inspector(cx))
                .expect("close inspector");
            assert_closed_inspector(windows[0].into(), cx);
        }
    }

    struct InspectorTestRoot {
        child: Entity<InspectorTestView>,
    }

    impl Render for InspectorTestRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.child
                .clone()
                .cached(StyleRefinement::default().size(px(100.)))
        }
    }

    #[derive(Default)]
    struct InspectorTestView {
        child_widths: Rc<RefCell<Vec<Pixels>>>,
    }

    impl Render for InspectorTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let child_widths = self.child_widths.clone();
            div()
                .on_children_prepainted(move |bounds, _, _| {
                    *child_widths.borrow_mut() =
                        bounds.iter().map(|bounds| bounds.size.width).collect();
                })
                .with_dynamic_prepaint_order(|_, _| SmallVec::from_iter([2, 0, 1]))
                .id("inspector-root")
                .flex()
                .children((0..3).map(|_| div().size(px(10.)).flex_shrink_0()))
        }
    }

    fn assert_closed_inspector(window: AnyWindowHandle, cx: &mut TestAppContext) {
        cx.update_window(window, |root, window, cx| {
            window.draw(cx).clear(cx);
            window.draw(cx).clear(cx);
            let root = root.downcast::<InspectorTestRoot>().expect("test root");
            assert_eq!(
                root.read(cx)
                    .child
                    .read(cx)
                    .child_widths
                    .borrow()
                    .as_slice(),
                &[px(10.); 3]
            );
            for frame in [&window.rendered_frame, &window.next_frame] {
                assert_eq!(frame.next_inspector_instance_ids.capacity(), 0);
                assert_eq!(frame.inspector_hitboxes.capacity(), 0);
            }
        })
        .expect("closed inspector has no bookkeeping and no style overrides");
    }
}
