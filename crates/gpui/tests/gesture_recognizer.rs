use gpux::{
    Context, GestureDirection, GestureMap, GestureMapLayer, GesturePattern, GestureRecognizer,
    GestureRule, GestureWheelSource, InputLayerId, Instant, IntoElement,
    KeyBindingContextPredicate, KeyContext, Modifiers, MouseButton, NavigationDirection,
    PointerEvent, PointerId, PointerPhase, Render, ScrollDelta, ScrollWheelEvent,
    ScrollWheelSource, TestApp, WheelDirection, Window, actions, div, point, prelude::*, px,
};
use std::{cell::Cell, rc::Rc, time::Duration};

actions!(
    gesture_recognizer_test,
    [
        SingleClick,
        DoubleClick,
        DragAction,
        WheelAction,
        ArenaAction,
        ScaleAction,
        LongPressAction,
        TapAction,
        DoubleTapAction,
        HorizontalAction,
        FreeAction,
        PanAction,
        RotateAction,
        TrackpadScaleAction,
        ButtonAction,
        RoutedAction,
        UnavailableAction
    ]
);

fn snapshot(
    rules: impl IntoIterator<Item = GestureRule>,
) -> (Rc<gpux::GestureMapSnapshot>, gpux::GestureMapVersion) {
    let mut map = GestureMap::default();
    map.set_layer(GestureMapLayer::new(InputLayerId::new("test"), 0, rules))
        .unwrap();
    (map.snapshot(), map.version())
}

fn down(button: MouseButton, count: usize, modifiers: Modifiers) -> PointerEvent {
    PointerEvent::mouse_down(point(px(10.0), px(10.0)), button, modifiers, count, false)
}

fn up(button: MouseButton, count: usize, modifiers: Modifiers) -> PointerEvent {
    PointerEvent::mouse_up(point(px(10.0), px(10.0)), button, modifiers, count)
}

fn touch(id: u64, phase: PointerPhase, x: f32, y: f32, timestamp: Instant) -> PointerEvent {
    let mut event = PointerEvent::touch(
        PointerId::new(id),
        phase,
        point(px(x), px(y)),
        Modifiers::none(),
    );
    event.click_count = 1;
    event.timestamp = Some(timestamp);
    event
}

#[test]
fn click_count_arbitration_dispatches_each_exact_count_in_order() {
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

    recognizer.pointer_down(
        &down(MouseButton::Left, 1, Modifiers::none()),
        snapshot.clone(),
        revision,
        41,
    );
    let single = recognizer
        .pointer_up(&up(MouseButton::Left, 1, Modifiers::none()))
        .unwrap();
    assert!(single.action().partial_eq(&SingleClick));
    assert_eq!(single.target(), 41);

    recognizer.pointer_down(
        &down(MouseButton::Left, 2, Modifiers::none()),
        snapshot,
        revision,
        41,
    );
    let double = recognizer
        .pointer_up(&up(MouseButton::Left, 2, Modifiers::none()))
        .unwrap();
    assert!(double.action().partial_eq(&DoubleClick));
}

