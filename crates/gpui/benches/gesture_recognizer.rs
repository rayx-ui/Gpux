use gpux::{
    GestureDirection, GestureMap, GestureMapLayer, GesturePattern, GestureRecognizer, GestureRule,
    InputLayerId, Modifiers, PointerEvent, PointerId, PointerPhase, actions, point, px,
};
use std::time::{Duration, Instant};

actions!(gesture_recognizer_bench, [PanAction]);

fn touch(id: u64, phase: PointerPhase, x: f32, y: f32) -> PointerEvent {
    PointerEvent::touch(
        PointerId::new(id),
        phase,
        point(px(x), px(y)),
        Modifiers::none(),
    )
}

fn main() {
    let mut map = GestureMap::default();
    map.set_layer(GestureMapLayer::new(
        InputLayerId::new("benchmark"),
        0,
        (0..1_000).map(|_| {
            GestureRule::new(
                GesturePattern::Pan {
                    direction: GestureDirection::Free,
                    modifiers: Modifiers::none(),
                    threshold: 1,
                },
                &PanAction,
            )
        }),
    ))
    .expect("benchmark layer is valid");
    let snapshot = map.snapshot();
    let revision = map.version();
    let mut recognizer = GestureRecognizer::default();

    for _ in 0..1_000 {
        run_workload(&mut recognizer, snapshot.clone(), revision);
    }
    let mut samples = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let started = Instant::now();
        run_workload(&mut recognizer, snapshot.clone(), revision);
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let p95 = samples[samples.len() * 95 / 100];
    println!("gesture-recognizer p95 <= 1ms: {p95:?}");
    assert!(
        p95 <= Duration::from_millis(1),
        "gesture-recognizer p95 exceeded the 1 ms budget: {p95:?}"
    );
}

fn run_workload(
    recognizer: &mut GestureRecognizer,
    snapshot: std::rc::Rc<gpux::GestureMapSnapshot>,
    revision: gpux::GestureMapVersion,
) {
    recognizer.cancel();
    for id in 0..5 {
        recognizer.pointer_down(
            &touch(id, PointerPhase::Down, id as f32 * 10.0, 0.0),
            snapshot.clone(),
            revision,
            id as usize,
        );
    }
    for id in 0..5 {
        let _ =
            recognizer.pointer_move(&touch(id, PointerPhase::Move, id as f32 * 10.0 + 2.0, 0.0));
    }
}
