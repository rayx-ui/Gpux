use std::{cell::Cell, rc::Rc};

use gpux::{
    ActionDispatchEvent, ActionDispatchOutcome, Context, IntoElement, MouseButton, Render, TestApp,
    Window, actions, div, point, prelude::*, px,
};

actions!(test, [PointerAction]);

struct PointerHarness {
    outer_actions: Rc<Cell<usize>>,
    inner_actions: Rc<Cell<usize>>,
    raw_mouse_events: Rc<Cell<usize>>,
}

impl Render for PointerHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let outer_actions = self.outer_actions.clone();
        let inner_actions = self.inner_actions.clone();
        let raw_mouse_events = self.raw_mouse_events.clone();

        div()
            .w(px(100.))
            .h(px(100.))
            .on_mouse_down(MouseButton::Left, move |_, _, _| {
                raw_mouse_events.set(raw_mouse_events.get() + 1)
            })
            .on_action(move |_: &PointerAction, _, _| {
                outer_actions.set(outer_actions.get() + 1);
            })
            .child(
                div()
                    .w(px(100.))
                    .h(px(100.))
                    .on_mouse_down(MouseButton::Left, |_, _, _| {})
                    .on_action(move |_: &PointerAction, _, _| {
                        inner_actions.set(inner_actions.get() + 1);
                    }),
            )
    }
}

#[test]
fn pointer_action_targets_the_deepest_hitbox_dispatch_node() {
    let mut app = TestApp::new();
    let outer_actions = Rc::new(Cell::new(0));
    let inner_actions = Rc::new(Cell::new(0));
    let raw_mouse_events = Rc::new(Cell::new(0));
    let mut window = app.open_window({
        let outer_actions = outer_actions.clone();
        let inner_actions = inner_actions.clone();
        let raw_mouse_events = raw_mouse_events.clone();
        move |_, _| PointerHarness {
            outer_actions,
            inner_actions,
            raw_mouse_events,
        }
    });
    window.draw();
    let target = point(px(20.), px(20.));

    window.simulate_mouse_down(target, MouseButton::Left);
    assert!(window.update(|_, window, _| window.begin_pointer_action(&PointerAction)));
    window.simulate_mouse_up(target, MouseButton::Left);

    assert_eq!(inner_actions.get(), 1);
    assert_eq!(outer_actions.get(), 0);
}

#[test]
fn pointer_capture_retains_its_starting_target_and_preserves_raw_mouse_events() {
    let mut app = TestApp::new();
    let outer_actions = Rc::new(Cell::new(0));
    let inner_actions = Rc::new(Cell::new(0));
    let raw_mouse_events = Rc::new(Cell::new(0));
    let mut window = app.open_window({
        let outer_actions = outer_actions.clone();
        let inner_actions = inner_actions.clone();
        let raw_mouse_events = raw_mouse_events.clone();
        move |_, _| PointerHarness {
            outer_actions,
            inner_actions,
            raw_mouse_events,
        }
    });
    window.draw();
    let target = point(px(20.), px(20.));

    window.simulate_mouse_down(target, MouseButton::Left);
    assert!(window.update(|_, window, _| window.begin_pointer_action(&PointerAction)));
    window.simulate_mouse_move(point(px(300.), px(300.)));
    window.simulate_mouse_up(point(px(300.), px(300.)), MouseButton::Left);

    assert_eq!(inner_actions.get(), 1);
    assert_eq!(outer_actions.get(), 0);
    assert_eq!(raw_mouse_events.get(), 1);
}

#[test]
fn capture_loss_cancels_once_without_dispatching_the_action() {
    let mut app = TestApp::new();
    let outer_actions = Rc::new(Cell::new(0));
    let inner_actions = Rc::new(Cell::new(0));
    let raw_mouse_events = Rc::new(Cell::new(0));
    let mut window = app.open_window({
        let outer_actions = outer_actions.clone();
        let inner_actions = inner_actions.clone();
        let raw_mouse_events = raw_mouse_events.clone();
        move |_, _| PointerHarness {
            outer_actions,
            inner_actions,
            raw_mouse_events,
        }
    });
    window.draw();
    let target = point(px(20.), px(20.));
    let events = Rc::new(std::cell::RefCell::new(Vec::new()));
    let _subscription = window.update({
        let events = events.clone();
        move |_, window, _| {
            window
                .observe_action_dispatch(move |event, _, _| events.borrow_mut().push(event.clone()))
        }
    });

    window.simulate_mouse_down(target, MouseButton::Left);
    assert!(window.update(|_, window, _| window.begin_pointer_action(&PointerAction)));
    window.update(|_, window, _| window.release_pointer());
    window.simulate_mouse_up(target, MouseButton::Left);

    assert_eq!(inner_actions.get(), 0);
    assert_eq!(outer_actions.get(), 0);
    assert!(matches!(events.borrow().as_slice(), [
        ActionDispatchEvent::Began(_),
        ActionDispatchEvent::Ended(report),
    ] if report.outcome() == ActionDispatchOutcome::Cancelled));
}