#[test]
fn contextual_rule_wins_before_layer_priority() {
    let pattern = GesturePattern::Click {
        button: MouseButton::Left,
        count: 1,
        modifiers: Modifiers::none(),
    };
    let mut map = GestureMap::default();
    map.set_layer(GestureMapLayer::new(
        InputLayerId::new("contextual"),
        0,
        [GestureRule::load(
            pattern,
            &DoubleClick,
            Some(Rc::new(KeyBindingContextPredicate::parse("Inner").unwrap())),
        )],
    ))
    .unwrap();
    map.set_layer(GestureMapLayer::new(
        InputLayerId::new("generic-higher-priority"),
        100,
        [GestureRule::new(pattern, &SingleClick)],
    ))
    .unwrap();
    let contexts = [KeyContext::parse("Inner").unwrap()];
    let snapshot = map.snapshot().filtered_for_target(&contexts, |_| true);
    let revision = map.version();
    let mut recognizer = GestureRecognizer::default();

    recognizer.pointer_down(
        &down(MouseButton::Left, 1, Modifiers::none()),
        snapshot,
        revision,
        41,
    );
    let recognized = recognizer
        .pointer_up(&up(MouseButton::Left, 1, Modifiers::none()))
        .unwrap();

    assert!(recognized.action().partial_eq(&DoubleClick));
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
        recognizer.pointer_down(&down(button, 1, modifiers), snapshot.clone(), revision, 9);
        let action = recognizer.pointer_up(&up(button, 1, modifiers)).unwrap();
        assert!(action.action().partial_eq(&ButtonAction));
        assert_eq!(action.target(), 9);

        recognizer.pointer_down(&down(button, 1, Modifiers::none()), snapshot, revision, 9);
        assert!(
            recognizer
                .pointer_up(&up(button, 1, Modifiers::none()))
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
    recognizer.pointer_down(
        &down(MouseButton::Left, 1, Modifiers::none()),
        snapshot,
        revision,
        77,
    );
    let drag = recognizer
        .pointer_move(&PointerEvent::mouse_move(
            point(px(16.0), px(10.0)),
            Some(MouseButton::Left),
            Modifiers::none(),
        ))
        .unwrap();
    assert!(drag.action().partial_eq(&DragAction));
    assert_eq!(drag.target(), 77);
    assert_eq!(drag.revision(), revision);
    assert!(
        recognizer
            .pointer_up(&up(MouseButton::Left, 1, Modifiers::none()))
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
            .on_pointer_down(MouseButton::Left, |_, _, _| {})
            .on_action(move |_: &RoutedAction, _, _| outer.set(outer.get() + 1))
            .child(
                div()
                    .w(px(100.0))
                    .h(px(100.0))
                    .on_pointer_down(MouseButton::Left, |_, _, _| {})
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

#[test]
fn gesture_arena_required_behavior() {
    let started_at = Instant::now();
    let (snapshot, revision) = snapshot([GestureRule::new(
        GesturePattern::Pan {
            direction: GestureDirection::Horizontal,
            modifiers: Modifiers::none(),
            threshold: 5,
        },
        &ArenaAction,
    )]);
    let mut recognizer = GestureRecognizer::default();
    recognizer.pointer_down(
        &touch(1, PointerPhase::Down, 0.0, 0.0, started_at),
        snapshot.clone(),
        revision,
        7,
    );
    recognizer.pointer_down(
        &touch(2, PointerPhase::Down, 30.0, 0.0, started_at),
        snapshot,
        revision,
        9,
    );
    assert_eq!(recognizer.active_pointer_count(), 2);

    let action = recognizer
        .pointer_move(&touch(1, PointerPhase::Move, 8.0, 0.0, started_at))
        .expect("the first pointer owns an independent pan arena");
    assert!(action.action().partial_eq(&ArenaAction));
    assert_eq!(action.target(), 7);
    assert_eq!(recognizer.active_pointer_count(), 1);
}

#[test]
fn gesture_scale_and_long_press_are_resolved_without_stale_arenas() {
    let started_at = Instant::now();
    let (scale_snapshot, scale_revision) = snapshot([
        GestureRule::new(
            GesturePattern::Pan {
                direction: GestureDirection::Free,
                modifiers: Modifiers::none(),
                threshold: 5,
            },
            &PanAction,
        ),
        GestureRule::new(
            GesturePattern::Scale {
                threshold: 5,
                modifiers: Modifiers::none(),
            },
            &ScaleAction,
        ),
    ]);
    let mut recognizer = GestureRecognizer::default();
    recognizer.pointer_down(
        &touch(1, PointerPhase::Down, 0.0, 0.0, started_at),
        scale_snapshot.clone(),
        scale_revision,
        11,
    );
    recognizer.pointer_down(
        &touch(2, PointerPhase::Down, 10.0, 0.0, started_at),
        scale_snapshot,
        scale_revision,
        11,
    );
    let action = recognizer
        .pointer_move(&touch(2, PointerPhase::Move, 20.0, 0.0, started_at))
        .expect("the two touch arenas form one scale team");
    assert!(action.action().partial_eq(&ScaleAction));
    assert_eq!(recognizer.active_pointer_count(), 0);

    let (press_snapshot, press_revision) = snapshot([
        GestureRule::new(
            GesturePattern::Tap {
                count: 1,
                modifiers: Modifiers::none(),
            },
            &TapAction,
        ),
        GestureRule::new(
            GesturePattern::LongPress {
                modifiers: Modifiers::none(),
                duration_ms: 20,
                threshold: 3,
            },
            &LongPressAction,
        ),
    ]);
    recognizer.pointer_down(
        &touch(3, PointerPhase::Down, 0.0, 0.0, started_at),
        press_snapshot,
        press_revision,
        13,
    );
    let actions = recognizer.advance_time(started_at + Duration::from_millis(20));
    assert_eq!(actions.len(), 1);
    assert!(actions[0].action().partial_eq(&LongPressAction));
    assert!(
        recognizer
            .advance_time(started_at + Duration::from_millis(30))
            .is_empty()
    );
    assert!(
        recognizer
            .pointer_up(&touch(3, PointerPhase::Up, 0.0, 0.0, started_at))
            .is_none()
    );
    assert_eq!(recognizer.active_pointer_count(), 0);
}

#[test]
fn touch_taps_directional_drags_rotation_and_trackpad_scale_are_deterministic() {
    let started_at = Instant::now();
    let (tap_snapshot, tap_revision) = snapshot([
        GestureRule::new(
            GesturePattern::Tap {
                count: 1,
                modifiers: Modifiers::none(),
            },
            &TapAction,
        ),
        GestureRule::new(
            GesturePattern::Tap {
                count: 2,
                modifiers: Modifiers::none(),
            },
            &DoubleTapAction,
        ),
    ]);
    let mut recognizer = GestureRecognizer::default();
    recognizer.pointer_down(
        &touch(1, PointerPhase::Down, 0.0, 0.0, started_at),
        tap_snapshot.clone(),
        tap_revision,
        1,
    );
    assert!(
        recognizer
            .pointer_up(&touch(1, PointerPhase::Up, 0.0, 0.0, started_at))
            .is_some_and(|gesture| gesture.action().partial_eq(&TapAction))
    );
    let mut double_up = touch(2, PointerPhase::Up, 0.0, 0.0, started_at);
    double_up.click_count = 2;
    recognizer.pointer_down(
        &touch(2, PointerPhase::Down, 0.0, 0.0, started_at),
        tap_snapshot,
        tap_revision,
        1,
    );
    assert!(
        recognizer
            .pointer_up(&double_up)
            .is_some_and(|gesture| gesture.action().partial_eq(&DoubleTapAction))
    );

    let (drag_snapshot, drag_revision) = snapshot([
        GestureRule::new(
            GesturePattern::DirectionalDrag {
                button: MouseButton::Left,
                direction: GestureDirection::Free,
                modifiers: Modifiers::none(),
                threshold: 5,
            },
            &FreeAction,
        ),
        GestureRule::new(
            GesturePattern::DirectionalDrag {
                button: MouseButton::Left,
                direction: GestureDirection::Horizontal,
                modifiers: Modifiers::none(),
                threshold: 5,
            },
            &HorizontalAction,
        ),
    ]);
    recognizer.pointer_down(
        &touch(3, PointerPhase::Down, 0.0, 0.0, started_at),
        drag_snapshot.clone(),
        drag_revision,
        2,
    );
    assert!(
        recognizer
            .pointer_move(&touch(3, PointerPhase::Move, 5.0, 0.0, started_at))
            .is_some_and(|gesture| gesture.action().partial_eq(&HorizontalAction))
    );
    recognizer.pointer_down(
        &touch(4, PointerPhase::Down, 0.0, 0.0, started_at),
        drag_snapshot,
        drag_revision,
        2,
    );
    assert!(
        recognizer
            .pointer_move(&touch(4, PointerPhase::Move, 0.0, 5.0, started_at))
            .is_some_and(|gesture| gesture.action().partial_eq(&FreeAction))
    );

    let (rotate_snapshot, rotate_revision) = snapshot([GestureRule::new(
        GesturePattern::Rotate {
            threshold_degrees: 45,
            modifiers: Modifiers::none(),
        },
        &RotateAction,
    )]);
    recognizer.pointer_down(
        &touch(5, PointerPhase::Down, 0.0, 0.0, started_at),
        rotate_snapshot.clone(),
        rotate_revision,
        3,
    );
    recognizer.pointer_down(
        &touch(6, PointerPhase::Down, 10.0, 0.0, started_at),
        rotate_snapshot,
        rotate_revision,
        3,
    );
    assert!(
        recognizer
            .pointer_move(&touch(6, PointerPhase::Move, 0.0, 10.0, started_at))
            .is_some_and(|gesture| gesture.action().partial_eq(&RotateAction))
    );

    let (trackpad_snapshot, trackpad_revision) = snapshot([GestureRule::new(
        GesturePattern::TrackpadScale {
            threshold: 4,
            modifiers: Modifiers::none(),
        },
        &TrackpadScaleAction,
    )]);
    let wheel = ScrollWheelEvent {
        delta: ScrollDelta::Pixels(point(px(0.0), px(-4.0))),
        modifiers: Modifiers::none(),
        scroll_source: ScrollWheelSource::PrecisionWheel,
        precise_delta: true,
        ..Default::default()
    };
    assert!(
        recognizer
            .scroll_wheel(&wheel, trackpad_snapshot, trackpad_revision, 4)
            .is_some_and(|gesture| gesture.action().partial_eq(&TrackpadScaleAction))
    );
}
