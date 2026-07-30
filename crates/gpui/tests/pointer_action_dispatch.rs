use std::{cell::Cell, rc::Rc};

use gpux::{
    ActionDispatchEvent, ActionDispatchOutcome, ActionInvocationSource, Context, GestureMapLayer,
    GesturePattern, GestureRule, GestureWheelSource, InputLayerId, IntoElement,
    KeyBindingContextPredicate, Modifiers, MouseButton, Render, ScrollDelta, TestApp,
    WheelDirection, Window, actions, div, point, prelude::*, px,
};

actions!(test, [PointerAction, WheelAction]);

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
            .on_pointer_down(MouseButton::Left, move |_, _, _| {
                raw_mouse_events.set(raw_mouse_events.get() + 1)
            })
            .on_action(move |_: &PointerAction, _, _| {
                outer_actions.set(outer_actions.get() + 1);
            })
            .child(
                div()
                    .w(px(50.))
                    .h(px(50.))
                    .key_context("Inner")
                    .on_pointer_down(MouseButton::Left, |_, _, _| {})
                    .on_action(move |_: &PointerAction, _, _| {
                        inner_actions.set(inner_actions.get() + 1);
                    }),
            )
            // Positioned traversal chrome can contribute a top-layer hitbox whose
            // dispatch path reaches only the outer action handler. The mapped
            // gesture must still target the deeper handler painted underneath it.
            .child(
                div()
                    .fixed()
                    .left(px(0.))
                    .top(px(0.))
                    .w(px(100.))
                    .h(px(100.))
                    .on_pointer_down(MouseButton::Left, |_, _, _| {}),
            )
    }
}

fn install_pointer_click(app: &mut TestApp) {
    app.update(|cx| {
        cx.set_gesture_map_layer(GestureMapLayer::new(
            InputLayerId::new("pointer-action-dispatch-test"),
            0,
            [GestureRule::new(
                GesturePattern::Click {
                    button: MouseButton::Left,
                    count: 1,
                    modifiers: Modifiers::none(),
                },
                &PointerAction,
            )],
        ))
        .unwrap();
    });
}

fn install_pointer_click_counts(app: &mut TestApp) {
    app.update(|cx| {
        cx.set_gesture_map_layer(GestureMapLayer::new(
            InputLayerId::new("pointer-action-dispatch-test"),
            0,
            [1, 2].map(|count| {
                GestureRule::new(
                    GesturePattern::Click {
                        button: MouseButton::Left,
                        count,
                        modifiers: Modifiers::none(),
                    },
                    &PointerAction,
                )
            }),
        ))
        .unwrap();
    });
}

fn install_contextual_pointer_click(app: &mut TestApp) {
    app.update(|cx| {
        cx.set_gesture_map_layer(GestureMapLayer::new(
            InputLayerId::new("contextual-pointer-action-dispatch-test"),
            0,
            [GestureRule::load(
                GesturePattern::Click {
                    button: MouseButton::Left,
                    count: 1,
                    modifiers: Modifiers::none(),
                },
                &PointerAction,
                Some(Rc::new(KeyBindingContextPredicate::parse("Inner").unwrap())),
            )],
        ))
        .unwrap();
    });
}

fn open_harness(
    app: &mut TestApp,
) -> (
    gpux::TestAppWindow<PointerHarness>,
    Rc<Cell<usize>>,
    Rc<Cell<usize>>,
    Rc<Cell<usize>>,
) {
    let outer_actions = Rc::new(Cell::new(0));
    let inner_actions = Rc::new(Cell::new(0));
    let raw_mouse_events = Rc::new(Cell::new(0));
    let window = app.open_window({
        let outer_actions = outer_actions.clone();
        let inner_actions = inner_actions.clone();
        let raw_mouse_events = raw_mouse_events.clone();
        move |_, _| PointerHarness {
            outer_actions,
            inner_actions,
            raw_mouse_events,
        }
    });
    (window, outer_actions, inner_actions, raw_mouse_events)
}

#[test]
fn mapped_pointer_action_targets_the_deepest_hitbox_dispatch_node() {
    let mut app = TestApp::new();
    install_pointer_click(&mut app);
    let (mut window, outer_actions, inner_actions, raw_mouse_events) = open_harness(&mut app);
    window.draw();
    let target = point(px(20.), px(20.));

    window.simulate_click(target, MouseButton::Left);

    assert_eq!(inner_actions.get(), 1);
    assert_eq!(outer_actions.get(), 0);
    assert_eq!(raw_mouse_events.get(), 1);
}

