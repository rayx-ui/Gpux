use gpux::{
    Context, GestureMap, GestureMapLayer, GesturePattern, GestureRecognizer, GestureRule,
    GestureWheelSource, InputLayerId, IntoElement, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, NavigationDirection, Render, ScrollDelta, ScrollWheelEvent,
    ScrollWheelSource, TestApp, WheelDirection, Window, actions, div, point, prelude::*, px,
};
use std::{cell::Cell, rc::Rc, sync::Arc};

actions!(
    gesture_recognizer_test,
    [
        SingleClick,
        DoubleClick,
        DragAction,
        WheelAction,
        ButtonAction,
        RoutedAction,
        UnavailableAction
    ]
);

fn snapshot(
    rules: impl IntoIterator<Item = GestureRule>,
) -> (Arc<gpux::GestureMapSnapshot>, gpux::GestureMapVersion) {
    let mut map = GestureMap::default();
    map.set_layer(GestureMapLayer::new(InputLayerId::new("test"), 0, rules))
        .unwrap();
    (map.snapshot(), map.version())
}

fn down(button: MouseButton, count: usize, modifiers: Modifiers) -> MouseDownEvent {
    MouseDownEvent {
        button,
        position: point(px(10.0), px(10.0)),
        modifiers,
        click_count: count,
        first_mouse: false,
    }
}

fn up(button: MouseButton, count: usize, modifiers: Modifiers) -> MouseUpEvent {
    MouseUpEvent {
        button,
        position: point(px(10.0), px(10.0)),
        modifiers,
        click_count: count,
    }
}

#[test]
fn click_count_arbitration_defers_only_overlapping_higher_count_candidates() {
    let (snapshot, revision) = snapshot([
        GestureRule::new(
            GesturePattern::Click {
                button: MouseButton::Left,
                count: 1,
                modifiers: Modifiers::none(),
            },
            &SingleClick,
        ),
        GestureRule::new(
            GesturePattern::Click {
                button: MouseButton::Left,
                count: 2,
                modifiers: Modifiers::none(),
            },
            &DoubleClick,
        ),
    ]);
    let mut recognizer = GestureRecognizer::default();

    recognizer.mouse_down(
        &down(MouseButton::Left, 1, Modifiers::none()),
        snapshot.clone(),
        revision,
        41,
    );
    assert!(
        recognizer
            .mouse_up(&up(MouseButton::Left, 1, Modifiers::none()))
            .is_none()
    );
    let delayed = recognizer.flush_pending_click().unwrap();
    assert!(delayed.action().partial_eq(&SingleClick));
    assert_eq!(delayed.target(), 41);

    recognizer.mouse_down(
        &down(MouseButton::Left, 2, Modifiers::none()),
        snapshot,
        revision,
        41,
    );
    let double = recognizer
        .mouse_up(&up(MouseButton::Left, 2, Modifiers::none()))
        .unwrap();
    assert!(double.action().partial_eq(&DoubleClick));
}

#[test]
fn buttons_and_modifiers_match_exactly() {
    let buttons = [
        MouseButton::Left,
        MouseButton::Middle,
        MouseButton::Right,
        MouseButton::Navigate(NavigationDirection::Back),
        MouseButton::Navigate(NavigationDirection::Forward),
        MouseButton::Other(7),
    ];
    for button in buttons {
        let modifiers = Modifiers::control();
        let (snapshot, revision) = snapshot([GestureRule::new(
            GesturePattern::Click {
                button,
                count: 1,
                modifiers,
            },
            &ButtonAction,
        )]);
        let mut recognizer = GestureRecognizer::default();
        recognizer.mouse_down(&down(button, 1, modifiers), snapshot.clone(), revision, 9);
        let action = recognizer.mouse_up(&up(button, 1, modifiers)).unwrap();
        assert!(action.action().partial_eq(&ButtonAction));
        assert_eq!(action.target(), 9);

        recognizer.mouse_down(&down(button, 1, Modifiers::none()), snapshot, revision, 9);
        assert!(
            recognizer
                .mouse_up(&up(button, 1, Modifiers::none()))
                .is_none()
        );
    }
}

