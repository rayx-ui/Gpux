use std::{cell::Cell, rc::Rc, time::Duration};

use gpux::{
    ActionDispatchOutcome, ActionDispatchReport, ActionInvocation, ActionInvocationSource, Context,
    GestureMapLayer, GesturePattern, GestureRule, InputLayerId, IntoElement, Modifiers,
    MouseButton, PointerEvent, PointerId, PointerInteractionCancellationReason,
    PointerInteractionClaimError, PointerInteractionPhase, Render, TestApp, Window, actions, div,
    point, prelude::*, px,
};

actions!(
    pointer_owner_test,
    [ClickAction, DragAction, UnhandledDragAction]
);

#[derive(Default)]
struct Counters {
    raw_down: Cell<usize>,
    owned_move: Cell<usize>,
    owned_up: Cell<usize>,
    inner_drag: Cell<usize>,
    outer_drag: Cell<usize>,
    click: Cell<usize>,
    unhandled_drag: Cell<usize>,
}

struct PointerOwnerHarness {
    counters: Rc<Counters>,
    unhandled: bool,
    inner_handles_drag: bool,
    show_inner: bool,
}

impl Render for PointerOwnerHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let outer_drag = self.counters.clone();
        let raw_down = self.counters.clone();
        let owned_move = self.counters.clone();
        let owned_up = self.counters.clone();
        let inner_drag = self.counters.clone();
        let click = self.counters.clone();
        let unhandled_drag = self.counters.clone();
        let unhandled = self.unhandled;
        let inner_handles_drag = self.inner_handles_drag;

        let inner = div()
            .w(px(120.))
            .h(px(120.))
            .on_pointer_down(MouseButton::Left, move |_, _, _| {
                raw_down.raw_down.set(raw_down.raw_down.get() + 1);
            })
            .on_pointer_move(move |_, window, _| {
                if window.active_pointer_interaction().is_some() {
                    owned_move.owned_move.set(owned_move.owned_move.get() + 1);
                }
            })
            .on_pointer_up(MouseButton::Left, move |_, window, _| {
                if window.active_pointer_interaction().is_some() {
                    owned_up.owned_up.set(owned_up.owned_up.get() + 1);
                }
            })
            .on_action_with_outcome(move |_: &DragAction, _, _| {
                if inner_handles_drag {
                    inner_drag.inner_drag.set(inner_drag.inner_drag.get() + 1);
                    ActionDispatchOutcome::Handled
                } else {
                    ActionDispatchOutcome::Unhandled
                }
            })
            .on_action(move |_: &ClickAction, _, _| {
                click.click.set(click.click.get() + 1);
            })
            .on_action_with_outcome(move |_: &UnhandledDragAction, _, _| {
                unhandled_drag
                    .unhandled_drag
                    .set(unhandled_drag.unhandled_drag.get() + 1);
                if unhandled {
                    ActionDispatchOutcome::Unhandled
                } else {
                    ActionDispatchOutcome::Handled
                }
            });

        div()
            .w(px(120.))
            .h(px(120.))
            .on_pointer_down(MouseButton::Left, |_, _, _| {})
            .on_action(move |_: &DragAction, _, _| {
                outer_drag.outer_drag.set(outer_drag.outer_drag.get() + 1);
            })
            .when(self.show_inner, |this| this.child(inner))
    }
}

fn install_gestures(app: &mut TestApp, drag: &dyn gpux::Action) {
    app.update(|cx| {
        cx.set_gesture_map_layer(GestureMapLayer::new(
            InputLayerId::new("pointer-owner-test"),
            0,
            [
                GestureRule::new(
                    GesturePattern::Click {
                        button: MouseButton::Left,
                        count: 1,
                        modifiers: Modifiers::none(),
                    },
                    &ClickAction,
                ),
                GestureRule::new(
                    GesturePattern::Drag {
                        button: MouseButton::Left,
                        modifiers: Modifiers::none(),
                        threshold: 5,
                    },
                    drag,
                ),
            ],
        ))
        .unwrap();
    });
}

fn simulate_drag_move(
    window: &mut gpux::TestAppWindow<PointerOwnerHarness>,
    position: gpux::Point<gpux::Pixels>,
) {
    window.simulate_event(PointerEvent::mouse_move(
        position,
        Some(MouseButton::Left),
        Modifiers::none(),
    ));
}

