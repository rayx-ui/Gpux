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
    ScrollActivity, ScrollMetrics, ScrollMutationSource, direction_for_delta,
};
use crate::{
    Action, ActionDispatchOutcome, AnyDrag, AnyElement, AnyTooltip, AnyView, App, Bounds,
    ClickEvent, DispatchPhase, Display, DragAxis, Element, ElementId, Entity, EntityId,
    FocusHandle, Global, GlobalElementId, Hitbox, HitboxBehavior, HitboxId, InputEvent,
    InspectorElementId, IntoElement, IsZero, KeyContext, KeyDownEvent, KeyUpEvent, KeyboardButton,
    KeyboardClickEvent, LayoutId, ModifiersChangedEvent, MouseButton, Overflow, ParentElement,
    Pixels, Point, PointerClickEvent, PointerEvent, Position, PositionedBounds, Render,
    ScrollContainerContext, ScrollDelta, ScrollWheelEvent, SharedString, Size, StickyState,
    StickyStateChangedEvent, Style, StyleRefinement, Styled, Task, TooltipId, Visibility, Window,
    WindowControlArea, point, px, size,
};
use crate::{PinchEvent, RotateEvent};
use collections::HashMap;
use gpui_util::ResultExt;
use refineable::Refineable;
use smallvec::SmallVec;
use stacksafe::{StackSafe, stacksafe};
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    cmp::Ordering,
    fmt::Debug,
    marker::PhantomData,
    mem,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use super::ImageCacheProvider;

const DRAG_THRESHOLD: f64 = 2.;
const TOOLTIP_SHOW_DELAY: Duration = Duration::from_millis(500);
const HOVERABLE_TOOLTIP_HIDE_DELAY: Duration = Duration::from_millis(500);

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
    pub event: PointerEvent,

    /// The bounds of this element.
    pub bounds: Bounds<Pixels>,
    drag: PhantomData<T>,
    dragged_item: Arc<dyn Any>,
}

/// An event for when a compatible drag is released over this element.
pub struct DropEvent<T> {
    /// The pointer event that completed the drag.
    pub event: PointerEvent,
    dragged_item: Arc<dyn Any>,
    drag: PhantomData<T>,
}

impl<T: 'static> DropEvent<T> {
    /// Returns the typed value carried by the drag.
    pub fn drag(&self) -> &T {
        self.dragged_item
            .downcast_ref::<T>()
            .expect("DropEvent is only valid when the stored active drag is of the same type.")
    }
}

