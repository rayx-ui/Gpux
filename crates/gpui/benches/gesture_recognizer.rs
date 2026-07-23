use gpux::{
    GestureMap, GestureMapLayer, GesturePattern, GestureRecognizer, GestureRule,
    GestureWheelSource, InputLayerId, Modifiers, ScrollDelta, ScrollWheelEvent, ScrollWheelSource,
    WheelDirection, actions, point, px,
};
use std::time::{Duration, Instant};

actions!(gesture_recognizer_bench, [WheelAction]);

fn main() {
    let mut map = GestureMap::default();
    map.set_layer(GestureMapLayer::new(
        InputLayerId::new("benchmark"),
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
    .expect("benchmark layer is valid");
    let snapshot = map.snapshot();
    let revision = map.version();
    let event = ScrollWheelEvent {
        delta: ScrollDelta::Pixels(point(px(0.0), px(-1.0))),
        scroll_source: ScrollWheelSource::PrecisionWheel,
        precise_delta: true,
        ..Default::default()
    };
    let mut recognizer = GestureRecognizer::default();

    for _ in 0..1_000 {
        let _ = recognizer.scroll_wheel(&event, snapshot.clone(), revision, 1);
    }
    let mut samples = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let started = Instant::now();
        let _ = recognizer.scroll_wheel(&event, snapshot.clone(), revision, 1);
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let p95 = samples[samples.len() * 95 / 100];
    println!("gesture-recognizer p95: {p95:?}");
    assert!(
        p95 <= Duration::from_millis(1),
        "gesture-recognizer p95 exceeded the 1 ms budget: {p95:?}"
    );
}
