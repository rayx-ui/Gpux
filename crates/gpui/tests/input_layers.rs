use gpux::{Action, InputLayerId, KeyBinding, KeyContext, Keymap, KeymapLayer, Keystroke, actions};

actions!(
    input_layers_test,
    [FrameworkAction, UserAction, ContextAction]
);

fn bindings_for(map: &Keymap, input: &str, contexts: &[&str]) -> Vec<Box<dyn Action>> {
    map.bindings_for_input(
        &[Keystroke::parse(input).unwrap()],
        &contexts
            .iter()
            .map(|context| KeyContext::parse(context).unwrap())
            .collect::<Vec<_>>(),
    )
    .0
    .into_iter()
    .map(|binding| binding.action().boxed_clone())
    .collect()
}

#[test]
fn replacing_one_layer_is_atomic_and_preserves_unrelated_layers() {
    let framework = KeymapLayer::new(
        InputLayerId::new("framework"),
        0,
        [KeyBinding::new("ctrl-s", FrameworkAction, None)],
    );
    let rayx_user = KeymapLayer::new(
        InputLayerId::new("rayx.user"),
        10,
        [KeyBinding::new("ctrl-s", UserAction, None)],
    );
    let mut map = Keymap::from_layers([framework.clone(), rayx_user.clone()]).unwrap();
    assert!(bindings_for(&map, "ctrl-s", &[])[0].partial_eq(&UserAction));
    let captured_revision = map.snapshot();

    let replacement = KeymapLayer::new(
        InputLayerId::new("rayx.user"),
        10,
        [KeyBinding::new("ctrl-s", ContextAction, Some("editor"))],
    );
    let previous = map.set_layer(replacement).unwrap().unwrap();
    assert_eq!(previous.id(), rayx_user.id());
    assert!(bindings_for(&map, "ctrl-s", &[])[0].partial_eq(&FrameworkAction));
    assert!(bindings_for(&map, "ctrl-s", &["editor"])[0].partial_eq(&ContextAction));
    let retained = map.bindings_for_input_at(
        &captured_revision,
        &[Keystroke::parse("ctrl-s").unwrap()],
        &[],
    );
    assert!(retained.0[0].action().partial_eq(&UserAction));

    let before_failure = map.version();
    let duplicate = Keymap::from_layers([framework.clone(), framework]);
    assert!(duplicate.is_err());
    assert_eq!(map.version(), before_failure);
    assert!(bindings_for(&map, "ctrl-s", &[])[0].partial_eq(&FrameworkAction));
}

#[test]
fn context_then_layer_then_rule_order_are_deterministic() {
    let lower = KeymapLayer::new(
        InputLayerId::new("framework"),
        0,
        [KeyBinding::new("ctrl-p", FrameworkAction, Some("editor"))],
    );
    let higher = KeymapLayer::new(
        InputLayerId::new("rayx.user"),
        10,
        [
            KeyBinding::new("ctrl-p", UserAction, Some("editor")),
            KeyBinding::new("ctrl-p", ContextAction, Some("editor")),
        ],
    );
    let map = Keymap::from_layers([higher, lower]).unwrap();

    let actions = bindings_for(&map, "ctrl-p", &["editor"]);
    assert!(actions[0].partial_eq(&ContextAction));
    assert!(actions[1].partial_eq(&UserAction));
    assert!(actions[2].partial_eq(&FrameworkAction));
}

#[test]
fn removing_a_missing_layer_is_a_no_op() {
    let mut map = Keymap::from_layers([KeymapLayer::new(
        InputLayerId::new("framework"),
        0,
        [KeyBinding::new("ctrl-s", FrameworkAction, None)],
    )])
    .unwrap();
    let version = map.version();

    assert!(map.remove_layer(&InputLayerId::new("missing")).is_none());
    assert_eq!(map.version(), version);
}