impl<T: 'static> DragMoveEvent<T> {
    /// Returns the drag state for this event.
    pub fn drag(&self, _: &App) -> &T {
        self.dragged_item
            .downcast_ref::<T>()
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
    /// The imperative API equivalent of [`InteractiveElement::on_pointer_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to the view state from this callback.
    pub fn on_pointer_down(
        &mut self,
        button: MouseButton,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if event.phase == crate::PointerPhase::Down
                    && phase == DispatchPhase::Bubble
                    && event.button == button
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse down event for any button, during the capture phase.
    /// The imperative API equivalent of [`InteractiveElement::capture_any_pointer_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_any_pointer_down(
        &mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if event.phase == crate::PointerPhase::Down
                    && phase == DispatchPhase::Capture
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse down event for any button, during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_any_pointer_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_any_pointer_down(
        &mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if event.phase == crate::PointerPhase::Down
                    && phase == DispatchPhase::Bubble
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to a pointer event that carries contact facts.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_pointer_contact(
        &mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_contact_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if event.contact != Default::default()
                    && phase == DispatchPhase::Bubble
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to a pointer event that carries contact facts.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_pointer_contact(
        &mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_contact_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if event.contact != Default::default()
                    && phase == DispatchPhase::Capture
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event for the given button, during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_pointer_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_pointer_up(
        &mut self,
        button: MouseButton,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if event.phase == crate::PointerPhase::Up
                    && phase == DispatchPhase::Bubble
                    && event.button == button
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event for any button, during the capture phase.
    /// The imperative API equivalent to [`InteractiveElement::capture_any_pointer_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_any_pointer_up(
        &mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if event.phase == crate::PointerPhase::Up
                    && phase == DispatchPhase::Capture
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event for any button, during the bubble phase.
    /// The imperative API equivalent to [`Interactivity::on_any_pointer_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_any_pointer_up(
        &mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if event.phase == crate::PointerPhase::Up
                    && phase == DispatchPhase::Bubble
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse down event, on any button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The imperative API equivalent to [`InteractiveElement::on_pointer_down_out`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_pointer_down_out(
        &mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if event.phase == crate::PointerPhase::Down
                    && phase == DispatchPhase::Capture
                    && !hitbox.contains(&window.mouse_position())
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event, for the given button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The imperative API equivalent to [`InteractiveElement::on_pointer_up_out`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_pointer_up_out(
        &mut self,
        button: MouseButton,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if event.phase == crate::PointerPhase::Up
                    && phase == DispatchPhase::Capture
                    && event.button == button
                    && !hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to the mouse move event, during the bubble phase.
    /// The imperative API equivalent to [`InteractiveElement::on_pointer_move`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_pointer_move(
        &mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_move_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if matches!(
                    event.phase,
                    crate::PointerPhase::Move | crate::PointerPhase::Hover
                ) && phase == DispatchPhase::Bubble
                    && hitbox.is_hovered(window)
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
        self.pointer_move_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                let dragged_item = (phase == DispatchPhase::Capture)
                    .then(|| window.active_drag_for(event.id))
                    .flatten()
                    .filter(|drag| drag.value.as_ref().type_id() == TypeId::of::<T>())
                    .map(|drag| Arc::clone(&drag.value));
                if let Some(dragged_item) = dragged_item {
                    (listener)(
                        &DragMoveEvent {
                            event: event.clone(),
                            bounds: hitbox.bounds,
                            drag: PhantomData,
                            dragged_item,
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

    /// Bind the given callback to rotate gesture events during the bubble phase.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_rotate(&mut self, listener: impl Fn(&RotateEvent, &mut Window, &mut App) + 'static) {
        self.rotate_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to rotate gesture events during the capture phase.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_rotate(
        &mut self,
        listener: impl Fn(&RotateEvent, &mut Window, &mut App) + 'static,
    ) {
        self.rotate_listeners
            .push(Box::new(move |event, phase, _hitbox, window, cx| {
                if phase == DispatchPhase::Capture {
                    (listener)(event, window, cx);
                } else {
                    cx.propagate();
                }
            }));
    }

    /// Bind the given callback to pointer events during the bubble phase.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_pointer(
        &mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to pointer events during the capture phase.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_pointer(
        &mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pointer_listeners
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
        self.modifiers_changed_listeners
            .push(Box::new(move |event, window, cx| {
                listener(event, window, cx)
            }));
    }

    /// Bind the given callback to drop events of the given type, whether or not the drag started on this element.
    /// The imperative API equivalent to [`InteractiveElement::on_drop`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_drop<T: 'static>(&mut self, listener: impl Fn(&T, &mut Window, &mut App) + 'static) {
        self.drop_listeners.push((
            TypeId::of::<T>(),
            Box::new(move |_, dragged_value, window, cx| {
                listener(dragged_value.downcast_ref().unwrap(), window, cx);
                true
            }),
        ));
    }

    /// Bind a typed drop callback that may reject a compatible drag without consuming it.
    ///
    /// Returning `false` allows an ancestor drop target to evaluate the same pointer sequence.
    pub fn on_drop_with_event<T: 'static>(
        &mut self,
        listener: impl Fn(&DropEvent<T>, &mut Window, &mut App) -> bool + 'static,
    ) {
        self.drop_listeners.push((
            TypeId::of::<T>(),
            Box::new(move |event, dragged_value, window, cx| {
                listener(
                    &DropEvent {
                        event: event.clone(),
                        dragged_item: Arc::clone(dragged_value),
                        drag: PhantomData,
                    },
                    window,
                    cx,
                )
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
    pub fn on_click(&mut self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static)
    where
        Self: Sized,
    {
        self.click_listeners.push(Rc::new(move |event, window, cx| {
            listener(event, window, cx)
        }));
    }

    /// Bind the given callback to non-primary click events of this element.
    /// The imperative API equivalent to [`StatefulInteractiveElement::on_aux_click`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_aux_click(&mut self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static)
    where
        Self: Sized,
    {
        self.aux_click_listeners
            .push(Rc::new(move |event, window, cx| {
                listener(event, window, cx)
            }));
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
        self.drag_listener = Some((
            Arc::new(value),
            Box::new(move |value, offset, window, cx| DragVisual {
                view: constructor(value.downcast_ref().unwrap(), offset, window, cx).into(),
                on_end: None,
            }),
        ));
    }

    /// Starts a drag with a source-owned terminal callback.
    pub fn on_drag_with_end<T, W>(
        &mut self,
        value: T,
        constructor: impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W> + 'static,
        on_end: crate::DragEndCallback,
    ) where
        Self: Sized,
        T: 'static,
        W: 'static + Render,
    {
        debug_assert!(
            self.drag_listener.is_none(),
            "calling on_drag more than once on the same element is not supported"
        );
        self.drag_listener = Some((
            Arc::new(value),
            Box::new(move |value, offset, window, cx| DragVisual {
                view: constructor(value.downcast_ref().unwrap(), offset, window, cx).into(),
                on_end: Some(on_end.clone()),
            }),
        ));
    }

    /// Restricts feedback movement for a drag started by this element.
    pub fn drag_axis(&mut self, axis: Option<DragAxis>) {
        self.drag_axis = axis;
    }

    /// Places drag feedback directly under the pointer instead of preserving child offset.
    pub fn drag_anchor_at_pointer(&mut self, enabled: bool) {
        self.drag_anchor_at_pointer = enabled;
    }

    /// Bind the given callback on the hover start and end events of this element. Note that the boolean
    /// passed to the callback is true when the hover starts and false when it ends.
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

    /// Use the given callback to construct a new tooltip view when the mouse hovers over this element.
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

    /// Use the given callback to construct a new tooltip view when the mouse hovers over this element.
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

    fn has_rotate_listeners(&self) -> bool {
        !self.rotate_listeners.is_empty()
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
    /// The fluent API equivalent to [`Interactivity::on_pointer_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to the view state from this callback.
    fn on_pointer_down(
        mut self,
        button: MouseButton,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_pointer_down(button, listener);
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Set a key that can be used to look up this element's bounds
    /// in the [`crate::VisualTestContext::debug_bounds`] map
    /// This is a noop in release builds
    fn debug_selector(mut self, f: impl FnOnce() -> String) -> Self {
        self.interactivity().debug_selector = Some(f());
        self
    }

    #[cfg(not(any(test, feature = "test-support")))]
    /// Set a key that can be used to look up this element's bounds
    /// in the [`crate::VisualTestContext::debug_bounds`] map
    /// This is a noop in release builds
    #[inline]
    fn debug_selector(self, _: impl FnOnce() -> String) -> Self {
        self
    }

    /// Bind the given callback to the mouse down event for any button, during the capture phase.
    /// The fluent API equivalent to [`Interactivity::capture_any_pointer_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_any_pointer_down(
        mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_any_pointer_down(listener);
        self
    }

    /// Bind the given callback to the mouse down event for any button, during the capture phase.
    /// The fluent API equivalent to [`Interactivity::on_any_pointer_down`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_any_pointer_down(
        mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_any_pointer_down(listener);
        self
    }

    /// Bind the given callback to the mouse up event for the given button, during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_pointer_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_pointer_up(
        mut self,
        button: MouseButton,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_pointer_up(button, listener);
        self
    }

    /// Bind the given callback to the mouse up event for any button, during the capture phase.
    /// The fluent API equivalent to [`Interactivity::capture_any_pointer_up`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_any_pointer_up(
        mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_any_pointer_up(listener);
        self
    }

    /// Bind the given callback to a pointer event that carries contact facts.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_pointer_contact(
        mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_pointer_contact(listener);
        self
    }

    /// Bind the given callback to a pointer event that carries contact facts.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_pointer_contact(
        mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_pointer_contact(listener);
        self
    }

    /// Bind the given callback to the mouse down event, on any button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The fluent API equivalent to [`Interactivity::on_pointer_down_out`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_pointer_down_out(
        mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_pointer_down_out(listener);
        self
    }

    /// Bind the given callback to the mouse up event, for the given button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The fluent API equivalent to [`Interactivity::on_pointer_up_out`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_pointer_up_out(
        mut self,
        button: MouseButton,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_pointer_up_out(button, listener);
        self
    }

    /// Bind the given callback to the mouse move event, during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_pointer_move`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_pointer_move(
        mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_pointer_move(listener);
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

    /// Bind the given callback to rotate gesture events during the bubble phase.
    /// The fluent API equivalent to [`Interactivity::on_rotate`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_rotate(
        mut self,
        listener: impl Fn(&RotateEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_rotate(listener);
        self
    }

    /// Bind the given callback to rotate gesture events during the capture phase.
    /// The fluent API equivalent to [`Interactivity::capture_rotate`].
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_rotate(
        mut self,
        listener: impl Fn(&RotateEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_rotate(listener);
        self
    }

    /// Bind the given callback to pointer events during the bubble phase.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_pointer(
        mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_pointer(listener);
        self
    }

    /// Bind the given callback to pointer events during the capture phase.
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_pointer(
        mut self,
        listener: impl Fn(&PointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_pointer(listener);
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

    /// Bind a typed drop callback that may reject a compatible drag without consuming it.
    /// The fluent API equivalent to [`Interactivity::on_drop_with_event`].
    fn on_drop_with_event<T: 'static>(
        mut self,
        listener: impl Fn(&DropEvent<T>, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.interactivity().on_drop_with_event(listener);
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

    /// Starts a drag with a source-owned terminal callback.
    fn on_drag_with_end<T, W>(
        mut self,
        value: T,
        constructor: impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W> + 'static,
        on_end: crate::DragEndCallback,
    ) -> Self
    where
        Self: Sized,
        T: 'static,
        W: 'static + Render,
    {
        self.interactivity()
            .on_drag_with_end(value, constructor, on_end);
        self
    }

    /// Restricts feedback movement for a drag started by this element.
    fn drag_axis(mut self, axis: Option<DragAxis>) -> Self {
        self.interactivity().drag_axis(axis);
        self
    }

    /// Places drag feedback directly under the pointer.
    fn drag_anchor_at_pointer(mut self, enabled: bool) -> Self {
        self.interactivity().drag_anchor_at_pointer(enabled);
        self
    }

    /// Bind the given callback on the hover start and end events of this element. Note that the boolean
    /// passed to the callback is true when the hover starts and false when it ends.
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

    /// Use the given callback to construct a new tooltip view when the mouse hovers over this element.
    /// The fluent API equivalent to [`Interactivity::tooltip`].
    fn tooltip(mut self, build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self
    where
        Self: Sized,
    {
        self.interactivity().tooltip(build_tooltip);
        self
    }

    /// Use the given callback to construct a new tooltip view when the mouse hovers over this element.
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
}

pub(crate) type PointerDownListener =
    Box<dyn Fn(&PointerEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;
pub(crate) type PointerUpListener =
    Box<dyn Fn(&PointerEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;
pub(crate) type PointerContactListener =
    Box<dyn Fn(&PointerEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;
pub(crate) type PointerMoveListener =
    Box<dyn Fn(&PointerEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type ScrollWheelListener =
    Box<dyn Fn(&ScrollWheelEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type PinchListener =
    Box<dyn Fn(&PinchEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type RotateListener =
    Box<dyn Fn(&RotateEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type PointerListener =
    Box<dyn Fn(&PointerEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type ClickListener = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub(crate) struct DragVisual {
    pub view: AnyView,
    pub on_end: Option<crate::DragEndCallback>,
}

pub(crate) type DragListener =
    Box<dyn Fn(&dyn Any, Point<Pixels>, &mut Window, &mut App) -> DragVisual + 'static>;

pub(crate) type StickyStateChangedListener =
    Box<dyn Fn(&StickyStateChangedEvent, &mut Window, &mut App) + 'static>;

type DropListener =
    Box<dyn Fn(&PointerEvent, &Arc<dyn Any>, &mut Window, &mut App) -> bool + 'static>;

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
        self.children
            .extend(elements.into_iter().map(StackSafe::new))
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

    #[stacksafe]
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
            self.interactivity.request_layout(
                global_id,
                inspector_id,
                window,
                cx,
                |style, window, cx| {
                    sticky_occurrence = window.reserve_sticky_candidate(&style);
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
                        window.request_layout(style, child_layout_ids.iter().copied(), cx)
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

    #[stacksafe]
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

                    if let Some(scroll_context) = scroll_context {
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

                    if let Some(listener) = self.prepaint_listener.as_ref() {
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

    #[stacksafe]
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
                        for idx in _request_layout.paint_order.iter().copied() {
                            if let Some(child) = self.children.get_mut(idx) {
                                child.paint(window, cx);
                            }
                        }
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
    pub hovered: Option<bool>,
    pub(crate) tooltip_id: Option<TooltipId>,
    pub(crate) content_size: Size<Pixels>,
    pub(crate) key_context: Option<KeyContext>,
    pub(crate) focusable: bool,
    pub(crate) tracked_focus_handle: Option<FocusHandle>,
    pub(crate) tracked_scroll_handle: Option<ScrollHandle>,
    pub(crate) scroll_anchor: Option<ScrollAnchor>,
    pub(crate) scroll_offset: Option<Rc<RefCell<Point<Pixels>>>>,
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
    pub(crate) pointer_down_listeners: Vec<PointerDownListener>,
    pub(crate) pointer_up_listeners: Vec<PointerUpListener>,
    pub(crate) pointer_contact_listeners: Vec<PointerContactListener>,
    pub(crate) pointer_move_listeners: Vec<PointerMoveListener>,
    pub(crate) scroll_wheel_listeners: Vec<ScrollWheelListener>,
    pub(crate) pinch_listeners: Vec<PinchListener>,
    pub(crate) rotate_listeners: Vec<RotateListener>,
    pub(crate) pointer_listeners: Vec<PointerListener>,
    pub(crate) key_down_listeners: Vec<KeyDownListener>,
    pub(crate) key_up_listeners: Vec<KeyUpListener>,
    pub(crate) modifiers_changed_listeners: Vec<ModifiersChangedListener>,
    pub(crate) action_listeners: Vec<(TypeId, ActionListener)>,
    pub(crate) action_outcome_listeners: Vec<(TypeId, ActionOutcomeListener)>,
    pub(crate) drop_listeners: Vec<(TypeId, DropListener)>,
    pub(crate) can_drop_predicate: Option<CanDropPredicate>,
    pub(crate) click_listeners: Vec<ClickListener>,
    pub(crate) aux_click_listeners: Vec<ClickListener>,
    pub(crate) drag_listener: Option<(Arc<dyn Any>, DragListener)>,
    pub(crate) drag_axis: Option<DragAxis>,
    pub(crate) drag_anchor_at_pointer: bool,
    pub(crate) hover_listener: Option<Box<dyn Fn(&bool, &mut Window, &mut App)>>,
    pub(crate) sticky_state_changed_listener: Option<StickyStateChangedListener>,
    pub(crate) tooltip_builder: Option<TooltipBuilder>,
    pub(crate) window_control: Option<WindowControlArea>,
    pub(crate) hitbox_behavior: HitboxBehavior,
    pub(crate) tab_index: Option<isize>,
    pub(crate) tab_group: bool,
    pub(crate) tab_stop: bool,

    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) source_location: Option<&'static core::panic::Location<'static>>,

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) debug_selector: Option<String>,

    pub(crate) current_sticky_state: Option<StickyState>,
    pub(crate) sticky_occurrence: Option<usize>,
}

impl Interactivity {
    /// Layout this element according to this interactivity state's configured styles
    pub fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(Style, &mut Window, &mut App) -> LayoutId,
    ) -> LayoutId {
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
                    && window.has_active_drags()
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
                        self.scroll_offset = Some(state.offset.clone());
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
                }

                let style =
                    self.compute_style_internal(None, element_state.as_mut(), None, window, cx);
                let layout_id = f(style, window, cx);
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
        self.content_size = content_size;

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
        }
        window.with_optional_element_state::<InteractiveElementState, _>(
            global_id,
            |element_state, window| {
                let mut element_state =
                    element_state.map(|element_state| element_state.unwrap_or_default());
                let style =
                    self.compute_style_internal(None, element_state.as_mut(), None, window, cx);
                let (positioned_bounds, sticky_state) = window
                    .resolve_positioned_bounds_and_sticky_state(
                        &style,
                        bounds,
                        self.sticky_occurrence,
                    );
                self.current_sticky_state = sticky_state;
                self.update_sticky_state(element_state.as_mut(), sticky_state, window, cx);
                let style = self.compute_style_internal(
                    None,
                    element_state.as_mut(),
                    sticky_state.as_ref(),
                    window,
                    cx,
                );
                let bounds = positioned_bounds.visual_bounds;

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
                            element_state.active_tooltip.take();
                        }
                    }
                }

                let prepaint = |window: &mut Window| {
                    window.with_text_style(style.text_style().cloned(), |window| {
                        window.with_content_mask(
                            style.overflow_mask(bounds, window.rem_size()),
                            |window| {
                                let hitbox = if self.should_insert_hitbox(&style, window, cx) {
                                    Some(match style.position {
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
                                (result, element_state)
                            },
                        )
                    })
                };

                prepaint(window)
            },
        )
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
            || !self.pointer_up_listeners.is_empty()
            || !self.pointer_contact_listeners.is_empty()
            || !self.pointer_down_listeners.is_empty()
            || !self.pointer_move_listeners.is_empty()
            || !self.click_listeners.is_empty()
            || !self.aux_click_listeners.is_empty()
            || !self.scroll_wheel_listeners.is_empty()
            || self.has_pinch_listeners()
            || self.has_rotate_listeners()
            || !self.pointer_listeners.is_empty()
            || self.drag_listener.is_some()
            || !self.drop_listeners.is_empty()
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
            if let Some(scroll_handle) = self.tracked_scroll_handle.as_ref()
                && let Ok(mut state) = scroll_handle.0.try_borrow_mut()
            {
                state.overflow = style.overflow;
                scroll_to_bottom = mem::take(&mut state.scroll_to_bottom);
                pending_offset = state.pending_offset.take();
            }

            let scroll_max = Self::scroll_max_for_bounds(
                self.content_size,
                bounds.size,
                style,
                window.rem_size(),
            );
            let mut candidate = pending_offset
                .map(|(offset, _)| offset)
                .unwrap_or_else(|| *scroll_offset.borrow());
            if scroll_to_bottom {
                candidate.y = -scroll_max.y;
            }
            if let Some(scroll_handle) = self.tracked_scroll_handle.as_ref() {
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

    fn scroll_max_for_bounds(
        content_size: Size<Pixels>,
        bounds_size: Size<Pixels>,
        style: &Style,
        rem_size: Pixels,
    ) -> Point<Pixels> {
        fn round_to_two_decimals(pixels: Pixels) -> Pixels {
            const ROUNDING_FACTOR: f32 = 100.0;
            (pixels * ROUNDING_FACTOR).round() / ROUNDING_FACTOR
        }

        let padding = style.padding.to_pixels(bounds_size.into(), rem_size);
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
        self.hovered = hitbox.map(|hitbox| hitbox.is_hovered(window));
        window.with_optional_element_state::<InteractiveElementState, _>(
            global_id,
            |element_state, window| {
                let mut element_state =
                    element_state.map(|element_state| element_state.unwrap_or_default());

                let style = self.compute_style_internal(
                    hitbox,
                    element_state.as_mut(),
                    self.current_sticky_state.as_ref(),
                    window,
                    cx,
                );

                #[cfg(any(feature = "test-support", test))]
                if let Some(debug_selector) = &self.debug_selector {
                    window
                        .next_frame
                        .debug_bounds
                        .insert(debug_selector.clone(), bounds);
                }

                self.paint_hover_group_handler(window, cx);

                if style.visibility == Visibility::Hidden {
                    return ((), element_state);
                }

                let mut tab_group = None;
                if self.tab_group {
                    tab_group = self.tab_index;
                }
                if let Some(focus_handle) = &self.tracked_focus_handle {
                    window.next_frame.tab_stops.insert(focus_handle);
                }

                let paint = |window: &mut Window| {
                    window.with_element_opacity(style.opacity, |window| {
                        style.paint(bounds, window, cx, |window: &mut Window, cx: &mut App| {
                            window.with_text_style(style.text_style().cloned(), |window| {
                                window.with_content_mask(
                                    style.overflow_mask(bounds, window.rem_size()),
                                    |window| {
                                        window.with_tab_group(tab_group, |window| {
                                            if let Some(hitbox) = hitbox {
                                                #[cfg(debug_assertions)]
                                                self.paint_debug_info(
                                                    global_id, hitbox, &style, window, cx,
                                                );

                                                if let Some(drag) =
                                                    window.active_drag_for(crate::PointerId::new(0))
                                                {
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
                                                self.paint_pointer_listeners(
                                                    hitbox,
                                                    element_state.as_mut(),
                                                    window,
                                                    cx,
                                                );
                                            }

                                            self.paint_keyboard_listeners(window, cx);
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
                                    window.refresh();
                                }
                            }
                        });

                        let was_hovered = hitbox.is_hovered(window);
                        let current_view = window.current_view();
                        window.on_pointer_event({
                            let hitbox = hitbox.clone();
                            move |_: &PointerEvent, phase, window, cx| {
                                if phase == DispatchPhase::Capture {
                                    let hovered = hitbox.is_hovered(window);
                                    if hovered != was_hovered {
                                        cx.notify(current_view)
                                    }
                                }
                            }
                        });

                        window.on_pointer_event({
                            let hitbox = hitbox.clone();
                            move |e: &crate::PointerEvent, phase, window, cx| {
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

    fn paint_pointer_listeners(
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
            window.on_pointer_event(move |event: &PointerEvent, phase, window, cx| {
                if event.phase == crate::PointerPhase::Down
                    && phase == DispatchPhase::Bubble
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

        for listener in self.pointer_down_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_pointer_event(move |event: &PointerEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.pointer_up_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_pointer_event(move |event: &PointerEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.pointer_contact_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_pointer_event(move |event: &PointerEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.pointer_move_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_pointer_event(move |event: &PointerEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.scroll_wheel_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_pointer_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.pinch_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_pointer_event(move |event: &PinchEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.rotate_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_pointer_event(move |event: &RotateEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.pointer_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_pointer_event(move |event: &PointerEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        if self.hover_style.is_some()
            || self.base_style.mouse_cursor.is_some()
            || window.has_active_drags() && !self.drag_over_styles.is_empty()
        {
            let hitbox = hitbox.clone();
            let hover_state = self.hover_style.as_ref().and_then(|_| {
                element_state
                    .as_ref()
                    .and_then(|state| state.hover_state.as_ref())
                    .cloned()
            });
            let current_view = window.current_view();

            window.on_pointer_event(move |_: &PointerEvent, phase, window, cx| {
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

                window.on_pointer_event(move |_: &PointerEvent, phase, window, cx| {
                    let group_hovered = group_hitbox_id.is_hovered(window);
                    let was_group_hovered = hover_state
                        .as_ref()
                        .is_some_and(|state| state.borrow().group);
                    if phase == DispatchPhase::Capture && group_hovered != was_group_hovered {
                        if let Some(hover_state) = &hover_state {
                            hover_state.borrow_mut().group = group_hovered;
                        }
                        cx.notify(current_view);
                    }
                });
            }
        }

        let drag_cursor_style = self.base_style.as_ref().mouse_cursor;

        let drag_listener = mem::take(&mut self.drag_listener);
        let drag_axis = self.drag_axis;
        let drag_anchor_at_pointer = self.drag_anchor_at_pointer;
        let drop_listeners = mem::take(&mut self.drop_listeners);
        let click_listeners = mem::take(&mut self.click_listeners);
        let aux_click_listeners = mem::take(&mut self.aux_click_listeners);
        let can_drop_predicate = mem::take(&mut self.can_drop_predicate);

        if !drop_listeners.is_empty() {
            let hitbox = hitbox.clone();
            window.on_pointer_event({
                move |event: &PointerEvent, phase, window, cx| {
                    if event.phase == crate::PointerPhase::Up
                        && phase == DispatchPhase::Bubble
                        && hitbox.bounds.contains(&event.position)
                    {
                        let drag_state_type = window
                            .active_drag_for(event.id)
                            .map(|drag| drag.value.as_ref().type_id());
                        if let Some(drag_state_type) = drag_state_type {
                            for (drop_state_type, listener) in &drop_listeners {
                                if *drop_state_type != drag_state_type {
                                    continue;
                                }

                                let dragged_value = window
                                    .active_drag_for(event.id)
                                    .map(|drag| Arc::clone(&drag.value))
                                    .expect("checked for active drag state above");
                                let can_drop =
                                    can_drop_predicate.as_ref().is_none_or(|predicate| {
                                        predicate(dragged_value.as_ref(), window, cx)
                                    });
                                let accepted =
                                    can_drop && listener(event, &dragged_value, window, cx);

                                if accepted {
                                    window.finish_drag(event.id, true, event.position, cx);
                                    window.refresh();
                                    cx.stop_propagation();
                                    break;
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

                let clicked_state = element_state
                    .clicked_state
                    .get_or_insert_with(Default::default)
                    .clone();

                window.on_pointer_event({
                    let pending_mouse_down = pending_mouse_down.clone();
                    let hitbox = hitbox.clone();
                    let has_aux_click_listeners = !aux_click_listeners.is_empty();
                    move |event: &PointerEvent, phase, window, _cx| {
                        if event.phase == crate::PointerPhase::Down
                            && phase == DispatchPhase::Bubble
                            && (event.button == MouseButton::Left || has_aux_click_listeners)
                            && hitbox.is_hovered(window)
                        {
                            *pending_mouse_down.borrow_mut() = Some(event.clone());
                            window.refresh();
                        }
                    }
                });

                window.on_pointer_event({
                    let pending_mouse_down = pending_mouse_down.clone();
                    let hitbox = hitbox.clone();
                    move |event: &PointerEvent, phase, window, cx| {
                        if !matches!(
                            event.phase,
                            crate::PointerPhase::Move | crate::PointerPhase::Hover
                        ) || phase == DispatchPhase::Capture
                        {
                            return;
                        }

                        let mut pending_mouse_down = pending_mouse_down.borrow_mut();
                        if let Some(mouse_down) = pending_mouse_down.clone()
                            && (event.position - mouse_down.position).magnitude() > DRAG_THRESHOLD
                            && let Some((drag_value, drag_listener)) = drag_listener.as_ref()
                            && mouse_down.button == MouseButton::Left
                        {
                            *clicked_state.borrow_mut() = ElementClickedState::default();
                            let cursor_offset = if drag_anchor_at_pointer {
                                point(px(0.), px(0.))
                            } else {
                                event.position - hitbox.origin
                            };
                            let drag =
                                (drag_listener)(drag_value.as_ref(), cursor_offset, window, cx);
                            window.start_drag(
                                event.id,
                                AnyDrag {
                                    view: drag.view,
                                    value: Arc::clone(drag_value),
                                    cursor_offset,
                                    position: event.position,
                                    axis: drag_axis,
                                    on_end: drag.on_end,
                                    cursor_style: drag_cursor_style,
                                },
                            );
                            pending_mouse_down.take();
                            window.refresh();
                            cx.stop_propagation();
                        }
                    }
                });

                if is_focused {
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
                                    let click_event = ClickEvent::Keyboard(KeyboardClickEvent {
                                        button,
                                        bounds: hitbox.bounds,
                                    });

                                    for listener in &click_listeners {
                                        listener(&click_event, window, cx);
                                    }
                                }
                            }
                        }
                    });
                }

                window.on_pointer_event({
                    let mut captured_mouse_down = None;
                    let hitbox = hitbox.clone();
                    move |event: &PointerEvent, phase, window, cx| match phase {
                        _ if event.phase != crate::PointerPhase::Up => {}
                        // Clear the pending mouse down during the capture phase,
                        // so that it happens even if another event handler stops
                        // propagation.
                        DispatchPhase::Capture => {
                            let mut pending_mouse_down = pending_mouse_down.borrow_mut();
                            if pending_mouse_down.is_some() && hitbox.is_hovered(window) {
                                captured_mouse_down = pending_mouse_down.take();
                                window.refresh();
                            } else if pending_mouse_down.is_some() {
                                // Clear the pending mouse down event (without firing click handlers)
                                // if the hitbox is not being hovered.
                                // This avoids dragging elements that changed their position
                                // immediately after being clicked.
                                // See https://github.com/zed-industries/zed/issues/24600 for more details
                                pending_mouse_down.take();
                                window.refresh();
                            }
                        }
                        // Fire click handlers during the bubble phase.
                        DispatchPhase::Bubble => {
                            if let Some(mouse_down) = captured_mouse_down.take() {
                                let btn = mouse_down.button;

                                let mouse_click = ClickEvent::Pointer(PointerClickEvent {
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
                let hitbox = hitbox.clone();
                let was_hovered = element_state
                    .hover_listener_state
                    .get_or_insert_with(Default::default)
                    .clone();
                let has_mouse_down = element_state
                    .pending_mouse_down
                    .get_or_insert_with(Default::default)
                    .clone();

                window.on_pointer_event(move |_: &PointerEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let is_hovered = has_mouse_down.borrow().is_none()
                        && !window.has_active_drags()
                        && hitbox.is_hovered(window);
                    let mut was_hovered = was_hovered.borrow_mut();

                    if is_hovered != *was_hovered {
                        *was_hovered = is_hovered;
                        drop(was_hovered);

                        hover_listener(&is_hovered, window, cx);
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
                    window,
                );
            }

            // We unconditionally bind both the mouse up and mouse down active state handlers
            // Because we might not get a chance to render a frame before the mouse up event arrives.
            let active_state = element_state
                .clicked_state
                .get_or_insert_with(Default::default)
                .clone();

            {
                let active_state = active_state.clone();
                window.on_pointer_event(move |_: &PointerEvent, phase, window, _cx| {
                    if phase == DispatchPhase::Capture && active_state.borrow().is_clicked() {
                        *active_state.borrow_mut() = ElementClickedState::default();
                        window.refresh();
                    }
                });
            }

            {
                let active_group_hitbox = self
                    .group_active_style
                    .as_ref()
                    .and_then(|group_active| GroupHitboxes::get(&group_active.group, cx));
                let hitbox = hitbox.clone();
                window.on_pointer_event(move |_: &PointerEvent, phase, window, _cx| {
                    if phase == DispatchPhase::Bubble && !window.default_prevented() {
                        let group_hovered = active_group_hitbox
                            .is_some_and(|group_hitbox_id| group_hitbox_id.is_hovered(window));
                        let element_hovered = hitbox.is_hovered(window);
                        if group_hovered || element_hovered {
                            *active_state.borrow_mut() = ElementClickedState {
                                group: group_hovered,
                                element: element_hovered,
                            };
                            window.refresh();
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
            window.on_pointer_event(move |_: &PointerEvent, phase, window, cx| {
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
            let overflow = style.overflow;
            let allow_concurrent_scroll = style.allow_concurrent_scroll;
            let restrict_scroll_to_axis = style.restrict_scroll_to_axis;
            let scroll_max = Self::scroll_max_for_bounds(
                self.content_size,
                hitbox.bounds.size,
                style,
                window.rem_size(),
            );
            let line_height = window.line_height();
            let hitbox = hitbox.clone();
            let current_view = window.current_view();
            window.on_pointer_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                let owns_direct_touch = event.scroll_source == crate::ScrollWheelSource::Touch
                    && event.scroll_phase == crate::ScrollWheelPhase::Ended
                    && activity.borrow().has_direct_touch();
                if phase == DispatchPhase::Bubble
                    && (hitbox.should_handle_scroll(window) || owns_direct_touch)
                {
                    match event.scroll_source {
                        crate::ScrollWheelSource::Touch
                            if event.scroll_phase == crate::ScrollWheelPhase::Started =>
                        {
                            activity.borrow_mut().begin_direct_touch(
                                event.timestamp.unwrap_or_else(crate::Instant::now),
                            );
                            cx.stop_propagation();
                            return;
                        }
                        crate::ScrollWheelSource::Touch
                            if event.scroll_phase == crate::ScrollWheelPhase::Cancelled =>
                        {
                            activity.borrow_mut().stop();
                        }
                        crate::ScrollWheelSource::Touch
                            if event.scroll_phase == crate::ScrollWheelPhase::Moved
                                && !activity.borrow().has_direct_touch() =>
                        {
                            activity.borrow_mut().begin_direct_touch(
                                event.timestamp.unwrap_or_else(crate::Instant::now),
                            );
                        }
                        crate::ScrollWheelSource::Touch => {}
                        _ => activity.borrow_mut().stop(),
                    }
                    let old_scroll_offset = tracked_scroll_handle
                        .as_ref()
                        .map(ScrollHandle::offset)
                        .unwrap_or_else(|| *scroll_offset_state.borrow());
                    let delta = event.delta.pixel_delta(line_height);
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

                    let advanced = new_scroll_offset != old_scroll_offset;
                    let residual = normalize_scroll_residual(
                        point(delta_x, delta_y) - (new_scroll_offset - old_scroll_offset),
                    );
                    if advanced {
                        if let Some(scroll_handle) = tracked_scroll_handle.as_ref() {
                            let source = if event.scroll_source == crate::ScrollWheelSource::Touch {
                                ScrollMutationSource::Touch
                            } else {
                                ScrollMutationSource::Wheel
                            };
                            scroll_handle.publish_offset(
                                new_scroll_offset,
                                scroll_max,
                                hitbox.bounds,
                                source,
                                current_view,
                                cx,
                            );
                        } else {
                            *scroll_offset_state.borrow_mut() = new_scroll_offset;
                            cx.notify(current_view);
                        }
                    }
                    if !residual.x.is_zero() || !residual.y.is_zero() {
                        if owns_direct_touch {
                            activity.borrow_mut().stop();
                        }
                        cx.stop_propagation();
                        if advanced && event.scroll_source != crate::ScrollWheelSource::Touch {
                            return;
                        }
                        let mut residual_event = event.clone();
                        residual_event.delta = ScrollDelta::Pixels(residual);
                        window.dispatch_event(residual_event.to_platform_input(), cx);
                        return;
                    }

                    if advanced {
                        cx.stop_propagation();
                    }

                    let release_velocity = (event.scroll_source == crate::ScrollWheelSource::Touch
                        && event.scroll_phase == crate::ScrollWheelPhase::Ended)
                        .then_some(event.release_velocity)
                        .flatten();

                    if let Some(velocity) = release_velocity {
                        activity.borrow_mut().start_at(
                            velocity,
                            event.timestamp.unwrap_or_else(crate::Instant::now),
                        );
                        schedule_scroll_activity(
                            activity.clone(),
                            scroll_offset_state.clone(),
                            tracked_scroll_handle.clone(),
                            overflow,
                            scroll_max,
                            hitbox.bounds,
                            event.position,
                            current_view,
                            window,
                        );
                        window.refresh();
                    }

                    if owns_direct_touch {
                        cx.stop_propagation();
                    }
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

        if !window.has_active_drags() {
            if let Some(group_hover) = self.group_hover_style.as_ref() {
                let is_group_hovered =
                    if let Some(group_hitbox_id) = GroupHitboxes::get(&group_hover.group, cx) {
                        group_hitbox_id.is_hovered(window)
                    } else if let Some(element_state) = element_state.as_ref() {
                        element_state
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
                    hitbox.is_hovered(window)
                } else if let Some(element_state) = element_state.as_ref() {
                    element_state
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
            if let Some(drag) = window.take_active_drag(crate::PointerId::new(0)) {
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
                window.start_drag(crate::PointerId::new(0), drag);
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
}

/// The per-frame state of an interactive element. Used for tracking stateful interactions like clicks
/// and scroll offsets.
#[derive(Default)]
pub struct InteractiveElementState {
    pub(crate) focus_handle: Option<FocusHandle>,
    pub(crate) clicked_state: Option<Rc<RefCell<ElementClickedState>>>,
    pub(crate) hover_state: Option<Rc<RefCell<ElementHoverState>>>,
    pub(crate) hover_listener_state: Option<Rc<RefCell<bool>>>,
    pub(crate) pending_mouse_down: Option<Rc<RefCell<Option<PointerEvent>>>>,
    pub(crate) scroll_offset: Option<Rc<RefCell<Point<Pixels>>>>,
    pub(crate) active_tooltip: Option<Rc<RefCell<Option<ActiveTooltip>>>>,
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

/// Shared tooltip lifecycle state used by interactive elements.
pub(crate) enum ActiveTooltip {
    /// Currently delaying before showing the tooltip.
    WaitingForShow {
        /// The delayed show task.
        _task: Task<()>,
    },
    /// Tooltip is visible, element was hovered or for hoverable tooltips, the tooltip was hovered.
    Visible {
        /// The tooltip currently shown on the window.
        tooltip: AnyTooltip,
        /// Whether the tooltip itself may keep the hover session alive.
        is_hoverable: bool,
    },
    /// Tooltip is visible and hoverable, but the mouse is no longer hovering. Currently delaying
    /// before hiding it.
    WaitingForHide {
        /// The tooltip currently shown on the window.
        tooltip: AnyTooltip,
        /// The delayed hide task.
        _task: Task<()>,
    },
}

/// Clears any tooltip state for the supplied interactive element.
pub(crate) fn clear_active_tooltip(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    window: &mut Window,
) {
    match active_tooltip.borrow_mut().take() {
        None => {}
        Some(ActiveTooltip::WaitingForShow { .. }) => {}
        Some(ActiveTooltip::Visible { .. }) => window.refresh(),
        Some(ActiveTooltip::WaitingForHide { .. }) => window.refresh(),
    }
}

/// Clears tooltip state when the currently visible tooltip is not hoverable.
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

/// Re-applies the current tooltip to the window and returns its tooltip id when visible.
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

/// Registers the shared tooltip mouse handlers used by GPUI interactive elements.
pub(crate) fn register_tooltip_mouse_handlers(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    tooltip_id: Option<TooltipId>,
    build_tooltip: Rc<dyn Fn(&mut Window, &mut App) -> Option<(AnyView, bool)>>,
    check_is_hovered: Rc<dyn Fn(&Window) -> bool>,
    check_is_hovered_during_prepaint: Rc<dyn Fn(&Window) -> bool>,
    window: &mut Window,
) {
    window.on_pointer_event({
        let active_tooltip = active_tooltip.clone();
        let build_tooltip = build_tooltip.clone();
        let check_is_hovered = check_is_hovered.clone();
        move |_: &PointerEvent, phase, window, cx| {
            handle_tooltip_mouse_move(
                &active_tooltip,
                &build_tooltip,
                &check_is_hovered,
                &check_is_hovered_during_prepaint,
                phase,
                window,
                cx,
            )
        }
    });

    window.on_pointer_event({
        let active_tooltip = active_tooltip.clone();
        move |_: &PointerEvent, _phase, window: &mut Window, _cx| {
            if !tooltip_id.is_some_and(|tooltip_id| tooltip_id.is_hovered(window)) {
                clear_active_tooltip_if_not_hoverable(&active_tooltip, window);
            }
        }
    });

    window.on_pointer_event({
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
    phase: DispatchPhase,
    window: &mut Window,
    cx: &mut App,
) {
    // Separates logic for what mutation should occur from applying it, to avoid overlapping
    // RefCell borrows.
    enum Action {
        None,
        CancelShow,
        ScheduleShow,
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
        // These are handled in check_visible_and_update.
        Some(ActiveTooltip::Visible { .. }) | Some(ActiveTooltip::WaitingForHide { .. }) => {
            Action::None
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
                    cx.background_executor().timer(TOOLTIP_SHOW_DELAY).await;
                    let Some(active_tooltip) = weak_active_tooltip.upgrade() else {
                        return;
                    };
                    cx.update(|window, cx| {
                        let new_tooltip =
                            build_tooltip(window, cx).map(|(view, tooltip_is_hoverable)| {
                                let weak_active_tooltip = Rc::downgrade(&active_tooltip);
                                ActiveTooltip::Visible {
                                    tooltip: AnyTooltip {
                                        view,
                                        mouse_position: window.mouse_position(),
                                        check_visible_and_update: Rc::new(
                                            move |tooltip_bounds, window, cx| {
                                                let Some(active_tooltip) =
                                                    weak_active_tooltip.upgrade()
                                                else {
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
                                            },
                                        ),
                                    },
                                    is_hoverable: tooltip_is_hoverable,
                                }
                            });
                        *active_tooltip.borrow_mut() = new_tooltip;
                        window.refresh();
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
    }
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
/// Contrary to [ScrollHandle::try_scroll_to_active_item], an anchored element does not have to be an immediate child of the parent.
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
struct ScrollHandleState {
    offset: Rc<RefCell<Point<Pixels>>>,
    bounds: Bounds<Pixels>,
    max_offset: Point<Pixels>,
    child_bounds: Vec<Bounds<Pixels>>,
    scroll_to_bottom: bool,
    overflow: Point<Overflow>,
    active_item: Option<ScrollActiveItem>,
    dependent_views: SmallVec<[EntityId; 2]>,
    activity: Rc<RefCell<ScrollActivity>>,
    metrics: ScrollMetrics,
    pending_offset: Option<(Point<Pixels>, ScrollMutationSource)>,
}

#[derive(Default, Debug, Clone, Copy)]
struct ScrollActiveItem {
    index: usize,
    strategy: ScrollStrategy,
}

#[derive(Default, Debug, Clone, Copy)]
enum ScrollStrategy {
    #[default]
    FirstVisible,
    Top,
}

/// A handle to the scrollable aspects of an element.
/// Used for accessing scroll state, like the current scroll offset,
/// and for mutating the scroll state, like scrolling to a specific child.
#[derive(Clone, Debug)]
pub struct ScrollHandle(Rc<RefCell<ScrollHandleState>>);

impl Default for ScrollHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollHandle {
    /// Construct a new scroll handle.
    pub fn new() -> Self {
        Self(Rc::default())
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
        self.0.borrow().metrics
    }

    /// Try to return the latest metrics snapshot without panicking on a re-entrant borrow.
    pub fn try_metrics(&self) -> Option<ScrollMetrics> {
        self.0.try_borrow().ok().map(|state| state.metrics)
    }

    /// Get the current scroll offset.
    pub fn offset(&self) -> Point<Pixels> {
        *self.0.borrow().offset.borrow()
    }

    /// Try to get the current scroll offset without panicking if the handle is already borrowed.
    pub fn try_offset(&self) -> Option<Point<Pixels>> {
        self.0.try_borrow().ok().map(|state| *state.offset.borrow())
    }

    /// Get the maximum scroll offset.
    pub fn max_offset(&self) -> Point<Pixels> {
        self.0.borrow().max_offset
    }

    /// Try to get the maximum scroll offset without panicking if the handle is already borrowed.
    pub fn try_max_offset(&self) -> Option<Point<Pixels>> {
        self.0.try_borrow().ok().map(|state| state.max_offset)
    }

    /// Get the top child that's scrolled into view.
    pub fn top_item(&self) -> usize {
        let state = self.0.borrow();
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
        self.0.borrow().bounds
    }

    /// Try to get the viewport bounds without panicking if the handle is already borrowed.
    pub fn try_bounds(&self) -> Option<Bounds<Pixels>> {
        self.0.try_borrow().ok().map(|state| state.bounds)
    }

    /// Get the bounds for a specific child.
    pub fn bounds_for_item(&self, ix: usize) -> Option<Bounds<Pixels>> {
        self.0.borrow().child_bounds.get(ix).cloned()
    }

    /// Update [ScrollHandleState]'s active item for scrolling to in prepaint
    pub fn scroll_to_item(&self, ix: usize) {
        let mut state = self.0.borrow_mut();
        state.active_item = Some(ScrollActiveItem {
            index: ix,
            strategy: ScrollStrategy::default(),
        });
    }

    /// Update [ScrollHandleState]'s active item for scrolling to in prepaint
    /// This scrolls the minimal amount to ensure that the child is the first visible element
    pub fn scroll_to_top_of_item(&self, ix: usize) {
        let mut state = self.0.borrow_mut();
        state.active_item = Some(ScrollActiveItem {
            index: ix,
            strategy: ScrollStrategy::Top,
        });
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
        state.pending_offset = Some((position, source));
        true
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
                ScrollMutationSource::Wheel
                | ScrollMutationSource::Scrollbar
                | ScrollMutationSource::Deferred => state.activity.borrow_mut().stop(),
                ScrollMutationSource::Programmatic => {
                    state.activity.borrow_mut().begin_programmatic()
                }
                ScrollMutationSource::Touch
                | ScrollMutationSource::Momentum
                | ScrollMutationSource::LayoutClamp => {}
            }
            let activity = state.activity.borrow().kind();
            let geometry_changed = state.metrics.offset != logical_offset
                || state.metrics.max_offset != max_offset
                || state.metrics.viewport_size != viewport_size
                || state.metrics.content_size != content_size;
            if !geometry_changed {
                return false;
            }

            *offset = next;
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

        if changed {
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

fn schedule_scroll_activity(
    activity: Rc<RefCell<ScrollActivity>>,
    scroll_offset: Rc<RefCell<Point<Pixels>>>,
    scroll_handle: Option<ScrollHandle>,
    overflow: Point<Overflow>,
    scroll_max: Point<Pixels>,
    bounds: Bounds<Pixels>,
    position: Point<Pixels>,
    current_view: EntityId,
    window: &Window,
) {
    window.on_next_frame(move |window, cx| {
        let mut activity_state = activity.borrow_mut();
        let Some(delta) = activity_state.next_delta_at(crate::Instant::now()) else {
            return;
        };
        let velocity = activity_state.velocity();
        drop(activity_state);

        if delta.x.is_zero() && delta.y.is_zero() && velocity.is_some() {
            schedule_scroll_activity(
                activity,
                scroll_offset,
                scroll_handle,
                overflow,
                scroll_max,
                bounds,
                position,
                current_view,
                window,
            );
            window.refresh();
            return;
        }

        let previous = scroll_handle
            .as_ref()
            .map(ScrollHandle::offset)
            .unwrap_or_else(|| *scroll_offset.borrow());
        let mut next = previous;
        if overflow.x == Overflow::Scroll {
            next.x = (next.x + delta.x).clamp(-scroll_max.x, px(0.));
        }
        if overflow.y == Overflow::Scroll {
            next.y = (next.y + delta.y).clamp(-scroll_max.y, px(0.));
        }
        let residual = normalize_scroll_residual(delta - (next - previous));
        let advanced = next != previous;

        if !residual.x.is_zero() || !residual.y.is_zero() {
            activity.borrow_mut().stop();
            window.dispatch_event(
                ScrollWheelEvent {
                    position,
                    delta: ScrollDelta::Pixels(residual),
                    modifiers: Default::default(),
                    touch_phase: crate::TouchPhase::Ended,
                    scroll_source: crate::ScrollWheelSource::Touch,
                    scroll_phase: crate::ScrollWheelPhase::Ended,
                    precise_delta: true,
                    native_momentum_phase: None,
                    release_velocity: velocity,
                    timestamp: Some(crate::Instant::now()),
                }
                .to_platform_input(),
                cx,
            );
            return;
        }

        if !advanced {
            activity.borrow_mut().stop();
            return;
        }

        if let Some(scroll_handle) = scroll_handle.as_ref() {
            scroll_handle.publish_offset(
                next,
                scroll_max,
                bounds,
                ScrollMutationSource::Momentum,
                current_view,
                cx,
            );
        } else {
            *scroll_offset.borrow_mut() = next;
            cx.notify(current_view);
        }
        schedule_scroll_activity(
            activity,
            scroll_offset,
            scroll_handle,
            overflow,
            scroll_max,
            bounds,
            position,
            current_view,
            window,
        );
        window.refresh();
    });
}

fn normalize_scroll_residual(residual: Point<Pixels>) -> Point<Pixels> {
    point(
        if residual.x.abs() < px(0.01) {
            Pixels::ZERO
        } else {
            residual.x
        },
        if residual.y.abs() < px(0.01) {
            Pixels::ZERO
        } else {
            residual.y
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppContext as _, Context, InputEvent, PointerEvent, ScrollActivityKind, ScrollDirection,
        StickyAxisState, TestAppContext,
    };
    use std::{cell::Cell, rc::Weak};

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
            app.update_window(window, |_, window, cx| window.draw(cx).clear())
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
            app.update_window(any_window, |_, window, cx| window.draw(cx).clear())
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
                ScrollMutationSource::Touch,
                ScrollActivityKind::Direct,
                ScrollActivityKind::Direct,
            ),
            (
                ScrollMutationSource::Momentum,
                ScrollActivityKind::Ballistic,
                ScrollActivityKind::Ballistic,
            ),
            (
                ScrollMutationSource::Scrollbar,
                ScrollActivityKind::Idle,
                ScrollActivityKind::Ballistic,
            ),
            (
                ScrollMutationSource::Programmatic,
                ScrollActivityKind::Programmatic,
                ScrollActivityKind::Ballistic,
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
            app.update_window(any_window, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            let baseline_revision = handle.metrics().revision;

            let activity = handle.0.borrow().activity.clone();
            match prior_activity {
                ScrollActivityKind::Idle => activity.borrow_mut().stop(),
                ScrollActivityKind::Direct => activity
                    .borrow_mut()
                    .begin_direct_touch(crate::Instant::now()),
                ScrollActivityKind::Ballistic => activity
                    .borrow_mut()
                    .start_at(point(px(0.0), px(-600.0)), crate::Instant::now()),
                ScrollActivityKind::Programmatic => activity.borrow_mut().begin_programmatic(),
            }

            assert!(handle.request_offset(point(px(-10.0), px(-20.0)), source));
            app.update_window(any_window, |_, window, cx| window.draw(cx).clear())
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
            app.update_window(any_window, |_, window, cx| window.draw(cx).clear())
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
        app.update_window(any_window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        owner_notifications.set(0);
        dependent_notifications.set(0);

        assert!(handle.request_offset(
            point(px(-40.0), px(-80.0)),
            ScrollMutationSource::Programmatic,
        ));
        app.update_window(any_window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        assert_eq!(owner_notifications.get(), 1);
        assert_eq!(dependent_notifications.get(), 1);

        assert!(handle.request_offset(
            point(px(-40.0), px(-80.0)),
            ScrollMutationSource::Programmatic,
        ));
        app.update_window(any_window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        assert_eq!(owner_notifications.get(), 1);
        assert_eq!(dependent_notifications.get(), 1);
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
                            .tooltip(|_, cx| cx.new(|_| TestTooltipView).into()),
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                    window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                    window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                    window.draw(cx).clear();
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
                window.draw(cx).clear();
                assert_eq!(0, window.rendered_frame.scene.shadows.len());
            })
            .unwrap();

        scroll_handle.request_offset(point(px(0.), px(-40.)), ScrollMutationSource::Programmatic);
        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
    fn sticky_div_hit_tests_above_later_overlapping_normal_content() {
        let mut test_app = TestAppContext::single();
        let window = test_app.add_window(|_, _| StickyOverNormalZOrderOwner);
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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
                window.draw(cx).clear();
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

    #[test]
    fn nested_scroll_activity_hands_a_child_fling_to_the_parent() {
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
                window.draw(cx).clear();
                window.dispatch_event(
                    crate::ScrollWheelEvent {
                        position: point(px(50.), px(50.)),
                        delta: crate::ScrollDelta::Pixels(point(px(0.), px(-200.))),
                        modifiers: Default::default(),
                        touch_phase: crate::TouchPhase::Ended,
                        scroll_source: crate::ScrollWheelSource::Touch,
                        scroll_phase: crate::ScrollWheelPhase::Ended,
                        precise_delta: true,
                        native_momentum_phase: None,
                        release_velocity: Some(point(px(0.), px(-600.))),
                        timestamp: Some(crate::Instant::now() - Duration::from_millis(20)),
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear();
            })
            .unwrap();
        test_app.run_until_parked();

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
                        scroll_source: crate::ScrollWheelSource::PrecisionWheel,
                        scroll_phase: crate::ScrollWheelPhase::Moved,
                        precise_delta: true,
                        native_momentum_phase: None,
                        release_velocity: None,
                        timestamp: Some(crate::Instant::now()),
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

    fn setup_tooltip_owner_test() -> (
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
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear();
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.dispatch_event(
                    PointerEvent::mouse_move(point(px(10.), px(10.)), None, Default::default())
                        .to_platform_input(),
                    cx,
                );
            })
            .unwrap();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear();
            })
            .unwrap();

        (test_app, any_window, captured_active_tooltip)
    }

    #[test]
    fn tooltip_waiting_for_show_is_released_when_its_owner_disappears() {
        let (mut test_app, any_window, captured_active_tooltip) = setup_tooltip_owner_test();

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
    fn tooltip_is_released_when_its_owner_disappears() {
        let (mut test_app, any_window, captured_active_tooltip) = setup_tooltip_owner_test();

        let weak_active_tooltip = captured_active_tooltip.borrow().clone().unwrap();
        let active_tooltip = weak_active_tooltip.upgrade().unwrap();

        test_app.dispatcher.advance_clock(TOOLTIP_SHOW_DELAY);
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
}