#[test]
fn drag_cancels_click_and_retains_starting_target_and_revision() {
    let (snapshot, revision) = snapshot([
        GestureRule::new(
            GesturePattern::Click {
                button: MouseButton::Left,
                count: 1,
                modifiers: Modifiers::none(),
            },
            &SingleClick,
        ),
        GestureRule::new(
            GesturePattern::Drag {
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                threshold: 5,
            },
            &DragAction,
        ),
    ]);
    let mut recognizer = GestureRecognizer::default();
    recognizer.mouse_down(
        &down(MouseButton::Left, 1, Modifiers::none()),
        snapshot,
        revision,
        77,
    );
    let drag = recognizer
        .mouse_move(&MouseMoveEvent {
            position: point(px(16.0), px(10.0)),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::none(),
        })
        .unwrap();
    assert!(drag.action().partial_eq(&DragAction));
    assert_eq!(drag.target(), 77);
    assert_eq!(drag.revision(), revision);
    assert!(
        recognizer
            .mouse_up(&up(MouseButton::Left, 1, Modifiers::none()))
            .is_none()
    );
}

#[test]
fn precise_wheel_accumulates_to_one_bounded_dispatch() {
    let (snapshot, revision) = snapshot([GestureRule::new(
        GesturePattern::Wheel {
            direction: WheelDirection::Up,
            source: GestureWheelSource::PrecisionTouchpad,
            modifiers: Modifiers::none(),
            threshold: 3,
        },
        &WheelAction,
    )]);
    let mut recognizer = GestureRecognizer::default();
    let event = |delta| ScrollWheelEvent {
        delta: ScrollDelta::Pixels(point(px(0.0), px(delta))),
        modifiers: Modifiers::none(),
        scroll_source: ScrollWheelSource::PrecisionWheel,
        precise_delta: true,
        ..Default::default()
    };
    assert!(
        recognizer
            .scroll_wheel(&event(-1.0), snapshot.clone(), revision, 13)
            .is_none()
    );
    assert!(
        recognizer
            .scroll_wheel(&event(-1.0), snapshot.clone(), revision, 13)
            .is_none()
    );
    let action = recognizer
        .scroll_wheel(&event(-1.0), snapshot, revision, 13)
        .unwrap();
    assert!(action.action().partial_eq(&WheelAction));
    assert_eq!(action.target(), 13);
}

struct RoutedHarness {
    outer: Rc<Cell<usize>>,
    inner: Rc<Cell<usize>>,
}

impl Render for RoutedHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let outer = self.outer.clone();
        let inner = self.inner.clone();
        div()
            .w(px(100.0))
            .h(px(100.0))
            .on_mouse_down(MouseButton::Left, |_, _, _| {})
            .on_action(move |_: &RoutedAction, _, _| outer.set(outer.get() + 1))
            .child(
                div()
                    .w(px(100.0))
                    .h(px(100.0))
                    .on_mouse_down(MouseButton::Left, |_, _, _| {})
                    .on_action(move |_: &RoutedAction, _, _| inner.set(inner.get() + 1)),
            )
    }
}

#[test]
fn mapped_click_dispatches_once_to_the_deepest_captured_target() {
    let mut app = TestApp::new();
    app.update(|cx| {
        cx.set_gesture_map_layer(GestureMapLayer::new(
            InputLayerId::new("test"),
            0,
            [
                GestureRule::new(
                    GesturePattern::Click {
                        button: MouseButton::Left,
                        count: 1,
                        modifiers: Modifiers::none(),
                    },
                    &RoutedAction,
                ),
                // This later rule wins global precedence, but has no handler on the
                // captured target. It must not suppress the target's routed action.
                GestureRule::new(
                    GesturePattern::Click {
                        button: MouseButton::Left,
                        count: 1,
                        modifiers: Modifiers::none(),
                    },
                    &UnavailableAction,
                ),
            ],
        ))
        .unwrap();
    });
    let outer = Rc::new(Cell::new(0));
    let inner = Rc::new(Cell::new(0));
    let mut window = app.open_window({
        let outer = outer.clone();
        let inner = inner.clone();
        move |_, _| RoutedHarness { outer, inner }
    });
    window.draw();
    let target = point(px(20.0), px(20.0));
    window.simulate_mouse_down(target, MouseButton::Left);
    window.simulate_mouse_up(target, MouseButton::Left);

    assert_eq!(inner.get(), 1);
    assert_eq!(outer.get(), 0);
}