#[test]
fn handled_click_reports_pointer_source_without_claiming_continuation() {
    let mut app = TestApp::new();
    install_pointer_click(&mut app);
    let (mut window, outer_actions, inner_actions, _) = open_harness(&mut app);
    window.draw();
    let events = Rc::new(std::cell::RefCell::new(Vec::new()));
    let _subscription = window.update({
        let events = events.clone();
        move |_, window, _| {
            window
                .observe_action_dispatch(move |event, _, _| events.borrow_mut().push(event.clone()))
        }
    });

    window.simulate_click(point(px(20.), px(20.)), MouseButton::Left);

    assert_eq!(inner_actions.get(), 1);
    assert_eq!(outer_actions.get(), 0);
    assert!(matches!(events.borrow().as_slice(), [
        ActionDispatchEvent::Began(invocation),
        ActionDispatchEvent::Ended(report),
    ] if invocation.source() == ActionInvocationSource::Pointer
        && report.outcome() == ActionDispatchOutcome::Handled));
    window.update(|_, window, _| {
        assert!(window.active_pointer_interaction().is_none());
        assert!(window.captured_hitbox().is_none());
    });
}

#[test]
fn single_click_dispatches_immediately_when_multi_click_rules_overlap() {
    let mut app = TestApp::new();
    install_pointer_click_counts(&mut app);
    let (mut window, outer_actions, inner_actions, _) = open_harness(&mut app);
    window.draw();

    window.simulate_click(point(px(20.), px(20.)), MouseButton::Left);
    assert_eq!(inner_actions.get(), 1);
    assert_eq!(outer_actions.get(), 0);
}

#[test]
fn mapped_pointer_action_honors_its_context_predicate() {
    let mut app = TestApp::new();
    install_contextual_pointer_click(&mut app);
    let (mut window, outer_actions, inner_actions, _) = open_harness(&mut app);
    window.draw();

    window.simulate_click(point(px(20.), px(20.)), MouseButton::Left);
    window.simulate_click(point(px(80.), px(80.)), MouseButton::Left);

    assert_eq!(inner_actions.get(), 1);
    assert_eq!(outer_actions.get(), 0);
}

#[test]
fn mapped_wheel_action_observes_the_current_raw_wheel_sample() {
    struct WheelHarness {
        raw_delta_y: Rc<Cell<f32>>,
        action_delta_y: Rc<Cell<f32>>,
    }

    impl Render for WheelHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let raw_delta_y = self.raw_delta_y.clone();
            let action_delta_y = self.action_delta_y.clone();
            let action_raw_delta_y = self.raw_delta_y.clone();
            div()
                .w(px(100.))
                .h(px(100.))
                .on_scroll_wheel(move |event, _, _| {
                    let ScrollDelta::Pixels(delta) = event.delta else {
                        panic!("test emits pixel wheel deltas");
                    };
                    raw_delta_y.set(f32::from(delta.y));
                })
                .on_action(move |_: &WheelAction, _, _| {
                    action_delta_y.set(action_raw_delta_y.get());
                })
        }
    }

    let mut app = TestApp::new();
    app.update(|cx| {
        cx.set_gesture_map_layer(GestureMapLayer::new(
            InputLayerId::new("wheel-action-dispatch-test"),
            0,
            [GestureRule::new(
                GesturePattern::Wheel {
                    direction: WheelDirection::Up,
                    source: GestureWheelSource::PrecisionTouchpad,
                    modifiers: Modifiers::none(),
                    threshold: 1,
                },
                &WheelAction,
            )],
        ))
        .unwrap();
    });
    let raw_delta_y = Rc::new(Cell::new(0.));
    let action_delta_y = Rc::new(Cell::new(0.));
    let mut window = app.open_window({
        let raw_delta_y = raw_delta_y.clone();
        let action_delta_y = action_delta_y.clone();
        move |_, _| WheelHarness {
            raw_delta_y,
            action_delta_y,
        }
    });
    window.draw();

    window.simulate_scroll(point(px(20.), px(20.)), point(px(0.), px(-24.)));

    assert_eq!(raw_delta_y.get(), -24.);
    assert_eq!(action_delta_y.get(), -24.);
}
