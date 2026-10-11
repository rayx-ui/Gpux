//! Div is the central, reusable element that most GPUI trees will be built from.
//! It functions as a container for other elements, and provides a number of
//! useful features for laying out and styling its children as well as binding
//! mouse events and action handlers. It is meant to be similar to the HTML `<div>`
//! element, but for GPUI.
//!
//! # Build your own div
//!
//! GPUI does not directly provide APIs for stateful, multi step events like `click`
//! and `drag`. We want GPUI users to be able to build their own abstractions for
//! their own needs. However, as a UI framework, we're also obliged to provide some
//! building blocks to make the process of building your own elements easier.
//! For this we have the [`Interactivity`] and the [`StyleRefinement`] structs, as well
//! as several associated traits. Together, these provide the full suite of Dom-like events
//! and Tailwind-like styling that you can use to build your own custom elements. Div is
//! constructed by combining these two systems into an all-in-one element.

use super::scroll_activity::{
    ProgrammaticScrollFrame, ScrollActivity, ScrollActivityKind, ScrollMetrics,
    ScrollMutationSource, direction_for_delta,
};
use crate::{
    Action, ActionDispatchOutcome, AnyDrag, AnyElement, AnyTooltip, AnyView, App, Bounds,
    ClickEvent, DispatchPhase, Display, Element, ElementId, Entity, EntityId, ExternalDragPayload,
    ExternalDragPayloadSource, FileDropEvent, FocusHandle, Global, GlobalElementId, Hitbox,
    HitboxBehavior, HitboxId, InspectorElementId, Instant, IntoElement, IsZero, KeyContext,
    KeyDownEvent, KeyUpEvent, KeyboardButton, KeyboardClickEvent, LayoutId, LongPressEvent,
    ModifiersChangedEvent, MouseButton, MouseClickEvent, MouseDownEvent, MouseExitEvent,
    MouseMoveEvent, MousePressureEvent, MouseUpEvent, OngoingScroll, Overflow, ParentElement,
    PinchEvent, Pixels, Point, Position, PositionedBounds, Render, ScrollBehavior,
    ScrollContainerContext, ScrollWheelEvent, SharedString, Size, StickyState,
    StickyStateChangedEvent, Style, StyleRefinement, Styled, Task, TooltipId, TouchPhase,
    Visibility, Window, WindowControlArea, point, px, size,
};
use collections::HashMap;
use gpui_util::ResultExt;
use refineable::Refineable;
use smallvec::SmallVec;
use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    cmp::Ordering,
    fmt::Debug,
    marker::PhantomData,
    mem,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use super::ImageCacheProvider;

#[cfg(feature = "stacker")]
type StackSafe<T> = stacksafe::StackSafe<T>;
#[cfg(not(feature = "stacker"))]
type StackSafe<T> = T;

const DRAG_THRESHOLD: f64 = 2.;
const DEFAULT_TOOLTIP_SHOW_DELAY: Duration = Duration::from_millis(500);
const HOVERABLE_TOOLTIP_HIDE_DELAY: Duration = Duration::from_millis(500);

#[cfg(feature = "diagnostics")]
thread_local! {
    static STYLE_PHASE_DIAGNOSTICS: RefCell<[u64; 14]> = const { RefCell::new([0; 14]) };
}

#[cfg(feature = "diagnostics")]
pub(crate) fn reset_style_phase_diagnostics() {
    STYLE_PHASE_DIAGNOSTICS.with_borrow_mut(|diagnostics| *diagnostics = [0; 14]);
}

#[cfg(feature = "diagnostics")]
pub(crate) fn style_phase_diagnostics() -> [u64; 14] {
    STYLE_PHASE_DIAGNOSTICS.with_borrow(|diagnostics| *diagnostics)
}

#[cfg(feature = "diagnostics")]
fn record_style_phase(index: usize) {
    STYLE_PHASE_DIAGNOSTICS.with_borrow_mut(|diagnostics| diagnostics[index] += 1);
}

#[cfg(feature = "diagnostics")]
fn gpux_owns_request_style(file: &str) -> bool {
    file.contains("crates/gpui/src/elements/") || file.contains("crates\\gpui\\src\\elements\\")
}

/// The styling information for a given group.
pub struct GroupStyle {
    /// The identifier for this group.
    pub group: SharedString,

    /// The specific style refinement that this group would apply
    /// to its children.
    pub style: Box<StyleRefinement>,
}

/// An event for when a drag is moving over this element, with the given state type.
pub struct DragMoveEvent<T> {
    /// The mouse move event that triggered this drag move event.
    pub event: MouseMoveEvent,

    /// The bounds of this element.
    pub bounds: Bounds<Pixels>,
    drag: PhantomData<T>,
    dragged_item: Arc<dyn Any>,
}

impl<T: 'static> DragMoveEvent<T> {
    /// Returns the drag state for this event.
    pub fn drag<'b>(&self, cx: &'b App) -> &'b T {
        cx.active_drag
            .as_ref()
            .and_then(|drag| drag.value.downcast_ref::<T>())
            .expect("DragMoveEvent is only valid when the stored active drag is of the same type.")
    }

    /// An item that is about to be dropped.
    pub fn dragged_item(&self) -> &dyn Any {
        self.dragged_item.as_ref()
    }
}

impl Interactivity {
    /// Create an `Interactivity`, capturing the caller location in debug mode.
    #[cfg(any(feature = "inspector", debug_assertions))]
    #[track_caller]
    pub fn new() -> Interactivity {
        Interactivity {
            source_location: Some(core::panic::Location::caller()),
            ..Default::default()
        }
    }

    /// Create an `Interactivity`, capturing the caller location in debug mode.
    #[cfg(not(any(feature = "inspector", debug_assertions)))]
    pub fn new() -> Interactivity {
        Interactivity::default()
    }

    /// Gets the source location of construction. Returns `None` when not in debug mode.
    pub fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        #[cfg(any(feature = "inspector", debug_assertions))]
        {
            self.source_location
        }

        #[cfg(not(any(feature = "inspector", debug_assertions)))]
        {
            None
        }
    }

    /// Bind the given callback to the mouse down event for the given mouse button, during the bubble phase.
    /// The imperative API equivalent of [`InteractiveElement::on_mouse_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to the view state from this callback.
    pub fn on_mouse_down(
        &mut self,
        button: MouseButton,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == button
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse down event for any button, during the capture phase.
    /// The imperative API equivalent of [`InteractiveElement::capture_any_mouse_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_any_mouse_down(
        &mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture && hitbox.is_hovered(window) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse down event for any button, during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_any_mouse_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_any_mouse_down(
        &mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse pressure event, during the bubble phase
    /// the imperative API equivalent to [`InteractiveElement::on_mouse_pressure`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_mouse_pressure(
        &mut self,
        listener: impl Fn(&MousePressureEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_pressure_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse pressure event, during the capture phase
    /// the imperative API equivalent to [`InteractiveElement::on_mouse_pressure`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_mouse_pressure(
        &mut self,
        listener: impl Fn(&MousePressureEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_pressure_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture && hitbox.is_hovered(window) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event for the given button, during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_mouse_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_mouse_up(
        &mut self,
        button: MouseButton,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == button
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event for any button, during the capture phase.
    /// The imperative API equivalent to [`InteractiveElement::capture_any_mouse_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_any_mouse_up(
        &mut self,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture && hitbox.is_hovered(window) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event for any button, during the bubble phase.
    /// The imperative API equivalent to [`Interactivity::on_any_mouse_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_any_mouse_up(
        &mut self,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse down event, on any button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The imperative API equivalent to [`InteractiveElement::on_mouse_down_out`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_mouse_down_out(
        &mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture
                    && !window.has_active_prompt()
                    && !hitbox.contains(&window.mouse_position())
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event, for the given button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The imperative API equivalent to [`InteractiveElement::on_mouse_up_out`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_mouse_up_out(
        &mut self,
        button: MouseButton,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture
                    && event.button == button
                    && !hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to the mouse move event, during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_mouse_move`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_mouse_move(
        &mut self,
        listener: impl Fn(&MouseMoveEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_move_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to the mouse exit event, during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_mouse_exit`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_mouse_exit(
        &mut self,
        listener: impl Fn(&MouseExitEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_exit_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to [`FileDropEvent::Exited`] when a platform file drag
    /// leaves this element's window while the element is hovered.
    ///
    /// This is a window-local exit event, not notification that the platform drag session ended.
    /// The imperative API equivalent to [`InteractiveElement::on_file_drop_exit`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_file_drop_exit(
        &mut self,
        listener: impl Fn(&FileDropEvent, &mut Window, &mut App) + 'static,
    ) {
        self.file_drop_exit_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble
                    && matches!(event, FileDropEvent::Exited)
                    && hitbox.id.is_hovered_ignoring_last_input(window)
                {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to the mouse drag event of the given type. Note that this
    /// will be called for all move events, inside or outside of this element, as long as the
    /// drag was started with this element under the mouse. Useful for implementing draggable
    /// UIs that don't conform to a drag and drop style interaction, like resizing.
    /// The imperative API equivalent to [`InteractiveElement::on_drag_move`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_drag_move<T>(
        &mut self,
        listener: impl Fn(&DragMoveEvent<T>, &mut Window, &mut App) + 'static,
    ) where
        T: 'static,
    {
        self.mouse_move_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture
                    && let Some(drag) = &cx.active_drag
                    && drag.value.as_ref().type_id() == TypeId::of::<T>()
                {
                    (listener)(
                        &DragMoveEvent {
                            event: event.clone(),
                            bounds: hitbox.bounds,
                            drag: PhantomData,
                            dragged_item: Arc::clone(&drag.value),
                        },
                        window,
                        cx,
                    );
                }
            }));
    }

    /// Bind the given callback to scroll wheel events during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_scroll_wheel`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_scroll_wheel(
        &mut self,
        listener: impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static,
    ) {
        self.scroll_wheel_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to scroll wheel events during the capture phase.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_scroll_wheel(
        &mut self,
        listener: impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static,
    ) {
        self.scroll_wheel_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture && hitbox.should_handle_scroll(window) {
                    (listener)(event, window, cx);
                } else {
                    cx.propagate();
                }
            }));
    }

    /// Bind the given callback to pinch gesture events during the bubble phase.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_pinch(&mut self, listener: impl Fn(&PinchEvent, &mut Window, &mut App) + 'static) {
        self.pinch_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to pinch gesture events during the capture phase.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_pinch(
        &mut self,
        listener: impl Fn(&PinchEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pinch_listeners
            .push(Box::new(move |event, phase, _hitbox, window, cx| {
                if phase == DispatchPhase::Capture {
                    (listener)(event, window, cx);
                } else {
                    cx.propagate();
                }
            }));
    }

    /// Bind the given callback to an action dispatch during the capture phase.
    /// The imperative API equivalent to [`InteractiveElement::capture_action`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_action<A: Action>(
        &mut self,
        listener: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) {
        self.action_listeners.push((
            TypeId::of::<A>(),
            Box::new(move |action, phase, window, cx| {
                let action = action.downcast_ref().unwrap();
                if phase == DispatchPhase::Capture {
                    (listener)(action, window, cx)
                } else {
                    cx.propagate();
                }
            }),
        ));
    }

    /// Bind the given callback to an action dispatch during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_action`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    #[track_caller]
    #[inline(always)]
    pub fn on_action<A: Action>(&mut self, listener: impl Fn(&A, &mut Window, &mut App) + 'static) {
        self.action_listeners.push((
            TypeId::of::<A>(),
            Box::new(move |action, phase, window, cx| {
                let action = action.downcast_ref().unwrap();
                if phase == DispatchPhase::Bubble {
                    (listener)(action, window, cx)
                }
            }),
        ));
    }

    /// Bind an action callback that reports an explicit dispatch outcome.
    pub fn on_action_with_outcome<A: Action>(
        &mut self,
        listener: impl Fn(&A, &mut Window, &mut App) -> ActionDispatchOutcome + 'static,
    ) {
        self.action_outcome_listeners.push((
            TypeId::of::<A>(),
            Box::new(move |action, window, cx| {
                (listener)(action.downcast_ref().unwrap(), window, cx)
            }),
        ));
    }

    /// Bind the given callback to an action dispatch, based on a dynamic action parameter
    /// instead of a type parameter. Useful for component libraries that want to expose
    /// action bindings to their users.
    /// The imperative API equivalent to [`InteractiveElement::on_boxed_action`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_boxed_action(
        &mut self,
        action: &dyn Action,
        listener: impl Fn(&dyn Action, &mut Window, &mut App) + 'static,
    ) {
        let action = action.boxed_clone();
        self.action_listeners.push((
            (*action).type_id(),
            Box::new(move |_, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    (listener)(&*action, window, cx)
                }
            }),
        ));
    }

    /// Bind the given callback to key down events during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_key_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_key_down(
        &mut self,
        listener: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.key_down_listeners
            .push(Box::new(move |event, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to key down events during the capture phase.
    /// The imperative API equivalent to [`InteractiveElement::capture_key_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_key_down(
        &mut self,
        listener: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.key_down_listeners
            .push(Box::new(move |event, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    listener(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to key up events during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_key_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_key_up(&mut self, listener: impl Fn(&KeyUpEvent, &mut Window, &mut App) + 'static) {
        self.key_up_listeners
            .push(Box::new(move |event, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    listener(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to key up events during the capture phase.
    /// The imperative API equivalent to [`InteractiveElement::on_key_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_key_up(
        &mut self,
        listener: impl Fn(&KeyUpEvent, &mut Window, &mut App) + 'static,
    ) {
        self.key_up_listeners
            .push(Box::new(move |event, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    listener(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to modifiers changing events.
    /// The imperative API equivalent to [`InteractiveElement::on_modifiers_changed`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_modifiers_changed(
        &mut self,
        listener: impl Fn(&ModifiersChangedEvent, &mut Window, &mut App) + 'static,
    ) {
        self.modifiers_changed_listeners.push(Box::new(listener));
    }

    /// Bind the given callback to drop events of the given type, whether or not the drag started on this element.
    /// The imperative API equivalent to [`InteractiveElement::on_drop`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_drop<T: 'static>(&mut self, listener: impl Fn(&T, &mut Window, &mut App) + 'static) {
        self.drop_listeners.push((
            TypeId::of::<T>(),
            Box::new(move |dragged_value, window, cx| {
                listener(dragged_value.downcast_ref().unwrap(), window, cx);
            }),
        ));
    }

    /// Use the given predicate to determine whether or not a drop event should be dispatched to this element.
    /// The imperative API equivalent to [`InteractiveElement::can_drop`].
    pub fn can_drop(
        &mut self,
        predicate: impl Fn(&dyn Any, &mut Window, &mut App) -> bool + 'static,
    ) {
        self.can_drop_predicate = Some(Box::new(predicate));
    }

    /// Bind the given callback to click events of this element.
    /// The imperative API equivalent to [`StatefulInteractiveElement::on_click`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    #[inline(always)]
    pub fn on_click(&mut self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static)
    where
        Self: Sized,
    {
        self.click_listeners.push(Rc::new(listener));
    }

    /// Bind the given callback to non-primary click events of this element.
    /// The imperative API equivalent to [`StatefulInteractiveElement::on_aux_click`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_aux_click(&mut self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static)
    where
        Self: Sized,
    {
        self.aux_click_listeners.push(Rc::new(listener));
    }

    /// On drag initiation, this callback will be used to create a new view to render the dragged value for a
    /// drag and drop operation. This API should also be used as the equivalent of 'on drag start' with
    /// the [`Self::on_drag_move`] API.
    /// The imperative API equivalent to [`StatefulInteractiveElement::on_drag`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_drag<T, W>(
        &mut self,
        value: T,
        constructor: impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W> + 'static,
    ) where
        Self: Sized,
        T: 'static,
        W: 'static + Render,
    {
        debug_assert!(
            self.drag_listener.is_none(),
            "calling on_drag more than once on the same element is not supported"
        );
        self.drag_listener = Some(DragListener {
            value: Arc::new(value),
            render: Box::new(move |value, offset, window, cx| {
                constructor(value.downcast_ref().unwrap(), offset, window, cx).into()
            }),
            external_payload: None,
        });
    }

    /// Registers a callback resolving a payload to offer the platform if a drag started by this
    /// element leaves the window. It is invoked at most once per drag gesture, when the pointer
    /// exits the viewport. Must be called after [`Self::on_drag`], with the same dragged value
    /// type `T`.
    pub fn external_drag_payload<T>(
        &mut self,
        resolver: impl Fn(&T, &mut Window, &mut App) -> Option<ExternalDragPayload> + 'static,
    ) where
        Self: Sized,
        T: 'static,
    {
        let Some(drag_listener) = self.drag_listener.as_mut() else {
            debug_assert!(false, "external_drag_payload must be called after on_drag");
            return;
        };
        debug_assert!(
            drag_listener.value.as_ref().type_id() == TypeId::of::<T>(),
            "external_drag_payload must use the same dragged value type as on_drag"
        );
        debug_assert!(
            drag_listener.external_payload.is_none(),
            "calling external_drag_payload more than once on the same element is not supported"
        );
        drag_listener.external_payload = Some(Box::new(move |value, window, cx| {
            resolver(value.downcast_ref::<T>()?, window, cx)
        }));
    }

    /// Bind the given callback on the hover start and end events of this element. Note that the boolean
    /// passed to the callback is true when the hover starts and false when it ends.
    /// Transitions caused by layout changes under a stationary mouse also invoke the callback.
    ///
    /// By default, keyboard input suppresses hover until the next mouse move, mouse down, or touch. Set
    /// [`HoverListenerMode::InputModalityIndependent`] with [`Self::hover_listener_mode`] to
    /// continue hit-testing hover after keyboard input.
    /// The imperative API equivalent to [`StatefulInteractiveElement::on_hover`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_hover(&mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static)
    where
        Self: Sized,
    {
        debug_assert!(
            self.hover_listener.is_none(),
            "calling on_hover more than once on the same element is not supported"
        );
        self.hover_listener = Some(Box::new(listener));
    }

    /// Bind the given callback to sticky phase changes for this element.
    ///
    /// This requires an element id so the previous sticky phase can be retained across frames.
    /// Use [`InteractiveElement::when_stuck`] for visual-only styling such as a drop shadow.
    pub fn on_sticky_state_changed(
        &mut self,
        listener: impl Fn(&StickyStateChangedEvent, &mut Window, &mut App) + 'static,
    ) where
        Self: Sized,
    {
        debug_assert!(
            self.sticky_state_changed_listener.is_none(),
            "calling on_sticky_state_changed more than once on the same element is not supported"
        );
        self.sticky_state_changed_listener = Some(Box::new(listener));
    }

    /// Sets how [`Self::on_hover`] responds to key presses while the mouse is stationary.
    /// This affects only the hover listener, not hover styles or tooltips. The imperative API
    /// equivalent to [`StatefulInteractiveElement::hover_listener_mode`].
    pub fn hover_listener_mode(&mut self, mode: HoverListenerMode)
    where
        Self: Sized,
    {
        self.hover_listener_mode = mode;
    }

    /// Constructs a tooltip when the element is hovered or long-pressed.
    /// The imperative API equivalent to [`StatefulInteractiveElement::tooltip`].
    pub fn tooltip(&mut self, build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static)
    where
        Self: Sized,
    {
        debug_assert!(
            self.tooltip_builder.is_none(),
            "calling tooltip more than once on the same element is not supported"
        );
        self.tooltip_builder = Some(TooltipBuilder {
            build: Rc::new(build_tooltip),
            hoverable: false,
        });
    }

    /// Constructs a tooltip when the element is hovered or long-pressed.
    /// The tooltip itself is also hoverable and won't disappear when the user moves the mouse into
    /// the tooltip. The imperative API equivalent to [`StatefulInteractiveElement::hoverable_tooltip`].
    pub fn hoverable_tooltip(
        &mut self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) where
        Self: Sized,
    {
        debug_assert!(
            self.tooltip_builder.is_none(),
            "calling tooltip more than once on the same element is not supported"
        );
        self.tooltip_builder = Some(TooltipBuilder {
            build: Rc::new(build_tooltip),
            hoverable: true,
        });
    }

    /// Sets the delay before this element's tooltip is shown on hover.
    ///
    /// Touch long presses show the tooltip immediately once the gesture is recognized.
    /// The imperative API equivalent to [`StatefulInteractiveElement::tooltip_show_delay`].
    pub fn tooltip_show_delay(&mut self, delay: Duration) {
        self.tooltip_show_delay = Some(delay);
    }

    /// Block the mouse from all interactions with elements behind this element's hitbox. Typically
    /// `block_mouse_except_scroll` should be preferred.
    ///
    /// The imperative API equivalent to [`InteractiveElement::occlude`]
    pub fn occlude_mouse(&mut self) {
        self.hitbox_behavior = HitboxBehavior::BlockMouse;
    }

    /// Set the bounds of this element as a window control area for the platform window.
    /// The imperative API equivalent to [`InteractiveElement::window_control_area`]
    pub fn window_control_area(&mut self, area: WindowControlArea) {
        self.window_control = Some(area);
    }

    /// Block non-scroll mouse interactions with elements behind this element's hitbox.
    /// The imperative API equivalent to [`InteractiveElement::block_mouse_except_scroll`].
    ///
    /// See [`Hitbox::is_hovered`] for details.
    pub fn block_mouse_except_scroll(&mut self) {
        self.hitbox_behavior = HitboxBehavior::BlockMouseExceptScroll;
    }

    fn has_pinch_listeners(&self) -> bool {
        !self.pinch_listeners.is_empty()
    }
}

/// A trait for elements that want to use the standard GPUI event handlers that don't
/// require any state.
pub trait InteractiveElement: Sized {
    /// Retrieve the interactivity state associated with this element
    fn interactivity(&mut self) -> &mut Interactivity;

    /// Assign this element to a group of elements that can be styled together
    fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.interactivity().group = Some(group.into());
        self
    }

    /// Assign this element an ID, so that it can be used with interactivity
    fn id(mut self, id: impl Into<ElementId>) -> Stateful<Self> {
        self.interactivity().element_id = Some(id.into());

        Stateful { element: self }
    }

    /// Track the focus state of the given focus handle on this element.
    /// If the focus handle is focused by the application, this element will
    /// apply its focused styles.
    fn track_focus(mut self, focus_handle: &FocusHandle) -> Self {
        self.interactivity().focusable = true;
        self.interactivity().tracked_focus_handle = Some(focus_handle.clone());
        self
    }

    /// Set whether this element is a tab stop.
    ///
    /// When false, the element remains in tab-index order but cannot be reached via keyboard navigation.
    /// Useful for container elements: focus the container, then call `window.focus_next(cx)` to focus
    /// the first tab stop inside it while having the container element itself be unreachable via the keyboard.
    /// Should only be used with `tab_index`.
    fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.interactivity().tab_stop = tab_stop;
        self
    }

    /// Set index of the tab stop order, and set this node as a tab stop.
    /// This will default the element to being a tab stop. See [`Self::tab_stop`] for more information.
    /// This should only be used in conjunction with `tab_group`
    /// in order to not interfere with the tab index of other elements.
    fn tab_index(mut self, index: isize) -> Self {
        self.interactivity().focusable = true;
        self.interactivity().tab_index = Some(index);
        self.interactivity().tab_stop = true;
        self
    }

    /// Designate this div as a "tab group". Tab groups have their own location in the tab-index order,
    /// but for children of the tab group, the tab index is reset to 0. This can be useful for swapping
    /// the order of tab stops within the group, without having to renumber all the tab stops in the whole
    /// application.
    fn tab_group(mut self) -> Self {
        self.interactivity().tab_group = true;
        if self.interactivity().tab_index.is_none() {
            self.interactivity().tab_index = Some(0);
        }
        self
    }

    /// Set the keymap context for this element. This will be used to determine
    /// which action to dispatch from the keymap.
    fn key_context<C, E>(mut self, key_context: C) -> Self
    where
        C: TryInto<KeyContext, Error = E>,
        E: std::fmt::Display,
    {
        if let Some(key_context) = key_context.try_into().log_err() {
            self.interactivity().key_context = Some(key_context);
        }
        self
    }

    /// Apply the given style to this element when the mouse hovers over it
    fn hover(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        debug_assert!(
            self.interactivity().hover_style.is_none(),
            "hover style already set"
        );
        self.interactivity().hover_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    /// Apply the given visual style when a sticky element is actively stuck.
    ///
    /// This style is resolved after layout, so it should be used for paint-only changes such as
    /// shadows, colors, and opacity.
    fn when_stuck(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        debug_assert!(
            self.interactivity().stuck_style.is_none(),
            "stuck style already set"
        );
        self.interactivity().stuck_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    /// Apply the given style to this element when the mouse hovers over a group member
    fn group_hover(
        mut self,
        group_name: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self {
        self.interactivity().group_hover_style = Some(GroupStyle {
            group: group_name.into(),
            style: Box::new(f(StyleRefinement::default())),
        });
        self
    }

    /// Bind the given callback to the mouse down event for the given mouse button.
    /// The fluent API equivalent to [`Interactivity::on_mouse_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to the view state from this callback.
    #[inline(always)]
    fn on_mouse_down(
        mut self,
        button: MouseButton,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_down(button, listener);
        self
    }

    /// Set a key that can be used to look up this element's bounds
    /// in the [`crate::VisualTestContext::debug_bounds`] map.
    ///
    /// Selectors are only recorded in debug builds with the `test-support`
    /// feature. Elsewhere this is a no-op, so `test-support` can stay enabled
    /// in release builds without the per-frame cost.
    #[cfg(any(test, all(debug_assertions, feature = "test-support")))]
    fn debug_selector(mut self, f: impl FnOnce() -> String) -> Self {
        self.interactivity().debug_selector = Some(f());
        self
    }

    /// Set a key that can be used to look up this element's bounds
    /// in the [`crate::VisualTestContext::debug_bounds`] map.
    ///
    /// Selectors are only recorded in debug builds with the `test-support`
    /// feature. Elsewhere this is a no-op, so `test-support` can stay enabled
    /// in release builds without the per-frame cost.
    #[cfg(not(any(test, all(debug_assertions, feature = "test-support"))))]
    #[inline]
    fn debug_selector(self, _: impl FnOnce() -> String) -> Self {
        self
    }

    /// Bind the given callback to the mouse down event for any button, during the capture phase.
    /// The fluent API equivalent to [`Interactivity::capture_any_mouse_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_any_mouse_down(
        mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_any_mouse_down(listener);
        self
    }

    /// Bind the given callback to the mouse down event for any button, during the capture phase.
    /// The fluent API equivalent to [`Interactivity::on_any_mouse_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_any_mouse_down(
        mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_any_mouse_down(listener);
        self
    }

    /// Bind the given callback to the mouse up event for the given button, during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_mouse_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_mouse_up(
        mut self,
        button: MouseButton,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_up(button, listener);
        self
    }

    /// Bind the given callback to the mouse up event for any button, during the capture phase.
    /// The fluent API equivalent to [`Interactivity::capture_any_mouse_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_any_mouse_up(
        mut self,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_any_mouse_up(listener);
        self
    }

    /// Bind the given callback to the mouse pressure event, during the bubble phase
    /// the fluent API equivalent to [`Interactivity::on_mouse_pressure`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_mouse_pressure(
        mut self,
        listener: impl Fn(&MousePressureEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_pressure(listener);
        self
    }

    /// Bind the given callback to the mouse pressure event, during the capture phase
    /// the fluent API equivalent to [`Interactivity::on_mouse_pressure`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_mouse_pressure(
        mut self,
        listener: impl Fn(&MousePressureEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_mouse_pressure(listener);
        self
    }

    /// Bind the given callback to the mouse down event, on any button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The fluent API equivalent to [`Interactivity::on_mouse_down_out`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_mouse_down_out(
        mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_down_out(listener);
        self
    }

    /// Bind the given callback to the mouse up event, for the given button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The fluent API equivalent to [`Interactivity::on_mouse_up_out`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_mouse_up_out(
        mut self,
        button: MouseButton,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_up_out(button, listener);
        self
    }

    /// Bind the given callback to the mouse move event, during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_mouse_move`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_mouse_move(
        mut self,
        listener: impl Fn(&MouseMoveEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_move(listener);
        self
    }

    /// Bind the given callback to the mouse exit event, during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_mouse_exit`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_mouse_exit(
        mut self,
        listener: impl Fn(&MouseExitEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_exit(listener);
        self
    }

    /// Bind the given callback to [`FileDropEvent::Exited`] when a platform file drag
    /// leaves this element's window while the element is hovered.
    ///
    /// This is a window-local exit event, not notification that the platform drag session ended.
    /// The fluent API equivalent to [`Interactivity::on_file_drop_exit`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_file_drop_exit(
        mut self,
        listener: impl Fn(&FileDropEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_file_drop_exit(listener);
        self
    }

    /// Bind the given callback to the mouse drag event of the given type. Note that this
    /// will be called for all move events, inside or outside of this element, as long as the
    /// drag was started with this element under the mouse. Useful for implementing draggable
    /// UIs that don't conform to a drag and drop style interaction, like resizing.
    /// The fluent API equivalent to [`Interactivity::on_drag_move`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_drag_move<T: 'static>(
        mut self,
        listener: impl Fn(&DragMoveEvent<T>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_drag_move(listener);
        self
    }

    /// Bind the given callback to scroll wheel events during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_scroll_wheel`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_scroll_wheel(
        mut self,
        listener: impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_scroll_wheel(listener);
        self
    }

    /// Bind the given callback to scroll wheel events during the capture phase.
    /// The fluent API equivalent to [`Interactivity::capture_scroll_wheel`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_scroll_wheel(
        mut self,
        listener: impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_scroll_wheel(listener);
        self
    }

    /// Bind the given callback to pinch gesture events during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_pinch`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_pinch(mut self, listener: impl Fn(&PinchEvent, &mut Window, &mut App) + 'static) -> Self {
        self.interactivity().on_pinch(listener);
        self
    }

    /// Bind the given callback to pinch gesture events during the capture phase.
    /// The fluent API equivalent to [`Interactivity::capture_pinch`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_pinch(
        mut self,
        listener: impl Fn(&PinchEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_pinch(listener);
        self
    }
    /// Capture the given action, before normal action dispatch can fire.
    /// The fluent API equivalent to [`Interactivity::capture_action`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_action<A: Action>(
        mut self,
        listener: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_action(listener);
        self
    }

    /// Bind the given callback to an action dispatch during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_action`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    #[track_caller]
    #[inline(always)]
    fn on_action<A: Action>(
        mut self,
        listener: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_action(listener);
        self
    }

    /// Bind an action callback that reports an explicit dispatch outcome.
    fn on_action_with_outcome<A: Action>(
        mut self,
        listener: impl Fn(&A, &mut Window, &mut App) -> ActionDispatchOutcome + 'static,
    ) -> Self {
        self.interactivity().on_action_with_outcome(listener);
        self
    }

    /// Bind the given callback to an action dispatch, based on a dynamic action parameter
    /// instead of a type parameter. Useful for component libraries that want to expose
    /// action bindings to their users.
    /// The fluent API equivalent to [`Interactivity::on_boxed_action`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_boxed_action(
        mut self,
        action: &dyn Action,
        listener: impl Fn(&dyn Action, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_boxed_action(action, listener);
        self
    }

    /// Bind the given callback to key down events during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_key_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_key_down(
        mut self,
        listener: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_key_down(listener);
        self
    }

    /// Bind the given callback to key down events during the capture phase.
    /// The fluent API equivalent to [`Interactivity::capture_key_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_key_down(
        mut self,
        listener: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_key_down(listener);
        self
    }

    /// Bind the given callback to key up events during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_key_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_key_up(
        mut self,
        listener: impl Fn(&KeyUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_key_up(listener);
        self
    }

    /// Bind the given callback to key up events during the capture phase.
    /// The fluent API equivalent to [`Interactivity::capture_key_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_key_up(
        mut self,
        listener: impl Fn(&KeyUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_key_up(listener);
        self
    }

    /// Bind the given callback to modifiers changing events.
    /// The fluent API equivalent to [`Interactivity::on_modifiers_changed`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    #[inline(always)]
    fn on_modifiers_changed(
        mut self,
        listener: impl Fn(&ModifiersChangedEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_modifiers_changed(listener);
        self
    }

    /// Apply the given style when the given data type is dragged over this element
    fn drag_over<S: 'static>(
        mut self,
        f: impl 'static + Fn(StyleRefinement, &S, &mut Window, &mut App) -> StyleRefinement,
    ) -> Self {
        self.interactivity().drag_over_styles.push((
            TypeId::of::<S>(),
            Box::new(move |currently_dragged: &dyn Any, window, cx| {
                f(
                    StyleRefinement::default(),
                    currently_dragged.downcast_ref::<S>().unwrap(),
                    window,
                    cx,
                )
            }),
        ));
        self
    }

    /// Apply the given style when the given data type is dragged over this element's group
    fn group_drag_over<S: 'static>(
        mut self,
        group_name: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self {
        self.interactivity().group_drag_over_styles.push((
            TypeId::of::<S>(),
            GroupStyle {
                group: group_name.into(),
                style: Box::new(f(StyleRefinement::default())),
            },
        ));
        self
    }

    /// Bind the given callback to drop events of the given type, whether or not the drag started on this element.
    /// The fluent API equivalent to [`Interactivity::on_drop`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_drop<T: 'static>(
        mut self,
        listener: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_drop(listener);
        self
    }

    /// Use the given predicate to determine whether or not a drop event should be dispatched to this element.
    /// The fluent API equivalent to [`Interactivity::can_drop`].
    fn can_drop(
        mut self,
        predicate: impl Fn(&dyn Any, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.interactivity().can_drop(predicate);
        self
    }

    /// Block the mouse from all interactions with elements behind this element's hitbox. Typically
    /// `block_mouse_except_scroll` should be preferred.
    /// The fluent API equivalent to [`Interactivity::occlude_mouse`].
    fn occlude(mut self) -> Self {
        self.interactivity().occlude_mouse();
        self
    }

    /// Set the bounds of this element as a window control area for the platform window.
    /// The fluent API equivalent to [`Interactivity::window_control_area`].
    fn window_control_area(mut self, area: WindowControlArea) -> Self {
        self.interactivity().window_control_area(area);
        self
    }

    /// Block non-scroll mouse interactions with elements behind this element's hitbox.
    /// The fluent API equivalent to [`Interactivity::block_mouse_except_scroll`].
    ///
    /// See [`Hitbox::is_hovered`] for details.
    fn block_mouse_except_scroll(mut self) -> Self {
        self.interactivity().block_mouse_except_scroll();
        self
    }

    /// Set the given styles to be applied when this element, specifically, is focused.
    /// Requires that the element is focusable. Elements can be made focusable using [`InteractiveElement::track_focus`].
    fn focus(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self
    where
        Self: Sized,
    {
        self.interactivity().focus_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    /// Set the given styles to be applied when this element is inside another element that is focused.
    /// Requires that the element is focusable. Elements can be made focusable using [`InteractiveElement::track_focus`].
    fn in_focus(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self
    where
        Self: Sized,
    {
        self.interactivity().in_focus_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    /// Set the given styles to be applied when this element is focused via keyboard navigation.
    /// This is similar to CSS's `:focus-visible` pseudo-class - it only applies when the element
    /// is focused AND the user is navigating via keyboard (not mouse clicks).
    /// Requires that the element is focusable. Elements can be made focusable using [`InteractiveElement::track_focus`].
    fn focus_visible(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self
    where
        Self: Sized,
    {
        self.interactivity().focus_visible_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }
}

/// A trait for elements that want to use the standard GPUI interactivity features
/// that require state.
pub trait StatefulInteractiveElement: InteractiveElement {
    /// Set the accessible role for this element.
    fn role(mut self, role: accesskit::Role) -> Self {
        debug_assert!(role != accesskit::Role::GenericContainer);
        self.interactivity().override_role = Some(role);
        self
    }

    /// Set the author-provided identifier exposed to accessibility clients.
    ///
    /// Unlike the GPUI element ID, this value is visible outside the process.
    /// Keep it stable and unique within its accessibility tree.
    /// AccessKit maps it to platform identifiers where supported, including
    /// UIA `AutomationId` on Windows, `AXIdentifier` on macOS, and AT-SPI
    /// `AccessibleId` on Linux stacks whose deployed adapter exposes it.
    fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
        self.interactivity().aria.author_id = Some(id.into());
        self
    }

    /// Set the accessible label for this element.
    fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.interactivity().aria.label = Some(label.into());
        self
    }

    /// Set the accessible description for this element. Unlike the label (which
    /// names the element), the description provides supplementary information
    /// that assistive technology announces after the name, role, and value -
    /// for example a settings subtitle or a hint.
    fn aria_description(mut self, description: impl Into<SharedString>) -> Self {
        self.interactivity().aria.description = Some(description.into());
        self
    }

    /// Set the keyboard shortcut(s) that activate this element, announced by
    /// assistive technology (maps to AccessKit's `keyboard_shortcut`).
    ///
    /// Note that this does not create a keymap. It simply instructs assistive
    /// technology what the keymap is.
    fn aria_keyshortcuts(mut self, keyshortcuts: impl Into<SharedString>) -> Self {
        self.interactivity().aria.keyshortcuts = Some(keyshortcuts.into());
        self
    }

    /// Report this element as the focused node in the accessibility tree,
    /// overriding the element that holds real keyboard focus — but only while
    /// one of its ancestors actually holds focus.
    ///
    /// This implements the `aria-activedescendant` pattern for composite
    /// widgets that keep keyboard focus on a container (e.g. a menu or
    /// listbox) while a child is "selected": set this on the selected child so
    /// assistive technology announces and highlights it as focused.
    ///
    /// The element must also have a [`role`][Self::role] (and an id) so it
    /// produces an accessibility node. Unlike the web's container-side
    /// `aria-activedescendant`, this is set on the descendant; GPUI honors it
    /// only when a focused ancestor is present in the tree, so it is safe to
    /// set unconditionally on the selected child — if the container isn't
    /// focused, the claim is ignored.
    fn aria_active_descendant(mut self) -> Self {
        self.interactivity().report_active_descendant_focus = true;
        self
    }

    /// Contribute synthetic accessibility nodes — nodes that don't correspond
    /// to any element — as children of this element's a11y node. For example,
    /// text runs describing an editor's text content.
    ///
    /// The closure is called after this element is prepainted, and only if it
    /// contributed a node to the accessibility tree (i.e. it has an id and a
    /// [`role`][StatefulInteractiveElement::role]).
    ///
    /// See [`Element::a11y_synthetic_children`] for details.
    fn a11y_synthetic_children(
        mut self,
        f: impl FnOnce(&mut crate::A11ySubtreeBuilder) + 'static,
    ) -> Self {
        self.interactivity().a11y_synthetic_children = Some(Box::new(f));
        self
    }

    /// Set the selected state for this element.
    fn aria_selected(mut self, selected: bool) -> Self {
        self.interactivity().aria.selected = Some(selected);
        self
    }

    /// Set whether this element's value is invalid.
    fn aria_invalid(mut self, invalid: bool) -> Self {
        self.interactivity().aria.invalid = Some(invalid);
        self
    }

    /// Set whether this element is disabled for accessibility clients.
    fn aria_disabled(mut self, disabled: bool) -> Self {
        self.interactivity().aria.disabled = Some(disabled);
        self
    }

    /// Set the expanded state for this element.
    fn aria_expanded(mut self, expanded: bool) -> Self {
        self.interactivity().aria.expanded = Some(expanded);
        self
    }

    /// Set the toggled state for this element.
    fn aria_toggled(mut self, toggled: accesskit::Toggled) -> Self {
        self.interactivity().aria.toggled = Some(toggled);
        self
    }

    /// Set the numeric value for this element.
    fn aria_numeric_value(mut self, value: f64) -> Self {
        self.interactivity().aria.numeric_value = Some(value);
        self
    }

    /// Set the step by which assistive technology should expect the numeric
    /// value of this element to change (e.g. when incrementing a spin button).
    fn aria_numeric_value_step(mut self, step: f64) -> Self {
        self.interactivity().aria.numeric_value_step = Some(step);
        self
    }

    /// Set the string value of this element, e.g. the text content of a simple
    /// text input.
    fn aria_value(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().aria.value = Some(value.into());
        self
    }

    /// Set the placeholder text reported to assistive technology for this
    /// element, shown when a text input is empty.
    fn aria_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.interactivity().aria.placeholder = Some(placeholder.into());
        self
    }

    /// Set the minimum numeric value for this element.
    fn aria_min_numeric_value(mut self, value: f64) -> Self {
        self.interactivity().aria.min_numeric_value = Some(value);
        self
    }

    /// Set the maximum numeric value for this element.
    fn aria_max_numeric_value(mut self, value: f64) -> Self {
        self.interactivity().aria.max_numeric_value = Some(value);
        self
    }

    /// Set the orientation of this element.
    fn aria_orientation(mut self, orientation: accesskit::Orientation) -> Self {
        self.interactivity().aria.orientation = Some(orientation);
        self
    }

    /// Set the heading level of this element.
    fn aria_level(mut self, level: usize) -> Self {
        self.interactivity().aria.level = Some(level);
        self
    }

    /// Set the position in set of this element.
    fn aria_position_in_set(mut self, position: usize) -> Self {
        self.interactivity().aria.position_in_set = Some(position);
        self
    }

    /// Set the size of set for this element.
    fn aria_size_of_set(mut self, size: usize) -> Self {
        self.interactivity().aria.size_of_set = Some(size);
        self
    }

    /// Set the row index for this element.
    fn aria_row_index(mut self, index: usize) -> Self {
        self.interactivity().aria.row_index = Some(index);
        self
    }

    /// Set the column index for this element.
    fn aria_column_index(mut self, index: usize) -> Self {
        self.interactivity().aria.column_index = Some(index);
        self
    }

    /// Set the row count for this element.
    fn aria_row_count(mut self, count: usize) -> Self {
        self.interactivity().aria.row_count = Some(count);
        self
    }

    /// Set the column count for this element.
    fn aria_column_count(mut self, count: usize) -> Self {
        self.interactivity().aria.column_count = Some(count);
        self
    }

    /// Register a handler for an accessibility action on this element.
    /// The handler is called when a screen reader requests the given action.
    ///
    /// See the [accessibility guide](crate::_accessibility) for an overview.
    #[inline(always)]
    fn on_a11y_action(
        mut self,
        action: accesskit::Action,
        listener: impl FnMut(Option<&accesskit::ActionData>, &mut crate::Window, &mut crate::App)
        + 'static,
    ) -> Self {
        self.interactivity()
            .a11y_action_listeners
            .push((action, Box::new(listener)));
        self
    }

    /// Set this element to focusable.
    fn focusable(mut self) -> Self {
        self.interactivity().focusable = true;
        self
    }

    /// Set the overflow x and y to scroll.
    fn overflow_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.x = Some(Overflow::Scroll);
        self.interactivity().base_style.overflow.y = Some(Overflow::Scroll);
        self
    }

    /// Set the overflow x to scroll.
    fn overflow_x_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.x = Some(Overflow::Scroll);
        self
    }

    /// Set the overflow y to scroll.
    fn overflow_y_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.y = Some(Overflow::Scroll);
        self
    }

    /// Track the scroll state of this element with the given handle.
    fn track_scroll(mut self, scroll_handle: &ScrollHandle) -> Self {
        self.interactivity().tracked_scroll_handle = Some(scroll_handle.clone());
        self
    }

    /// Track the scroll state of this element with the given handle.
    fn anchor_scroll(mut self, scroll_anchor: Option<ScrollAnchor>) -> Self {
        self.interactivity().scroll_anchor = scroll_anchor;
        self
    }

    /// Set the given styles to be applied when this element is active.
    fn active(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self
    where
        Self: Sized,
    {
        self.interactivity().active_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    /// Set the given styles to be applied when this element's group is active.
    fn group_active(
        mut self,
        group_name: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().group_active_style = Some(GroupStyle {
            group: group_name.into(),
            style: Box::new(f(StyleRefinement::default())),
        });
        self
    }

    /// Bind the given callback to click events of this element.
    /// The fluent API equivalent to [`Interactivity::on_click`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    #[inline(always)]
    fn on_click(mut self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_click(listener);
        self
    }

    /// Bind the given callback to non-primary click events of this element.
    /// The fluent API equivalent to [`Interactivity::on_aux_click`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_aux_click(
        mut self,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_aux_click(listener);
        self
    }

    /// On drag initiation, this callback will be used to create a new view to render the dragged value for a
    /// drag and drop operation. This API should also be used as the equivalent of 'on drag start' with
    /// the [`InteractiveElement::on_drag_move`] API.
    /// The callback also has access to the offset of triggering click from the origin of parent element.
    /// The fluent API equivalent to [`Interactivity::on_drag`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_drag<T, W>(
        mut self,
        value: T,
        constructor: impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W> + 'static,
    ) -> Self
    where
        Self: Sized,
        T: 'static,
        W: 'static + Render,
    {
        self.interactivity().on_drag(value, constructor);
        self
    }

    /// Registers a callback resolving a payload to offer the platform if a drag started by this
    /// element leaves the window. It is invoked at most once per drag gesture, when the pointer
    /// exits the viewport. Must be called after [`Self::on_drag`], with the same dragged value
    /// type `T`.
    /// The fluent API equivalent to [`Interactivity::external_drag_payload`].
    fn external_drag_payload<T>(
        mut self,
        resolver: impl Fn(&T, &mut Window, &mut App) -> Option<ExternalDragPayload> + 'static,
    ) -> Self
    where
        Self: Sized,
        T: 'static,
    {
        self.interactivity().external_drag_payload(resolver);
        self
    }

    /// Bind the given callback on the hover start and end events of this element. Note that the boolean
    /// passed to the callback is true when the hover starts and false when it ends.
    /// Transitions caused by layout changes under a stationary mouse also invoke the callback.
    ///
    /// By default, keyboard input suppresses hover until the next mouse move, mouse down, or touch. Set
    /// [`HoverListenerMode::InputModalityIndependent`] with [`Self::hover_listener_mode`] to
    /// continue hit-testing hover after keyboard input.
    /// The fluent API equivalent to [`Interactivity::on_hover`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_hover(mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_hover(listener);
        self
    }

    /// Bind the given callback to sticky phase changes for this element.
    ///
    /// The element must have an id so GPUI can compare the current sticky phase with the previous
    /// frame. Use [`InteractiveElement::when_stuck`] for visual-only styling such as a drop shadow.
    fn on_sticky_state_changed(
        mut self,
        listener: impl Fn(&StickyStateChangedEvent, &mut Window, &mut App) + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_sticky_state_changed(listener);
        self
    }

    /// Sets how [`Self::on_hover`] responds to key presses while the mouse is stationary.
    /// This affects only the hover listener, not hover styles or tooltips. The fluent API
    /// equivalent to [`Interactivity::hover_listener_mode`].
    fn hover_listener_mode(mut self, mode: HoverListenerMode) -> Self
    where
        Self: Sized,
    {
        self.interactivity().hover_listener_mode(mode);
        self
    }

    /// Constructs a tooltip when the element is hovered or long-pressed.
    /// The fluent API equivalent to [`Interactivity::tooltip`].
    fn tooltip(mut self, build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self
    where
        Self: Sized,
    {
        self.interactivity().tooltip(build_tooltip);
        self
    }

    /// Constructs a tooltip when the element is hovered or long-pressed.
    /// The tooltip itself is also hoverable and won't disappear when the user moves the mouse into
    /// the tooltip. The fluent API equivalent to [`Interactivity::hoverable_tooltip`].
    fn hoverable_tooltip(
        mut self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().hoverable_tooltip(build_tooltip);
        self
    }

    /// Sets the delay before this element's tooltip is shown on hover.
    ///
    /// Touch long presses show the tooltip immediately once the gesture is recognized.
    /// The fluent API equivalent to [`Interactivity::tooltip_show_delay`].
    fn tooltip_show_delay(mut self, delay: Duration) -> Self
    where
        Self: Sized,
    {
        self.interactivity().tooltip_show_delay(delay);
        self
    }
}

