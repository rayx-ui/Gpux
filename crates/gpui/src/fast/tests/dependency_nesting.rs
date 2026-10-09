//! Nested recordings: a record's own dependencies exclude what the records
//! nested in it read, its subtree dependencies include them, and replaying a
//! reused record counts as reading through a nested subtree and re-registers
//! its entities as accessed, so the window's invalidation still reaches it.

use crate::{
    AppContext as _, Entity, Global, ScrollHandle, TestAppContext,
    fast::dependencies::RecordedDependencies,
};

struct Model;

struct Inner;

impl Global for Inner {}

struct Outer;

impl Global for Outer {}

fn models(cx: &mut TestAppContext) -> [Entity<Model>; 3] {
    cx.update(|cx| [cx.new(|_| Model), cx.new(|_| Model), cx.new(|_| Model)])
}

/// An outer record reads `a`, a nested one reads `b`, and the outer reads
/// `c` after it, with a global and a state each.
fn nested_reads(
    cx: &mut TestAppContext,
    [a, b, c]: &[Entity<Model>; 3],
    inner_state: &ScrollHandle,
) -> (RecordedDependencies, RecordedDependencies) {
    cx.update(|cx| {
        cx.set_global(Inner);
        cx.set_global(Outer);
        let outer = cx.begin_recording_dependencies();
        a.read(cx);
        cx.global::<Outer>();
        let inner = cx.begin_recording_dependencies();
        b.read(cx);
        cx.global::<Inner>();
        crate::fast::dependencies::note_state_read(cx, &inner_state.fast_version());
        let inner = cx.finish_recording_dependencies(inner);
        c.read(cx);
        (cx.finish_recording_dependencies(outer), inner)
    })
}

/// A record's own reads leave out what a nested record read; its subtree
/// reads include it; the nested record's own reads are all its reads.
#[test]
fn own_dependencies_exclude_nested_records_and_subtree_ones_include_them() {
    let mut cx = TestAppContext::single();
    let models = models(&mut cx);
    let state = ScrollHandle::new();
    let (outer, inner) = nested_reads(&mut cx, &models, &state);
    let [a, b, c] = models.map(|model| model.entity_id());
    let sorted = |mut ids: Vec<_>| {
        ids.sort();
        ids
    };

    assert_eq!(&*outer.own.entities, &sorted(vec![a, c])[..]);
    assert_eq!(&*outer.all.entities, &sorted(vec![a, b, c])[..]);
    assert_eq!(&*inner.own.entities, &[b]);
    assert_eq!(&*inner.all.entities, &[b]);

    assert_eq!(outer.own.globals.len(), 1);
    assert_eq!(outer.all.globals.len(), 2);
    assert_eq!(inner.own.globals.len(), 1);

    assert!(outer.own.states.is_empty());
    assert_eq!(outer.all.states.len(), 1);
    assert_eq!(inner.own.states.len(), 1);
}

/// A change to what only the nested record read leaves the outer record's
/// own reads standing and makes its subtree reads stale.
#[test]
fn a_change_inside_a_nested_record_is_only_the_subtree_s() {
    let mut cx = TestAppContext::single();
    let models = models(&mut cx);
    let state = ScrollHandle::new();
    let (outer, _) = nested_reads(&mut cx, &models, &state);
    models[1].update(&mut cx, |_, cx| cx.notify());
    state.scroll_to_item(4);
    cx.update(|cx| {
        cx.global_mut::<Inner>();
        // Notified after an update: a change for every reader.
        assert!(!cx.dependencies_changed(&outer.own, false));
        assert!(cx.dependencies_changed(&outer.all, false));
    });
}

/// Replaying a reused record inside an open one counts as reading through a
/// nested subtree: in the open record's subtree reads, not its own.
#[test]
fn a_replayed_record_is_read_through_a_nested_subtree() {
    let mut cx = TestAppContext::single();
    let models = models(&mut cx);
    let state = ScrollHandle::new();
    let (_, reused) = nested_reads(&mut cx, &models, &state);
    let [a, b, _] = models.clone().map(|model| model.entity_id());
    let open = cx.update(|cx| {
        let open = cx.begin_recording_dependencies();
        models[0].read(cx);
        cx.replay_dependencies(&reused.all);
        cx.finish_recording_dependencies(open)
    });
    assert_eq!(&*open.own.entities, &[a]);
    let mut all = [a, b];
    all.sort();
    assert_eq!(&*open.all.entities, &all[..]);
    assert_eq!(open.all.globals.len(), 1);
    assert_eq!(open.all.states.len(), 1);
}

/// Replaying a reused record registers its entities as accessed, as
/// building it would have, so a notify of one still reaches the window.
#[test]
fn replaying_a_record_re_registers_its_entities() {
    let mut cx = TestAppContext::single();
    let models = models(&mut cx);
    let state = ScrollHandle::new();
    let (_, reused) = nested_reads(&mut cx, &models, &state);
    cx.update(|cx| {
        cx.entities.clear_accessed();
        cx.replay_dependencies(&reused.all);
        assert!(
            cx.entities
                .accessed_entities
                .borrow()
                .contains(&models[1].entity_id())
        );
    });
}
