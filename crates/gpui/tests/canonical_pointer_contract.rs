use gpux::{Modifiers, PointerEvent, PointerId, PointerKind, PointerPhase, point, px};

#[test]
fn canonical_pointer_contract_required() {
    let event = PointerEvent::new(
        PointerId::new(7),
        PointerKind::Touch,
        PointerPhase::Down,
        point(px(12.), px(24.)),
        Modifiers::none(),
    );

    assert_eq!(event.id, PointerId::new(7));
    assert_eq!(event.kind, PointerKind::Touch);
    assert_eq!(event.phase, PointerPhase::Down);
    assert!(event.is_primary);
    assert!(event.buttons.is_empty());

    let touch = PointerEvent::touch(
        PointerId::new(8),
        PointerPhase::Move,
        point(px(18.), px(30.)),
        Modifiers::none(),
    );
    assert_eq!(touch.kind, PointerKind::Touch);
}