pub(crate) type MouseDownListener =
    Box<dyn Fn(&MouseDownEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;
pub(crate) type MouseUpListener =
    Box<dyn Fn(&MouseUpEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;
pub(crate) type MousePressureListener =
    Box<dyn Fn(&MousePressureEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;
pub(crate) type MouseMoveListener =
    Box<dyn Fn(&MouseMoveEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;
pub(crate) type MouseExitListener =
    Box<dyn Fn(&MouseExitEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type FileDropExitListener =
    Box<dyn Fn(&FileDropEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type ScrollWheelListener =
    Box<dyn Fn(&ScrollWheelEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type PinchListener =
    Box<dyn Fn(&PinchEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type ClickListener = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub(crate) type StickyStateChangedListener =
    Box<dyn Fn(&StickyStateChangedEvent, &mut Window, &mut App) + 'static>;

/// Controls how [`StatefulInteractiveElement::on_hover`] responds to key presses while the mouse
/// is stationary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HoverListenerMode {
    /// Use input-modality-aware hit testing. Keyboard input suppresses hover until the mouse moves
    /// again, unless pointer capture or an active mouse-down interaction keeps the listener hovered.
    #[default]
    InputModalityAware,
    /// Use hit testing even when the last input was from the keyboard. This changes only
    /// keyboard-modality filtering; all other [`StatefulInteractiveElement::on_hover`] behavior
    /// remains unchanged.
    InputModalityIndependent,
}

impl HoverListenerMode {
    fn is_hovered(self, hitbox: &Hitbox, window: &Window) -> bool {
        match self {
            Self::InputModalityAware => hitbox.is_hovered(window),
            Self::InputModalityIndependent => hitbox.id.is_hovered_ignoring_last_input(window),
        }
    }
}

pub(crate) struct DragListener {
    value: Arc<dyn Any>,
    render: Box<dyn Fn(&dyn Any, Point<Pixels>, &mut Window, &mut App) -> AnyView + 'static>,
    external_payload: Option<ExternalDragPayloadResolver>,
}

type ExternalDragPayloadResolver =
    Box<dyn Fn(&dyn Any, &mut Window, &mut App) -> Option<ExternalDragPayload> + 'static>;

type DropListener = Box<dyn Fn(&dyn Any, &mut Window, &mut App) + 'static>;

type CanDropPredicate = Box<dyn Fn(&dyn Any, &mut Window, &mut App) -> bool + 'static>;

pub(crate) struct TooltipBuilder {
    build: Rc<dyn Fn(&mut Window, &mut App) -> AnyView + 'static>,
    hoverable: bool,
}

pub(crate) type KeyDownListener =
    Box<dyn Fn(&KeyDownEvent, DispatchPhase, &mut Window, &mut App) + 'static>;

pub(crate) type KeyUpListener =
    Box<dyn Fn(&KeyUpEvent, DispatchPhase, &mut Window, &mut App) + 'static>;

pub(crate) type ModifiersChangedListener =
    Box<dyn Fn(&ModifiersChangedEvent, &mut Window, &mut App) + 'static>;

pub(crate) type ActionListener =
    Box<dyn Fn(&dyn Any, DispatchPhase, &mut Window, &mut App) + 'static>;

pub(crate) type ActionOutcomeListener =
    Box<dyn Fn(&dyn Any, &mut Window, &mut App) -> ActionDispatchOutcome + 'static>;

/// Construct a new [`Div`] element
#[track_caller]
pub fn div() -> Div {
    Div {
        interactivity: Interactivity::new(),
        children: SmallVec::default(),
        prepaint_listener: None,
        image_cache: None,
        prepaint_order_fn: None,
    }
}

/// A [`Div`] element, the all-in-one element for building complex UIs in GPUI
pub struct Div {
    interactivity: Interactivity,
    children: SmallVec<[StackSafe<AnyElement>; 2]>,
    prepaint_listener: Option<Box<dyn Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static>>,
    image_cache: Option<Box<dyn ImageCacheProvider>>,
    prepaint_order_fn: Option<Box<dyn Fn(&mut Window, &mut App) -> SmallVec<[usize; 8]>>>,
}

impl Div {
    /// Assigns this sticky div to a contiguous stacking run.
    ///
    /// Equal adjacent identifiers stack on the configured `top` or `left` edge. A different
    /// identifier replaces the prior active run when its root reaches the sticky origin.
    pub fn sticky_stack_id(mut self, id: impl Into<ElementId>) -> Self {
        self.interactivity.base_style.sticky_stack_id = Some(id.into());
        self
    }

    /// Add a listener to be called when the children of this `Div` are prepainted.
    /// This allows you to store the [`Bounds`] of the children for later use.
    pub fn on_children_prepainted(
        mut self,
        listener: impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.prepaint_listener = Some(Box::new(listener));
        self
    }

    /// Add an image cache at the location of this div in the element tree.
    pub fn image_cache(mut self, cache: impl ImageCacheProvider) -> Self {
        self.image_cache = Some(Box::new(cache));
        self
    }

    /// Specify a function that determines the order in which children are prepainted.
    ///
    /// The function is called at prepaint time and should return a vector of child indices
    /// in the desired prepaint order. Each index should appear exactly once.
    ///
    /// This is useful when the prepaint of one child affects state that another child reads.
    /// For example, in split editor views, the editor with an autoscroll request should
    /// be prepainted first so its scroll position update is visible to the other editor.
    pub fn with_dynamic_prepaint_order(
        mut self,
        order_fn: impl Fn(&mut Window, &mut App) -> SmallVec<[usize; 8]> + 'static,
    ) -> Self {
        self.prepaint_order_fn = Some(Box::new(order_fn));
        self
    }
}

/// A frame state for a `Div` element, which contains layout IDs for its children.
///
/// This struct is used internally by the `Div` element to manage the layout state of its children
/// during the UI update cycle. It holds a small vector of `LayoutId` values, each corresponding to
/// a child element of the `Div`. These IDs are used to query the layout engine for the computed
/// bounds of the children after the layout phase is complete.
pub struct DivFrameState {
    child_layout_ids: SmallVec<[LayoutId; 2]>,
    sticky_layout_context_id: Option<usize>,
    paint_order: SmallVec<[usize; 8]>,
}

/// A frame's prepaint result for a `Div` element.
pub struct DivPrepaintState {
    hitbox: Option<Hitbox>,
    positioned_bounds: PositionedBounds,
    sticky_state: Option<StickyState>,
}

/// Interactivity state displayed an manipulated in the inspector.
#[derive(Clone)]
pub struct DivInspectorState {
    /// The inspected element's base style. This is used for both inspecting and modifying the
    /// state. In the future it will make sense to separate the read and write, possibly tracking
    /// the modifications.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub base_style: Box<StyleRefinement>,
    /// Inspects the bounds of the element.
    pub bounds: Bounds<Pixels>,
    /// Size of the children of the element, or `bounds.size` if it has no children.
    pub content_size: Size<Pixels>,
}

impl Styled for Div {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for Div {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl ParentElement for Div {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        #[cfg(feature = "stacker")]
        self.children
            .extend(elements.into_iter().map(StackSafe::new));
        #[cfg(not(feature = "stacker"))]
        self.children.extend(elements);
    }
}

impl Element for Div {
    type RequestLayoutState = DivFrameState;
    type PrepaintState = DivPrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.interactivity.source_location()
    }

    fn a11y_role(&self) -> Option<accesskit::Role> {
        // Nodes with `GenericContainer` should never be reported to accesskit.
        // Equivalent to an HTML div with no role.
        self.interactivity
            .override_role
            .filter(|role| *role != accesskit::Role::GenericContainer)
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.interactivity.write_a11y_info(node);
    }

    fn a11y_synthetic_children(
        &mut self,
        _prepaint: &mut Self::PrepaintState,
        builder: &mut crate::A11ySubtreeBuilder,
    ) {
        if let Some(f) = self.interactivity.a11y_synthetic_children.take() {
            f(builder);
        }
    }

    #[cfg_attr(feature = "stacker", stacksafe::stacksafe)]
    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut child_layout_ids = SmallVec::new();
        let mut sticky_layout_context_id = None;
        let mut sticky_occurrence = None;
        let image_cache = self
            .image_cache
            .as_mut()
            .map(|provider| provider.provide(window, cx));

        let layout_id = window.with_image_cache(image_cache, |window| {
            self.interactivity.request_layout_with_style_handoff(
                global_id,
                inspector_id,
                window,
                cx,
                |style, window, cx| {
                    sticky_occurrence = window.reserve_sticky_candidate(style);
                    sticky_layout_context_id = window.reserve_sticky_layout_context(style.overflow);
                    let layout_id = window.with_text_style(style.text_style().cloned(), |window| {
                        let mut request_children = |window: &mut Window| {
                            child_layout_ids = self
                                .children
                                .iter_mut()
                                .map(|child| child.request_layout(window, cx))
                                .collect::<SmallVec<_>>();
                        };
                        if let Some(context_id) = sticky_layout_context_id {
                            window.with_sticky_layout_context(
                                context_id,
                                style.overflow,
                                |window| request_children(window),
                            );
                        } else {
                            request_children(window);
                        }
                        window.request_layout_ref(style, child_layout_ids.iter().copied(), cx)
                    });
                    window.complete_sticky_candidate(sticky_occurrence, layout_id);
                    layout_id
                },
            )
        });
        self.interactivity.sticky_occurrence = sticky_occurrence;

        (
            layout_id,
            DivFrameState {
                paint_order: (0..child_layout_ids.len()).collect(),
                child_layout_ids,
                sticky_layout_context_id,
            },
        )
    }

    #[cfg_attr(feature = "stacker", stacksafe::stacksafe)]
    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> DivPrepaintState {
        let image_cache = self
            .image_cache
            .as_mut()
            .map(|provider| provider.provide(window, cx));

        let has_prepaint_listener = self.prepaint_listener.is_some();
        let mut children_bounds = Vec::with_capacity(if has_prepaint_listener {
            request_layout.child_layout_ids.len()
        } else {
            0
        });

        let mut child_min = point(Pixels::MAX, Pixels::MAX);
        let mut child_max = Point::default();
        if let Some(handle) = self.interactivity.scroll_anchor.as_ref() {
            *handle.last_origin.borrow_mut() = bounds.origin - window.element_offset();
        }
        let content_size = if request_layout.child_layout_ids.is_empty() {
            bounds.size
        } else if let Some(scroll_handle) = self.interactivity.tracked_scroll_handle.as_ref() {
            crate::fast::layers::input::track_handle(window, &scroll_handle.0);
            if let Ok(mut state) = scroll_handle.0.try_borrow_mut() {
                state.child_bounds = Vec::with_capacity(request_layout.child_layout_ids.len());
                for child_layout_id in &request_layout.child_layout_ids {
                    let child_bounds = window.layout_bounds(*child_layout_id);
                    child_min = child_min.min(&child_bounds.origin);
                    child_max = child_max.max(&child_bounds.bottom_right());
                    state.child_bounds.push(child_bounds);
                }
                (child_max - child_min).into()
            } else {
                for child_layout_id in &request_layout.child_layout_ids {
                    let child_bounds = window.layout_bounds(*child_layout_id);
                    child_min = child_min.min(&child_bounds.origin);
                    child_max = child_max.max(&child_bounds.bottom_right());
                }
                (child_max - child_min).into()
            }
        } else {
            for child_layout_id in &request_layout.child_layout_ids {
                let child_bounds = window.layout_bounds(*child_layout_id);
                child_min = child_min.min(&child_bounds.origin);
                child_max = child_max.max(&child_bounds.bottom_right());

                if has_prepaint_listener {
                    children_bounds.push(child_bounds);
                }
            }
            (child_max - child_min).into()
        };

        if let Some(scroll_handle) = self.interactivity.tracked_scroll_handle.as_ref() {
            let _ = scroll_handle.try_request_active_item();
        }

        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            content_size,
            window,
            cx,
            |style, scroll_offset, hitbox, positioned_bounds, sticky_state, window, cx| {
                // skip children
                if style.display == Display::None {
                    return DivPrepaintState {
                        hitbox,
                        positioned_bounds,
                        sticky_state,
                    };
                }

                window.with_image_cache(image_cache, |window| {
                    let bounds = positioned_bounds.visual_bounds;
                    let fast_layer = crate::fast::layers::paint::begin_children(
                        window,
                        cx,
                        global_id,
                        bounds,
                        child_min,
                        content_size,
                        scroll_offset,
                        style,
                        positioned_bounds.delta != Point::default(),
                    );
                    let scroll_offset = fast_layer.scroll_offset();
                    let scroll_context = (style.overflow.x == Overflow::Scroll
                        || style.overflow.y == Overflow::Scroll)
                        .then(|| ScrollContainerContext {
                            scrollport_bounds: bounds,
                            content_bounds: Bounds::new(
                                bounds.origin + scroll_offset,
                                content_size,
                            ),
                            scroll_offset,
                            overflow: style.overflow,
                        });

                    let mut prepaint_children = |window: &mut Window, cx: &mut App| {
                        request_layout.paint_order.clear();
                        request_layout.paint_order.extend(0..self.children.len());
                        window.reorder_sticky_children(
                            &request_layout.child_layout_ids,
                            &mut request_layout.paint_order,
                        );

                        let prepaint_order = if let Some(order_fn) = &self.prepaint_order_fn {
                            let mut order = order_fn(window, cx);
                            window.reorder_sticky_children(
                                &request_layout.child_layout_ids,
                                &mut order,
                            );
                            order
                        } else {
                            request_layout.paint_order.clone()
                        };
                        for idx in prepaint_order {
                            if let Some(child) = self.children.get_mut(idx) {
                                child.prepaint(window, cx);
                            }
                        }
                    };

                    // Children whose scroll layer is composited are not prepainted.
                    if !fast_layer.prepainted() {
                    } else if let Some(scroll_context) = scroll_context {
                        window.with_scroll_container_context(scroll_context, |window| {
                            window.with_element_offset(
                                positioned_bounds.delta + scroll_offset,
                                |window| {
                                    if let Some(context_id) =
                                        request_layout.sticky_layout_context_id
                                    {
                                        window.prepare_sticky_context(context_id, scroll_context);
                                    }
                                    prepaint_children(window, cx);
                                },
                            );
                        });
                    } else {
                        window.with_element_offset(
                            positioned_bounds.delta + scroll_offset,
                            |window| {
                                prepaint_children(window, cx);
                            },
                        );
                    }
                    crate::fast::layers::paint::end_children(window, cx, fast_layer);

                    if let Some(listener) = self.prepaint_listener.as_ref() {
                        crate::fast::layers::invalidate::note_uncarried(window);
                        listener(children_bounds, window, cx);
                    }
                });

                DivPrepaintState {
                    hitbox,
                    positioned_bounds,
                    sticky_state,
                }
            },
        )
    }

    #[cfg_attr(feature = "stacker", stacksafe::stacksafe)]
    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut DivPrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let image_cache = self
            .image_cache
            .as_mut()
            .map(|provider| provider.provide(window, cx));
        let content_size = self.interactivity.content_size;
        let scroll_offset = self
            .interactivity
            .scroll_offset
            .as_ref()
            .map(|offset| *offset.borrow())
            .unwrap_or_default();
        let bounds = prepaint.positioned_bounds.visual_bounds;
        self.interactivity.current_sticky_state = prepaint.sticky_state;

        window.with_image_cache(image_cache, |window| {
            self.interactivity.paint(
                global_id,
                inspector_id,
                bounds,
                prepaint.hitbox.as_ref(),
                window,
                cx,
                |style, window, cx| {
                    // skip children
                    if style.display == Display::None {
                        return;
                    }

                    let scroll_context = (style.overflow.x == Overflow::Scroll
                        || style.overflow.y == Overflow::Scroll)
                        .then(|| ScrollContainerContext {
                            scrollport_bounds: bounds,
                            content_bounds: Bounds::new(
                                bounds.origin + scroll_offset,
                                content_size,
                            ),
                            scroll_offset,
                            overflow: style.overflow,
                        });

                    let mut paint_children = |window: &mut Window, cx: &mut App| {
                        crate::fast::layers::paint::paint_children(
                            window,
                            cx,
                            global_id,
                            |window, cx| {
                                for idx in _request_layout.paint_order.iter().copied() {
                                    if let Some(child) = self.children.get_mut(idx) {
                                        child.paint(window, cx);
                                    }
                                }
                            },
                        );
                    };

                    if let Some(scroll_context) = scroll_context {
                        window.with_scroll_container_context(scroll_context, |window| {
                            paint_children(window, cx);
                        });
                    } else {
                        paint_children(window, cx);
                    }
                },
            )
        });
    }
}

impl IntoElement for Div {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[derive(Default)]
pub(crate) struct AriaProperties {
    pub(crate) author_id: Option<SharedString>,
    pub(crate) label: Option<SharedString>,
    pub(crate) description: Option<SharedString>,
    pub(crate) keyshortcuts: Option<SharedString>,
    pub(crate) selected: Option<bool>,
    pub(crate) invalid: Option<bool>,
    pub(crate) disabled: Option<bool>,
    pub(crate) expanded: Option<bool>,
    pub(crate) toggled: Option<accesskit::Toggled>,
    pub(crate) numeric_value: Option<f64>,
    pub(crate) min_numeric_value: Option<f64>,
    pub(crate) max_numeric_value: Option<f64>,
    pub(crate) numeric_value_step: Option<f64>,
    pub(crate) value: Option<SharedString>,
    pub(crate) placeholder: Option<SharedString>,
    pub(crate) orientation: Option<accesskit::Orientation>,
    pub(crate) level: Option<usize>,
    pub(crate) position_in_set: Option<usize>,
    pub(crate) size_of_set: Option<usize>,
    pub(crate) row_index: Option<usize>,
    pub(crate) column_index: Option<usize>,
    pub(crate) row_count: Option<usize>,
    pub(crate) column_count: Option<usize>,
}

/// The interactivity struct. Powers all of the general-purpose
/// interactivity in the `Div` element.
#[derive(Default)]
pub struct Interactivity {
    /// The element ID of the element. In id is required to support a stateful subset of the interactivity such as on_click.
    pub element_id: Option<ElementId>,
    /// Whether the element was clicked. This will only be present after layout.
    pub active: Option<bool>,
    /// Whether the element was hovered. This will only be present after paint if an hitbox
    /// was created for the interactive element.
    ///
    /// It is published, not painted from: reading it does not make the element's
    /// cached view depend on the hover. An element that paints by hover must ask
    /// [`Hitbox::is_hovered`], so a cached view holding it is never replayed
    /// with a stale hover.
    pub hovered: Option<bool>,
    pub(crate) tooltip_id: Option<TooltipId>,
    pub(crate) content_size: Size<Pixels>,
    pub(crate) key_context: Option<KeyContext>,
    pub(crate) focusable: bool,
    pub(crate) tracked_focus_handle: Option<FocusHandle>,
    pub(crate) tracked_scroll_handle: Option<ScrollHandle>,
    pub(crate) scroll_anchor: Option<ScrollAnchor>,
    pub(crate) scroll_offset: Option<Rc<RefCell<Point<Pixels>>>>,
    pub(crate) ongoing_scroll: Option<Rc<RefCell<OngoingScroll>>>,
    pub(crate) group: Option<SharedString>,
    /// The base style of the element, before any modifications are applied
    /// by focus, active, etc.
    pub base_style: Box<StyleRefinement>,
    pub(crate) focus_style: Option<Box<StyleRefinement>>,
    pub(crate) in_focus_style: Option<Box<StyleRefinement>>,
    pub(crate) focus_visible_style: Option<Box<StyleRefinement>>,
    pub(crate) hover_style: Option<Box<StyleRefinement>>,
    pub(crate) group_hover_style: Option<GroupStyle>,
    pub(crate) stuck_style: Option<Box<StyleRefinement>>,
    pub(crate) active_style: Option<Box<StyleRefinement>>,
    pub(crate) group_active_style: Option<GroupStyle>,
    pub(crate) drag_over_styles: Vec<(
        TypeId,
        Box<dyn Fn(&dyn Any, &mut Window, &mut App) -> StyleRefinement>,
    )>,
    pub(crate) group_drag_over_styles: Vec<(TypeId, GroupStyle)>,
    pub(crate) mouse_down_listeners: Vec<MouseDownListener>,
    pub(crate) mouse_up_listeners: Vec<MouseUpListener>,
    pub(crate) mouse_pressure_listeners: Vec<MousePressureListener>,
    pub(crate) mouse_move_listeners: Vec<MouseMoveListener>,
    pub(crate) mouse_exit_listeners: Vec<MouseExitListener>,
    pub(crate) file_drop_exit_listeners: Vec<FileDropExitListener>,
    pub(crate) scroll_wheel_listeners: Vec<ScrollWheelListener>,
    pub(crate) pinch_listeners: Vec<PinchListener>,
    pub(crate) key_down_listeners: Vec<KeyDownListener>,
    pub(crate) key_up_listeners: Vec<KeyUpListener>,
    pub(crate) modifiers_changed_listeners: Vec<ModifiersChangedListener>,
    pub(crate) action_listeners: Vec<(TypeId, ActionListener)>,
    pub(crate) action_outcome_listeners: Vec<(TypeId, ActionOutcomeListener)>,
    pub(crate) drop_listeners: Vec<(TypeId, DropListener)>,
    pub(crate) can_drop_predicate: Option<CanDropPredicate>,
    pub(crate) click_listeners: Vec<ClickListener>,
    pub(crate) aux_click_listeners: Vec<ClickListener>,
    pub(crate) drag_listener: Option<DragListener>,
    pub(crate) hover_listener: Option<Box<dyn Fn(&bool, &mut Window, &mut App)>>,
    pub(crate) hover_listener_mode: HoverListenerMode,
    pub(crate) sticky_state_changed_listener: Option<StickyStateChangedListener>,
    pub(crate) tooltip_builder: Option<TooltipBuilder>,
    pub(crate) tooltip_show_delay: Option<Duration>,
    pub(crate) window_control: Option<WindowControlArea>,
    pub(crate) hitbox_behavior: HitboxBehavior,
    pub(crate) tab_index: Option<isize>,
    pub(crate) tab_group: bool,
    pub(crate) tab_stop: bool,

    pub(crate) a11y_action_listeners:
        Vec<(accesskit::Action, crate::window::a11y::A11yActionListener)>,
    pub(crate) a11y_synthetic_children: Option<Box<dyn FnOnce(&mut crate::A11ySubtreeBuilder)>>,
    pub(crate) report_active_descendant_focus: bool,
    pub(crate) override_role: Option<accesskit::Role>,
    pub(crate) aria: AriaProperties,

    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) source_location: Option<&'static core::panic::Location<'static>>,

    #[cfg(any(test, all(debug_assertions, feature = "test-support")))]
    pub(crate) debug_selector: Option<String>,

    pub(crate) current_sticky_state: Option<StickyState>,
    pub(crate) sticky_occurrence: Option<usize>,
    pub(crate) style_handoff_index: Option<usize>,
    #[cfg(feature = "diagnostics")]
    pub(crate) request_prepaint_style_candidate: bool,
    #[cfg(feature = "diagnostics")]
    pub(crate) request_style_is_gpux_owned: bool,
}

impl Interactivity {
    /// Layout this element according to this interactivity state's configured styles
    #[cfg_attr(feature = "diagnostics", track_caller)]
    pub fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(Style, &mut Window, &mut App) -> LayoutId,
    ) -> LayoutId {
        #[cfg(feature = "diagnostics")]
        let request_style_is_gpux_owned =
            gpux_owns_request_style(std::panic::Location::caller().file());
        #[cfg(not(feature = "diagnostics"))]
        let request_style_is_gpux_owned = false;

        self.request_layout_inner(
            global_id,
            _inspector_id,
            window,
            cx,
            request_style_is_gpux_owned,
            false,
            |style, window, cx| f(mem::take(style), window, cx),
        )
    }

    pub(crate) fn request_layout_with_style_handoff(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&mut Style, &mut Window, &mut App) -> LayoutId,
    ) -> LayoutId {
        self.request_layout_inner(global_id, inspector_id, window, cx, true, true, f)
    }

    fn request_layout_inner(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
        request_style_is_gpux_owned: bool,
        retain_request_style: bool,
        f: impl FnOnce(&mut Style, &mut Window, &mut App) -> LayoutId,
    ) -> LayoutId {
        debug_assert!(!retain_request_style || request_style_is_gpux_owned);
        self.style_handoff_index = None;
        #[cfg(feature = "diagnostics")]
        record_style_phase(0);

        #[cfg(any(feature = "inspector", debug_assertions))]
        window.with_inspector_state(
            _inspector_id,
            cx,
            |inspector_state: &mut Option<DivInspectorState>, _window| {
                if let Some(inspector_state) = inspector_state {
                    self.base_style = inspector_state.base_style.clone();
                } else {
                    *inspector_state = Some(DivInspectorState {
                        base_style: self.base_style.clone(),
                        bounds: Default::default(),
                        content_size: Default::default(),
                    })
                }
            },
        );

        window.with_optional_element_state::<InteractiveElementState, _>(
            global_id,
            |element_state, window| {
                let mut element_state =
                    element_state.map(|element_state| element_state.unwrap_or_default());

                if let Some(element_state) = element_state.as_ref()
                    && cx.has_active_drag()
                {
                    if let Some(pending_mouse_down) = element_state.pending_mouse_down.as_ref() {
                        *pending_mouse_down.borrow_mut() = None;
                    }
                    if let Some(clicked_state) = element_state.clicked_state.as_ref() {
                        *clicked_state.borrow_mut() = ElementClickedState::default();
                    }
                }

                // Ensure we store a focus handle in our element state if we're focusable.
                // If there's an explicit focus handle we're tracking, use that. Otherwise
                // create a new handle and store it in the element state, which lives for as
                // as frames contain an element with this id.
                if self.focusable
                    && self.tracked_focus_handle.is_none()
                    && let Some(element_state) = element_state.as_mut()
                {
                    let mut handle = element_state
                        .focus_handle
                        .get_or_insert_with(|| cx.focus_handle())
                        .clone()
                        .tab_stop(self.tab_stop);

                    if let Some(index) = self.tab_index {
                        handle = handle.tab_index(index);
                    }

                    self.tracked_focus_handle = Some(handle);
                }

                if let Some(scroll_handle) = self.tracked_scroll_handle.as_ref() {
                    if let Ok(state) = scroll_handle.0.try_borrow() {
                        crate::fast::dependencies::note_state_read(cx, &state.fast_version);
                        self.scroll_offset = Some(state.offset.clone());
                        self.ongoing_scroll = Some(state.ongoing_scroll.clone());
                    } else {
                        self.scroll_offset.get_or_insert_with(Rc::default);
                    }
                } else if (self.base_style.overflow.x == Some(Overflow::Scroll)
                    || self.base_style.overflow.y == Some(Overflow::Scroll))
                    && let Some(element_state) = element_state.as_mut()
                {
                    self.scroll_offset = Some(
                        element_state
                            .scroll_offset
                            .get_or_insert_with(Rc::default)
                            .clone(),
                    );
                    self.ongoing_scroll = Some(
                        element_state
                            .ongoing_scroll
                            .get_or_insert_with(|| Rc::new(RefCell::new(OngoingScroll::default())))
                            .clone(),
                    );
                }

                #[cfg(feature = "diagnostics")]
                if self.base_style.as_ref() == &StyleRefinement::default() {
                    record_style_phase(13);
                }

                let mut style =
                    self.compute_style_internal(None, element_state.as_mut(), None, window, cx);
                let request_prepaint_style_candidate = !self.has_dynamic_paint_refinement();
                #[cfg(feature = "diagnostics")]
                {
                    self.request_prepaint_style_candidate = request_prepaint_style_candidate;
                    self.request_style_is_gpux_owned = request_style_is_gpux_owned;
                    if self.request_prepaint_style_candidate {
                        record_style_phase(6);
                        record_style_phase(if request_style_is_gpux_owned { 8 } else { 9 });
                    }
                }
                let layout_id = f(&mut style, window, cx);
                if retain_request_style && request_prepaint_style_candidate {
                    self.style_handoff_index = Some(window.retain_draw_style(style));
                }
                (layout_id, element_state)
            },
        )
    }

    /// Commit the bounds of this element according to this interactivity state's configured styles.
    pub fn prepaint<R>(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        content_size: Size<Pixels>,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(
            &Style,
            Point<Pixels>,
            Option<Hitbox>,
            PositionedBounds,
            Option<StickyState>,
            &mut Window,
            &mut App,
        ) -> R,
    ) -> R {
        #[cfg(feature = "diagnostics")]
        {
            record_style_phase(1);
            let request_style_is_gpux_owned = self.request_style_is_gpux_owned;
            self.request_style_is_gpux_owned = false;
            if std::mem::take(&mut self.request_prepaint_style_candidate)
                && !self.has_dynamic_paint_refinement()
            {
                record_style_phase(7);
                record_style_phase(if request_style_is_gpux_owned { 10 } else { 11 });
            }
        }

        self.content_size = content_size;
        let style_handoff_index = self.style_handoff_index.take();
        let mut request_style = style_handoff_index.and_then(|index| window.take_draw_style(index));

        #[cfg(any(feature = "inspector", debug_assertions))]
        window.with_inspector_state(
            _inspector_id,
            cx,
            |inspector_state: &mut Option<DivInspectorState>, _window| {
                if let Some(inspector_state) = inspector_state {
                    inspector_state.bounds = bounds;
                    inspector_state.content_size = content_size;
                }
            },
        );

        if let Some(focus_handle) = self.tracked_focus_handle.as_ref() {
            window.set_focus_handle(focus_handle, cx);

            if window.a11y.is_active() {
                if let Some(global_id) = global_id {
                    let node_id = global_id.accesskit_node_id();
                    window.a11y.set_focusable(node_id, focus_handle.id);
                    if focus_handle.is_focused(window) {
                        window.a11y.set_focus(node_id);
                    }
                } else if focus_handle.is_focused(window) {
                    window
                        .a11y
                        .note_focus_without_node(focus_handle.id, "it has no element id");
                }
            }
        }
        if self.report_active_descendant_focus && window.a11y.is_active() {
            if let Some(global_id) = global_id {
                window
                    .a11y
                    .set_active_descendant(global_id.accesskit_node_id());
            }
        }
        window.with_optional_element_state::<InteractiveElementState, _>(
            global_id,
            |element_state, window| {
                let mut element_state =
                    element_state.map(|element_state| element_state.unwrap_or_default());
                let can_reuse_request_style =
                    request_style.is_some() && !self.has_dynamic_paint_refinement();
                #[cfg(feature = "diagnostics")]
                if can_reuse_request_style {
                    record_style_phase(12);
                }
                let mut style = if can_reuse_request_style {
                    request_style.take().unwrap()
                } else {
                    self.compute_style_internal(None, element_state.as_mut(), None, window, cx)
                };
                let (positioned_bounds, sticky_state) = window
                    .resolve_positioned_bounds_and_sticky_state(
                        &style,
                        bounds,
                        self.sticky_occurrence,
                    );
                self.current_sticky_state = sticky_state;
                self.update_sticky_state(element_state.as_mut(), sticky_state, window, cx);
                let should_recompute_style = self.sticky_state_changed_listener.is_some()
                    || sticky_state
                        .is_some_and(|state| state.is_stuck() && self.stuck_style.is_some());
                if should_recompute_style {
                    #[cfg(feature = "diagnostics")]
                    record_style_phase(2);
                    style = self.compute_style_internal(
                        None,
                        element_state.as_mut(),
                        sticky_state.as_ref(),
                        window,
                        cx,
                    );
                }
                let bounds = positioned_bounds.visual_bounds;
                let position = style.position;
                if matches!(position, Position::Fixed | Position::Sticky) {
                    window.report_visual_bounds(bounds);
                }
                let retain_style = !self.has_dynamic_paint_refinement();

                if let Some(element_state) = element_state.as_mut() {
                    if let Some(clicked_state) = element_state.clicked_state.as_ref() {
                        let clicked_state = clicked_state.borrow();
                        self.active = Some(clicked_state.element);
                    }
                    if self.hover_style.is_some() || self.group_hover_style.is_some() {
                        element_state
                            .hover_state
                            .get_or_insert_with(Default::default);
                    }
                    if let Some(active_tooltip) = element_state.active_tooltip.as_ref() {
                        if self.tooltip_builder.is_some() {
                            self.tooltip_id = set_tooltip_on_window(active_tooltip, window);
                        } else {
                            // If there is no longer a tooltip builder, remove the active tooltip.
                            if let Some(long_press_tooltip_active) =
                                element_state.long_press_tooltip_active.as_ref()
                            {
                                long_press_tooltip_active.set(false);
                            }
                            element_state.active_tooltip.take();
                        }
                    }
                }

                let opacity = style.opacity;
                let transformation = style.transformation(bounds, window.scale_factor());
                let prepaint = |window: &mut Window| {
                    window.with_text_style(style.text_style().cloned(), |window| {
                        window.with_content_mask(
                            style.overflow_mask(bounds, window.rem_size()),
                            |window| {
                                let hitbox = if self.should_insert_hitbox(&style, window, cx) {
                                    Some(match position {
                                        Position::Fixed | Position::Sticky => window
                                            .insert_positioned_hitbox(bounds, self.hitbox_behavior),
                                        Position::Relative | Position::Absolute => {
                                            window.insert_hitbox(bounds, self.hitbox_behavior)
                                        }
                                    })
                                } else {
                                    None
                                };

                                let scroll_offset =
                                    self.clamp_scroll_position(bounds, &style, window, cx);
                                let result = f(
                                    &style,
                                    scroll_offset,
                                    hitbox,
                                    positioned_bounds,
                                    sticky_state,
                                    window,
                                    cx,
                                );
                                if retain_style {
                                    let index = if let Some(index) = style_handoff_index {
                                        window.replace_draw_style(index, style);
                                        index
                                    } else {
                                        window.retain_draw_style(style)
                                    };
                                    self.style_handoff_index = Some(index);
                                }
                                (result, element_state)
                            },
                        )
                    })
                };

                // Painting applies the style's opacity and transform to the
                // children, so cached views below must see them while prepainting.
                let prepaint = |window: &mut Window| {
                    window.with_element_paint_context(opacity, transformation, prepaint)
                };

                // A fixed or sticky subtree paints in the positioned layer
                // (see `paint`), so its descendants hit-test there too.
                match position {
                    Position::Fixed | Position::Sticky => window.with_positioned_hitboxes(prepaint),
                    Position::Relative | Position::Absolute => prepaint(window),
                }
            },
        )
    }

    fn has_dynamic_paint_refinement(&self) -> bool {
        self.focus_style.is_some()
            || self.in_focus_style.is_some()
            || self.focus_visible_style.is_some()
            || self.hover_style.is_some()
            || self.group_hover_style.is_some()
            || self.active_style.is_some()
            || self.group_active_style.is_some()
    }

    fn should_insert_hitbox(&self, style: &Style, window: &Window, cx: &App) -> bool {
        self.hitbox_behavior != HitboxBehavior::Normal
            || self.window_control.is_some()
            || style.mouse_cursor.is_some()
            || self.group.is_some()
            || self.scroll_offset.is_some()
            || self.tracked_focus_handle.is_some()
            || self.hover_style.is_some()
            || self.group_hover_style.is_some()
            || self.hover_listener.is_some()
            || !self.mouse_up_listeners.is_empty()
            || !self.mouse_pressure_listeners.is_empty()
            || !self.mouse_down_listeners.is_empty()
            || !self.mouse_move_listeners.is_empty()
            || !self.mouse_exit_listeners.is_empty()
            || !self.file_drop_exit_listeners.is_empty()
            || !self.click_listeners.is_empty()
            || !self.aux_click_listeners.is_empty()
            || !self.scroll_wheel_listeners.is_empty()
            || self.has_pinch_listeners()
            || self.drag_listener.is_some()
            || !self.drop_listeners.is_empty()
            || !self.drag_over_styles.is_empty()
            || self.tooltip_builder.is_some()
            || window.is_inspector_picking(cx)
    }

    fn clamp_scroll_position(
        &self,
        bounds: Bounds<Pixels>,
        style: &Style,
        window: &mut Window,
        cx: &mut App,
    ) -> Point<Pixels> {
        if let Some(scroll_offset) = self.scroll_offset.as_ref() {
            let mut scroll_to_bottom = false;
            let mut pending_offset = None;
            let mut pending_programmatic_offset = None;
            if let Some(scroll_handle) = self.tracked_scroll_handle.as_ref()
                && let Ok(mut state) = scroll_handle.0.try_borrow_mut()
            {
                state.overflow = style.overflow;
                scroll_to_bottom = mem::take(&mut state.scroll_to_bottom);
                pending_offset = state.pending_offset.take();
                pending_programmatic_offset = state.pending_programmatic_offset.take();
            }

            let scroll_max = Self::scroll_max_for_bounds(
                self.content_size,
                bounds.size,
                style,
                window.rem_size(),
                window.scale_factor(),
            );
            let mut candidate = pending_offset
                .map(|(offset, _)| offset)
                .unwrap_or_else(|| *scroll_offset.borrow());
            if scroll_to_bottom {
                candidate.y = -scroll_max.y;
            }
            if let Some(scroll_handle) = self.tracked_scroll_handle.as_ref() {
                if let Some((target, behavior)) = pending_programmatic_offset {
                    let current = *scroll_offset.borrow();
                    let logical = |offset: Point<Pixels>| point(-offset.x, -offset.y);
                    let frame = {
                        let state = scroll_handle.0.borrow();
                        state.activity.borrow_mut().start_programmatic_at(
                            logical(current),
                            logical(target),
                            scroll_max,
                            behavior,
                            cx.background_executor().now(),
                        )
                    };
                    if let Some(frame) = frame {
                        scroll_handle.publish_programmatic_offset(
                            point(-frame.offset.x, -frame.offset.y),
                            scroll_max,
                            bounds,
                            window.current_view(),
                            cx,
                        );
                        if frame.request_next_frame {
                            // Publication notified the owner and its dependents; the
                            // scheduled callback keeps frames coming.
                            schedule_programmatic_scroll(
                                scroll_handle.clone(),
                                frame.generation,
                                window.current_view(),
                                window,
                            );
                        }
                    }
                    return scroll_handle.offset();
                }
                let source = pending_offset
                    .map(|(_, source)| source)
                    .unwrap_or(ScrollMutationSource::LayoutClamp);
                scroll_handle.publish_offset(
                    candidate,
                    scroll_max,
                    bounds,
                    source,
                    window.current_view(),
                    cx,
                );
                scroll_handle.offset()
            } else {
                let mut scroll_offset = scroll_offset.borrow_mut();
                scroll_offset.x = candidate.x.clamp(-scroll_max.x, Pixels::ZERO);
                scroll_offset.y = candidate.y.clamp(-scroll_max.y, Pixels::ZERO);
                *scroll_offset
            }
        } else {
            Point::default()
        }
    }

    pub(crate) fn scroll_max_for_bounds(
        content_size: Size<Pixels>,
        bounds_size: Size<Pixels>,
        style: &Style,
        rem_size: Pixels,
        scale_factor: f32,
    ) -> Point<Pixels> {
        fn round_to_two_decimals(pixels: Pixels) -> Pixels {
            const ROUNDING_FACTOR: f32 = 100.0;
            (pixels * ROUNDING_FACTOR).round() / ROUNDING_FACTOR
        }

        // Taffy lays the box out with the padding snapped to the device pixel
        // grid (`to_taffy`); recomputed unsnapped, e.g. py_1 at a fractional
        // rem size, it exceeds `bounds` and leaves the box scrollable by the
        // sub-pixel difference.
        let padding = style
            .padding
            .to_pixels(bounds_size.into(), rem_size)
            .map(
                |edge| px(crate::util::round_to_device_pixel(edge.0, scale_factor) / scale_factor),
            );
        let padding_size = size(padding.left + padding.right, padding.top + padding.bottom);
        // The floating point values produced by Taffy and ours often vary
        // slightly after ~5 decimal places. This can lead to cases where after
        // subtracting these, the container becomes scrollable for less than
        // 0.00000x pixels. As we generally don't benefit from a precision that
        // high for the maximum scroll, we round the scroll max to 2 decimal
        // places here.
        let padded_content_size = content_size + padding_size;
        Point::from(padded_content_size - bounds_size)
            .map(round_to_two_decimals)
            .max(&Default::default())
    }

    /// Paint this element according to this interactivity state's configured styles
    /// and bind the element's mouse and keyboard events.
    ///
    /// content_size is the size of the content of the element, which may be larger than the
    /// element's bounds if the element is scrollable.
    ///
    /// the final computed style will be passed to the provided function, along
    /// with the current scroll offset
    pub fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        hitbox: Option<&Hitbox>,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&Style, &mut Window, &mut App),
    ) {
        let mut prepaint_style = self
            .style_handoff_index
            .take()
            .and_then(|index| window.take_draw_style(index));
        let can_reuse_prepaint_style = prepaint_style.is_some()
            && !self.has_dynamic_paint_refinement()
            && (hitbox.is_none() || !cx.has_active_drag());
        #[cfg(feature = "diagnostics")]
        {
            record_style_phase(3);
            if !self.has_dynamic_paint_refinement() {
                record_style_phase(4);
                if can_reuse_prepaint_style {
                    record_style_phase(5);
                }
            }
        }

        // Published for callers, not painted from: a hover nothing paints by
        // is not a dependency of a cached view (`fast::dependencies`).
        self.hovered = hitbox.map(|hitbox| hitbox.id.hovered_unrecorded(window));
        window.with_optional_element_state::<InteractiveElementState, _>(
            global_id,
            |element_state, window| {
                let mut element_state =
                    element_state.map(|element_state| element_state.unwrap_or_default());

                let style = if can_reuse_prepaint_style {
                    prepaint_style.take().unwrap()
                } else {
                    self.compute_style_internal(
                        hitbox,
                        element_state.as_mut(),
                        self.current_sticky_state.as_ref(),
                        window,
                        cx,
                    )
                };

                #[cfg(any(test, all(debug_assertions, feature = "test-support")))]
                if let Some(debug_selector) = &self.debug_selector {
                    window
                        .next_frame
                        .record_debug_bounds(debug_selector.clone(), bounds);
                }

                self.paint_hover_group_handler(window, cx);

                if style.visibility == Visibility::Hidden {
                    return ((), element_state);
                }

                let mut tab_group = None;
                if self.tab_group {
                    tab_group = self.tab_index;
                }

                let paint = |window: &mut Window| {
                    window.with_element_opacity(style.opacity, |window| {
                        style.paint(bounds, window, cx, |window: &mut Window, cx: &mut App| {
                            window.with_text_style(style.text_style().cloned(), |window| {
                                window.with_content_mask(
                                    style.overflow_mask(bounds, window.rem_size()),
                                    |window| {
                                        window.with_tab_group(tab_group, |window| {
                                            // Register the container's own focus handle *inside*
                                            // its tab group, so that focusing the container and
                                            // then calling `focus_next` descends into this group's
                                            // first item instead of the window's first item.
                                            if let Some(focus_handle) = &self.tracked_focus_handle {
                                                window.next_frame.tab_stops.insert(focus_handle);
                                            }
                                            if let Some(hitbox) = hitbox {
                                                #[cfg(debug_assertions)]
                                                self.paint_debug_info(
                                                    global_id, hitbox, &style, window, cx,
                                                );

                                                if let Some(drag) = cx.active_drag.as_ref() {
                                                    if let Some(mouse_cursor) = drag.cursor_style {
                                                        window
                                                            .set_window_cursor_style(mouse_cursor);
                                                    }
                                                } else {
                                                    if let Some(mouse_cursor) = style.mouse_cursor {
                                                        window
                                                            .set_cursor_style(mouse_cursor, hitbox);
                                                    }
                                                }

                                                if let Some(group) = self.group.clone() {
                                                    GroupHitboxes::push(group, hitbox.id, cx);
                                                }

                                                if let Some(area) = self.window_control {
                                                    window.insert_window_control_hitbox(
                                                        area,
                                                        hitbox.clone(),
                                                    );
                                                }

                                                self.paint_scroll_listener(
                                                    hitbox, &style, window, cx,
                                                );
                                                self.paint_mouse_listeners(
                                                    hitbox,
                                                    element_state.as_mut(),
                                                    window,
                                                    cx,
                                                );
                                            }

                                            self.paint_keyboard_listeners(window, cx);

                                            if window.a11y.is_active()
                                                && let Some(global_id) = global_id
                                            {
                                                let node_id = global_id.accesskit_node_id();
                                                for (action, listener) in
                                                    self.a11y_action_listeners.drain(..)
                                                {
                                                    window
                                                        .on_a11y_action(node_id, action, listener);
                                                }
                                            }
                                            f(&style, window, cx);

                                            if let Some(_hitbox) = hitbox {
                                                #[cfg(any(
                                                    feature = "inspector",
                                                    debug_assertions
                                                ))]
                                                window.insert_inspector_hitbox(
                                                    _hitbox.id,
                                                    _inspector_id,
                                                    cx,
                                                );

                                                if let Some(group) = self.group.as_ref() {
                                                    GroupHitboxes::pop(group, cx);
                                                }
                                            }
                                        })
                                    },
                                );
                            });
                        });
                    });
                };

                match style.position {
                    Position::Fixed => {
                        window.paint_positioned_layer(bounds, paint);
                    }
                    Position::Sticky => {
                        window.paint_positioned_layer(bounds, paint);
                    }
                    Position::Relative | Position::Absolute => {
                        paint(window);
                    }
                }

                ((), element_state)
            },
        );
    }

    #[cfg(debug_assertions)]
    fn paint_debug_info(
        &self,
        global_id: Option<&GlobalElementId>,
        hitbox: &Hitbox,
        style: &Style,
        window: &mut Window,
        cx: &mut App,
    ) {
        use crate::{BorderStyle, TextAlign};

        if let Some(global_id) = global_id
            && (style.debug || style.debug_below || cx.has_global::<crate::DebugBelow>())
            && hitbox.is_hovered(window)
        {
            const FONT_SIZE: crate::Pixels = crate::Pixels(10.);
            let element_id = format!("{global_id:?}");
            let str_len = element_id.len();

            let render_debug_text = |window: &mut Window| {
                if let Some(text) = window
                    .text_system()
                    .shape_text(
                        element_id.into(),
                        FONT_SIZE,
                        crate::Pixels::ZERO,
                        &[window.text_style().to_run(str_len)],
                        None,
                        None,
                    )
                    .ok()
                    .and_then(|mut text| text.pop())
                {
                    text.paint(hitbox.origin, FONT_SIZE, TextAlign::Left, None, window, cx)
                        .ok();

                    let text_bounds = crate::Bounds {
                        origin: hitbox.origin,
                        size: text.size(FONT_SIZE),
                    };
                    if let Some(source_location) = self.source_location
                        && text_bounds.contains(&window.mouse_position())
                        && window.modifiers().secondary()
                    {
                        let secondary_held = window.modifiers().secondary();
                        window.on_key_event({
                            move |e: &crate::ModifiersChangedEvent, _phase, window, _cx| {
                                if e.modifiers.secondary() != secondary_held
                                    && text_bounds.contains(&window.mouse_position())
                                {
                                    // Inspector chrome; inspection draws without caches.
                                    window.refresh();
                                }
                            }
                        });

                        let was_hovered = hitbox.is_hovered(window);
                        let current_view = window.current_view();
                        window.on_mouse_event({
                            let hitbox = hitbox.clone();
                            move |_: &MouseMoveEvent, phase, window, cx| {
                                if phase == DispatchPhase::Capture {
                                    let hovered = hitbox.is_hovered(window);
                                    if hovered != was_hovered {
                                        cx.notify(current_view)
                                    }
                                }
                            }
                        });

                        window.on_mouse_event({
                            let hitbox = hitbox.clone();
                            move |e: &crate::MouseDownEvent, phase, window, cx| {
                                if text_bounds.contains(&e.position)
                                    && phase.capture()
                                    && hitbox.is_hovered(window)
                                {
                                    cx.stop_propagation();
                                    let Ok(dir) = std::env::current_dir() else {
                                        return;
                                    };

                                    eprintln!(
                                        "This element was created at:\n{}:{}:{}",
                                        dir.join(source_location.file()).to_string_lossy(),
                                        source_location.line(),
                                        source_location.column()
                                    );
                                }
                            }
                        });
                        window.paint_quad(crate::outline(
                            crate::Bounds {
                                origin: hitbox.origin
                                    + crate::point(crate::px(0.), FONT_SIZE - px(2.)),
                                size: crate::Size {
                                    width: text_bounds.size.width,
                                    height: crate::px(1.),
                                },
                            },
                            crate::red(),
                            BorderStyle::default(),
                        ))
                    }
                }
            };

            window.with_text_style(
                Some(crate::TextStyleRefinement {
                    color: Some(crate::red().into()),
                    line_height: Some(FONT_SIZE.into()),
                    background_color: Some(crate::white().into()),
                    ..Default::default()
                }),
                render_debug_text,
            )
        }
    }

    fn paint_mouse_listeners(
        &mut self,
        hitbox: &Hitbox,
        element_state: Option<&mut InteractiveElementState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let is_focused = self
            .tracked_focus_handle
            .as_ref()
            .map(|handle| handle.is_focused(window))
            .unwrap_or(false);

        // If this element can be focused, register a mouse down listener
        // that will automatically transfer focus when hitting the element.
        // This behavior can be suppressed by using `cx.prevent_default()`.
        if let Some(focus_handle) = self.tracked_focus_handle.clone() {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && hitbox.is_hovered(window)
                    && !window.default_prevented()
                {
                    window.focus(&focus_handle, cx);
                    // If there is a parent that is also focusable, prevent it
                    // from transferring focus because we already did so.
                    window.prevent_default();
                }
            });
        }

        for listener in self.mouse_down_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.mouse_up_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.mouse_pressure_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MousePressureEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.mouse_move_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.mouse_exit_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseExitEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.file_drop_exit_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &FileDropEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.scroll_wheel_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.pinch_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        if self.hover_style.is_some()
            || self.base_style.mouse_cursor.is_some()
            || cx.active_drag.is_some() && !self.drag_over_styles.is_empty()
        {
            let hitbox = hitbox.clone();
            let hover_state = self.hover_style.as_ref().and_then(|_| {
                element_state
                    .as_ref()
                    .and_then(|state| state.hover_state.as_ref())
                    .cloned()
            });
            let current_view = window.current_view();
            crate::fast::hover::painted(hover_state.as_ref(), &hitbox, window);

            window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                let hovered = hitbox.is_hovered(window);
                let was_hovered = hover_state
                    .as_ref()
                    .is_some_and(|state| state.borrow().element);
                if phase == DispatchPhase::Capture && hovered != was_hovered {
                    if let Some(hover_state) = &hover_state {
                        hover_state.borrow_mut().element = hovered;
                        cx.notify(current_view);
                    }
                }
            });
        }

        if let Some(group_hover) = self.group_hover_style.as_ref() {
            if let Some(group_hitbox_id) = GroupHitboxes::get(&group_hover.group, cx) {
                let hover_state = element_state
                    .as_ref()
                    .and_then(|element| element.hover_state.as_ref())
                    .cloned();
                let current_view = window.current_view();
                crate::fast::hover::painted_group(hover_state.as_ref(), group_hitbox_id, window);

                window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                    let group_hovered = group_hitbox_id.is_hovered(window);
                    let was_group_hovered = hover_state
                        .as_ref()
                        .is_some_and(|state| state.borrow().group);
                    if phase == DispatchPhase::Capture && group_hovered != was_group_hovered {
                        if let Some(hover_state) = &hover_state {
                            hover_state.borrow_mut().group = group_hovered;
                            cx.notify(current_view);
                        }
                    }
                });
            }
        }

        let drag_cursor_style = self.base_style.as_ref().mouse_cursor;

        let mut drag_listener = mem::take(&mut self.drag_listener);
        let drop_listeners = mem::take(&mut self.drop_listeners);
        let click_listeners = mem::take(&mut self.click_listeners);
        let aux_click_listeners = mem::take(&mut self.aux_click_listeners);
        let can_drop_predicate = mem::take(&mut self.can_drop_predicate);

        if !drop_listeners.is_empty() {
            let hitbox = hitbox.clone();
            window.on_mouse_event({
                move |_: &MouseUpEvent, phase, window, cx| {
                    if let Some(drag) = &cx.active_drag
                        && phase == DispatchPhase::Bubble
                        && hitbox.is_hovered(window)
                    {
                        let drag_state_type = drag.value.as_ref().type_id();
                        for (drop_state_type, listener) in &drop_listeners {
                            if *drop_state_type == drag_state_type {
                                let drag = cx
                                    .active_drag
                                    .take()
                                    .expect("checked for type drag state type above");

                                let mut can_drop = true;
                                if let Some(predicate) = &can_drop_predicate {
                                    can_drop = predicate(drag.value.as_ref(), window, cx);
                                }

                                if can_drop {
                                    listener(drag.value.as_ref(), window, cx);
                                    // The drag preview is window-level chrome: the whole
                                    // window redraws without it.
                                    window.refresh();
                                    cx.stop_propagation();
                                }
                            }
                        }
                    }
                }
            });
        }

        if let Some(element_state) = element_state {
            if !click_listeners.is_empty()
                || !aux_click_listeners.is_empty()
                || drag_listener.is_some()
            {
                let pending_mouse_down = element_state
                    .pending_mouse_down
                    .get_or_insert_with(Default::default)
                    .clone();

                let pending_keyboard_down = element_state
                    .pending_keyboard_down
                    .get_or_insert_with(Default::default)
                    .clone();

                let clicked_state = element_state
                    .clicked_state
                    .get_or_insert_with(Default::default)
                    .clone();

                window.on_mouse_event({
                    let pending_mouse_down = pending_mouse_down.clone();
                    let hitbox = hitbox.clone();
                    let has_aux_click_listeners = !aux_click_listeners.is_empty();
                    let current_view = window.current_view();
                    move |event: &MouseDownEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble
                            && (event.button == MouseButton::Left || has_aux_click_listeners)
                            && hitbox.is_hovered(window)
                        {
                            *pending_mouse_down.borrow_mut() = Some(event.clone());
                            cx.notify(current_view);
                        }
                    }
                });

                window.on_mouse_event({
                    let pending_mouse_down = pending_mouse_down.clone();
                    let hitbox = hitbox.clone();
                    move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            return;
                        }

                        let mut pending_mouse_down = pending_mouse_down.borrow_mut();
                        if let Some(mouse_down) = pending_mouse_down.clone()
                            && !cx.has_active_drag()
                            && (event.position - mouse_down.position).magnitude() > DRAG_THRESHOLD
                            && let Some(listener) = drag_listener.take()
                            && mouse_down.button == MouseButton::Left
                        {
                            *clicked_state.borrow_mut() = ElementClickedState::default();
                            let cursor_offset = event.position - hitbox.origin;
                            let drag = (listener.render)(
                                listener.value.as_ref(),
                                cursor_offset,
                                window,
                                cx,
                            );
                            let external_payload_source =
                                listener.external_payload.map(|external_payload| {
                                    let value = listener.value.clone();
                                    Box::new(move |window: &mut Window, cx: &mut App| {
                                        external_payload(value.as_ref(), window, cx)
                                    })
                                        as ExternalDragPayloadSource
                                });
                            cx.active_drag = Some(AnyDrag {
                                view: drag,
                                value: listener.value,
                                cursor_offset,
                                cursor_style: drag_cursor_style,
                                external_payload_source,
                            });
                            pending_mouse_down.take();
                            // The drag preview is window-level chrome drawn over every view.
                            window.refresh();
                            cx.stop_propagation();
                        }
                    }
                });

                if is_focused {
                    // Record the focus generation at which an enter/space key
                    // down event happened on this element. The next key up
                    // event will be mapped to a click event if both of the
                    // following are true:
                    // - no other key events happen in between
                    // - the focus generation is the same (implying focus did not move)
                    //
                    // This design avoids an ABA problem that happens if you
                    // store the focus handle that registered the keypress.
                    window.on_key_event({
                        let pending_keyboard_down = pending_keyboard_down.clone();
                        move |event: &KeyDownEvent, phase, window, _cx| {
                            if phase.bubble() && !window.default_prevented() {
                                let stroke = &event.keystroke;
                                let is_activation_key = (stroke.key.eq("enter")
                                    || stroke.key.eq("space"))
                                    && !stroke.modifiers.modified();
                                *pending_keyboard_down.borrow_mut() =
                                    is_activation_key.then_some(window.focus_generation);
                            }
                        }
                    });

                    // Press enter, space to trigger click, when the element is focused.
                    window.on_key_event({
                        let click_listeners = click_listeners.clone();
                        let hitbox = hitbox.clone();
                        move |event: &KeyUpEvent, phase, window, cx| {
                            if phase.bubble() && !window.default_prevented() {
                                let stroke = &event.keystroke;
                                let keyboard_button = if stroke.key.eq("enter") {
                                    Some(KeyboardButton::Enter)
                                } else if stroke.key.eq("space") {
                                    Some(KeyboardButton::Space)
                                } else {
                                    None
                                };

                                if let Some(button) = keyboard_button
                                    && !stroke.modifiers.modified()
                                {
                                    let pending =
                                        std::mem::take(&mut *pending_keyboard_down.borrow_mut());
                                    if pending != Some(window.focus_generation) {
                                        return;
                                    }

                                    let click_event = ClickEvent::Keyboard(KeyboardClickEvent {
                                        button,
                                        bounds: hitbox.bounds,
                                    });

                                    for listener in &click_listeners {
                                        listener(&click_event, window, cx);
                                    }
                                } else {
                                    // Releasing any other key mid-press means
                                    // this isn't a clean activation, so cancel
                                    // the pending keydown.
                                    *pending_keyboard_down.borrow_mut() = None;
                                }
                            }
                        }
                    });
                }

                window.on_mouse_event({
                    let mut captured_mouse_down = None;
                    let hitbox = hitbox.clone();
                    let current_view = window.current_view();
                    move |event: &MouseUpEvent, phase, window, cx| match phase {
                        // Clear the pending mouse down during the capture phase,
                        // so that it happens even if another event handler stops
                        // propagation.
                        DispatchPhase::Capture => {
                            let mut pending_mouse_down = pending_mouse_down.borrow_mut();
                            if pending_mouse_down.is_some() && hitbox.is_hovered(window) {
                                captured_mouse_down = pending_mouse_down.take();
                                cx.notify(current_view);
                            } else if pending_mouse_down.is_some() {
                                // Clear the pending mouse down event (without firing click handlers)
                                // if the hitbox is not being hovered.
                                // This avoids dragging elements that changed their position
                                // immediately after being clicked.
                                // See https://github.com/zed-industries/zed/issues/24600 for more details
                                pending_mouse_down.take();
                                cx.notify(current_view);
                            }
                        }
                        // Fire click handlers during the bubble phase.
                        DispatchPhase::Bubble => {
                            if let Some(mouse_down) = captured_mouse_down.take() {
                                let btn = mouse_down.button;

                                let mouse_click = ClickEvent::Mouse(MouseClickEvent {
                                    down: mouse_down,
                                    up: event.clone(),
                                });

                                match btn {
                                    MouseButton::Left => {
                                        for listener in &click_listeners {
                                            listener(&mouse_click, window, cx);
                                        }
                                    }
                                    _ => {
                                        for listener in &aux_click_listeners {
                                            listener(&mouse_click, window, cx);
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
            }

            if let Some(hover_listener) = self.hover_listener.take() {
                let was_hovered = element_state
                    .hover_listener_state
                    .get_or_insert_with(Default::default)
                    .clone();
                let has_mouse_down = element_state
                    .pending_mouse_down
                    .get_or_insert_with(Default::default)
                    .clone();
                let hover_listener = Rc::new(hover_listener);
                let hover_listener_state = was_hovered.clone();
                let update_hover = move |is_hovered: bool, window: &mut Window, cx: &mut App| {
                    let mut was_hovered = hover_listener_state.borrow_mut();
                    if is_hovered != *was_hovered {
                        *was_hovered = is_hovered;
                        drop(was_hovered);
                        hover_listener(&is_hovered, window, cx);
                    }
                };
                let hover_listener_mode = self.hover_listener_mode;

                if has_mouse_down.borrow().is_none() {
                    let is_hovered =
                        !cx.has_active_drag() && hover_listener_mode.is_hovered(hitbox, window);
                    if is_hovered != *was_hovered.borrow() {
                        let update_hover = update_hover.clone();
                        window.defer(cx, move |window, cx| {
                            update_hover(is_hovered, window, cx);
                        });
                    }
                }

                window.on_mouse_event({
                    let update_hover = update_hover.clone();
                    let hitbox = hitbox.clone();
                    move |_: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble {
                            let is_hovered = has_mouse_down.borrow().is_none()
                                && !cx.has_active_drag()
                                && hover_listener_mode.is_hovered(&hitbox, window);
                            update_hover(is_hovered, window, cx);
                        }
                    }
                });

                // The pointer can leave the window without a final MouseMove, so also
                // clear hover on MouseExited.
                window.on_mouse_event(move |_: &MouseExitEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        update_hover(false, window, cx);
                    }
                });
            }

            if let Some(tooltip_builder) = self.tooltip_builder.take() {
                let active_tooltip = element_state
                    .active_tooltip
                    .get_or_insert_with(Default::default)
                    .clone();
                let pending_mouse_down = element_state
                    .pending_mouse_down
                    .get_or_insert_with(Default::default)
                    .clone();
                let long_press_tooltip_active = element_state
                    .long_press_tooltip_active
                    .get_or_insert_with(Default::default)
                    .clone();

                let tooltip_is_hoverable = tooltip_builder.hoverable;
                let build_tooltip = Rc::new(move |window: &mut Window, cx: &mut App| {
                    Some(((tooltip_builder.build)(window, cx), tooltip_is_hoverable))
                });
                // Use bounds instead of testing hitbox since this is called during prepaint.
                let check_is_hovered_during_prepaint = Rc::new({
                    let pending_mouse_down = pending_mouse_down.clone();
                    let source_bounds = hitbox.bounds;
                    move |window: &Window| {
                        !window.last_input_was_keyboard()
                            && pending_mouse_down.borrow().is_none()
                            && source_bounds.contains(&window.mouse_position())
                    }
                });
                let check_is_hovered = Rc::new({
                    let hitbox = hitbox.clone();
                    move |window: &Window| {
                        pending_mouse_down.borrow().is_none() && hitbox.is_hovered(window)
                    }
                });
                register_tooltip_mouse_handlers(
                    &active_tooltip,
                    self.tooltip_id,
                    build_tooltip,
                    check_is_hovered,
                    check_is_hovered_during_prepaint,
                    long_press_tooltip_active,
                    self.tooltip_show_delay,
                    window,
                );
            }

            // We unconditionally bind both the mouse up and mouse down active state handlers
            // Because we might not get a chance to render a frame before the mouse up event arrives.
            let active_state = element_state
                .clicked_state
                .get_or_insert_with(Default::default)
                .clone();

            let current_view = window.current_view();
            {
                let active_state = active_state.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, cx| {
                    if phase == DispatchPhase::Capture && active_state.borrow().is_clicked() {
                        *active_state.borrow_mut() = ElementClickedState::default();
                        cx.notify(current_view);
                    }
                });
            }

            {
                let active_group_hitbox = self
                    .group_active_style
                    .as_ref()
                    .and_then(|group_active| GroupHitboxes::get(&group_active.group, cx));
                let hitbox = hitbox.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && !window.default_prevented() {
                        let group_hovered = active_group_hitbox
                            .is_some_and(|group_hitbox_id| group_hitbox_id.is_hovered(window));
                        let element_hovered = hitbox.is_hovered(window);
                        if group_hovered || element_hovered {
                            *active_state.borrow_mut() = ElementClickedState {
                                group: group_hovered,
                                element: element_hovered,
                            };
                            cx.notify(current_view);
                        }
                    }
                });
            }
        }
    }

    fn paint_keyboard_listeners(&mut self, window: &mut Window, _cx: &mut App) {
        let key_down_listeners = mem::take(&mut self.key_down_listeners);
        let key_up_listeners = mem::take(&mut self.key_up_listeners);
        let modifiers_changed_listeners = mem::take(&mut self.modifiers_changed_listeners);
        let action_listeners = mem::take(&mut self.action_listeners);
        let action_outcome_listeners = mem::take(&mut self.action_outcome_listeners);
        if let Some(context) = self.key_context.clone() {
            window.set_key_context(context);
        }

        for listener in key_down_listeners {
            window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
                listener(event, phase, window, cx);
            })
        }

        for listener in key_up_listeners {
            window.on_key_event(move |event: &KeyUpEvent, phase, window, cx| {
                listener(event, phase, window, cx);
            })
        }

        for listener in modifiers_changed_listeners {
            window.on_modifiers_changed(move |event: &ModifiersChangedEvent, window, cx| {
                listener(event, window, cx);
            })
        }

        for (action_type, listener) in action_listeners {
            window.on_action(action_type, listener)
        }

        for (action_type, listener) in action_outcome_listeners {
            window.on_action_with_outcome(action_type, listener)
        }
    }

    fn paint_hover_group_handler(&self, window: &mut Window, cx: &mut App) {
        let group_hitbox = self
            .group_hover_style
            .as_ref()
            .and_then(|group_hover| GroupHitboxes::get(&group_hover.group, cx));

        if let Some(group_hitbox) = group_hitbox {
            let was_hovered = group_hitbox.is_hovered(window);
            let current_view = window.current_view();
            window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                let hovered = group_hitbox.is_hovered(window);
                if phase == DispatchPhase::Capture && hovered != was_hovered {
                    cx.notify(current_view);
                }
            });
        }
    }

    fn paint_scroll_listener(
        &self,
        hitbox: &Hitbox,
        style: &Style,
        window: &mut Window,
        _cx: &mut App,
    ) {
        if let Some(scroll_offset_state) = self.scroll_offset.clone() {
            let tracked_scroll_handle = self.tracked_scroll_handle.clone();
            let activity = tracked_scroll_handle
                .as_ref()
                .map(|handle| handle.0.borrow().activity.clone())
                .unwrap_or_default();
            let ongoing_scroll = self.ongoing_scroll.clone();
            let overflow = style.overflow;
            let allow_concurrent_scroll = style.allow_concurrent_scroll;
            let restrict_scroll_to_axis = style.restrict_scroll_to_axis;
            let scroll_max = Self::scroll_max_for_bounds(
                self.content_size,
                hitbox.bounds.size,
                style,
                window.rem_size(),
                window.scale_factor(),
            );
            let line_height = window.line_height();
            let hitbox = hitbox.clone();
            let current_view = window.current_view();
            let fast_container =
                crate::fast::layers::invalidate::painted_container(window, self, &hitbox);
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                    // Wheel, trackpad, and recognized touch scrolling replace any
                    // programmatic driver owned by this scroll handle.
                    activity.borrow_mut().stop();
                    let old_scroll_offset = tracked_scroll_handle
                        .as_ref()
                        .map(ScrollHandle::offset)
                        .unwrap_or_else(|| *scroll_offset_state.borrow());
                    let mut delta = event.delta.pixel_delta(line_height);

                    if restrict_scroll_to_axis
                        && event.delta.precise()
                        && let Some(ongoing_scroll) = &ongoing_scroll
                    {
                        ongoing_scroll.borrow_mut().filter_at(
                            &mut delta,
                            event.touch_phase,
                            cx.background_executor().now(),
                        );
                    }

                    let mut delta_x = Pixels::ZERO;
                    if overflow.x == Overflow::Scroll {
                        if !delta.x.is_zero() {
                            delta_x = delta.x;
                        } else if !restrict_scroll_to_axis && overflow.y != Overflow::Scroll {
                            delta_x = delta.y;
                        }
                    }
                    let mut delta_y = Pixels::ZERO;
                    if overflow.y == Overflow::Scroll {
                        if !delta.y.is_zero() {
                            delta_y = delta.y;
                        } else if !restrict_scroll_to_axis && overflow.x != Overflow::Scroll {
                            delta_y = delta.x;
                        }
                    }
                    if !allow_concurrent_scroll && !delta_x.is_zero() && !delta_y.is_zero() {
                        if delta_x.abs() > delta_y.abs() {
                            delta_y = Pixels::ZERO;
                        } else {
                            delta_x = Pixels::ZERO;
                        }
                    }
                    let mut new_scroll_offset = old_scroll_offset;
                    if overflow.x == Overflow::Scroll {
                        new_scroll_offset.x =
                            (new_scroll_offset.x + delta_x).clamp(-scroll_max.x, px(0.));
                    }
                    if overflow.y == Overflow::Scroll {
                        new_scroll_offset.y =
                            (new_scroll_offset.y + delta_y).clamp(-scroll_max.y, px(0.));
                    }

                    // At the scroll edge the unconsumed delta keeps bubbling, so an
                    // enclosing scroll owner continues the gesture.
                    if new_scroll_offset == old_scroll_offset {
                        return;
                    }
                    if let Some(scroll_handle) = tracked_scroll_handle.as_ref() {
                        scroll_handle.publish_offset(
                            new_scroll_offset,
                            scroll_max,
                            hitbox.bounds,
                            ScrollMutationSource::Wheel,
                            current_view,
                            cx,
                        );
                    } else {
                        *scroll_offset_state.borrow_mut() = new_scroll_offset;
                        // A handle's publish notes its own scroll.
                        crate::fast::layers::invalidate::note_scrolled(window, &fast_container);
                        cx.notify(current_view);
                    }
                    cx.stop_propagation();
                }
            });
        }
    }

    /// Compute the visual style for this element, based on the current bounds and the element's state.
    pub fn compute_style(
        &self,
        global_id: Option<&GlobalElementId>,
        hitbox: Option<&Hitbox>,
        window: &mut Window,
        cx: &mut App,
    ) -> Style {
        window.with_optional_element_state(global_id, |element_state, window| {
            let mut element_state =
                element_state.map(|element_state| element_state.unwrap_or_default());
            let style = self.compute_style_internal(
                hitbox,
                element_state.as_mut(),
                self.current_sticky_state.as_ref(),
                window,
                cx,
            );
            (style, element_state)
        })
    }

    /// Called from internal methods that have already called with_element_state.
    fn compute_style_internal(
        &self,
        hitbox: Option<&Hitbox>,
        element_state: Option<&mut InteractiveElementState>,
        sticky_state: Option<&StickyState>,
        window: &mut Window,
        cx: &mut App,
    ) -> Style {
        let mut style = Style::default();
        style.refine(&self.base_style);

        if let Some(sticky_state) = sticky_state
            && sticky_state.is_stuck()
            && let Some(stuck_style) = self.stuck_style.as_ref()
        {
            style.refine(stuck_style);
        }

        if let Some(focus_handle) = self.tracked_focus_handle.as_ref() {
            if let Some(in_focus_style) = self.in_focus_style.as_ref()
                && focus_handle.within_focused(window, cx)
            {
                style.refine(in_focus_style);
            }

            if let Some(focus_style) = self.focus_style.as_ref()
                && focus_handle.is_focused(window)
            {
                style.refine(focus_style);
            }

            if let Some(focus_visible_style) = self.focus_visible_style.as_ref()
                && focus_handle.is_focused(window)
                && window.last_input_was_keyboard()
            {
                style.refine(focus_visible_style);
            }
        }

        if !cx.has_active_drag() {
            if let Some(group_hover) = self.group_hover_style.as_ref() {
                let is_group_hovered =
                    if let Some(group_hitbox_id) = GroupHitboxes::get(&group_hover.group, cx) {
                        !window.last_input_was_touch() && group_hitbox_id.is_hovered(window)
                    } else if let Some(element_state) = element_state.as_ref() {
                        !window.last_input_was_touch()
                            && element_state
                                .hover_state
                                .as_ref()
                                .map(|state| state.borrow().group)
                                .unwrap_or(false)
                    } else {
                        false
                    };

                if is_group_hovered {
                    style.refine(&group_hover.style);
                }
            }

            if let Some(hover_style) = self.hover_style.as_ref() {
                let is_hovered = if let Some(hitbox) = hitbox {
                    !window.last_input_was_touch() && hitbox.is_hovered(window)
                } else if let Some(element_state) = element_state.as_ref() {
                    !window.last_input_was_touch()
                        && element_state
                            .hover_state
                            .as_ref()
                            .map(|state| state.borrow().element)
                            .unwrap_or(false)
                } else {
                    false
                };

                if is_hovered {
                    style.refine(hover_style);
                }
            }
        }

        if let Some(hitbox) = hitbox {
            if let Some(drag) = cx.active_drag.take() {
                let mut can_drop = true;
                if let Some(can_drop_predicate) = &self.can_drop_predicate {
                    can_drop = can_drop_predicate(drag.value.as_ref(), window, cx);
                }

                if can_drop {
                    for (state_type, group_drag_style) in &self.group_drag_over_styles {
                        if let Some(group_hitbox_id) =
                            GroupHitboxes::get(&group_drag_style.group, cx)
                            && *state_type == drag.value.as_ref().type_id()
                            && group_hitbox_id.is_hovered(window)
                        {
                            style.refine(&group_drag_style.style);
                        }
                    }

                    for (state_type, build_drag_over_style) in &self.drag_over_styles {
                        if *state_type == drag.value.as_ref().type_id() && hitbox.is_hovered(window)
                        {
                            style.refine(&build_drag_over_style(drag.value.as_ref(), window, cx));
                        }
                    }
                }

                style.mouse_cursor = drag.cursor_style;
                cx.active_drag = Some(drag);
            }
        }

        if let Some(element_state) = element_state {
            let clicked_state = element_state
                .clicked_state
                .get_or_insert_with(Default::default)
                .borrow();
            if clicked_state.group
                && let Some(group) = self.group_active_style.as_ref()
            {
                style.refine(&group.style)
            }

            if let Some(active_style) = self.active_style.as_ref()
                && clicked_state.element
            {
                style.refine(active_style)
            }
        }

        style
    }

    fn update_sticky_state(
        &self,
        element_state: Option<&mut InteractiveElementState>,
        sticky_state: Option<StickyState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(element_state) = element_state else {
            debug_assert!(
                self.sticky_state_changed_listener.is_none(),
                "on_sticky_state_changed requires an element id"
            );
            return;
        };

        let Some(sticky_state) = sticky_state else {
            element_state.sticky_state = None;
            return;
        };

        let previous = element_state.sticky_state;
        element_state.sticky_state = Some(sticky_state);

        if previous.is_none_or(|previous| !previous.same_phase_as(sticky_state))
            && let Some(listener) = self.sticky_state_changed_listener.as_ref()
        {
            listener(
                &StickyStateChangedEvent {
                    previous,
                    current: sticky_state,
                },
                window,
                cx,
            );
        }
    }

    pub(crate) fn write_a11y_info(&self, node: &mut accesskit::Node) {
        if let Some(id) = &self.aria.author_id {
            node.set_author_id(id.to_string());
        }
        if let Some(label) = &self.aria.label {
            node.set_label(label.to_string());
        }
        if let Some(description) = &self.aria.description {
            node.set_description(description.to_string());
        }
        if let Some(keyshortcuts) = &self.aria.keyshortcuts {
            node.set_keyboard_shortcut(keyshortcuts.to_string());
        }
        if let Some(selected) = self.aria.selected {
            node.set_selected(selected);
        }
        if let Some(invalid) = self.aria.invalid {
            if invalid {
                node.set_invalid(accesskit::Invalid::True);
            } else {
                node.clear_invalid();
            }
        }
        if let Some(disabled) = self.aria.disabled {
            if disabled {
                node.set_disabled();
            } else {
                node.clear_disabled();
            }
        }
        if let Some(expanded) = self.aria.expanded {
            node.set_expanded(expanded);
        }
        if let Some(toggled) = self.aria.toggled {
            node.set_toggled(toggled);
        }
        if let Some(value) = self.aria.numeric_value {
            node.set_numeric_value(value);
        }
        if let Some(value) = self.aria.min_numeric_value {
            node.set_min_numeric_value(value);
        }
        if let Some(value) = self.aria.max_numeric_value {
            node.set_max_numeric_value(value);
        }
        if let Some(step) = self.aria.numeric_value_step {
            node.set_numeric_value_step(step);
        }
        if let Some(value) = &self.aria.value {
            node.set_value(value.to_string());
        }
        if let Some(placeholder) = &self.aria.placeholder {
            node.set_placeholder(placeholder.to_string());
        }
        if let Some(orientation) = self.aria.orientation {
            node.set_orientation(orientation);
        }
        if let Some(level) = self.aria.level {
            node.set_level(level);
        }
        if let Some(position) = self.aria.position_in_set {
            node.set_position_in_set(position);
        }
        if let Some(size) = self.aria.size_of_set {
            node.set_size_of_set(size);
        }
        if let Some(index) = self.aria.row_index {
            node.set_row_index(index);
        }
        if let Some(index) = self.aria.column_index {
            node.set_column_index(index);
        }
        if let Some(count) = self.aria.row_count {
            node.set_row_count(count);
        }
        if let Some(count) = self.aria.column_count {
            node.set_column_count(count);
        }
        if !self.click_listeners.is_empty() {
            node.add_action(accesskit::Action::Click);
        }
        if self.tracked_focus_handle.is_some() || self.focusable {
            node.add_action(accesskit::Action::Focus);
        }
        for (action, _) in &self.a11y_action_listeners {
            node.add_action(*action);
        }
    }
}