fn simulate_touch(
    window: &mut gpux::TestAppWindow<PointerOwnerHarness>,
    pointer_id: u64,
    phase: gpux::PointerPhase,
    position: gpux::Point<gpux::Pixels>,
) {
    window.simulate_event(PointerEvent::touch(
        PointerId::new(pointer_id),
        phase,
        position,
        Modifiers::none(),
    ));
}

#[test]
fn simultaneous_touches_retain_independent_action_owners_and_captures() {
    let mut app = TestApp::new();
    install_gestures(&mut app, &DragAction);
    let counters = Rc::new(Counters::default());
    let mut window = app.open_window({
        let counters = counters.clone();
        move |_, _| PointerOwnerHarness {
            counters,
            unhandled: false,
            inner_handles_drag: true,
            show_inner: true,
        }
    });
    window.draw();

    simulate_touch(
        &mut window,
        1,
        gpux::PointerPhase::Down,
        point(px(20.), px(20.)),
    );
    simulate_touch(
        &mut window,
        2,
        gpux::PointerPhase::Down,
        point(px(24.), px(24.)),
    );
    simulate_touch(
        &mut window,
        1,
        gpux::PointerPhase::Move,
        point(px(32.), px(20.)),
    );
    simulate_touch(
        &mut window,
        2,
        gpux::PointerPhase::Move,
        point(px(36.), px(24.)),
    );

    window.update(|_, window, _| {
        let first = window
            .active_pointer_interaction_for(PointerId::new(1))
            .expect("first touch must retain its owner");
        let second = window
            .active_pointer_interaction_for(PointerId::new(2))
            .expect("second touch must retain its owner");
        assert_ne!(first.id(), second.id());
        assert_eq!(first.pointer_id(), PointerId::new(1));
        assert_eq!(second.pointer_id(), PointerId::new(2));
        assert_eq!(
            window.captured_hitbox_for(PointerId::new(1)),
            Some(first.hitbox_id())
        );
        assert_eq!(
            window.captured_hitbox_for(PointerId::new(2)),
            Some(second.hitbox_id())
        );
    });

    simulate_touch(
        &mut window,
        1,
        gpux::PointerPhase::Up,
        point(px(32.), px(20.)),
    );
    window.update(|_, window, _| {
        assert!(
            window
                .active_pointer_interaction_for(PointerId::new(1))
                .is_none()
        );
        assert!(
            window
                .active_pointer_interaction_for(PointerId::new(2))
                .is_some()
        );
    });
    simulate_touch(
        &mut window,
        2,
        gpux::PointerPhase::Cancel,
        point(px(36.), px(24.)),
    );
    window.update(|_, window, _| {
        assert!(
            window
                .active_pointer_interaction_for(PointerId::new(2))
                .is_none()
        );
        assert!(window.captured_hitbox_for(PointerId::new(2)).is_none());
    });
}

#[test]
fn handled_drag_claims_once_and_raw_packets_only_continue_that_owner() {
    let mut app = TestApp::new();
    install_gestures(&mut app, &DragAction);
    let counters = Rc::new(Counters::default());
    let mut window = app.open_window({
        let counters = counters.clone();
        move |_, _| PointerOwnerHarness {
            counters,
            unhandled: false,
            inner_handles_drag: true,
            show_inner: true,
        }
    });
    window.draw();
    let start = point(px(20.), px(20.));

    window.simulate_mouse_down(start, MouseButton::Left);
    simulate_drag_move(&mut window, point(px(30.), px(20.)));
    let owner = window
        .update(|_, window, _| window.active_pointer_interaction().cloned())
        .expect("handled drag must claim the pointer sequence");
    assert!(owner.action_name().ends_with("DragAction"));
    assert_eq!(owner.phase(), PointerInteractionPhase::Active);
    assert_eq!(counters.raw_down.get(), 1);
    assert_eq!(counters.inner_drag.get(), 1);
    assert_eq!(counters.outer_drag.get(), 0);
    assert_eq!(counters.owned_move.get(), 1);

    window.update(|_, window, _| {
        let competing_report = ActionDispatchReport::new(
            ActionInvocation::new(
                999,
                owner.action_name(),
                ActionInvocationSource::Pointer,
                None,
                None,
                Some(owner.target()),
                Vec::new(),
            ),
            ActionDispatchOutcome::Handled,
            1,
            Duration::ZERO,
        );
        assert_eq!(
            window.claim_pointer_interaction(
                &competing_report,
                owner.hitbox_id(),
                owner.button(),
                owner.start(),
                owner.current(),
                owner.gesture_revision(),
            ),
            Err(PointerInteractionClaimError::AlreadyOwned(owner.id()))
        );
        assert_eq!(
            window.active_pointer_interaction().unwrap().id(),
            owner.id()
        );
        assert_eq!(
            window
                .pointer_interaction_diagnostics()
                .claim_conflict_count(),
            1
        );
    });

    simulate_drag_move(&mut window, point(px(36.), px(22.)));
    window.simulate_mouse_up(point(px(40.), px(24.)), MouseButton::Left);

    assert_eq!(counters.inner_drag.get(), 1, "drag action dispatches once");
    assert_eq!(counters.click.get(), 0, "drag and click are exclusive");
    assert_eq!(counters.owned_move.get(), 2);
    assert_eq!(counters.owned_up.get(), 1);
    window.update(|_, window, _| {
        assert!(window.active_pointer_interaction().is_none());
        let diagnostics = window.pointer_interaction_diagnostics();
        assert_eq!(diagnostics.last_terminal().unwrap().id(), owner.id());
        assert_eq!(
            diagnostics.last_terminal().unwrap().phase(),
            PointerInteractionPhase::Completed
        );
    });
}

