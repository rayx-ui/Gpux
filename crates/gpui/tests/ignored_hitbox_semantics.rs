//! Hitboxes with `HitboxBehavior::Ignore` stay out of hit testing without dropping the component
//! semantics DevKit and accessibility read (RayX's interactive pointer detectors rely on this).

#[test]
fn ignored_hitboxes_do_not_remove_component_semantics() {
    let window = include_str!("../src/window.rs");
    for required in [
        "HitboxBehavior::Ignore",
        "hitbox.behavior != HitboxBehavior::Ignore",
        "DevtoolsPointerAction",
        "with_pointer_semantics_suppressed",
        "semantics.pointer_actions.clear()",
    ] {
        assert!(
            window.contains(required),
            "window.rs must retain {required}"
        );
    }
}