/// The per-frame state of an interactive element. Used for tracking stateful interactions like clicks
/// and scroll offsets.
#[derive(Default)]
pub struct InteractiveElementState {
    pub(crate) focus_handle: Option<FocusHandle>,
    pub(crate) clicked_state: Option<Rc<RefCell<ElementClickedState>>>,
    pub(crate) hover_state: Option<Rc<RefCell<ElementHoverState>>>,
    pub(crate) hover_listener_state: Option<Rc<RefCell<bool>>>,
    pub(crate) pending_mouse_down: Option<Rc<RefCell<Option<MouseDownEvent>>>>,
    /// Set to the window's [`focus_generation`](crate::Window::focus_generation)
    /// when an Enter/Space keydown is received while this element is focused,
    /// recording that we are waiting for the matching keyup to fire a keyboard
    /// click. On keyup the click only fires if the stored generation still
    /// matches the window's current one, i.e. focus never moved during the
    /// press (mirroring the browser clearing a control's pressed state on
    /// blur). `None` means no activation key is pending.
    pub(crate) pending_keyboard_down: Option<Rc<RefCell<Option<u64>>>>,
    pub(crate) scroll_offset: Option<Rc<RefCell<Point<Pixels>>>>,
    ongoing_scroll: Option<Rc<RefCell<OngoingScroll>>>,
    pub(crate) active_tooltip: Option<Rc<RefCell<Option<ActiveTooltip>>>>,
    long_press_tooltip_active: Option<Rc<Cell<bool>>>,
    pub(crate) sticky_state: Option<StickyState>,
}