#[test]
fn handled_drag_recovers_repainted_start_hitbox_before_claiming_owner() {
    let mut app = TestApp::new();
    install_gestures(&mut app, &DragAction);
    let counters = Rc::new(Counters::default());
    let mut window = app.open_window({
        let counters = counters.clone();
        move |_, _| PointerOwnerHarness {
            counters,
            unhandled: false,
            inner_handles_drag: true,
            show_inner: true,
        }
    });
    window.draw();
    let start = point(px(20.), px(20.));

    window.simulate_mouse_down(start, MouseButton::Left);
    window.draw();
    simulate_drag_move(&mut window, point(px(30.), px(20.)));

    let owner_id = window.update(|_, window, _| {
        let owner = window
            .active_pointer_interaction()
            .expect("repainted drag target must still claim the pointer sequence");
        assert_eq!(owner.start(), start);
        assert_eq!(window.captured_hitbox(), Some(owner.hitbox_id()));
        owner.id()
    });
    assert_eq!(counters.inner_drag.get(), 1);
    assert_eq!(counters.outer_drag.get(), 0);
    assert_eq!(counters.owned_move.get(), 1);

    window.draw();
    simulate_drag_move(&mut window, point(px(36.), px(22.)));
    window.update(|_, window, _| {
        assert_eq!(window.active_pointer_interaction().unwrap().id(), owner_id);
    });
    assert_eq!(counters.owned_move.get(), 2);
}

#[test]
fn ancestor_handled_drag_survives_descendant_removal() {
    let mut app = TestApp::new();
    install_gestures(&mut app, &DragAction);
    let counters = Rc::new(Counters::default());
    let mut window = app.open_window({
        let counters = counters.clone();
        move |_, _| PointerOwnerHarness {
            counters,
            unhandled: false,
            inner_handles_drag: false,
            show_inner: true,
        }
    });
    window.draw();

    window.simulate_mouse_down(point(px(20.), px(20.)), MouseButton::Left);
    simulate_drag_move(&mut window, point(px(30.), px(20.)));
    let owner_id = window.update(|_, window, _| window.active_pointer_interaction().unwrap().id());
    assert_eq!(counters.inner_drag.get(), 0);
    assert_eq!(counters.outer_drag.get(), 1);

    window.update(|harness, _, cx| {
        harness.show_inner = false;
        cx.notify();
    });
    window.draw();
    simulate_drag_move(&mut window, point(px(36.), px(22.)));
    window.update(|_, window, _| {
        assert_eq!(window.active_pointer_interaction().unwrap().id(), owner_id);
    });
}

#[test]
fn unhandled_drag_never_claims_or_captures() {
    let mut app = TestApp::new();
    install_gestures(&mut app, &UnhandledDragAction);
    let counters = Rc::new(Counters::default());
    let mut window = app.open_window({
        let counters = counters.clone();
        move |_, _| PointerOwnerHarness {
            counters,
            unhandled: true,
            inner_handles_drag: true,
            show_inner: true,
        }
    });
    window.draw();

    window.simulate_mouse_down(point(px(20.), px(20.)), MouseButton::Left);
    simulate_drag_move(&mut window, point(px(30.), px(20.)));

    assert_eq!(counters.unhandled_drag.get(), 1);
    window.update(|_, window, _| {
        assert!(window.active_pointer_interaction().is_none());
        assert!(window.captured_hitbox().is_none());
    });
}