/// Whether or not the element or a group that contains it is clicked by the mouse.
#[derive(Copy, Clone, Default, Eq, PartialEq)]
pub struct ElementClickedState {
    /// True if this element's group has been clicked, false otherwise
    pub group: bool,

    /// True if this element has been clicked, false otherwise
    pub element: bool,
}

impl ElementClickedState {
    fn is_clicked(&self) -> bool {
        self.group || self.element
    }
}

/// Whether or not the element or a group that contains it is hovered.
#[derive(Copy, Clone, Default, Eq, PartialEq)]
pub struct ElementHoverState {
    /// True if this element's group is hovered, false otherwise
    pub group: bool,

    /// True if this element is hovered, false otherwise
    pub element: bool,
}

pub(crate) enum ActiveTooltip {
    /// Currently delaying before showing the tooltip.
    WaitingForShow { _task: Task<()> },
    /// Tooltip is visible, element was hovered or for hoverable tooltips, the tooltip was hovered.
    Visible {
        tooltip: AnyTooltip,
        is_hoverable: bool,
    },
    /// Tooltip is visible and hoverable, but the mouse is no longer hovering. Currently delaying
    /// before hiding it.
    WaitingForHide {
        tooltip: AnyTooltip,
        _task: Task<()>,
    },
}

pub(crate) fn clear_active_tooltip(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    window: &mut Window,
) {
    // Tooltips are window-level overlays: showing or hiding one redraws the window.
    match active_tooltip.borrow_mut().take() {
        None => {}
        Some(ActiveTooltip::WaitingForShow { .. }) => {}
        Some(ActiveTooltip::Visible { .. }) => window.refresh(),
        Some(ActiveTooltip::WaitingForHide { .. }) => window.refresh(),
    }
}

pub(crate) fn clear_active_tooltip_if_not_hoverable(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    window: &mut Window,
) {
    let should_clear = match active_tooltip.borrow().as_ref() {
        None => false,
        Some(ActiveTooltip::WaitingForShow { .. }) => false,
        Some(ActiveTooltip::Visible { is_hoverable, .. }) => !is_hoverable,
        Some(ActiveTooltip::WaitingForHide { .. }) => false,
    };
    if should_clear {
        active_tooltip.borrow_mut().take();
        window.refresh();
    }
}

pub(crate) fn set_tooltip_on_window(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    window: &mut Window,
) -> Option<TooltipId> {
    let tooltip = match active_tooltip.borrow().as_ref() {
        None => return None,
        Some(ActiveTooltip::WaitingForShow { .. }) => return None,
        Some(ActiveTooltip::Visible { tooltip, .. }) => tooltip.clone(),
        Some(ActiveTooltip::WaitingForHide { tooltip, .. }) => tooltip.clone(),
    };
    Some(window.set_tooltip(tooltip))
}