#[test]
fn capture_loss_and_profile_replacement_cancel_once_without_completion() {
    let mut app = TestApp::new();
    install_gestures(&mut app, &DragAction);
    let counters = Rc::new(Counters::default());
    let mut window = app.open_window({
        let counters = counters.clone();
        move |_, _| PointerOwnerHarness {
            counters,
            unhandled: false,
            inner_handles_drag: true,
            show_inner: true,
        }
    });
    window.draw();
    let start = point(px(20.), px(20.));

    window.simulate_mouse_down(start, MouseButton::Left);
    simulate_drag_move(&mut window, point(px(30.), px(20.)));
    let first_id = window.update(|_, window, _| window.active_pointer_interaction().unwrap().id());
    window.update(|_, window, _| {
        window.release_pointer();
        window.release_pointer();
        let terminal = window
            .pointer_interaction_diagnostics()
            .last_terminal()
            .unwrap();
        assert_eq!(terminal.id(), first_id);
        assert_eq!(
            terminal.phase(),
            PointerInteractionPhase::Cancelled(PointerInteractionCancellationReason::CaptureLost)
        );
    });
    window.simulate_mouse_up(start, MouseButton::Left);
    assert_eq!(counters.owned_up.get(), 0);

    window.simulate_mouse_down(start, MouseButton::Left);
    simulate_drag_move(&mut window, point(px(30.), px(20.)));
    let second_id = window.update(|_, window, _| window.active_pointer_interaction().unwrap().id());
    install_gestures(&mut app, &DragAction);
    simulate_drag_move(&mut window, point(px(40.), px(20.)));
    window.update(|_, window, _| {
        assert!(window.active_pointer_interaction().is_none());
        let terminal = window
            .pointer_interaction_diagnostics()
            .last_terminal()
            .unwrap();
        assert_eq!(terminal.id(), second_id);
        assert_eq!(
            terminal.phase(),
            PointerInteractionPhase::Cancelled(
                PointerInteractionCancellationReason::ProfileReplaced
            )
        );
    });
}

#[test]
fn target_removal_and_window_teardown_cancel_once_without_completion() {
    let mut app = TestApp::new();
    install_gestures(&mut app, &DragAction);
    let counters = Rc::new(Counters::default());
    let mut window = app.open_window({
        let counters = counters.clone();
        move |_, _| PointerOwnerHarness {
            counters,
            unhandled: false,
            inner_handles_drag: true,
            show_inner: true,
        }
    });
    window.draw();
    let start = point(px(20.), px(20.));

    window.simulate_mouse_down(start, MouseButton::Left);
    simulate_drag_move(&mut window, point(px(30.), px(20.)));
    let removed_target_id =
        window.update(|_, window, _| window.active_pointer_interaction().unwrap().id());
    window.update(|harness, _, cx| {
        harness.show_inner = false;
        cx.notify();
    });
    window.draw();
    simulate_drag_move(&mut window, point(px(36.), px(20.)));
    window.update(|_, window, _| {
        assert!(window.active_pointer_interaction().is_none());
        let terminal = window
            .pointer_interaction_diagnostics()
            .last_terminal()
            .unwrap();
        assert_eq!(terminal.id(), removed_target_id);
        assert_eq!(
            terminal.phase(),
            PointerInteractionPhase::Cancelled(PointerInteractionCancellationReason::TargetRemoved)
        );
    });

    window.update(|harness, _, cx| {
        harness.show_inner = true;
        cx.notify();
    });
    window.draw();
    window.simulate_mouse_down(start, MouseButton::Left);
    simulate_drag_move(&mut window, point(px(30.), px(20.)));
    let removed_window_id =
        window.update(|_, window, _| window.active_pointer_interaction().unwrap().id());
    window.update(|_, window, _| {
        window.remove_window();
        window.remove_window();
        assert!(window.active_pointer_interaction().is_none());
        let terminal = window
            .pointer_interaction_diagnostics()
            .last_terminal()
            .unwrap();
        assert_eq!(terminal.id(), removed_window_id);
        assert_eq!(
            terminal.phase(),
            PointerInteractionPhase::Cancelled(PointerInteractionCancellationReason::WindowRemoved)
        );
    });
}