pub(crate) fn register_tooltip_mouse_handlers(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    tooltip_id: Option<TooltipId>,
    build_tooltip: Rc<dyn Fn(&mut Window, &mut App) -> Option<(AnyView, bool)>>,
    check_is_hovered: Rc<dyn Fn(&Window) -> bool>,
    check_is_hovered_during_prepaint: Rc<dyn Fn(&Window) -> bool>,
    long_press_tooltip_active: Rc<Cell<bool>>,
    show_delay: Option<Duration>,
    window: &mut Window,
) {
    let current_view = window.current_view();
    let show_delay = show_delay.unwrap_or(DEFAULT_TOOLTIP_SHOW_DELAY);

    window.on_mouse_event({
        let active_tooltip = active_tooltip.clone();
        let build_tooltip = build_tooltip.clone();
        let check_is_hovered = check_is_hovered.clone();
        let check_is_hovered_during_prepaint = check_is_hovered_during_prepaint.clone();
        move |_: &MouseMoveEvent, phase, window, cx| {
            handle_tooltip_mouse_move(
                &active_tooltip,
                &build_tooltip,
                &check_is_hovered,
                &check_is_hovered_during_prepaint,
                tooltip_id,
                current_view,
                phase,
                show_delay,
                window,
                cx,
            )
        }
    });

    window.on_mouse_event({
        let active_tooltip = active_tooltip.clone();
        move |_: &MouseDownEvent, _phase, window: &mut Window, _cx| {
            if !tooltip_id.is_some_and(|tooltip_id| tooltip_id.is_hovered(window)) {
                clear_active_tooltip_if_not_hoverable(&active_tooltip, window);
            }
        }
    });

    window.on_mouse_event({
        let active_tooltip = active_tooltip.clone();
        let build_tooltip = build_tooltip.clone();
        let check_is_hovered_during_prepaint = check_is_hovered_during_prepaint.clone();
        move |event: &LongPressEvent, phase, window, cx| {
            if !phase.bubble() {
                return;
            }

            match event.phase {
                TouchPhase::Started if !window.default_prevented() && check_is_hovered(window) => {
                    if show_tooltip(
                        &active_tooltip,
                        &build_tooltip,
                        &check_is_hovered_during_prepaint,
                        Some(long_press_tooltip_active.clone()),
                        window,
                        cx,
                    ) {
                        long_press_tooltip_active.set(true);
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }
                TouchPhase::Ended | TouchPhase::Cancelled
                    if long_press_tooltip_active.replace(false) =>
                {
                    clear_active_tooltip(&active_tooltip, window);
                    cx.stop_propagation();
                }
                TouchPhase::Started
                | TouchPhase::Moved
                | TouchPhase::Ended
                | TouchPhase::Cancelled => {}
            }
        }
    });

    window.on_mouse_event({
        let active_tooltip = active_tooltip.clone();
        move |_: &ScrollWheelEvent, _phase, window: &mut Window, _cx| {
            if !tooltip_id.is_some_and(|tooltip_id| tooltip_id.is_hovered(window)) {
                clear_active_tooltip_if_not_hoverable(&active_tooltip, window);
            }
        }
    });
}

/// Handles displaying tooltips when an element is hovered.
///
/// The mouse hovering logic also relies on being called from window prepaint in order to handle the
/// case where the element the tooltip is on is not rendered - in that case its mouse listeners are
/// also not registered. During window prepaint, the hitbox information is not available, so
/// `check_is_hovered_during_prepaint` is used which bases the check off of the absolute bounds of
/// the element.
///
/// TODO: There's a minor bug due to the use of absolute bounds while checking during prepaint - it
/// does not know if the hitbox is occluded. In the case where a tooltip gets displayed and then
/// gets occluded after display, it will stick around until the mouse exits the hover bounds.
fn handle_tooltip_mouse_move(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    build_tooltip: &Rc<dyn Fn(&mut Window, &mut App) -> Option<(AnyView, bool)>>,
    check_is_hovered: &Rc<dyn Fn(&Window) -> bool>,
    check_is_hovered_during_prepaint: &Rc<dyn Fn(&Window) -> bool>,
    tooltip_id: Option<TooltipId>,
    current_view: EntityId,
    phase: DispatchPhase,
    show_delay: Duration,
    window: &mut Window,
    cx: &mut App,
) {
    // Separates logic for what mutation should occur from applying it, to avoid overlapping
    // RefCell borrows.
    enum Action {
        None,
        CancelShow,
        ScheduleShow,
        CheckVisible,
    }

    let action = match active_tooltip.borrow().as_ref() {
        None => {
            let is_hovered = check_is_hovered(window);
            if is_hovered && phase.bubble() {
                Action::ScheduleShow
            } else {
                Action::None
            }
        }
        Some(ActiveTooltip::WaitingForShow { .. }) => {
            let is_hovered = check_is_hovered(window);
            if is_hovered {
                Action::None
            } else {
                Action::CancelShow
            }
        }
        Some(ActiveTooltip::Visible { is_hoverable, .. }) => {
            if phase.capture()
                && !check_is_hovered(window)
                && (!*is_hoverable
                    || !tooltip_id.is_some_and(|tooltip_id| tooltip_id.is_hovered(window)))
            {
                Action::CheckVisible
            } else {
                Action::None
            }
        }
        Some(ActiveTooltip::WaitingForHide { .. }) => {
            if phase.capture()
                && (check_is_hovered(window)
                    || tooltip_id.is_some_and(|tooltip_id| tooltip_id.is_hovered(window)))
            {
                Action::CheckVisible
            } else {
                Action::None
            }
        }
    };

    match action {
        Action::None => {}
        Action::CancelShow => {
            // Cancel waiting to show tooltip when it is no longer hovered.
            active_tooltip.borrow_mut().take();
        }
        Action::ScheduleShow => {
            let delayed_show_task = window.spawn(cx, {
                let weak_active_tooltip = Rc::downgrade(active_tooltip);
                let build_tooltip = build_tooltip.clone();
                let check_is_hovered_during_prepaint = check_is_hovered_during_prepaint.clone();
                async move |cx| {
                    cx.background_executor().timer(show_delay).await;
                    let Some(active_tooltip) = weak_active_tooltip.upgrade() else {
                        return;
                    };
                    cx.update(|window, cx| {
                        show_tooltip(
                            &active_tooltip,
                            &build_tooltip,
                            &check_is_hovered_during_prepaint,
                            None,
                            window,
                            cx,
                        );
                    })
                    .ok();
                }
            });
            active_tooltip
                .borrow_mut()
                .replace(ActiveTooltip::WaitingForShow {
                    _task: delayed_show_task,
                });
        }
        Action::CheckVisible => cx.notify(current_view),
    }
}

fn show_tooltip(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    build_tooltip: &Rc<dyn Fn(&mut Window, &mut App) -> Option<(AnyView, bool)>>,
    check_is_hovered_during_prepaint: &Rc<dyn Fn(&Window) -> bool>,
    long_press_tooltip_active: Option<Rc<Cell<bool>>>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let new_tooltip = build_tooltip(window, cx).map(|(view, tooltip_is_hoverable)| {
        let weak_active_tooltip = Rc::downgrade(active_tooltip);
        let check_is_hovered_during_prepaint = check_is_hovered_during_prepaint.clone();
        ActiveTooltip::Visible {
            tooltip: AnyTooltip {
                view,
                mouse_position: window.mouse_position(),
                check_visible_and_update: Rc::new(move |tooltip_bounds, window, cx| {
                    if long_press_tooltip_active
                        .as_ref()
                        .is_some_and(|active| active.get())
                    {
                        return true;
                    }
                    let Some(active_tooltip) = weak_active_tooltip.upgrade() else {
                        return false;
                    };
                    handle_tooltip_check_visible_and_update(
                        &active_tooltip,
                        tooltip_is_hoverable,
                        &check_is_hovered_during_prepaint,
                        tooltip_bounds,
                        window,
                        cx,
                    )
                }),
            },
            is_hoverable: tooltip_is_hoverable,
        }
    });
    let did_show = new_tooltip.is_some();
    *active_tooltip.borrow_mut() = new_tooltip;
    window.refresh();
    did_show
}

/// Returns a callback which will be called by window prepaint to update tooltip visibility. The
/// purpose of doing this logic here instead of the mouse move handler is that the mouse move
/// handler won't get called when the element is not painted (e.g. via use of `visible_on_hover`).
fn handle_tooltip_check_visible_and_update(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    tooltip_is_hoverable: bool,
    check_is_hovered: &Rc<dyn Fn(&Window) -> bool>,
    tooltip_bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    // Separates logic for what mutation should occur from applying it, to avoid overlapping RefCell
    // borrows.
    enum Action {
        None,
        Hide,
        ScheduleHide(AnyTooltip),
        CancelHide(AnyTooltip),
    }

    let is_hovered = check_is_hovered(window)
        || (tooltip_is_hoverable && tooltip_bounds.contains(&window.mouse_position()));
    let action = match active_tooltip.borrow().as_ref() {
        Some(ActiveTooltip::Visible { tooltip, .. }) => {
            if is_hovered {
                Action::None
            } else {
                if tooltip_is_hoverable {
                    Action::ScheduleHide(tooltip.clone())
                } else {
                    Action::Hide
                }
            }
        }
        Some(ActiveTooltip::WaitingForHide { tooltip, .. }) => {
            if is_hovered {
                Action::CancelHide(tooltip.clone())
            } else {
                Action::None
            }
        }
        None | Some(ActiveTooltip::WaitingForShow { .. }) => Action::None,
    };

    match action {
        Action::None => {}
        Action::Hide => clear_active_tooltip(active_tooltip, window),
        Action::ScheduleHide(tooltip) => {
            let delayed_hide_task = window.spawn(cx, {
                let weak_active_tooltip = Rc::downgrade(active_tooltip);
                async move |cx| {
                    cx.background_executor()
                        .timer(HOVERABLE_TOOLTIP_HIDE_DELAY)
                        .await;
                    let Some(active_tooltip) = weak_active_tooltip.upgrade() else {
                        return;
                    };
                    if active_tooltip.borrow_mut().take().is_some() {
                        cx.update(|window, _cx| window.refresh()).ok();
                    }
                }
            });
            active_tooltip
                .borrow_mut()
                .replace(ActiveTooltip::WaitingForHide {
                    tooltip,
                    _task: delayed_hide_task,
                });
        }
        Action::CancelHide(tooltip) => {
            // Cancel waiting to hide tooltip when it becomes hovered.
            active_tooltip.borrow_mut().replace(ActiveTooltip::Visible {
                tooltip,
                is_hoverable: true,
            });
        }
    }

    active_tooltip.borrow().is_some()
}

#[derive(Default)]
pub(crate) struct GroupHitboxes(HashMap<SharedString, SmallVec<[HitboxId; 1]>>);

impl Global for GroupHitboxes {}

impl GroupHitboxes {
    pub fn get(name: &SharedString, cx: &mut App) -> Option<HitboxId> {
        cx.default_global::<Self>()
            .0
            .get(name)
            .and_then(|bounds_stack| bounds_stack.last())
            .cloned()
    }

    pub fn push(name: SharedString, hitbox_id: HitboxId, cx: &mut App) {
        cx.default_global::<Self>()
            .0
            .entry(name)
            .or_default()
            .push(hitbox_id);
    }

    pub fn pop(name: &SharedString, cx: &mut App) {
        cx.default_global::<Self>().0.get_mut(name).unwrap().pop();
    }
}

/// A wrapper around an element that can store state, produced after assigning an ElementId.
pub struct Stateful<E> {
    pub(crate) element: E,
}

impl<E> Styled for Stateful<E>
where
    E: Styled,
{
    fn style(&mut self) -> &mut StyleRefinement {
        self.element.style()
    }
}

impl<E> StatefulInteractiveElement for Stateful<E>
where
    E: Element,
    Self: InteractiveElement,
{
}

impl<E> InteractiveElement for Stateful<E>
where
    E: InteractiveElement,
{
    fn interactivity(&mut self) -> &mut Interactivity {
        self.element.interactivity()
    }
}

impl<E> Element for Stateful<E>
where
    E: Element,
{
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.element.id()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        self.element.source_location()
    }

    fn a11y_role(&self) -> Option<accesskit::Role> {
        self.element.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.element.write_a11y_info(node);
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut crate::A11ySubtreeBuilder,
    ) {
        self.element.a11y_synthetic_children(prepaint, builder);
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.element.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> E::PrepaintState {
        self.element
            .prepaint(id, inspector_id, bounds, state, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element.paint(
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
    }
}

impl<E> IntoElement for Stateful<E>
where
    E: Element,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E> ParentElement for Stateful<E>
where
    E: ParentElement,
{
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.element.extend(elements)
    }
}

/// Represents an element that can be scrolled *to* in its parent element.
/// Contrary to [ScrollHandle::scroll_to_active_item], an anchored element does not have to be an immediate child of the parent.
#[derive(Clone)]
pub struct ScrollAnchor {
    handle: ScrollHandle,
    last_origin: Rc<RefCell<Point<Pixels>>>,
}

impl ScrollAnchor {
    /// Creates a [ScrollAnchor] associated with a given [ScrollHandle].
    pub fn for_handle(handle: ScrollHandle) -> Self {
        Self {
            handle,
            last_origin: Default::default(),
        }
    }
    /// Request scroll to this item on the next frame.
    pub fn scroll_to(&self, window: &mut Window, _cx: &mut App) {
        let this = self.clone();

        window.on_next_frame(move |window, cx| {
            let viewport_bounds = this.handle.bounds();
            let self_bounds = *this.last_origin.borrow();
            if this.handle.request_offset(
                viewport_bounds.origin - self_bounds,
                ScrollMutationSource::Deferred,
            ) {
                cx.notify(window.current_view());
            }
        });
    }
}

#[derive(Default, Debug)]
pub(crate) struct ScrollHandleState {
    offset: Rc<RefCell<Point<Pixels>>>,
    /// Moves when a request changes where the owner will scroll to, for a
    /// retained owner to be built again to apply it.
    fast_version: crate::fast::dependencies::StateVersion,
    ongoing_scroll: Rc<RefCell<OngoingScroll>>,
    bounds: Bounds<Pixels>,
    max_offset: Point<Pixels>,
    child_bounds: Vec<Bounds<Pixels>>,
    scroll_to_bottom: bool,
    overflow: Point<Overflow>,
    active_item: Option<ScrollActiveItem>,
    dependent_views: SmallVec<[EntityId; 2]>,
    activity: Rc<RefCell<ScrollActivity>>,
    scrollbar_drag_active: bool,
    metrics: ScrollMetrics,
    pending_offset: Option<(Point<Pixels>, ScrollMutationSource)>,
    pending_programmatic_offset: Option<(Point<Pixels>, ScrollBehavior)>,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct ScrollActiveItem {
    index: usize,
    strategy: ScrollStrategy,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
enum ScrollStrategy {
    #[default]
    FirstVisible,
    Top,
}

/// A handle to the scrollable aspects of an element.
/// Used for accessing scroll state, like the current scroll offset,
/// and for mutating the scroll state, like scrolling to a specific child.
#[derive(Clone, Debug)]
pub struct ScrollHandle(pub(crate) Rc<RefCell<ScrollHandleState>>);

impl Default for ScrollHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollHandle {
    /// Returns whether both handles refer to the same scroll owner.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// Construct a new scroll handle.
    pub fn new() -> Self {
        Self(Rc::default())
    }

    /// The version a request to this handle moves, for a retained owner.
    pub(crate) fn fast_version(&self) -> crate::fast::dependencies::StateVersion {
        self.0.borrow().fast_version.clone()
    }

    /// Registers an entity whose rendered output reads this handle even though a different
    /// entity owns the scroll container. Registered dependents are invalidated after an
    /// interactive scroll offset mutation so their cached elements can recompute.
    pub fn register_dependent_view(&self, entity_id: EntityId) {
        let mut state = self.0.borrow_mut();
        if !state.dependent_views.contains(&entity_id) {
            state.dependent_views.push(entity_id);
        }
    }

    /// Return the latest immutable positive-logical metrics snapshot.
    pub fn metrics(&self) -> ScrollMetrics {
        let state = self.0.borrow();
        crate::fast::layers::invalidate::note_offset_read(&state.fast_version);
        state.metrics
    }

    /// Try to return the latest metrics snapshot without panicking on a re-entrant borrow.
    pub fn try_metrics(&self) -> Option<ScrollMetrics> {
        self.0.try_borrow().ok().map(|state| {
            crate::fast::layers::invalidate::note_offset_read(&state.fast_version);
            state.metrics
        })
    }

    /// Get the current scroll offset.
    pub fn offset(&self) -> Point<Pixels> {
        let state = self.0.borrow();
        crate::fast::layers::invalidate::note_offset_read(&state.fast_version);
        *state.offset.borrow()
    }

    /// Try to get the current scroll offset without panicking if the handle is already borrowed.
    pub fn try_offset(&self) -> Option<Point<Pixels>> {
        self.0.try_borrow().ok().map(|state| {
            crate::fast::layers::invalidate::note_offset_read(&state.fast_version);
            *state.offset.borrow()
        })
    }

    /// Return the latest queued direct-scrollbar offset without publishing it.
    pub fn pending_scrollbar_offset(&self) -> Option<Point<Pixels>> {
        self.0
            .try_borrow()
            .ok()?
            .pending_offset
            .and_then(|(offset, source)| {
                matches!(source, ScrollMutationSource::Scrollbar).then_some(offset)
            })
    }

    /// Get the maximum scroll offset.
    pub fn max_offset(&self) -> Point<Pixels> {
        let state = self.0.borrow();
        crate::fast::layers::invalidate::note_offset_read(&state.fast_version);
        state.max_offset
    }

    /// Try to get the maximum scroll offset without panicking if the handle is already borrowed.
    pub fn try_max_offset(&self) -> Option<Point<Pixels>> {
        self.0.try_borrow().ok().map(|state| {
            crate::fast::layers::invalidate::note_offset_read(&state.fast_version);
            state.max_offset
        })
    }

    /// Get the top child that's scrolled into view.
    pub fn top_item(&self) -> usize {
        let state = self.0.borrow();
        crate::fast::layers::invalidate::note_offset_read(&state.fast_version);
        let top = state.bounds.top() - state.offset.borrow().y;

        match state.child_bounds.binary_search_by(|bounds| {
            if top < bounds.top() {
                Ordering::Greater
            } else if top > bounds.bottom() {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        }) {
            Ok(ix) => ix,
            Err(ix) => ix.min(state.child_bounds.len().saturating_sub(1)),
        }
    }

    /// Get the bottom child that's scrolled into view.
    pub fn bottom_item(&self) -> usize {
        let state = self.0.borrow();
        crate::fast::layers::invalidate::note_offset_read(&state.fast_version);
        let bottom = state.bounds.bottom() - state.offset.borrow().y;

        match state.child_bounds.binary_search_by(|bounds| {
            if bottom < bounds.top() {
                Ordering::Greater
            } else if bottom > bounds.bottom() {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        }) {
            Ok(ix) => ix,
            Err(ix) => ix.min(state.child_bounds.len().saturating_sub(1)),
        }
    }

    /// Return the bounds into which this child is painted
    pub fn bounds(&self) -> Bounds<Pixels> {
        let bounds = self.0.borrow().bounds;
        crate::fast::layers::input::moved(&self.0, Some(bounds)).unwrap_or_default()
    }

    /// Try to get the viewport bounds without panicking if the handle is already borrowed.
    pub fn try_bounds(&self) -> Option<Bounds<Pixels>> {
        let bounds = self.0.try_borrow().ok().map(|state| state.bounds);
        crate::fast::layers::input::moved(&self.0, bounds)
    }

    /// Get the bounds for a specific child.
    pub fn bounds_for_item(&self, ix: usize) -> Option<Bounds<Pixels>> {
        let bounds = {
            let state = self.0.borrow();
            crate::fast::layers::invalidate::note_offset_read(&state.fast_version);
            state.child_bounds.get(ix).cloned()
        };
        crate::fast::layers::input::moved(&self.0, bounds)
    }

    /// Update [ScrollHandleState]'s active item for scrolling to in prepaint
    pub fn scroll_to_item(&self, ix: usize) {
        let mut state = self.0.borrow_mut();
        let item = Some(ScrollActiveItem {
            index: ix,
            strategy: ScrollStrategy::default(),
        });
        state.fast_version.bump_if(state.active_item != item);
        state.active_item = item;
    }

    /// Update [ScrollHandleState]'s active item for scrolling to in prepaint
    /// This scrolls the minimal amount to ensure that the child is the first visible element
    pub fn scroll_to_top_of_item(&self, ix: usize) {
        let mut state = self.0.borrow_mut();
        let item = Some(ScrollActiveItem {
            index: ix,
            strategy: ScrollStrategy::Top,
        });
        state.fast_version.bump_if(state.active_item != item);
        state.active_item = item;
    }

    /// Try to queue the active item request without panicking if the handle is already borrowed.
    fn try_request_active_item(&self) -> bool {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };

        let Some(active_item) = state.active_item else {
            return true;
        };

        let active_item = match state.child_bounds.get(active_item.index).copied() {
            Some(bounds) => {
                let Ok(scroll_offset) = state.offset.try_borrow() else {
                    return false;
                };
                let mut requested = *scroll_offset;
                match active_item.strategy {
                    ScrollStrategy::FirstVisible => {
                        if state.overflow.y == Overflow::Scroll {
                            let child_height = bounds.size.height;
                            let viewport_height = state.bounds.size.height;
                            if child_height > viewport_height {
                                requested.y = state.bounds.top() - bounds.top();
                            } else if bounds.top() + requested.y < state.bounds.top() {
                                requested.y = state.bounds.top() - bounds.top();
                            } else if bounds.bottom() + requested.y > state.bounds.bottom() {
                                requested.y = state.bounds.bottom() - bounds.bottom();
                            }
                        }
                    }
                    ScrollStrategy::Top => {
                        requested.y = state.bounds.top() - bounds.top();
                    }
                }

                if state.overflow.x == Overflow::Scroll {
                    let child_width = bounds.size.width;
                    let viewport_width = state.bounds.size.width;
                    if child_width > viewport_width {
                        requested.x = state.bounds.left() - bounds.left();
                    } else if bounds.left() + requested.x < state.bounds.left() {
                        requested.x = state.bounds.left() - bounds.left();
                    } else if bounds.right() + requested.x > state.bounds.right() {
                        requested.x = state.bounds.right() - bounds.right();
                    }
                }
                let changed = requested != *scroll_offset;
                drop(scroll_offset);
                if changed {
                    state.pending_offset = Some((requested, ScrollMutationSource::Deferred));
                }
                None
            }
            None => Some(active_item),
        };

        state.active_item = active_item;
        true
    }

    /// Scrolls to the bottom.
    pub fn scroll_to_bottom(&self) {
        let mut state = self.0.borrow_mut();
        state.fast_version.bump_if(!state.scroll_to_bottom);
        state.scroll_to_bottom = true;
    }

    /// Queue a physical offset request for the owning scroll element to clamp and publish.
    ///
    /// Requests are deliberately App-free so state owners can issue them outside render.
    /// The offset does not become observable until the owner accepts it through the single
    /// App-aware publication path during prepaint.
    pub fn request_offset(&self, position: Point<Pixels>, source: ScrollMutationSource) -> bool {
        if !position.x.as_f32().is_finite() || !position.y.as_f32().is_finite() {
            return false;
        }
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };
        if matches!(
            source,
            ScrollMutationSource::Wheel
                | ScrollMutationSource::Scrollbar
                | ScrollMutationSource::Deferred
        ) && state.activity.borrow().kind() == ScrollActivityKind::Programmatic
        {
            state.activity.borrow_mut().stop();
        }
        state.pending_programmatic_offset = None;
        let moves = state
            .offset
            .try_borrow()
            .map_or(true, |offset| *offset != position);
        state
            .fast_version
            .bump_if(moves && state.pending_offset != Some((position, source)));
        state.pending_offset = Some((position, source));
        true
    }

    /// Marks the beginning of a direct scrollbar-thumb drag.
    ///
    /// Keeping the shared activity in the direct state prevents semantic scroll
    /// consumers from treating intermediate thumb positions as settled input.
    pub fn begin_scrollbar_drag(&self) -> bool {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };
        state.scrollbar_drag_active = true;
        state.activity.borrow_mut().begin_direct();
        true
    }

    /// Marks the end of a direct scrollbar-thumb drag.
    pub fn end_scrollbar_drag(&self) -> bool {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };
        state.scrollbar_drag_active = false;
        let was_direct = state.activity.borrow().is_direct();
        if was_direct {
            state.activity.borrow_mut().stop();
        }
        true
    }

    /// Queue an explicit programmatic offset using the shared scroll activity driver.
    ///
    /// Positions use the same physical, non-positive convention as [`Self::offset`]. The owning
    /// element resolves current geometry during prepaint, so requests made before layout remain
    /// deterministic and are clamped against the first available viewport.
    pub fn scroll_to(&self, position: Point<Pixels>, behavior: ScrollBehavior) -> bool {
        if !position.x.as_f32().is_finite() || !position.y.as_f32().is_finite() {
            return false;
        }
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };
        state.activity.borrow_mut().stop();
        state.pending_offset = None;
        let moves = state
            .offset
            .try_borrow()
            .map_or(true, |offset| *offset != position);
        state
            .fast_version
            .bump_if(moves && state.pending_programmatic_offset != Some((position, behavior)));
        state.pending_programmatic_offset = Some((position, behavior));
        true
    }

    pub(crate) fn cancel_programmatic_scroll(&self) {
        if let Ok(mut state) = self.0.try_borrow_mut() {
            if state.activity.borrow().kind() == ScrollActivityKind::Programmatic {
                state.activity.borrow_mut().stop();
            }
            state.pending_programmatic_offset = None;
        }
    }

    /// Advance this owner's active programmatic driver without relying on a platform frame pump.
    /// Generation `1` is the first request on a fresh owner.
    #[cfg(any(test, feature = "test-support"))]
    pub fn advance_programmatic_scroll_for_test(
        &self,
        generation: u64,
        timestamp: Instant,
        current_view: EntityId,
        cx: &mut App,
    ) -> bool {
        apply_programmatic_scroll_frame(self, generation, timestamp, current_view, cx).is_some()
    }

    fn publish_offset(
        &self,
        candidate: Point<Pixels>,
        max_offset: Point<Pixels>,
        bounds: Bounds<Pixels>,
        source: ScrollMutationSource,
        owner: EntityId,
        cx: &mut App,
    ) -> bool {
        self.publish_offset_inner(
            candidate, max_offset, bounds, source, owner, false, false, cx,
        )
    }

    /// Clamp and publish a direct scrollbar request in its pointer-input turn.
    ///
    /// The scrollbar already has the last accepted viewport geometry, so publishing here lets
    /// the invalidated owner render the accepted metrics once instead of previewing a queued
    /// offset and scheduling a second post-prepaint owner frame.
    pub fn publish_scrollbar_offset(
        &self,
        candidate: Point<Pixels>,
        owner: EntityId,
        cx: &mut App,
    ) -> bool {
        let Ok(state) = self.0.try_borrow() else {
            return false;
        };
        let max_offset = state.max_offset;
        let bounds = state.bounds;
        drop(state);
        self.publish_offset_inner(
            candidate,
            max_offset,
            bounds,
            ScrollMutationSource::Scrollbar,
            owner,
            false,
            true,
            cx,
        )
    }

    fn publish_programmatic_offset(
        &self,
        candidate: Point<Pixels>,
        max_offset: Point<Pixels>,
        bounds: Bounds<Pixels>,
        owner: EntityId,
        cx: &mut App,
    ) -> bool {
        self.publish_offset_inner(
            candidate,
            max_offset,
            bounds,
            ScrollMutationSource::Programmatic,
            owner,
            true,
            false,
            cx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn publish_offset_inner(
        &self,
        candidate: Point<Pixels>,
        max_offset: Point<Pixels>,
        bounds: Bounds<Pixels>,
        source: ScrollMutationSource,
        owner: EntityId,
        preserve_programmatic_activity: bool,
        notify_immediately: bool,
        cx: &mut App,
    ) -> bool {
        if !candidate.x.as_f32().is_finite()
            || !candidate.y.as_f32().is_finite()
            || !max_offset.x.as_f32().is_finite()
            || !max_offset.y.as_f32().is_finite()
        {
            return false;
        }

        let (changed, dependent_views) = {
            let Ok(mut state) = self.0.try_borrow_mut() else {
                return false;
            };
            let offset_cell = state.offset.clone();
            let Ok(mut offset) = offset_cell.try_borrow_mut() else {
                return false;
            };
            let next = point(
                candidate.x.clamp(-max_offset.x, Pixels::ZERO),
                candidate.y.clamp(-max_offset.y, Pixels::ZERO),
            );
            let previous = *offset;
            let positive_logical = |value: Pixels| {
                if value.is_zero() {
                    Pixels::ZERO
                } else {
                    -value
                }
            };
            let logical_offset = point(positive_logical(next.x), positive_logical(next.y));
            let logical_delta = point(
                positive_logical(next.x - previous.x),
                positive_logical(next.y - previous.y),
            );
            let viewport_size = bounds.size;
            let content_size: Size<Pixels> = (max_offset + viewport_size.into()).into();

            match source {
                ScrollMutationSource::Scrollbar if state.scrollbar_drag_active => {}
                ScrollMutationSource::Wheel
                | ScrollMutationSource::Scrollbar
                | ScrollMutationSource::Deferred => state.activity.borrow_mut().stop(),
                ScrollMutationSource::Programmatic if !preserve_programmatic_activity => {
                    state.activity.borrow_mut().begin_programmatic()
                }
                ScrollMutationSource::Programmatic | ScrollMutationSource::LayoutClamp => {}
            }
            let activity = state.activity.borrow().kind();
            let metrics_changed = state.metrics.offset != logical_offset
                || state.metrics.max_offset != max_offset
                || state.metrics.viewport_size != viewport_size
                || state.metrics.content_size != content_size
                || state.metrics.activity != activity;
            if !metrics_changed {
                return false;
            }

            *offset = next;
            crate::fast::layers::invalidate::offset_published(
                &state.fast_version,
                next != previous,
                &state.dependent_views,
                !(source == ScrollMutationSource::Wheel || notify_immediately),
            );
            let primary_delta = if logical_delta.y.abs() >= logical_delta.x.abs() {
                logical_delta.y
            } else {
                logical_delta.x
            };
            state.max_offset = max_offset;
            state.bounds = bounds;
            state.metrics = ScrollMetrics {
                offset: logical_offset,
                max_offset,
                viewport_size,
                content_size,
                delta: logical_delta,
                direction: direction_for_delta(primary_delta),
                source,
                activity,
                revision: state.metrics.revision.wrapping_add(1),
            };
            (true, state.dependent_views.clone())
        };

        if changed && (source == ScrollMutationSource::Wheel || notify_immediately) {
            // Immediate input publishes outside prepaint. Invalidate its complete tracked view
            // set in the input turn so a concurrent content update cannot overtake the scroll
            // render and no redundant post-prepaint owner frame is needed.
            cx.notify(owner);
            for entity_id in dependent_views {
                if entity_id != owner {
                    cx.notify(entity_id);
                }
            }
        } else if changed {
            cx.defer(move |cx| {
                cx.notify(owner);
                for entity_id in dependent_views {
                    if entity_id != owner {
                        cx.notify(entity_id);
                    }
                }
            });
        }
        changed
    }

    /// Get the logical scroll top, based on a child index and a pixel offset.
    pub fn logical_scroll_top(&self) -> (usize, Pixels) {
        let ix = self.top_item();
        let state = self.0.borrow();

        if let Some(child_bounds) = state.child_bounds.get(ix) {
            (
                ix,
                child_bounds.top() + state.offset.borrow().y - state.bounds.top(),
            )
        } else {
            (ix, px(0.))
        }
    }

    /// Get the logical scroll bottom, based on a child index and a pixel offset.
    pub fn logical_scroll_bottom(&self) -> (usize, Pixels) {
        let ix = self.bottom_item();
        let state = self.0.borrow();

        if let Some(child_bounds) = state.child_bounds.get(ix) {
            (
                ix,
                child_bounds.bottom() + state.offset.borrow().y - state.bounds.bottom(),
            )
        } else {
            (ix, px(0.))
        }
    }

    /// Get the count of children for scrollable item.
    pub fn children_count(&self) -> usize {
        self.0.borrow().child_bounds.len()
    }
}

fn schedule_programmatic_scroll(
    scroll_handle: ScrollHandle,
    generation: u64,
    current_view: EntityId,
    window: &Window,
) {
    window.on_next_frame(move |window, cx| {
        let Some(frame) = apply_programmatic_scroll_frame(
            &scroll_handle,
            generation,
            cx.background_executor().now(),
            current_view,
            cx,
        ) else {
            return;
        };
        if frame.request_next_frame {
            schedule_programmatic_scroll(scroll_handle, generation, current_view, window);
            // A step redraws the scroll owner, not the whole window.
            cx.notify(current_view);
        }
    });
}

fn apply_programmatic_scroll_frame(
    scroll_handle: &ScrollHandle,
    generation: u64,
    timestamp: Instant,
    current_view: EntityId,
    cx: &mut App,
) -> Option<ProgrammaticScrollFrame> {
    let (activity, max_offset, bounds) = {
        let state = scroll_handle.0.try_borrow().ok()?;
        (state.activity.clone(), state.max_offset, state.bounds)
    };
    let frame = activity
        .borrow_mut()
        .next_programmatic_at(generation, timestamp, max_offset)?;
    scroll_handle.publish_programmatic_offset(
        point(-frame.offset.x, -frame.offset.y),
        max_offset,
        bounds,
        current_view,
        cx,
    );
    Some(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ScrollDirection, StickyAxisState};

    #[test]
    fn pending_scrollbar_offset_is_read_only_and_source_filtered() {
        let handle = ScrollHandle::new();
        let scrollbar_offset = point(px(-40.0), px(-120.0));

        assert!(handle.request_offset(
            point(px(-10.0), px(-20.0)),
            ScrollMutationSource::Programmatic,
        ));
        assert_eq!(handle.pending_scrollbar_offset(), None);
        assert!(handle.request_offset(scrollbar_offset, ScrollMutationSource::Scrollbar,));

        assert_eq!(handle.pending_scrollbar_offset(), Some(scrollbar_offset));
        assert_eq!(handle.offset(), Point::default());
    }

    struct ScrollMetricsOwner {
        handle: ScrollHandle,
    }

    impl Render for ScrollMetricsOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("scroll-metrics-owner")
                .w(px(100.0))
                .h(px(100.0))
                .overflow_scroll()
                .track_scroll(&self.handle)
                .child(div().w(px(400.0)).h(px(500.0)))
        }
    }

    struct ResizableScrollMetricsOwner {
        handle: ScrollHandle,
        viewport_size: Rc<Cell<Size<Pixels>>>,
        content_size: Rc<Cell<Size<Pixels>>>,
    }

    impl Render for ResizableScrollMetricsOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let viewport_size = self.viewport_size.get();
            let content_size = self.content_size.get();
            div()
                .id("resizable-scroll-metrics-owner")
                .w(viewport_size.width)
                .h(viewport_size.height)
                .overflow_scroll()
                .track_scroll(&self.handle)
                .child(div().w(content_size.width).h(content_size.height))
        }
    }

    #[test]
    fn scroll_metrics_publish_positive_logical_geometry_and_idempotent_requests() {
        let mut app = TestAppContext::single();
        let handle = ScrollHandle::new();
        let window = app.add_window({
            let handle = handle.clone();
            move |_, _| ScrollMetricsOwner { handle }
        });
        let window = window.into();
        let draw = |app: &mut TestAppContext| {
            app.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
        };

        draw(&mut app);
        let initial = handle.metrics();
        assert_eq!(initial.offset, Point::default());
        assert_eq!(initial.max_offset, point(px(300.0), px(400.0)));
        assert_eq!(initial.viewport_size, size(px(100.0), px(100.0)));
        assert_eq!(initial.content_size, size(px(400.0), px(500.0)));

        assert!(handle.request_offset(
            point(px(-40.0), px(-120.0)),
            ScrollMutationSource::Programmatic,
        ));
        draw(&mut app);
        let moved = handle.metrics();
        assert_eq!(moved.offset, point(px(40.0), px(120.0)));
        assert_eq!(moved.delta, point(px(40.0), px(120.0)));
        assert_eq!(moved.direction, ScrollDirection::Forward);
        assert_eq!(moved.source, ScrollMutationSource::Programmatic);
        assert_eq!(moved.activity, ScrollActivityKind::Programmatic);
        assert_eq!(moved.revision, initial.revision + 1);

        assert!(handle.request_offset(
            point(px(-40.0), px(-120.0)),
            ScrollMutationSource::Programmatic,
        ));
        draw(&mut app);
        assert_eq!(handle.metrics().revision, moved.revision);
        assert!(!handle.request_offset(
            point(px(f32::NAN), px(-120.0)),
            ScrollMutationSource::Programmatic,
        ));
        assert_eq!(handle.metrics(), moved);

        assert!(handle.request_offset(
            point(px(-10_000.0), px(-10_000.0)),
            ScrollMutationSource::Programmatic,
        ));
        draw(&mut app);
        let clamped = handle.metrics();
        assert_eq!(clamped.offset, clamped.max_offset);
    }

    #[test]
    fn scroll_metrics_axis_mapping_preserves_positive_coordinates_when_reversed() {
        let metrics = ScrollMetrics {
            offset: point(px(30.0), px(70.0)),
            max_offset: point(px(100.0), px(200.0)),
            delta: point(px(10.0), px(-20.0)),
            ..ScrollMetrics::default()
        };

        let horizontal = metrics.for_axis(crate::Axis::Horizontal, false);
        assert_eq!(horizontal.offset, point(px(30.0), Pixels::ZERO));
        assert_eq!(horizontal.delta, point(px(10.0), Pixels::ZERO));
        assert_eq!(horizontal.direction, ScrollDirection::Forward);

        let reversed_horizontal = metrics.for_axis(crate::Axis::Horizontal, true);
        assert_eq!(reversed_horizontal.offset, point(px(70.0), Pixels::ZERO));
        assert_eq!(reversed_horizontal.delta, point(px(-10.0), Pixels::ZERO));
        assert_eq!(reversed_horizontal.direction, ScrollDirection::Reverse);

        let reversed_vertical = metrics.for_axis(crate::Axis::Vertical, true);
        assert_eq!(reversed_vertical.offset, point(Pixels::ZERO, px(130.0)));
        assert_eq!(reversed_vertical.delta, point(Pixels::ZERO, px(20.0)));
        assert_eq!(reversed_vertical.direction, ScrollDirection::Forward);
    }

    #[test]
    fn scroll_metrics_correct_empty_extents_and_content_viewport_resizes() {
        let mut app = TestAppContext::single();
        let handle = ScrollHandle::new();
        let viewport_size = Rc::new(Cell::new(Size::default()));
        let content_size = Rc::new(Cell::new(Size::default()));
        let window = app.add_window({
            let handle = handle.clone();
            let viewport_size = viewport_size.clone();
            let content_size = content_size.clone();
            move |_, _| ResizableScrollMetricsOwner {
                handle,
                viewport_size,
                content_size,
            }
        });
        let any_window = window.into();
        let draw = |app: &mut TestAppContext| {
            app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
        };

        draw(&mut app);
        assert_eq!(handle.metrics(), ScrollMetrics::default());

        viewport_size.set(size(px(100.0), px(100.0)));
        content_size.set(size(px(400.0), px(500.0)));
        window.update(&mut app, |_, _, cx| cx.notify()).unwrap();
        draw(&mut app);
        let expanded = handle.metrics();
        assert_eq!(expanded.viewport_size, size(px(100.0), px(100.0)));
        assert_eq!(expanded.content_size, size(px(400.0), px(500.0)));
        assert_eq!(expanded.max_offset, point(px(300.0), px(400.0)));
        assert_eq!(expanded.revision, 1);

        assert!(handle.request_offset(
            point(px(-300.0), px(-400.0)),
            ScrollMutationSource::Programmatic,
        ));
        window.update(&mut app, |_, _, cx| cx.notify()).unwrap();
        draw(&mut app);
        assert_eq!(handle.metrics().offset, point(px(300.0), px(400.0)));

        content_size.set(size(px(120.0), px(130.0)));
        window.update(&mut app, |_, _, cx| cx.notify()).unwrap();
        draw(&mut app);
        let content_shrunk = handle.metrics();
        assert_eq!(content_shrunk.offset, point(px(20.0), px(30.0)));
        assert_eq!(content_shrunk.max_offset, point(px(20.0), px(30.0)));
        assert_eq!(content_shrunk.content_size, size(px(120.0), px(130.0)));
        assert_eq!(content_shrunk.delta, point(px(-280.0), px(-370.0)));
        assert_eq!(content_shrunk.source, ScrollMutationSource::LayoutClamp);
        assert_eq!(content_shrunk.revision, 3);

        viewport_size.set(size(px(200.0), px(200.0)));
        window.update(&mut app, |_, _, cx| cx.notify()).unwrap();
        draw(&mut app);
        let viewport_expanded = handle.metrics();
        assert_eq!(viewport_expanded.offset, Point::default());
        assert_eq!(viewport_expanded.max_offset, Point::default());
        assert_eq!(viewport_expanded.viewport_size, size(px(200.0), px(200.0)));
        assert_eq!(viewport_expanded.content_size, size(px(200.0), px(200.0)));
        assert_eq!(viewport_expanded.delta, point(px(-20.0), px(-30.0)));
        assert_eq!(viewport_expanded.revision, 4);

        viewport_size.set(Size::default());
        content_size.set(Size::default());
        window.update(&mut app, |_, _, cx| cx.notify()).unwrap();
        draw(&mut app);
        let emptied = handle.metrics();
        assert_eq!(emptied.offset, Point::default());
        assert_eq!(emptied.max_offset, Point::default());
        assert_eq!(emptied.viewport_size, Size::default());
        assert_eq!(emptied.content_size, Size::default());
        assert_eq!(emptied.revision, 5);
    }

    #[test]
    fn scroll_metrics_source_matrix_publishes_once_and_preserves_activity_contracts() {
        let cases = [
            (
                ScrollMutationSource::Wheel,
                ScrollActivityKind::Idle,
                ScrollActivityKind::Programmatic,
            ),
            (
                ScrollMutationSource::Scrollbar,
                ScrollActivityKind::Idle,
                ScrollActivityKind::Direct,
            ),
            (
                ScrollMutationSource::Programmatic,
                ScrollActivityKind::Programmatic,
                ScrollActivityKind::Direct,
            ),
            (
                ScrollMutationSource::Deferred,
                ScrollActivityKind::Idle,
                ScrollActivityKind::Programmatic,
            ),
            (
                ScrollMutationSource::LayoutClamp,
                ScrollActivityKind::Programmatic,
                ScrollActivityKind::Programmatic,
            ),
        ];

        for (source, expected_activity, prior_activity) in cases {
            let mut app = TestAppContext::single();
            let handle = ScrollHandle::new();
            let window = app.add_window({
                let handle = handle.clone();
                move |_, _| ScrollMetricsOwner { handle }
            });
            let any_window = window.into();
            app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            let baseline_revision = handle.metrics().revision;

            let activity = handle.0.borrow().activity.clone();
            match prior_activity {
                ScrollActivityKind::Idle => activity.borrow_mut().stop(),
                ScrollActivityKind::Direct => activity.borrow_mut().begin_direct(),
                ScrollActivityKind::Programmatic => activity.borrow_mut().begin_programmatic(),
            }

            assert!(handle.request_offset(point(px(-10.0), px(-20.0)), source));
            app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            let published = handle.metrics();
            assert_eq!(
                published.revision,
                baseline_revision + 1,
                "real {source:?} mutation"
            );
            assert_eq!(published.source, source);
            assert_eq!(published.activity, expected_activity, "{source:?} activity");
            assert_eq!(published.offset, point(px(10.0), px(20.0)));

            assert!(handle.request_offset(point(px(-10.0), px(-20.0)), source));
            app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            assert_eq!(
                handle.metrics().revision,
                baseline_revision + 1,
                "no-op {source:?} mutation"
            );
            assert_eq!(
                activity.borrow().kind(),
                expected_activity,
                "no-op {source:?} activity"
            );
        }
    }

    #[test]
    fn scrollbar_thumb_drag_remains_direct_until_release() {
        let mut app = TestAppContext::single();
        let handle = ScrollHandle::new();
        let window = app.add_window({
            let handle = handle.clone();
            move |_, _| ScrollMetricsOwner { handle }
        });
        let window = window.into();
        let draw = |app: &mut TestAppContext| {
            app.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
        };

        draw(&mut app);
        assert!(handle.begin_scrollbar_drag());
        assert!(handle.request_offset(point(px(0.0), px(-80.0)), ScrollMutationSource::Scrollbar,));
        draw(&mut app);
        assert_eq!(handle.metrics().activity, ScrollActivityKind::Direct);
        assert_eq!(handle.metrics().source, ScrollMutationSource::Scrollbar);

        assert!(handle.end_scrollbar_drag());
        draw(&mut app);
        assert_eq!(handle.metrics().activity, ScrollActivityKind::Idle);
    }

    #[test]
    fn programmatic_settlement_publishes_idle_when_the_final_offset_is_unchanged() {
        let mut app = TestAppContext::single();
        let handle = ScrollHandle::new();
        let window = app.add_window({
            let handle = handle.clone();
            move |_, _| ScrollMetricsOwner { handle }
        });
        let owner = window.entity(&app).unwrap().entity_id();
        let any_window = window.into();
        app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();

        assert!(handle.request_offset(
            point(px(0.0), px(-72.0)),
            ScrollMutationSource::Programmatic,
        ));
        app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let moving = handle.metrics();
        assert_eq!(moving.activity, ScrollActivityKind::Programmatic);

        let (activity, max_offset, bounds) = {
            let state = handle.0.borrow();
            (state.activity.clone(), state.max_offset, state.bounds)
        };
        activity.borrow_mut().stop();
        let published = app.update(|cx| {
            handle.publish_programmatic_offset(
                point(px(0.0), px(-72.0)),
                max_offset,
                bounds,
                owner,
                cx,
            )
        });

        assert!(published);
        let settled = handle.metrics();
        assert_eq!(settled.offset, moving.offset);
        assert_eq!(settled.delta, Point::default());
        assert_eq!(settled.direction, ScrollDirection::Idle);
        assert_eq!(settled.source, ScrollMutationSource::Programmatic);
        assert_eq!(settled.activity, ScrollActivityKind::Idle);
        assert_eq!(settled.revision, moving.revision + 1);
    }

    #[test]
    fn scroll_metrics_notifies_owner_and_unique_dependents_once_per_publication() {
        let mut app = TestAppContext::single();
        let handle = ScrollHandle::new();
        let dependent = app.update(|cx| cx.new(|_| ()));
        let window = app.add_window({
            let handle = handle.clone();
            move |_, _| ScrollMetricsOwner { handle }
        });
        let owner = window.entity(&app).unwrap();
        handle.register_dependent_view(owner.entity_id());
        handle.register_dependent_view(owner.entity_id());
        handle.register_dependent_view(dependent.entity_id());
        handle.register_dependent_view(dependent.entity_id());

        let owner_notifications = Rc::new(Cell::new(0));
        let dependent_notifications = Rc::new(Cell::new(0));
        app.update(|cx| {
            cx.observe(&owner, {
                let notifications = owner_notifications.clone();
                move |_, _| notifications.set(notifications.get() + 1)
            })
            .detach();
            cx.observe(&dependent, {
                let notifications = dependent_notifications.clone();
                move |_, _| notifications.set(notifications.get() + 1)
            })
            .detach();
        });

        let any_window = window.into();
        app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        owner_notifications.set(0);
        dependent_notifications.set(0);

        assert!(handle.request_offset(
            point(px(-40.0), px(-80.0)),
            ScrollMutationSource::Programmatic,
        ));
        app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        assert_eq!(owner_notifications.get(), 1);
        assert_eq!(dependent_notifications.get(), 1);

        assert!(handle.request_offset(
            point(px(-40.0), px(-80.0)),
            ScrollMutationSource::Programmatic,
        ));
        app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        assert_eq!(owner_notifications.get(), 1);
        assert_eq!(dependent_notifications.get(), 1);
    }

    #[test]
    fn immediate_input_publication_invalidates_the_owner_before_the_effect_cycle_ends() {
        let mut app = TestAppContext::single();
        let handle = ScrollHandle::new();
        let window = app.add_window({
            let handle = handle.clone();
            move |_, _| ScrollMetricsOwner { handle }
        });
        let owner = window.entity(&app).unwrap().entity_id();
        let any_window = window.into();
        app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let (max_offset, bounds) = {
            let state = handle.0.borrow();
            (state.max_offset, state.bounds)
        };

        let wheel_invalidated_in_turn = app
            .update_window(any_window, |_, window, cx| {
                assert!(!window.is_dirty());
                assert!(handle.publish_offset(
                    point(px(-10.0), px(-20.0)),
                    max_offset,
                    bounds,
                    ScrollMutationSource::Wheel,
                    owner,
                    cx,
                ));
                window.is_dirty()
            })
            .unwrap();
        assert!(wheel_invalidated_in_turn);

        app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let deferred_invalidated_in_turn = app
            .update_window(any_window, |_, window, cx| {
                assert!(!window.is_dirty());
                assert!(handle.publish_offset(
                    point(px(-20.0), px(-40.0)),
                    max_offset,
                    bounds,
                    ScrollMutationSource::Deferred,
                    owner,
                    cx,
                ));
                window.is_dirty()
            })
            .unwrap();
        assert!(!deferred_invalidated_in_turn);

        app.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let scrollbar_invalidated_in_turn = app
            .update_window(any_window, |_, window, cx| {
                assert!(!window.is_dirty());
                assert!(handle.publish_scrollbar_offset(point(px(-30.0), px(-60.0)), owner, cx,));
                assert_eq!(handle.metrics().source, ScrollMutationSource::Scrollbar);
                window.is_dirty()
            })
            .unwrap();
        assert!(scrollbar_invalidated_in_turn);
    }
    use crate::{
        AnyWindowHandle, AppContext as _, Context, GestureTuning, InputEvent, Keystroke,
        MouseMoveEvent, TestAppContext, TouchEvent, TouchId, canvas, util::FluentBuilder as _,
    };
    use std::{cell::Cell, rc::Weak};

    struct GroupHoverTestView {
        render_count: Rc<Cell<usize>>,
        anonymous_paint_count: Rc<Cell<usize>>,
        stateful_width: Rc<Cell<Pixels>>,
    }

    impl Render for GroupHoverTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.render_count.set(self.render_count.get() + 1);
            let anonymous_paint_count = self.anonymous_paint_count.clone();
            let stateful_width = self.stateful_width.clone();
            div().size_full().child(
                div()
                    .ml(px(20.))
                    .mt(px(20.))
                    .size(px(50.))
                    .relative()
                    .group("hover-group")
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .invisible()
                            .group_hover("hover-group", |style| style.visible())
                            .child(canvas(
                                |_, _, _| {},
                                move |_, _, _, _| {
                                    anonymous_paint_count.set(anonymous_paint_count.get() + 1)
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("stateful-group-hover-target")
                            .absolute()
                            .top_0()
                            .left_0()
                            .size(px(10.))
                            .group_hover("hover-group", |style| style.size(px(20.)))
                            .child(canvas(
                                move |bounds, _, _| stateful_width.set(bounds.size.width),
                                |_, _, _, _| {},
                            )),
                    ),
            )
        }
    }

    #[gpux::test]
    fn group_hover_styles_update_only_on_transitions(cx: &mut TestAppContext) {
        let render_count = Rc::new(Cell::new(0));
        let anonymous_paint_count = Rc::new(Cell::new(0));
        let stateful_width = Rc::new(Cell::new(px(0.)));
        let window = cx.add_window({
            let render_count = render_count.clone();
            let anonymous_paint_count = anonymous_paint_count.clone();
            let stateful_width = stateful_width.clone();
            move |_, _| GroupHoverTestView {
                render_count,
                anonymous_paint_count,
                stateful_width,
            }
        });
        let window = AnyWindowHandle::from(window);

        cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        assert_eq!(anonymous_paint_count.get(), 0);
        assert_eq!(stateful_width.get(), px(10.));

        let move_mouse = |cx: &mut TestAppContext, position| {
            cx.update_window(window, |_, window, cx| {
                window.simulate_mouse_move(position, cx)
            })
            .unwrap();
        };

        let initial_render_count = render_count.get();
        move_mouse(cx, point(px(25.), px(25.)));
        assert_eq!(render_count.get(), initial_render_count + 1);
        assert_eq!(anonymous_paint_count.get(), 1);
        assert_eq!(stateful_width.get(), px(20.));

        move_mouse(cx, point(px(30.), px(30.)));
        assert_eq!(render_count.get(), initial_render_count + 1);
        assert_eq!(anonymous_paint_count.get(), 1);
        assert_eq!(stateful_width.get(), px(20.));

        move_mouse(cx, point(px(5.), px(5.)));
        assert_eq!(render_count.get(), initial_render_count + 2);
        assert_eq!(anonymous_paint_count.get(), 1);
        assert_eq!(stateful_width.get(), px(10.));

        move_mouse(cx, point(px(10.), px(10.)));
        assert_eq!(render_count.get(), initial_render_count + 2);
        assert_eq!(anonymous_paint_count.get(), 1);
        assert_eq!(stateful_width.get(), px(10.));
    }

    struct HoverListenerLayoutTestView {
        target_left: Pixels,
        hover_transitions: Rc<RefCell<Vec<bool>>>,
    }

    impl Render for HoverListenerLayoutTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let hover_transitions = self.hover_transitions.clone();
            div().relative().size_full().child(
                div()
                    .id("hover-target")
                    .absolute()
                    .left(self.target_left)
                    .top_0()
                    .size(px(20.))
                    .on_click(|_, _, _| {})
                    .on_hover(move |is_hovered, _, _| {
                        hover_transitions.borrow_mut().push(*is_hovered);
                    }),
            )
        }
    }

    #[gpux::test]
    fn default_hover_listener_updates_when_layout_changes_under_stationary_mouse(
        cx: &mut TestAppContext,
    ) {
        let hover_transitions = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_window({
            let hover_transitions = hover_transitions.clone();
            move |_, _| HoverListenerLayoutTestView {
                target_left: px(40.),
                hover_transitions,
            }
        });
        let any_window = AnyWindowHandle::from(window);

        cx.update_window(any_window, |_, window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_mouse_move(point(px(10.), px(10.)), cx);
        })
        .unwrap();
        assert!(hover_transitions.borrow().is_empty());

        window
            .update(cx, |view, _, cx| {
                view.target_left = px(0.);
                cx.notify();
            })
            .unwrap();
        cx.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true]);

        window
            .update(cx, |view, _, cx| {
                view.target_left = px(40.);
                cx.notify();
            })
            .unwrap();
        cx.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true, false]);
    }

    #[gpux::test]
    fn default_hover_listener_ends_after_key_press(cx: &mut TestAppContext) {
        let hover_transitions = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_window({
            let hover_transitions = hover_transitions.clone();
            move |_, _| HoverListenerLayoutTestView {
                target_left: px(0.),
                hover_transitions,
            }
        });
        let any_window = AnyWindowHandle::from(window);

        cx.update_window(any_window, |_, window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_mouse_move(point(px(10.), px(10.)), cx);
        })
        .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true]);

        key_down(cx, any_window, "a");
        cx.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true, false]);
    }

    struct HoverListenerModeLayoutTestView {
        target_left: Pixels,
        hover_transitions: Rc<RefCell<Vec<bool>>>,
    }

    impl Render for HoverListenerModeLayoutTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let hover_transitions = self.hover_transitions.clone();
            div().relative().size_full().child(
                div()
                    .id("hover-target")
                    .absolute()
                    .left(self.target_left)
                    .top_0()
                    .size(px(20.))
                    .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
                    .on_hover(move |is_hovered, _, _| {
                        hover_transitions.borrow_mut().push(*is_hovered);
                    }),
            )
        }
    }

    #[gpux::test]
    fn input_modality_independent_hover_listener_updates_after_key_press(cx: &mut TestAppContext) {
        let hover_transitions = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_window({
            let hover_transitions = hover_transitions.clone();
            move |_, _| HoverListenerModeLayoutTestView {
                target_left: px(40.),
                hover_transitions,
            }
        });
        let any_window = AnyWindowHandle::from(window);
        let pointer_position = point(px(10.), px(10.));

        cx.update_window(any_window, |_, window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_mouse_move(pointer_position, cx);
        })
        .unwrap();
        assert!(hover_transitions.borrow().is_empty());

        key_down(cx, any_window, "a");
        window
            .update(cx, |view, _, cx| {
                view.target_left = px(0.);
                cx.notify();
            })
            .unwrap();
        cx.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true]);

        key_down(cx, any_window, "b");
        cx.update_window(any_window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true]);

        cx.update_window(any_window, |_, window, cx| {
            window.simulate_mouse_move(point(px(30.), px(10.)), cx);
        })
        .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true, false]);

        cx.update_window(any_window, |_, window, cx| {
            window.simulate_mouse_move(pointer_position, cx);
            window.dispatch_event(
                MouseExitEvent {
                    position: pointer_position,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
        })
        .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true, false, true, false]);
    }

    #[gpux::test]
    fn default_hover_listener_remains_hovered_during_stationary_mouse_press(
        cx: &mut TestAppContext,
    ) {
        let hover_transitions = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_window({
            let hover_transitions = hover_transitions.clone();
            move |_, _| HoverListenerLayoutTestView {
                target_left: px(0.),
                hover_transitions,
            }
        });
        let any_window = AnyWindowHandle::from(window);
        let mouse_position = point(px(10.), px(10.));

        cx.update_window(any_window, |_, window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_mouse_move(mouse_position, cx);
        })
        .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true]);

        cx.update_window(any_window, |_, window, cx| {
            window.dispatch_event(
                MouseDownEvent {
                    position: mouse_position,
                    button: MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
            window.draw(cx).clear(cx);
        })
        .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true]);

        cx.update_window(any_window, |_, window, cx| {
            window.dispatch_event(
                MouseUpEvent {
                    position: mouse_position,
                    button: MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                }
                .to_platform_input(),
                cx,
            );
            window.draw(cx).clear(cx);
        })
        .unwrap();
        assert_eq!(*hover_transitions.borrow(), [true]);
    }

    struct TestTooltipView;

    impl Render for TestTooltipView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().w(px(20.)).h(px(20.)).child("tooltip")
        }
    }

    type CapturedActiveTooltip = Rc<RefCell<Option<Weak<RefCell<Option<ActiveTooltip>>>>>>;

    struct TooltipCaptureElement {
        child: AnyElement,
        captured_active_tooltip: CapturedActiveTooltip,
    }

    impl IntoElement for TooltipCaptureElement {
        type Element = Self;

        fn into_element(self) -> Self::Element {
            self
        }
    }

    impl Element for TooltipCaptureElement {
        type RequestLayoutState = ();
        type PrepaintState = ();

        fn id(&self) -> Option<ElementId> {
            None
        }

        fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            _id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            window: &mut Window,
            cx: &mut App,
        ) -> (LayoutId, Self::RequestLayoutState) {
            (self.child.request_layout(window, cx), ())
        }

        fn prepaint(
            &mut self,
            _id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            _bounds: Bounds<Pixels>,
            _request_layout: &mut Self::RequestLayoutState,
            window: &mut Window,
            cx: &mut App,
        ) -> Self::PrepaintState {
            self.child.prepaint(window, cx);
        }

        fn paint(
            &mut self,
            _id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            _bounds: Bounds<Pixels>,
            _request_layout: &mut Self::RequestLayoutState,
            _prepaint: &mut Self::PrepaintState,
            window: &mut Window,
            cx: &mut App,
        ) {
            self.child.paint(window, cx);
            window.with_global_id("target".into(), |global_id, window| {
                window.with_element_state::<InteractiveElementState, _>(
                    global_id,
                    |state, _window| {
                        let state = state.unwrap();
                        *self.captured_active_tooltip.borrow_mut() =
                            state.active_tooltip.as_ref().map(Rc::downgrade);
                        ((), state)
                    },
                )
            });
        }
    }

    struct TooltipOwner {
        captured_active_tooltip: CapturedActiveTooltip,
        show_delay_override: Option<Duration>,
    }

    impl Render for TooltipOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            TooltipCaptureElement {
                child: div()
                    .size_full()
                    .child(
                        div()
                            .id("target")
                            .w(px(50.))
                            .h(px(50.))
                            .tooltip(|_, cx| cx.new(|_| TestTooltipView).into())
                            .when_some(self.show_delay_override, |this, delay| {
                                this.tooltip_show_delay(delay)
                            }),
                    )
                    .into_any_element(),
                captured_active_tooltip: self.captured_active_tooltip.clone(),
            }
        }
    }

    struct ScrollContextCaptureElement {
        captured_prepaint_context: Rc<RefCell<Option<ScrollContainerContext>>>,
        captured_paint_context: Rc<RefCell<Option<ScrollContainerContext>>>,
    }

    impl IntoElement for ScrollContextCaptureElement {
        type Element = Self;

        fn into_element(self) -> Self::Element {
            self
        }
    }

    impl Element for ScrollContextCaptureElement {
        type RequestLayoutState = ();
        type PrepaintState = ();

        fn id(&self) -> Option<ElementId> {
            None
        }

        fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            _id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            window: &mut Window,
            cx: &mut App,
        ) -> (LayoutId, Self::RequestLayoutState) {
            let style = Style {
                size: size(px(100.).into(), px(200.).into()),
                ..Default::default()
            };
            (window.request_layout(style, [], cx), ())
        }

        fn prepaint(
            &mut self,
            _id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            _bounds: Bounds<Pixels>,
            _request_layout: &mut Self::RequestLayoutState,
            window: &mut Window,
            _cx: &mut App,
        ) -> Self::PrepaintState {
            *self.captured_prepaint_context.borrow_mut() =
                window.nearest_scroll_container_context(crate::ScrollAxis::Vertical);
        }

        fn paint(
            &mut self,
            _id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            _bounds: Bounds<Pixels>,
            _request_layout: &mut Self::RequestLayoutState,
            _prepaint: &mut Self::PrepaintState,
            window: &mut Window,
            _cx: &mut App,
        ) {
            *self.captured_paint_context.borrow_mut() =
                window.nearest_scroll_container_context(crate::ScrollAxis::Vertical);
        }
    }

    struct ScrollContextOwner {
        captured_prepaint_context: Rc<RefCell<Option<ScrollContainerContext>>>,
        captured_paint_context: Rc<RefCell<Option<ScrollContainerContext>>>,
    }

    impl Render for ScrollContextOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("scroll-context-owner")
                .w(px(100.))
                .h(px(100.))
                .overflow_y_scroll()
                .child(ScrollContextCaptureElement {
                    captured_prepaint_context: self.captured_prepaint_context.clone(),
                    captured_paint_context: self.captured_paint_context.clone(),
                })
        }
    }

    #[test]
    fn scrollable_div_child_can_see_nearest_prepaint_scroll_context() {
        let mut test_app = TestAppContext::single();
        let captured_prepaint_context = Rc::new(RefCell::new(None));
        let captured_paint_context = Rc::new(RefCell::new(None));
        let window = test_app.add_window({
            let captured_prepaint_context = captured_prepaint_context.clone();
            move |_, _| ScrollContextOwner {
                captured_prepaint_context,
                captured_paint_context,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        let context = captured_prepaint_context
            .borrow()
            .expect("scrollable div should push a vertical scroll context during child prepaint");

        assert_eq!(size(px(100.), px(100.)), context.scrollport_bounds.size);
        assert_eq!(size(px(100.), px(200.)), context.content_bounds.size);
        assert_eq!(Point::default(), context.scroll_offset);
        assert_eq!(Overflow::Scroll, context.overflow.y);
    }

    #[test]
    fn scrollable_div_child_can_see_nearest_paint_scroll_context() {
        let mut test_app = TestAppContext::single();
        let captured_prepaint_context = Rc::new(RefCell::new(None));
        let captured_paint_context = Rc::new(RefCell::new(None));
        let window = test_app.add_window({
            let captured_paint_context = captured_paint_context.clone();
            move |_, _| ScrollContextOwner {
                captured_prepaint_context,
                captured_paint_context,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        let context = captured_paint_context
            .borrow()
            .expect("scrollable div should push a vertical scroll context during child paint");

        assert_eq!(size(px(100.), px(100.)), context.scrollport_bounds.size);
        assert_eq!(size(px(100.), px(200.)), context.content_bounds.size);
        assert_eq!(Point::default(), context.scroll_offset);
        assert_eq!(Overflow::Scroll, context.overflow.y);
    }

    struct TrackedScrollOwner {
        handle: ScrollHandle,
    }

    impl Render for TrackedScrollOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("tracked-scroll")
                .w(px(100.))
                .h(px(100.))
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .child(div().w(px(100.)).h(px(200.)))
        }
    }

    struct StickyScrollOwner {
        handle: ScrollHandle,
    }

    impl Render for StickyScrollOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("sticky-scroll")
                .w(px(100.))
                .h(px(100.))
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .child(
                    div()
                        .sticky()
                        .top(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .debug_selector(|| "sticky".into()),
                )
                .child(div().w(px(100.)).h(px(180.)))
        }
    }

    struct StickyBorderClipOwner;

    impl Render for StickyBorderClipOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().w(px(100.)).h(px(60.)).overflow_hidden().child(
                div()
                    .id("sticky-border-clip-scroll")
                    .w_full()
                    .h_full()
                    .overflow_y_scroll()
                    .child(div().w(px(100.)).h(px(20.)))
                    .child(div().w(px(100.)).h(px(30.)))
                    .child(
                        div()
                            .sticky()
                            .top_0()
                            .w(px(100.))
                            .h(px(20.))
                            .bg(crate::blue())
                            .border_b_1()
                            .border_color(crate::red()),
                    )
                    .child(div().w(px(100.)).h(px(100.))),
            )
        }
    }

    struct KeyedStickyScrollOwner {
        handle: ScrollHandle,
    }

    impl Render for KeyedStickyScrollOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let sticky = |stack_id: &'static str, selector: &'static str| {
                div()
                    .sticky()
                    .sticky_stack_id(stack_id)
                    .top(px(0.))
                    .w(px(100.))
                    .h(px(20.))
                    .occlude()
                    .debug_selector(move || selector.into())
            };

            div()
                .id("keyed-sticky-scroll")
                .w(px(100.))
                .h(px(60.))
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .child(sticky("a", "keyed-a-1"))
                .child(sticky("a", "keyed-a-2"))
                .child(div().w(px(100.)).h(px(20.)))
                .child(sticky("b", "keyed-b-1"))
                .child(sticky("b", "keyed-b-2"))
                .child(div().w(px(100.)).h(px(20.)))
                .child(sticky("a", "keyed-a-3"))
                .child(div().w(px(100.)).h(px(120.)))
        }
    }

    struct HorizontalKeyedStickyScrollOwner {
        handle: ScrollHandle,
    }

    impl Render for HorizontalKeyedStickyScrollOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let sticky = |stack_id: &'static str, selector: &'static str| {
                div()
                    .sticky()
                    .sticky_stack_id(stack_id)
                    .left(px(0.))
                    .w(px(20.))
                    .h(px(100.))
                    .flex_none()
                    .occlude()
                    .debug_selector(move || selector.into())
            };

            div()
                .id("horizontal-keyed-sticky-scroll")
                .w(px(60.))
                .h(px(100.))
                .flex()
                .overflow_x_scroll()
                .track_scroll(&self.handle)
                .child(sticky("a", "horizontal-keyed-a-1"))
                .child(sticky("a", "horizontal-keyed-a-2"))
                .child(div().w(px(20.)).h(px(100.)).flex_none())
                .child(sticky("b", "horizontal-keyed-b-1"))
                .child(div().w(px(120.)).h(px(100.)).flex_none())
        }
    }

    struct StickyReplacementScrollOwner {
        handle: ScrollHandle,
        nested: bool,
        horizontal: bool,
    }

    impl Render for StickyReplacementScrollOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            if self.horizontal {
                return div()
                    .id("horizontal-sticky-replacement-scroll")
                    .w(px(60.))
                    .h(px(100.))
                    .flex()
                    .overflow_x_scroll()
                    .track_scroll(&self.handle)
                    .child(
                        div()
                            .sticky()
                            .left_0()
                            .w(px(20.))
                            .h(px(100.))
                            .flex_none()
                            .debug_selector(|| "horizontal-flat-a".into()),
                    )
                    .child(div().w(px(20.)).h(px(100.)).flex_none())
                    .child(
                        div()
                            .sticky()
                            .left_0()
                            .w(px(20.))
                            .h(px(100.))
                            .flex_none()
                            .debug_selector(|| "horizontal-flat-b".into()),
                    )
                    .child(div().w(px(120.)).h(px(100.)).flex_none());
            }

            let sticky = |inset: Pixels, selector: &'static str| {
                div()
                    .sticky()
                    .top(inset)
                    .w(px(100.))
                    .h(px(20.))
                    .debug_selector(move || selector.into())
            };
            let mut scroll = div()
                .id("sticky-replacement-scroll")
                .w(px(100.))
                .h(px(60.))
                .overflow_y_scroll()
                .track_scroll(&self.handle);
            if self.nested {
                scroll = scroll
                    .child(sticky(px(0.), "nested-parent-a"))
                    .child(sticky(px(20.), "nested-child-a"))
                    .child(div().w(px(100.)).h(px(20.)))
                    .child(sticky(px(20.), "nested-child-b"))
                    .child(div().w(px(100.)).h(px(20.)))
                    .child(sticky(px(0.), "nested-parent-b"));
            } else {
                scroll = scroll
                    .child(sticky(px(0.), "flat-a"))
                    .child(div().w(px(100.)).h(px(20.)))
                    .child(sticky(px(0.), "flat-b"));
            }
            scroll.child(div().w(px(100.)).h(px(120.)))
        }
    }

    struct StickyHierarchyZOrderOwner {
        handle: ScrollHandle,
    }

    impl Render for StickyHierarchyZOrderOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let sticky = |inset: Pixels, color: crate::Hsla| {
                div()
                    .sticky()
                    .top(inset)
                    .w(px(100.))
                    .h(px(20.))
                    .bg(color)
                    .occlude()
            };

            div()
                .id("sticky-hierarchy-z-order-scroll")
                .w(px(100.))
                .h(px(60.))
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .child(sticky(px(0.), crate::green()))
                .child(sticky(px(20.), crate::blue()))
                .child(div().w(px(100.)).h(px(20.)))
                .child(sticky(px(20.), crate::red()))
                .child(div().w(px(100.)).h(px(20.)))
                .child(sticky(px(0.), crate::yellow()))
                .child(div().w(px(100.)).h(px(120.)))
        }
    }

    struct StickyStateChangedOwner {
        handle: ScrollHandle,
        events: Rc<RefCell<Vec<StickyStateChangedEvent>>>,
    }

    impl Render for StickyStateChangedOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let events = self.events.clone();

            div()
                .id("sticky-state-scroll")
                .w(px(100.))
                .h(px(60.))
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .child(
                    div()
                        .id("sticky-state-header")
                        .sticky()
                        .top(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .on_sticky_state_changed(move |event, _, _| {
                            events.borrow_mut().push(*event);
                        }),
                )
                .child(div().w(px(100.)).h(px(160.)))
        }
    }

    struct StuckStyleOwner {
        handle: ScrollHandle,
    }

    impl Render for StuckStyleOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("stuck-style-scroll")
                .w(px(100.))
                .h(px(60.))
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .child(
                    div()
                        .sticky()
                        .top(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .bg(crate::white())
                        .when_stuck(|style| style.shadow_sm()),
                )
                .child(div().w(px(100.)).h(px(160.)))
        }
    }

    struct FixedOverNormalZOrderOwner;

    impl Render for FixedOverNormalZOrderOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("fixed-over-normal-scroll")
                .relative()
                .w(px(100.))
                .h(px(60.))
                .overflow_y_scroll()
                .child(
                    div()
                        .fixed()
                        .top(px(0.))
                        .left(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .bg(crate::green())
                        .occlude(),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .bg(crate::red())
                        .occlude(),
                )
                .child(div().w(px(100.)).h(px(120.)))
        }
    }

    struct StickyOverNormalZOrderOwner;

    impl Render for StickyOverNormalZOrderOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("sticky-over-normal-scroll")
                .relative()
                .w(px(100.))
                .h(px(60.))
                .overflow_y_scroll()
                .child(
                    div()
                        .sticky()
                        .top(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .bg(crate::green())
                        .occlude(),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .bg(crate::red())
                        .occlude(),
                )
                .child(div().w(px(100.)).h(px(120.)))
        }
    }

    /// A positioned element whose occluding descendant overlaps later normal
    /// content that blocks the mouse.
    struct PositionedDescendantOwner {
        position: Position,
    }

    impl Render for PositionedDescendantOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let positioned = div().top(px(0.)).left(px(0.)).w(px(100.)).h(px(20.));
            let positioned = match self.position {
                Position::Fixed => positioned.fixed(),
                _ => positioned.sticky(),
            };
            div()
                .id("positioned-descendant-scroll")
                .relative()
                .w(px(100.))
                .h(px(60.))
                .overflow_y_scroll()
                .child(positioned.child(div().w(px(40.)).h(px(20.)).occlude()))
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .block_mouse_except_scroll(),
                )
                .child(div().w(px(100.)).h(px(120.)))
        }
    }

    struct LaterStickyZOrderOwner {
        handle: ScrollHandle,
    }

    impl Render for LaterStickyZOrderOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("later-sticky-scroll")
                .relative()
                .w(px(100.))
                .h(px(60.))
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .child(
                    div()
                        .sticky()
                        .top(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .bg(crate::green())
                        .occlude(),
                )
                .child(div().w(px(100.)).h(px(30.)))
                .child(
                    div()
                        .sticky()
                        .top(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .bg(crate::blue())
                        .occlude(),
                )
                .child(div().w(px(100.)).h(px(120.)))
        }
    }

    struct ShiftedStickyScrollOwner {
        handle: ScrollHandle,
    }

    impl Render for ShiftedStickyScrollOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().relative().w(px(200.)).h(px(200.)).child(
                div()
                    .id("shifted-scroll")
                    .absolute()
                    .left(px(25.))
                    .top(px(50.))
                    .w(px(100.))
                    .h(px(100.))
                    .overflow_y_scroll()
                    .track_scroll(&self.handle)
                    .child(
                        div()
                            .sticky()
                            .top(px(0.))
                            .w(px(100.))
                            .h(px(20.))
                            .bg(crate::green())
                            .debug_selector(|| "shifted-sticky".into()),
                    )
                    .child(div().w(px(100.)).h(px(180.))),
            )
        }
    }

    struct FixedMaskEscapeOwner;

    impl Render for FixedMaskEscapeOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .w(px(50.))
                .h(px(50.))
                .overflow_hidden()
                .child(
                    div()
                        .absolute()
                        .left(px(80.))
                        .top(px(0.))
                        .w(px(20.))
                        .h(px(20.))
                        .bg(crate::red())
                        .occlude()
                        .debug_selector(|| "ordinary-outside".into()),
                )
                .child(
                    div()
                        .fixed()
                        .left(px(80.))
                        .top(px(30.))
                        .w(px(20.))
                        .h(px(20.))
                        .bg(crate::green())
                        .occlude()
                        .debug_selector(|| "fixed-outside".into()),
                )
        }
    }

    struct FixedInClippedParentOwner {
        handle: ScrollHandle,
    }

    impl Render for FixedInClippedParentOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().relative().w(px(260.)).h(px(180.)).child(
                div()
                    .id("fixed-in-clipped-parent-scroll")
                    .absolute()
                    .left(px(25.))
                    .top(px(30.))
                    .w(px(200.))
                    .h(px(120.))
                    .overflow_y_scroll()
                    .track_scroll(&self.handle)
                    .border_1()
                    .child(
                        div()
                            .fixed()
                            .right(px(10.))
                            .bottom(px(12.))
                            .w(px(40.))
                            .h(px(20.))
                            .bg(crate::green())
                            .occlude()
                            .debug_selector(|| "fixed-in-clipped-parent".into()),
                    )
                    .child(div().w(px(200.)).h(px(260.))),
            )
        }
    }

    struct NestedStickyOwner {
        outer_handle: ScrollHandle,
        inner_handle: ScrollHandle,
    }

    impl Render for NestedStickyOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("outer-scroll")
                .w(px(100.))
                .h(px(100.))
                .overflow_y_scroll()
                .track_scroll(&self.outer_handle)
                .child(div().w(px(100.)).h(px(30.)))
                .child(
                    div()
                        .id("inner-scroll")
                        .w(px(100.))
                        .h(px(100.))
                        .overflow_y_scroll()
                        .track_scroll(&self.inner_handle)
                        .child(
                            div()
                                .sticky()
                                .top(px(0.))
                                .w(px(100.))
                                .h(px(20.))
                                .debug_selector(|| "nested-sticky".into()),
                        )
                        .child(div().w(px(100.)).h(px(180.))),
                )
        }
    }

    struct FixedWithChildPaintOwner;

    impl Render for FixedWithChildPaintOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .fixed()
                .top(px(12.))
                .left(px(24.))
                .w(px(80.))
                .h(px(30.))
                .bg(crate::green())
                .child(
                    div()
                        .w(px(10.))
                        .h(px(10.))
                        .bg(crate::blue())
                        .debug_selector(|| "fixed-painted-child".into()),
                )
        }
    }

    struct StickyWithChildPaintOwner {
        handle: ScrollHandle,
    }

    impl Render for StickyWithChildPaintOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("sticky-child-paint-scroll")
                .w(px(100.))
                .h(px(60.))
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .child(
                    div()
                        .sticky()
                        .top(px(0.))
                        .w(px(100.))
                        .h(px(20.))
                        .bg(crate::green())
                        .child(
                            div()
                                .w(px(10.))
                                .h(px(10.))
                                .bg(crate::blue())
                                .debug_selector(|| "sticky-painted-child".into()),
                        ),
                )
                .child(div().w(px(100.)).h(px(120.)))
        }
    }

    #[test]
    fn scroll_context_does_not_change_plain_scroll_handle_state() {
        let mut test_app = TestAppContext::single();
        let handle = ScrollHandle::new();
        let window = test_app.add_window({
            let handle = handle.clone();
            move |_, _| TrackedScrollOwner { handle }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        let state = handle.0.borrow();
        assert_eq!(
            Bounds::new(Point::default(), size(px(100.), px(100.))),
            state.bounds
        );
        assert_eq!(point(px(0.), px(100.)), state.max_offset);
        assert_eq!(
            vec![Bounds::new(Point::default(), size(px(100.), px(200.)))],
            state.child_bounds
        );
    }

    #[gpux::test]
    fn fixed_div_prepaint_and_paint_use_visual_bounds(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let (_, prepaint) =
            window.draw(point(px(70.), px(80.)), size(px(300.), px(200.)), |_, _| {
                div()
                    .fixed()
                    .top(px(12.))
                    .left(px(24.))
                    .w(px(80.))
                    .h(px(30.))
                    .occlude()
                    .debug_selector(|| "fixed".into())
            });

        let expected = Bounds::new(point(px(24.), px(12.)), size(px(80.), px(30.)));
        let hitbox = prepaint.hitbox.expect("fixed div should insert a hitbox");

        assert_eq!(expected, hitbox.bounds);
        assert_eq!(Some(expected), window.debug_bounds("fixed"));
    }

    #[gpux::test]
    fn fixed_div_child_paint_uses_parent_visual_delta(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        window.draw(point(px(70.), px(80.)), size(px(300.), px(200.)), |_, _| {
            div()
                .fixed()
                .top(px(12.))
                .left(px(24.))
                .w(px(80.))
                .h(px(30.))
                .child(
                    div()
                        .w(px(10.))
                        .h(px(10.))
                        .debug_selector(|| "fixed-child".into()),
                )
        });

        assert_eq!(
            Some(Bounds::new(point(px(24.), px(12.)), size(px(10.), px(10.)))),
            window.debug_bounds("fixed-child")
        );
    }

    #[test]
    fn fixed_div_child_quad_paints_at_parent_visual_bounds() {
        let mut test_app = TestAppContext::single();
        let window = test_app.add_window(|_, _| FixedWithChildPaintOwner);
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let expected = Bounds::new(point(px(24.), px(12.)), size(px(10.), px(10.)));
                assert_eq!(
                    Some(expected),
                    window
                        .rendered_frame
                        .debug_bounds
                        .get("fixed-painted-child")
                        .copied()
                );

                let expected_scaled = expected.scale(window.scale_factor());
                assert!(
                    window
                        .rendered_frame
                        .scene
                        .quads
                        .iter()
                        .any(|quad| quad.background.solid
                            == crate::Color::from(crate::blue()).paint_color()
                            && quad.bounds == expected_scaled),
                    "fixed child quad should paint at the fixed parent visual bounds"
                );
                assert!(
                    quad_order_for_color(window, crate::blue())
                        > quad_order_for_color(window, crate::green()),
                    "fixed child content should paint above the fixed parent background"
                );
            })
            .unwrap();
    }

    #[test]
    fn fixed_div_inside_clipped_scroll_parent_uses_parent_scrollport_bounds() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        scroll_handle.request_offset(point(px(0.), px(-80.)), ScrollMutationSource::Programmatic);
        let window = test_app.add_window({
            move |_, _| FixedInClippedParentOwner {
                handle: scroll_handle,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let parent_mask = Bounds::new(point(px(25.), px(30.)), size(px(200.), px(120.)));
                let fixed_bounds = Bounds::new(point(px(175.), px(118.)), size(px(40.), px(20.)));

                assert_eq!(
                    Some(fixed_bounds),
                    window
                        .rendered_frame
                        .debug_bounds
                        .get("fixed-in-clipped-parent")
                        .copied()
                );

                let fixed_hitbox = window
                    .rendered_frame
                    .hitboxes
                    .iter()
                    .find(|hitbox| hitbox.bounds == fixed_bounds)
                    .expect("fixed child should record a hitbox inside its clipped parent");

                assert_eq!(parent_mask, fixed_hitbox.content_mask.bounds);
                assert!(
                    window
                        .rendered_frame
                        .hit_test(fixed_bounds.center())
                        .ids
                        .contains(&fixed_hitbox.id),
                    "fixed child should be hit-testable inside the clipped parent"
                );
                assert!(
                    !window
                        .rendered_frame
                        .hit_test(point(px(235.), px(128.)))
                        .ids
                        .contains(&fixed_hitbox.id),
                    "fixed child should not be hit-testable outside the clipped parent"
                );

                let expected_scaled = fixed_bounds.scale(window.scale_factor());
                let fixed_quad = window
                    .rendered_frame
                    .scene
                    .quads
                    .iter()
                    .find(|quad| {
                        quad.background.solid == crate::Color::from(crate::green()).paint_color()
                            && quad.bounds == expected_scaled
                    })
                    .expect("fixed child should paint inside its clipped parent");

                assert_eq!(
                    parent_mask.scale(window.scale_factor()),
                    fixed_quad.content_mask.bounds
                );
            })
            .unwrap();
    }

    #[test]
    fn sticky_div_paint_uses_scrollport_visual_bounds() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        scroll_handle.request_offset(point(px(0.), px(-40.)), ScrollMutationSource::Programmatic);
        let window = test_app.add_window({
            move |_, _| StickyScrollOwner {
                handle: scroll_handle,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        let sticky_bounds = test_app
            .update_window(any_window, |_, window, _| {
                window.rendered_frame.debug_bounds.get("sticky").copied()
            })
            .unwrap();

        assert_eq!(
            Some(Bounds::new(Point::default(), size(px(100.), px(20.)))),
            sticky_bounds
        );
    }

    #[test]
    fn sticky_border_is_clipped_with_its_element_at_the_scrollport_edge() {
        let mut test_app = TestAppContext::single();
        let window = test_app.add_window(|_, _| StickyBorderClipOwner);
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let red = crate::Color::from(crate::red()).paint_color();
                let scrollport_bottom = crate::ScaledPixels(60. * window.scale_factor());
                assert!(
                    window
                        .rendered_frame
                        .scene
                        .quads
                        .iter()
                        .filter(|quad| quad.border_color.solid == red)
                        .all(|quad| quad.content_mask.bounds.bottom() <= scrollport_bottom),
                    "the offscreen bottom border of a partially visible sticky element must be clipped"
                );
            })
            .unwrap();
    }

    #[test]
    fn keyed_sticky_divs_stack_and_replace_contiguous_runs() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        let window = test_app.add_window({
            let handle = scroll_handle.clone();
            move |_, _| KeyedStickyScrollOwner { handle }
        });
        let any_window = window.into();

        let draw = |test_app: &mut TestAppContext| {
            test_app
                .update_window(any_window, |_, window, cx| {
                    window.draw(cx).clear(cx);
                })
                .unwrap();
        };
        let bounds = |test_app: &mut TestAppContext, selector: &str| {
            test_app
                .update_window(any_window, |_, window, _| {
                    window.rendered_frame.debug_bounds.get(selector).copied()
                })
                .unwrap()
                .expect("keyed sticky selector is rendered")
        };

        scroll_handle.request_offset(point(px(0.), px(-20.)), ScrollMutationSource::Programmatic);
        draw(&mut test_app);
        assert_eq!(bounds(&mut test_app, "keyed-a-1").origin.y, px(0.));
        assert_eq!(bounds(&mut test_app, "keyed-a-2").origin.y, px(20.));

        scroll_handle.request_offset(point(px(0.), px(-30.)), ScrollMutationSource::Programmatic);
        draw(&mut test_app);
        assert_eq!(bounds(&mut test_app, "keyed-a-1").origin.y, px(-10.));
        assert_eq!(bounds(&mut test_app, "keyed-a-2").origin.y, px(10.));
        assert_eq!(bounds(&mut test_app, "keyed-b-1").origin.y, px(30.));

        scroll_handle.request_offset(point(px(0.), px(-50.)), ScrollMutationSource::Programmatic);
        draw(&mut test_app);
        assert_eq!(bounds(&mut test_app, "keyed-a-1").origin.y, px(-30.));
        assert_eq!(bounds(&mut test_app, "keyed-a-2").origin.y, px(-10.));
        assert_eq!(bounds(&mut test_app, "keyed-b-1").origin.y, px(10.));

        scroll_handle.request_offset(point(px(0.), px(-70.)), ScrollMutationSource::Programmatic);
        draw(&mut test_app);
        assert_eq!(bounds(&mut test_app, "keyed-b-1").origin.y, px(0.));
        assert_eq!(bounds(&mut test_app, "keyed-b-2").origin.y, px(20.));
        assert!(
            bounds(&mut test_app, "keyed-a-1").bottom() <= px(0.),
            "the replaced A run returns to its normal offscreen bounds"
        );

        scroll_handle.request_offset(point(px(0.), px(-130.)), ScrollMutationSource::Programmatic);
        draw(&mut test_app);
        assert_eq!(bounds(&mut test_app, "keyed-a-3").origin.y, px(0.));
        assert!(
            bounds(&mut test_app, "keyed-b-2").bottom() <= px(0.),
            "the final A run does not retain members of the intervening B run"
        );
    }

    #[test]
    fn flat_sticky_div_scrolls_out_as_the_next_header_pushes_it() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        let window = test_app.add_window({
            let handle = scroll_handle.clone();
            move |_, _| StickyReplacementScrollOwner {
                handle,
                nested: false,
                horizontal: false,
            }
        });
        let any_window = window.into();

        scroll_handle.request_offset(point(px(0.), px(-30.)), ScrollMutationSource::Programmatic);
        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();
        let mut bounds = |selector: &str| {
            test_app
                .update_window(any_window, |_, window, _| {
                    window.rendered_frame.debug_bounds.get(selector).copied()
                })
                .unwrap()
                .expect("flat sticky selector is rendered")
        };

        assert_eq!(bounds("flat-a").origin.y, px(-10.));
        assert_eq!(bounds("flat-b").origin.y, px(10.));
    }

    #[test]
    fn nested_sticky_div_replacements_push_only_the_replaced_suffix() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        let window = test_app.add_window({
            let handle = scroll_handle.clone();
            move |_, _| StickyReplacementScrollOwner {
                handle,
                nested: true,
                horizontal: false,
            }
        });
        let any_window = window.into();
        let draw = |test_app: &mut TestAppContext| {
            test_app
                .update_window(any_window, |_, window, cx| {
                    window.draw(cx).clear(cx);
                })
                .unwrap();
        };
        let bounds = |test_app: &mut TestAppContext, selector: &str| {
            test_app
                .update_window(any_window, |_, window, _| {
                    window.rendered_frame.debug_bounds.get(selector).copied()
                })
                .unwrap()
                .expect("nested sticky selector is rendered")
        };

        scroll_handle.request_offset(point(px(0.), px(-50.)), ScrollMutationSource::Programmatic);
        draw(&mut test_app);
        assert_eq!(bounds(&mut test_app, "nested-parent-a").origin.y, px(0.));
        assert_eq!(bounds(&mut test_app, "nested-child-a").origin.y, px(10.));
        assert_eq!(bounds(&mut test_app, "nested-child-b").origin.y, px(30.));

        scroll_handle.request_offset(point(px(0.), px(-70.)), ScrollMutationSource::Programmatic);
        draw(&mut test_app);
        assert_eq!(bounds(&mut test_app, "nested-parent-a").origin.y, px(-10.));
        assert_eq!(bounds(&mut test_app, "nested-child-b").origin.y, px(10.));
        assert_eq!(bounds(&mut test_app, "nested-parent-b").origin.y, px(30.));
    }

    #[test]
    fn pushed_sticky_child_paints_and_hit_tests_below_its_retained_parent() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        scroll_handle.request_offset(point(px(0.), px(-50.)), ScrollMutationSource::Programmatic);
        let window = test_app.add_window({
            move |_, _| StickyHierarchyZOrderOwner {
                handle: scroll_handle,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let parent_order = quad_order_for_color(window, crate::green());
                let child_order = quad_order_for_color(window, crate::blue());
                assert!(
                    parent_order > child_order,
                    "the retained parent should paint above its pushed child"
                );

                let parent_bounds = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(20.)));
                let parent_hitbox = window
                    .rendered_frame
                    .hitboxes
                    .iter()
                    .find(|hitbox| hitbox.bounds == parent_bounds)
                    .expect("retained parent should insert a hitbox");
                let top_hitbox = window
                    .rendered_frame
                    .hit_test(point(px(10.), px(15.)))
                    .ids
                    .first()
                    .copied();
                assert_eq!(
                    Some(parent_hitbox.id),
                    top_hitbox,
                    "the retained parent should hit-test above its pushed child"
                );
            })
            .unwrap();
    }

    #[test]
    fn horizontal_flat_sticky_div_scrolls_out_as_the_next_header_pushes_it() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        let window = test_app.add_window({
            let handle = scroll_handle.clone();
            move |_, _| StickyReplacementScrollOwner {
                handle,
                nested: false,
                horizontal: true,
            }
        });
        let any_window = window.into();

        scroll_handle.request_offset(point(px(-30.), px(0.)), ScrollMutationSource::Programmatic);
        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();
        let mut bounds = |selector: &str| {
            test_app
                .update_window(any_window, |_, window, _| {
                    window.rendered_frame.debug_bounds.get(selector).copied()
                })
                .unwrap()
                .expect("horizontal sticky selector is rendered")
        };

        assert_eq!(bounds("horizontal-flat-a").origin.x, px(-10.));
        assert_eq!(bounds("horizontal-flat-b").origin.x, px(10.));
    }

    #[test]
    fn horizontal_keyed_sticky_run_scrolls_out_as_the_next_run_pushes_it() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        let window = test_app.add_window({
            let handle = scroll_handle.clone();
            move |_, _| HorizontalKeyedStickyScrollOwner { handle }
        });
        let any_window = window.into();

        scroll_handle.request_offset(point(px(-30.), px(0.)), ScrollMutationSource::Programmatic);
        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();
        let mut bounds = |selector: &str| {
            test_app
                .update_window(any_window, |_, window, _| {
                    window.rendered_frame.debug_bounds.get(selector).copied()
                })
                .unwrap()
                .expect("horizontal keyed sticky selector is rendered")
        };

        assert_eq!(bounds("horizontal-keyed-a-1").origin.x, px(-10.));
        assert_eq!(bounds("horizontal-keyed-a-2").origin.x, px(10.));
        assert_eq!(bounds("horizontal-keyed-b-1").origin.x, px(30.));
    }

    #[test]
    fn sticky_state_changed_callback_fires_only_when_phase_changes() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        let events = Rc::new(RefCell::new(Vec::new()));
        let window = test_app.add_window({
            let handle = scroll_handle.clone();
            let events = events.clone();
            move |_, _| StickyStateChangedOwner { handle, events }
        });
        let any_window = window.into();

        let mut draw = |test_app: &mut TestAppContext| {
            test_app
                .update_window(any_window, |_, window, cx| {
                    window.draw(cx).clear(cx);
                })
                .unwrap();
        };

        draw(&mut test_app);
        {
            let events = events.borrow();
            assert_eq!(1, events.len());
            assert_eq!(None, events[0].previous);
            assert_eq!(StickyAxisState::Unstuck, events[0].current.y);
            assert!(!events[0].current.is_stuck_y());
        }

        draw(&mut test_app);
        assert_eq!(
            1,
            events.borrow().len(),
            "unchanged sticky phase should not fire again"
        );

        scroll_handle.request_offset(point(px(0.), px(-40.)), ScrollMutationSource::Programmatic);
        draw(&mut test_app);
        {
            let events = events.borrow();
            assert_eq!(2, events.len());
            assert_eq!(
                Some(StickyAxisState::Unstuck),
                events[1].previous.map(|previous| previous.y)
            );
            assert_eq!(StickyAxisState::StuckToStart, events[1].current.y);
            assert!(events[1].current.is_stuck_y());
        }

        scroll_handle.request_offset(point(px(0.), px(-50.)), ScrollMutationSource::Programmatic);
        draw(&mut test_app);
        assert_eq!(
            2,
            events.borrow().len(),
            "scrolling while already stuck should not fire"
        );

        scroll_handle.request_offset(Point::default(), ScrollMutationSource::Programmatic);
        draw(&mut test_app);
        {
            let events = events.borrow();
            assert_eq!(3, events.len());
            assert_eq!(
                Some(StickyAxisState::StuckToStart),
                events[2].previous.map(|previous| previous.y)
            );
            assert_eq!(StickyAxisState::Unstuck, events[2].current.y);
        }
    }

    #[test]
    fn when_stuck_applies_visual_style_only_while_stuck() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        let window = test_app.add_window({
            let handle = scroll_handle.clone();
            move |_, _| StuckStyleOwner { handle }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
                assert_eq!(0, window.rendered_frame.scene.shadows.len());
            })
            .unwrap();

        scroll_handle.request_offset(point(px(0.), px(-40.)), ScrollMutationSource::Programmatic);
        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
                assert!(
                    !window.rendered_frame.scene.shadows.is_empty(),
                    "stuck style should add a shadow when sticky header is stuck"
                );
            })
            .unwrap();
    }

    #[test]
    fn sticky_div_paints_at_shifted_scrollport_after_scroll() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        scroll_handle.request_offset(point(px(0.), px(-40.)), ScrollMutationSource::Programmatic);
        let window = test_app.add_window({
            move |_, _| ShiftedStickyScrollOwner {
                handle: scroll_handle,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        let expected = Bounds::new(point(px(25.), px(50.)), size(px(100.), px(20.)));
        let sticky_bounds = test_app
            .update_window(any_window, |_, window, _| {
                window
                    .rendered_frame
                    .debug_bounds
                    .get("shifted-sticky")
                    .copied()
            })
            .unwrap();

        assert_eq!(Some(expected), sticky_bounds);

        let sticky_quad_painted = test_app
            .update_window(any_window, |_, window, _| {
                let expected_scaled = expected.scale(window.scale_factor());
                window
                    .rendered_frame
                    .scene
                    .quads
                    .iter()
                    .any(|quad| quad.bounds == expected_scaled)
            })
            .unwrap();

        assert!(
            sticky_quad_painted,
            "sticky background quad should paint at the shifted scrollport top"
        );
    }

    #[test]
    fn sticky_div_child_quad_paints_at_parent_visual_bounds() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        scroll_handle.request_offset(point(px(0.), px(-40.)), ScrollMutationSource::Programmatic);
        let window = test_app.add_window({
            move |_, _| StickyWithChildPaintOwner {
                handle: scroll_handle,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let expected = Bounds::new(Point::default(), size(px(10.), px(10.)));
                assert_eq!(
                    Some(expected),
                    window
                        .rendered_frame
                        .debug_bounds
                        .get("sticky-painted-child")
                        .copied()
                );

                let expected_scaled = expected.scale(window.scale_factor());
                assert!(
                    window
                        .rendered_frame
                        .scene
                        .quads
                        .iter()
                        .any(|quad| quad.background.solid
                            == crate::Color::from(crate::blue()).paint_color()
                            && quad.bounds == expected_scaled),
                    "sticky child quad should paint at the sticky parent visual bounds"
                );
                assert!(
                    quad_order_for_color(window, crate::blue())
                        > quad_order_for_color(window, crate::green()),
                    "sticky child content should paint above the sticky parent background"
                );
            })
            .unwrap();
    }

    fn quad_order_for_color(window: &Window, color: impl Into<crate::Color>) -> u32 {
        let color = color.into();
        window
            .rendered_frame
            .scene
            .quads
            .iter()
            .find(|quad| quad.background.solid == color.paint_color())
            .map(|quad| quad.order)
            .expect("expected quad with requested background color")
    }

    #[test]
    fn fixed_div_paints_above_later_overlapping_normal_content() {
        let mut test_app = TestAppContext::single();
        let window = test_app.add_window(|_, _| FixedOverNormalZOrderOwner);
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let fixed_order = quad_order_for_color(window, crate::green());
                let normal_order = quad_order_for_color(window, crate::red());

                assert!(
                    fixed_order > normal_order,
                    "fixed content should paint above later normal content at the same bounds"
                );
            })
            .unwrap();
    }

    #[test]
    fn fixed_div_hit_tests_above_later_overlapping_normal_content() {
        let mut test_app = TestAppContext::single();
        let window = test_app.add_window(|_, _| FixedOverNormalZOrderOwner);
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let fixed_hitbox = window
                    .rendered_frame
                    .hitboxes
                    .iter()
                    .find(|hitbox| {
                        hitbox.bounds == Bounds::new(Point::default(), size(px(100.), px(20.)))
                    })
                    .expect("fixed child should insert a hitbox");
                let top_hitbox = window
                    .rendered_frame
                    .hit_test(point(px(10.), px(10.)))
                    .ids
                    .first()
                    .copied();

                assert_eq!(
                    Some(fixed_hitbox.id),
                    top_hitbox,
                    "fixed hitbox should be topmost over later normal content"
                );
            })
            .unwrap();
    }

    #[test]
    fn sticky_div_paints_above_later_overlapping_normal_content() {
        let mut test_app = TestAppContext::single();
        let window = test_app.add_window(|_, _| StickyOverNormalZOrderOwner);
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let sticky_order = quad_order_for_color(window, crate::green());
                let normal_order = quad_order_for_color(window, crate::red());

                assert!(
                    sticky_order > normal_order,
                    "sticky content should paint above later normal content at the same bounds"
                );
            })
            .unwrap();
    }

    #[test]
    fn positioned_hitboxes_order_sticky_and_fixed_descendants_above_later_normal_content() {
        for position in [Position::Sticky, Position::Fixed] {
            let mut test_app = TestAppContext::single();
            let window = test_app.add_window(move |_, _| PositionedDescendantOwner { position });
            let any_window = window.into();

            test_app
                .update_window(any_window, |_, window, cx| {
                    window.draw(cx).clear(cx);
                })
                .unwrap();

            test_app
                .update_window(any_window, |_, window, _| {
                    let descendant_bounds = Bounds::new(Point::default(), size(px(40.), px(20.)));
                    let descendant = window
                        .rendered_frame
                        .hitboxes
                        .iter()
                        .find(|hitbox| hitbox.bounds == descendant_bounds)
                        .expect("the positioned descendant should insert a hitbox");
                    let top_hitbox = window
                        .rendered_frame
                        .hit_test(point(px(10.), px(10.)))
                        .ids
                        .first()
                        .copied();

                    assert_eq!(
                        Some(descendant.id),
                        top_hitbox,
                        "a {position:?} descendant should hit-test where it paints, above later normal content"
                    );
                })
                .unwrap();
        }
    }

    #[test]
    fn sticky_div_hit_tests_above_later_overlapping_normal_content() {
        let mut test_app = TestAppContext::single();
        let window = test_app.add_window(|_, _| StickyOverNormalZOrderOwner);
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let expected_bounds = Bounds::new(Point::default(), size(px(100.), px(20.)));
                let sticky_hitbox = window
                    .rendered_frame
                    .hitboxes
                    .iter()
                    .find(|hitbox| hitbox.bounds == expected_bounds)
                    .expect("sticky child should insert a hitbox");
                let top_hitbox = window
                    .rendered_frame
                    .hit_test(point(px(10.), px(10.)))
                    .ids
                    .first()
                    .copied();

                assert_eq!(
                    Some(sticky_hitbox.id),
                    top_hitbox,
                    "sticky hitbox should be topmost over later normal content"
                );
            })
            .unwrap();
    }

    #[test]
    fn later_sticky_div_replaces_the_earlier_sticky_div_without_overlap() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        scroll_handle.request_offset(point(px(0.), px(-50.)), ScrollMutationSource::Programmatic);
        let window = test_app.add_window({
            move |_, _| LaterStickyZOrderOwner {
                handle: scroll_handle,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                assert!(
                    window.rendered_frame.scene.quads.iter().all(|quad| {
                        quad.background.solid != crate::Color::from(crate::green()).paint_color()
                    }),
                    "the outgoing sticky header should be fully clipped once replacement completes"
                );
                let _later_sticky_order = quad_order_for_color(window, crate::blue());
            })
            .unwrap();
    }

    #[test]
    fn later_sticky_div_replacement_removes_the_outgoing_hitbox_from_the_viewport() {
        let mut test_app = TestAppContext::single();
        let scroll_handle = ScrollHandle::new();
        scroll_handle.request_offset(point(px(0.), px(-50.)), ScrollMutationSource::Programmatic);
        let window = test_app.add_window({
            move |_, _| LaterStickyZOrderOwner {
                handle: scroll_handle,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let stuck_bounds = Bounds::new(Point::default(), size(px(100.), px(20.)));
                let stuck_hitboxes = window
                    .rendered_frame
                    .hitboxes
                    .iter()
                    .filter(|hitbox| hitbox.bounds == stuck_bounds)
                    .collect::<Vec<_>>();
                assert_eq!(
                    1,
                    stuck_hitboxes.len(),
                    "only the incoming sticky hitbox remains"
                );

                let later_sticky_hitbox = stuck_hitboxes[0];
                let top_hitbox = window
                    .rendered_frame
                    .hit_test(point(px(10.), px(10.)))
                    .ids
                    .first()
                    .copied();

                assert_eq!(
                    Some(later_sticky_hitbox.id),
                    top_hitbox,
                    "the incoming sticky hitbox should own the replaced position"
                );
            })
            .unwrap();
    }

    #[test]
    fn fixed_div_keeps_ancestor_content_mask_like_ordinary_children() {
        let mut test_app = TestAppContext::single();
        let window = test_app.add_window(|_, _| FixedMaskEscapeOwner);
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, _| {
                let parent_mask = Bounds::new(Point::default(), size(px(50.), px(50.)));
                let ordinary_bounds = Bounds::new(point(px(80.), px(0.)), size(px(20.), px(20.)));
                let fixed_bounds = Bounds::new(point(px(80.), px(30.)), size(px(20.), px(20.)));
                let ordinary_hitbox = window
                    .rendered_frame
                    .hitboxes
                    .iter()
                    .find(|hitbox| hitbox.bounds == ordinary_bounds)
                    .expect("ordinary out-of-mask child should still record its clipped hitbox");
                let fixed_hitbox = window
                    .rendered_frame
                    .hitboxes
                    .iter()
                    .find(|hitbox| hitbox.bounds == fixed_bounds)
                    .expect("fixed child should record its clipped hitbox");

                assert_eq!(parent_mask, ordinary_hitbox.content_mask.bounds);
                assert_eq!(parent_mask, fixed_hitbox.content_mask.bounds);
                assert!(
                    !window
                        .rendered_frame
                        .hit_test(point(px(90.), px(10.)))
                        .ids
                        .contains(&ordinary_hitbox.id),
                    "ordinary child should not be hit-testable outside the ancestor mask"
                );
                assert!(
                    !window
                        .rendered_frame
                        .hit_test(point(px(90.), px(40.)))
                        .ids
                        .contains(&fixed_hitbox.id),
                    "fixed child should not be hit-testable outside the ancestor mask"
                );

                let scale = window.scale_factor();
                let ordinary_scaled_bounds = Bounds::new(
                    point(px(80.).scale(scale), px(0.).scale(scale)),
                    size(px(20.).scale(scale), px(20.).scale(scale)),
                );
                let fixed_scaled_bounds = Bounds::new(
                    point(px(80.).scale(scale), px(30.).scale(scale)),
                    size(px(20.).scale(scale), px(20.).scale(scale)),
                );

                assert!(
                    !window
                        .rendered_frame
                        .scene
                        .quads
                        .iter()
                        .any(|quad| quad.bounds == ordinary_scaled_bounds),
                    "ordinary out-of-mask child should not paint outside the ancestor mask"
                );
                assert!(
                    !window
                        .rendered_frame
                        .scene
                        .quads
                        .iter()
                        .any(|quad| quad.bounds == fixed_scaled_bounds),
                    "fixed child should not paint outside the ancestor mask"
                );
            })
            .unwrap();
    }

    #[test]
    fn sticky_div_uses_nearest_nested_scroll_context() {
        let mut test_app = TestAppContext::single();
        let outer_handle = ScrollHandle::new();
        let inner_handle = ScrollHandle::new();
        inner_handle.request_offset(point(px(0.), px(-40.)), ScrollMutationSource::Programmatic);
        let window = test_app.add_window({
            move |_, _| NestedStickyOwner {
                outer_handle,
                inner_handle,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        let sticky_bounds = test_app
            .update_window(any_window, |_, window, _| {
                window
                    .rendered_frame
                    .debug_bounds
                    .get("nested-sticky")
                    .copied()
            })
            .unwrap();

        assert_eq!(
            Some(Bounds::new(point(px(0.), px(30.)), size(px(100.), px(20.)))),
            sticky_bounds
        );
    }

    struct ScrollCaptureOwner {
        events: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Render for ScrollCaptureOwner {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let capture_events = self.events.clone();
            let bubble_events = self.events.clone();
            div()
                .size(px(100.))
                .capture_scroll_wheel(move |_, _, _| {
                    capture_events.borrow_mut().push("capture");
                })
                .child(div().size_full().on_scroll_wheel(move |_, _, _| {
                    bubble_events.borrow_mut().push("bubble");
                }))
        }
    }

    #[test]
    fn capture_scroll_wheel_runs_before_the_nested_bubble_listener() {
        let mut test_app = TestAppContext::single();
        let events = Rc::new(RefCell::new(Vec::new()));
        let window = test_app.add_window({
            let events = events.clone();
            move |_, _| ScrollCaptureOwner { events }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();
        dispatch_scroll_wheel(&mut test_app, any_window, point(px(50.), px(50.)), px(-40.));

        assert_eq!(&["capture", "bubble"], events.borrow().as_slice());
    }

    #[test]
    fn nested_scroll_wheel_consumes_delta_before_parent_until_inner_edge() {
        let mut test_app = TestAppContext::single();
        let outer_handle = ScrollHandle::new();
        let inner_handle = ScrollHandle::new();
        let window = test_app.add_window({
            let outer_handle = outer_handle.clone();
            let inner_handle = inner_handle.clone();
            move |_, _| NestedStickyOwner {
                outer_handle,
                inner_handle,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        dispatch_scroll_wheel(&mut test_app, any_window, point(px(50.), px(50.)), px(-40.));
        assert_eq!(point(px(0.), px(-40.)), inner_handle.offset());
        assert_eq!(Point::default(), outer_handle.offset());

        dispatch_scroll_wheel(
            &mut test_app,
            any_window,
            point(px(50.), px(50.)),
            px(-200.),
        );
        assert_eq!(point(px(0.), px(-100.)), inner_handle.offset());
        assert_eq!(Point::default(), outer_handle.offset());

        dispatch_scroll_wheel(&mut test_app, any_window, point(px(50.), px(50.)), px(-40.));
        assert_eq!(point(px(0.), px(-100.)), inner_handle.offset());
        assert_eq!(point(px(0.), px(-30.)), outer_handle.offset());
    }

    fn dispatch_scroll_wheel(
        test_app: &mut TestAppContext,
        window: crate::AnyWindowHandle,
        position: Point<Pixels>,
        delta_y: Pixels,
    ) {
        test_app
            .update_window(window, |_, window, cx| {
                window.dispatch_event(
                    crate::ScrollWheelEvent {
                        position,
                        delta: crate::ScrollDelta::Pixels(point(px(0.), delta_y)),
                        modifiers: Default::default(),
                        touch_phase: crate::TouchPhase::Moved,
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
        test_app.run_until_parked();
    }

    #[test]
    fn scroll_handle_aligns_wide_children_to_left_edge() {
        let handle = ScrollHandle::new();
        {
            let mut state = handle.0.borrow_mut();
            state.bounds = Bounds::new(point(px(0.), px(0.)), size(px(80.), px(20.)));
            state.child_bounds = vec![Bounds::new(point(px(25.), px(0.)), size(px(200.), px(20.)))];
            state.overflow.x = Overflow::Scroll;
            state.active_item = Some(ScrollActiveItem {
                index: 0,
                strategy: ScrollStrategy::default(),
            });
        }

        assert!(handle.try_request_active_item());
        assert_eq!(
            handle.0.borrow().pending_offset,
            Some((point(px(-25.), px(0.)), ScrollMutationSource::Deferred))
        );
    }

    #[test]
    fn scroll_handle_aligns_tall_children_to_top_edge() {
        let handle = ScrollHandle::new();
        {
            let mut state = handle.0.borrow_mut();
            state.bounds = Bounds::new(point(px(0.), px(0.)), size(px(20.), px(80.)));
            state.child_bounds = vec![Bounds::new(point(px(0.), px(25.)), size(px(20.), px(200.)))];
            state.overflow.y = Overflow::Scroll;
            state.active_item = Some(ScrollActiveItem {
                index: 0,
                strategy: ScrollStrategy::default(),
            });
        }

        assert!(handle.try_request_active_item());
        assert_eq!(
            handle.0.borrow().pending_offset,
            Some((point(px(0.), px(-25.)), ScrollMutationSource::Deferred))
        );
    }

    #[test]
    fn long_press_shows_tooltip_until_touch_ends() {
        let (mut test_app, any_window, captured_active_tooltip) = create_tooltip_owner_test(None);
        let active_tooltip = captured_active_tooltip
            .borrow()
            .clone()
            .unwrap()
            .upgrade()
            .unwrap();
        let touch_id = TouchId(1);
        let touch_position = point(px(49.), px(10.));

        test_app
            .update_window(any_window, |_, window, cx| {
                window.dispatch_event(
                    TouchEvent {
                        id: touch_id,
                        phase: TouchPhase::Started,
                        position: touch_position,
                        predicted_position: None,
                        force: None,
                        timestamp: None,
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
        test_app
            .dispatcher
            .advance_clock(GestureTuning::default().long_press_duration + Duration::from_millis(1));
        test_app.run_until_parked();

        assert!(matches!(
            active_tooltip.borrow().as_ref(),
            Some(ActiveTooltip::Visible { .. })
        ));

        let moved_position = point(px(55.), px(10.));
        test_app
            .update_window(any_window, |_, window, cx| {
                window.dispatch_event(
                    TouchEvent {
                        id: touch_id,
                        phase: TouchPhase::Moved,
                        position: moved_position,
                        predicted_position: None,
                        force: None,
                        timestamp: None,
                    }
                    .to_platform_input(),
                    cx,
                );
                window.draw(cx).clear(cx);
            })
            .unwrap();
        assert!(matches!(
            active_tooltip.borrow().as_ref(),
            Some(ActiveTooltip::Visible { .. })
        ));

        test_app
            .update_window(any_window, |_, window, cx| {
                window.dispatch_event(
                    TouchEvent {
                        id: touch_id,
                        phase: TouchPhase::Ended,
                        position: moved_position,
                        predicted_position: None,
                        force: None,
                        timestamp: None,
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();

        assert!(active_tooltip.borrow().is_none());
    }

    fn setup_tooltip_owner_test(
        show_delay_override: Option<Duration>,
    ) -> (
        TestAppContext,
        crate::AnyWindowHandle,
        CapturedActiveTooltip,
    ) {
        let (mut test_app, any_window, captured_active_tooltip) =
            create_tooltip_owner_test(show_delay_override);

        test_app
            .update_window(any_window, |_, window, cx| {
                window.dispatch_event(
                    MouseMoveEvent {
                        position: point(px(10.), px(10.)),
                        modifiers: Default::default(),
                        pressed_button: None,
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        (test_app, any_window, captured_active_tooltip)
    }

    fn create_tooltip_owner_test(
        show_delay_override: Option<Duration>,
    ) -> (
        TestAppContext,
        crate::AnyWindowHandle,
        CapturedActiveTooltip,
    ) {
        let mut test_app = TestAppContext::single();
        let captured_active_tooltip: CapturedActiveTooltip = Rc::new(RefCell::new(None));
        let window = test_app.add_window({
            let captured_active_tooltip = captured_active_tooltip.clone();
            move |_, _| TooltipOwner {
                captured_active_tooltip,
                show_delay_override,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        (test_app, any_window, captured_active_tooltip)
    }

    #[test]
    fn tooltip_waiting_for_show_is_released_when_its_owner_disappears() {
        let (mut test_app, any_window, captured_active_tooltip) = setup_tooltip_owner_test(None);

        let weak_active_tooltip = captured_active_tooltip.borrow().clone().unwrap();
        let active_tooltip = weak_active_tooltip.upgrade().unwrap();
        assert!(matches!(
            active_tooltip.borrow().as_ref(),
            Some(ActiveTooltip::WaitingForShow { .. })
        ));

        test_app
            .update_window(any_window, |_, window, _| {
                window.remove_window();
            })
            .unwrap();
        test_app.run_until_parked();
        drop(active_tooltip);

        assert!(weak_active_tooltip.upgrade().is_none());
    }

    #[test]
    fn tooltip_respects_custom_show_delay() {
        let extra_delay = Duration::from_secs(1);
        let show_delay_override = DEFAULT_TOOLTIP_SHOW_DELAY + extra_delay;
        let (mut test_app, _any_window, captured_active_tooltip) =
            setup_tooltip_owner_test(Some(show_delay_override));

        let weak_active_tooltip = captured_active_tooltip.borrow().clone().unwrap();
        let active_tooltip = weak_active_tooltip.upgrade().unwrap();

        test_app
            .dispatcher
            .advance_clock(DEFAULT_TOOLTIP_SHOW_DELAY);
        test_app.run_until_parked();

        assert!(matches!(
            active_tooltip.borrow().as_ref(),
            Some(ActiveTooltip::WaitingForShow { .. })
        ));

        test_app.dispatcher.advance_clock(extra_delay);
        test_app.run_until_parked();

        assert!(matches!(
            active_tooltip.borrow().as_ref(),
            Some(ActiveTooltip::Visible { .. })
        ));
    }

    #[test]
    fn tooltip_is_released_when_its_owner_disappears() {
        let (mut test_app, any_window, captured_active_tooltip) = setup_tooltip_owner_test(None);

        let weak_active_tooltip = captured_active_tooltip.borrow().clone().unwrap();
        let active_tooltip = weak_active_tooltip.upgrade().unwrap();

        test_app
            .dispatcher
            .advance_clock(DEFAULT_TOOLTIP_SHOW_DELAY);
        test_app.run_until_parked();

        assert!(matches!(
            active_tooltip.borrow().as_ref(),
            Some(ActiveTooltip::Visible { .. })
        ));

        test_app
            .update_window(any_window, |_, window, _| {
                window.remove_window();
            })
            .unwrap();
        test_app.run_until_parked();
        drop(active_tooltip);

        assert!(weak_active_tooltip.upgrade().is_none());
    }

    #[test]
    fn tooltip_hides_after_mouse_leaves_origin() {
        let (mut test_app, any_window, captured_active_tooltip) = setup_tooltip_owner_test(None);

        let weak_active_tooltip = captured_active_tooltip.borrow().clone().unwrap();
        let active_tooltip = weak_active_tooltip.upgrade().unwrap();

        test_app
            .dispatcher
            .advance_clock(DEFAULT_TOOLTIP_SHOW_DELAY);
        test_app.run_until_parked();

        assert!(matches!(
            active_tooltip.borrow().as_ref(),
            Some(ActiveTooltip::Visible { .. })
        ));

        test_app
            .update_window(any_window, |_, window, cx| {
                window.dispatch_event(
                    MouseMoveEvent {
                        position: point(px(75.), px(75.)),
                        modifiers: Default::default(),
                        pressed_button: None,
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();

        assert!(active_tooltip.borrow().is_none());
    }

    struct MouseDownOutOwner {
        mouse_down_out_count: Rc<RefCell<usize>>,
    }

    impl Render for MouseDownOutOwner {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let mouse_down_out_count = self.mouse_down_out_count.clone();
            div()
                .size_full()
                .child(div().id("target").w(px(50.)).h(px(50.)).on_mouse_down_out(
                    move |_, _, _| {
                        *mouse_down_out_count.borrow_mut() += 1;
                    },
                ))
        }
    }

    #[test]
    fn mouse_down_out_is_suppressed_while_window_prompt_is_active() {
        let mut test_app = TestAppContext::single();
        let mouse_down_out_count = Rc::new(RefCell::new(0));
        let window = test_app.add_window({
            let mouse_down_out_count = mouse_down_out_count.clone();
            move |_, _| MouseDownOutOwner {
                mouse_down_out_count,
            }
        });
        let any_window: AnyWindowHandle = window.into();

        fn dispatch_mouse_down_outside_target(
            test_app: &mut TestAppContext,
            any_window: AnyWindowHandle,
        ) {
            test_app
                .update_window(any_window, |_, window, cx| {
                    window.dispatch_event(
                        MouseDownEvent {
                            position: point(px(75.), px(75.)),
                            button: MouseButton::Left,
                            modifiers: Default::default(),
                            click_count: 1,
                            first_mouse: false,
                        }
                        .to_platform_input(),
                        cx,
                    );
                })
                .unwrap();
        }

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        dispatch_mouse_down_outside_target(&mut test_app, any_window);
        assert_eq!(
            *mouse_down_out_count.borrow(),
            1,
            "mouse down outside the element should fire mouse-down-out listeners"
        );

        test_app
            .update_window(any_window, |_, window, cx| {
                cx.set_prompt_builder(crate::fallback_prompt_renderer);
                let _receiver =
                    window.prompt(crate::PromptLevel::Warning, "message", None, &["Ok"], cx);
                assert!(window.has_active_prompt());
                window.draw(cx).clear(cx);
            })
            .unwrap();

        dispatch_mouse_down_outside_target(&mut test_app, any_window);
        assert_eq!(
            *mouse_down_out_count.borrow(),
            1,
            "mouse down over an active prompt should not fire mouse-down-out listeners"
        );
    }

    #[test]
    fn test_accessibility_id_builder_writes_author_id() {
        let mut element = div()
            .id("buffer-font-size")
            .accessibility_id("settings.buffer-font-size");
        let mut node = accesskit::Node::new(accesskit::Role::SpinButton);

        element.interactivity().write_a11y_info(&mut node);

        assert_eq!(node.author_id(), Some("settings.buffer-font-size"));
    }

    #[test]
    fn test_write_a11y_info_string_and_numeric_properties() {
        let mut interactivity = Interactivity::default();
        interactivity.aria.author_id = Some("settings.buffer-font-size".into());
        interactivity.aria.label = Some("Buffer Font Size".into());
        interactivity.aria.value = Some("15".into());
        interactivity.aria.placeholder = Some("Search".into());
        interactivity.aria.numeric_value = Some(15.0);
        interactivity.aria.min_numeric_value = Some(6.0);
        interactivity.aria.max_numeric_value = Some(72.0);
        interactivity.aria.numeric_value_step = Some(1.0);

        let mut node = accesskit::Node::new(accesskit::Role::SpinButton);
        interactivity.write_a11y_info(&mut node);

        assert_eq!(node.author_id(), Some("settings.buffer-font-size"));
        assert_eq!(node.label(), Some("Buffer Font Size"));
        assert_eq!(node.value(), Some("15"));
        assert_eq!(node.placeholder(), Some("Search"));
        assert_eq!(node.numeric_value(), Some(15.0));
        assert_eq!(node.min_numeric_value(), Some(6.0));
        assert_eq!(node.max_numeric_value(), Some(72.0));
        assert_eq!(node.numeric_value_step(), Some(1.0));
    }

    /// Two focusable, clickable elements ("a" and "b") used to exercise the
    /// Enter/Space -> synthesized click press/release pairing.
    struct KeyboardActivationTest {
        focus_a: FocusHandle,
        focus_b: FocusHandle,
        clicks: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Render for KeyboardActivationTest {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let clicks_a = self.clicks.clone();
            let clicks_b = self.clicks.clone();
            div()
                .size_full()
                .child(
                    div()
                        .id("a")
                        .w(px(50.))
                        .h(px(50.))
                        .track_focus(&self.focus_a)
                        .on_click(move |_, _, _| clicks_a.borrow_mut().push("a")),
                )
                .child(
                    div()
                        .id("b")
                        .w(px(50.))
                        .h(px(50.))
                        .track_focus(&self.focus_b)
                        .on_click(move |_, _, _| clicks_b.borrow_mut().push("b")),
                )
        }
    }

    fn setup_keyboard_activation_test() -> (
        TestAppContext,
        AnyWindowHandle,
        Rc<RefCell<Vec<&'static str>>>,
        FocusHandle,
        FocusHandle,
    ) {
        let mut cx = TestAppContext::single();
        let (focus_a, focus_b) = cx.update(|cx| (cx.focus_handle(), cx.focus_handle()));
        let clicks: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_window({
            let focus_a = focus_a.clone();
            let focus_b = focus_b.clone();
            let clicks = clicks.clone();
            move |_, _| KeyboardActivationTest {
                focus_a,
                focus_b,
                clicks,
            }
        });
        (cx, window.into(), clicks, focus_a, focus_b)
    }

    /// Move focus to `handle`, flush effects, then paint so the newly focused
    /// element registers its key handlers for the next dispatched event.
    fn focus_and_draw(cx: &mut TestAppContext, window: AnyWindowHandle, handle: &FocusHandle) {
        cx.update_window(window, |_, window, cx| window.focus(handle, cx))
            .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear(cx);
        })
        .unwrap();
    }

    fn key_down(cx: &mut TestAppContext, window: AnyWindowHandle, key: &str) {
        let keystroke = Keystroke::parse(key).unwrap();
        cx.update_window(window, |_, window, cx| {
            window.dispatch_event(
                KeyDownEvent {
                    keystroke,
                    is_held: false,
                    prefer_character_input: false,
                }
                .to_platform_input(),
                cx,
            );
        })
        .unwrap();
    }

    fn key_up(cx: &mut TestAppContext, window: AnyWindowHandle, key: &str) {
        let keystroke = Keystroke::parse(key).unwrap();
        cx.update_window(window, |_, window, cx| {
            window.dispatch_event(KeyUpEvent { keystroke }.to_platform_input(), cx);
        })
        .unwrap();
    }

    /// Pressing and releasing Enter on the same focused element fires a click.
    #[test]
    fn keyboard_activation_fires_click_on_same_element() {
        let (mut cx, window, clicks, focus_a, _focus_b) = setup_keyboard_activation_test();

        focus_and_draw(&mut cx, window, &focus_a);
        key_down(&mut cx, window, "enter");
        key_up(&mut cx, window, "enter");

        assert_eq!(*clicks.borrow(), vec!["a"]);
    }

    /// A key-down whose key-up lands on a *different* element (because focus
    /// moved in between) must not leak a synthesized click onto the newly
    /// focused element. This is the core regression: previously the key-up
    /// handler fired unconditionally on whatever was focused at key-up time.
    #[test]
    fn keyboard_activation_does_not_leak_across_focus_change() {
        let (mut cx, window, clicks, focus_a, focus_b) = setup_keyboard_activation_test();

        // Enter pressed while "a" is focused...
        focus_and_draw(&mut cx, window, &focus_a);
        key_down(&mut cx, window, "enter");

        // ...focus moves to "b" before the release (as a confirm action would)...
        focus_and_draw(&mut cx, window, &focus_b);
        key_up(&mut cx, window, "enter");

        // ...so neither element is clicked: "a" never saw the up, and "b"
        // never saw the down.
        assert!(clicks.borrow().is_empty(), "clicks: {:?}", clicks.borrow());
    }

    /// A keydown whose flag is left pending because focus moved away before
    /// the keyup must not fire a click when focus later *returns* to the same
    /// element (the menu trigger reopening case). The stamped focus generation
    /// no longer matches, so the stale pending state is ignored.
    #[test]
    fn keyboard_activation_does_not_leak_when_focus_returns() {
        let (mut cx, window, clicks, focus_a, focus_b) = setup_keyboard_activation_test();

        // Enter pressed on "a"...
        focus_and_draw(&mut cx, window, &focus_a);
        key_down(&mut cx, window, "enter");

        // ...focus leaves "a" before its keyup (so the pending state is never
        // consumed), then comes back to "a"...
        focus_and_draw(&mut cx, window, &focus_b);
        focus_and_draw(&mut cx, window, &focus_a);
        key_up(&mut cx, window, "enter");

        // ...and the now-stale pending keydown must not fire a click.
        assert!(clicks.borrow().is_empty(), "clicks: {:?}", clicks.borrow());
    }

    /// A non-activation key *released* during the press must cancel the pending
    /// activation. For the sequence escape-down, space-down, escape-up,
    /// space-up the space forms a clean down/up pair, but the intervening
    /// escape-up means this isn't a plain space activation, so no click fires.
    #[test]
    fn keyboard_activation_cleared_by_intervening_key_release() {
        let (mut cx, window, clicks, focus_a, _focus_b) = setup_keyboard_activation_test();

        focus_and_draw(&mut cx, window, &focus_a);
        key_down(&mut cx, window, "escape");
        key_down(&mut cx, window, "space");
        key_up(&mut cx, window, "escape");
        key_up(&mut cx, window, "space");

        assert!(clicks.borrow().is_empty(), "clicks: {:?}", clicks.borrow());
    }

    /// The flag is a single activation marker, not keyed by which activation
    /// key was used, so a Space down paired with an Enter up on the same
    /// element still fires a click.
    #[test]
    fn keyboard_activation_does_not_distinguish_space_and_enter() {
        let (mut cx, window, clicks, focus_a, _focus_b) = setup_keyboard_activation_test();

        focus_and_draw(&mut cx, window, &focus_a);
        key_down(&mut cx, window, "space");
        key_up(&mut cx, window, "enter");

        assert_eq!(*clicks.borrow(), vec!["a"]);
    }

    /// A non-activation key pressed between the activation down and up clears
    /// the pending flag, suppressing the click.
    #[test]
    fn keyboard_activation_cleared_by_intervening_keydown() {
        let (mut cx, window, clicks, focus_a, _focus_b) = setup_keyboard_activation_test();

        focus_and_draw(&mut cx, window, &focus_a);
        key_down(&mut cx, window, "enter");
        key_down(&mut cx, window, "a");
        key_up(&mut cx, window, "enter");

        assert!(clicks.borrow().is_empty(), "clicks: {:?}", clicks.borrow());
    }

    /// A modified Enter (e.g. cmd-enter) is not treated as an activation key,
    /// so it neither sets the pending flag nor fires a click on release.
    #[test]
    fn keyboard_activation_ignores_modified_keys() {
        let (mut cx, window, clicks, focus_a, _focus_b) = setup_keyboard_activation_test();

        focus_and_draw(&mut cx, window, &focus_a);
        key_down(&mut cx, window, "cmd-enter");
        key_up(&mut cx, window, "cmd-enter");

        assert!(clicks.borrow().is_empty(), "clicks: {:?}", clicks.borrow());
    }

    /// Two sibling tab groups, each a focusable container that is *not* itself a
    /// tab stop and holds a single tab stop. Mirrors how the title bar and
    /// status bar expose their controls as ARIA toolbars.
    struct TabGroupFocus {
        group_a: FocusHandle,
        item_a: FocusHandle,
        group_b: FocusHandle,
        item_b: FocusHandle,
    }

    impl Render for TabGroupFocus {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            fn group(container: &FocusHandle, item: &FocusHandle) -> Div {
                div()
                    .track_focus(container)
                    .tab_group()
                    .child(div().track_focus(item))
            }
            div()
                .child(group(&self.group_a, &self.item_a))
                .child(group(&self.group_b, &self.item_b))
        }
    }

    /// Focusing a tab-group container and pressing Tab (`focus_next`) must move
    /// focus to the first tab stop *inside that container*, as documented on
    /// [`InteractiveElement::tab_stop`].
    #[test]
    fn focus_next_from_tab_group_container_enters_that_group() {
        let mut cx = TestAppContext::single();
        let (group_a, item_a, group_b, item_b) = cx.update(|cx| {
            (
                cx.focus_handle(),
                cx.focus_handle().tab_stop(true),
                cx.focus_handle(),
                cx.focus_handle().tab_stop(true),
            )
        });
        let window: AnyWindowHandle = cx
            .add_window({
                let (group_a, item_a, group_b, item_b) =
                    (group_a, item_a, group_b.clone(), item_b.clone());
                move |_, _| TabGroupFocus {
                    group_a,
                    item_a,
                    group_b,
                    item_b,
                }
            })
            .into();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();

        // Focus the *second* group's container, then advance like Tab would.
        let focused = cx
            .update_window(window, |_, window, cx| {
                window.focus(&group_b, cx);
                window.focus_next(cx);
                window.focused(cx).map(|handle| handle.id)
            })
            .unwrap();

        assert_eq!(focused, Some(item_b.id));
    }

    #[gpux::test]
    fn test_fractional_padding_does_not_make_a_fitting_container_scrollable(
        cx: &mut TestAppContext,
    ) {
        struct PaddedContainer {
            scroll_handle: ScrollHandle,
        }

        impl Render for PaddedContainer {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                // 4.25px of padding snaps to 4px in layout, so a 42px child
                // fits the 50px box exactly.
                div().size_full().child(
                    div()
                        .id("container")
                        .h(px(50.))
                        .w(px(100.))
                        .py(px(4.25))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll_handle)
                        .child(div().w_full().h(px(42.))),
                )
            }
        }

        let scroll_handle = ScrollHandle::new();
        let window: AnyWindowHandle = cx
            .add_window({
                let scroll_handle = scroll_handle.clone();
                move |_, _| PaddedContainer { scroll_handle }
            })
            .into();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();

        assert_eq!(scroll_handle.max_offset().y, px(0.));
    }

    struct ContentSizedGrid;

    impl Render for ContentSizedGrid {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let widths = [px(100.), px(200.), px(50.)];
            div().size_full().child(
                div()
                    .w_full()
                    .grid()
                    .grid_cols_max_content(widths.len() as u16)
                    .children(widths.into_iter().enumerate().map(|(index, width)| {
                        div()
                            .debug_selector(move || format!("cell-{index}"))
                            .w(width)
                            .h(px(10.))
                    })),
            )
        }
    }

    #[gpux::test]
    fn grid_cols_max_content_sizes_columns_to_their_content(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| ContentSizedGrid);
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();

        let mut bounds = |selector: &'static str| {
            cx.update_window(window.into(), |_, window, _| {
                window.rendered_frame.debug_bounds.get(selector).copied()
            })
            .unwrap()
            .unwrap_or_else(|| panic!("{selector} was not rendered"))
        };

        assert_eq!(bounds("cell-0").origin.x, px(0.));
        assert_eq!(bounds("cell-1").origin.x, px(100.));
        assert_eq!(bounds("cell-2").origin.x, px(300.));
    }
}
