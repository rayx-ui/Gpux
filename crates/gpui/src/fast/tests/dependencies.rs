//! What a subtree reads while it is built, and the changes that make a
//! recording of it stale: one test per read kind (entities, globals, global
//! presence, scroll and list state, ambient input, hovers) and per change
//! rule (notify against update, writes while drawing), idempotent state
//! requests, and the text-input exemption.

use crate::{
    AppContext as _, Context, Entity, EntityInputHandler, Global, InputEvent as _,
    InputHandler as _, ListAlignment, ListOffset, ListState, Modifiers, MouseButton, Pixels,
    Render, ScrollHandle, ScrollMutationSource, TestAppContext, UTF16Selection, Window, div,
    fast::dependencies::RenderDependencies, point, prelude::*, px,
};

struct Model(usize);

struct Setting(usize);

impl Global for Setting {}

/// Records what `read` reads.
fn record(cx: &mut TestAppContext, read: impl FnOnce(&mut crate::App)) -> RenderDependencies {
    cx.update(|cx| {
        let recording = cx.begin_recording_dependencies();
        read(cx);
        cx.finish_recording_dependencies(recording)
    })
}

/// Whether anything `dependencies` recorded changed since, for a view drawn
/// inside a notified one or not.
fn changed(
    cx: &mut TestAppContext,
    dependencies: &RenderDependencies,
    inside_notified: bool,
) -> bool {
    cx.update(|cx| cx.dependencies_changed(dependencies, inside_notified))
}

fn model(cx: &mut TestAppContext) -> Entity<Model> {
    cx.update(|cx| cx.new(|_| Model(0)))
}

/// An entity read while a recording is open is in it, once; an update and a
/// notify after makes the recording stale.
#[test]
fn entity_reads_are_recorded() {
    let mut cx = TestAppContext::single();
    let (read, unread) = (model(&mut cx), model(&mut cx));
    let dependencies = record(&mut cx, |cx| {
        read.read(cx);
        read.read(cx);
    });
    assert_eq!(&*dependencies.entities, &[read.entity_id()]);
    assert!(!changed(&mut cx, &dependencies, true));

    unread.update(&mut cx, |model, cx| {
        model.0 += 1;
        cx.notify();
    });
    assert!(
        !changed(&mut cx, &dependencies, true),
        "an entity not read changed it"
    );

    read.update(&mut cx, |model, cx| {
        model.0 += 1;
        cx.notify();
    });
    assert!(changed(&mut cx, &dependencies, false));
}

/// A notify alone (a wheel, an animation asking the view to draw again)
/// leaves what the entity holds as it was: readers are not stale.
#[test]
fn a_pure_notify_does_not_change_readers() {
    let mut cx = TestAppContext::single();
    let entity = model(&mut cx);
    let dependencies = record(&mut cx, |cx| {
        entity.read(cx);
    });
    cx.update(|cx| cx.notify(entity.entity_id()));
    assert!(!changed(&mut cx, &dependencies, true));
}

/// An update without a notify (a subscriber updated for an event it does not
/// care about) counts only for a view drawn inside a notified one.
#[test]
fn an_update_without_notify_counts_only_inside_a_notified_view() {
    let mut cx = TestAppContext::single();
    let entity = model(&mut cx);
    let dependencies = record(&mut cx, |cx| {
        entity.read(cx);
    });
    entity.update(&mut cx, |model, _| model.0 += 1);
    assert!(!changed(&mut cx, &dependencies, false));
    assert!(changed(&mut cx, &dependencies, true));
    // The notify that follows makes it a change for everyone.
    cx.update(|cx| cx.notify(entity.entity_id()));
    assert!(changed(&mut cx, &dependencies, false));
}

/// An entity written while the window draws (by another subtree being
/// built) changes what a subtree that read it saw; what a subtree writes
/// while it is built itself does not.
#[test]
fn a_write_while_drawing_counts_for_foreign_readers() {
    let mut cx = TestAppContext::single();
    let entity = model(&mut cx);
    let reader = record(&mut cx, |cx| {
        entity.read(cx);
    });
    let writer = record(&mut cx, |cx| {
        entity.update(cx, |model, _| model.0 += 1);
        entity.read(cx);
    });
    assert!(
        changed(&mut cx, &reader, false),
        "a foreign write went unseen"
    );
    assert!(
        !changed(&mut cx, &writer, false),
        "a subtree's own write counted"
    );

    // A notify while drawing marks the entity changed too.
    let reader = record(&mut cx, |cx| {
        entity.read(cx);
    });
    record(&mut cx, |cx| cx.notify(entity.entity_id()));
    assert!(changed(&mut cx, &reader, false));
}

/// Rendering a view leases it without writing to it.
#[test]
fn rendering_a_view_is_not_a_write() {
    let mut cx = TestAppContext::single();
    let entity = model(&mut cx);
    let reader = record(&mut cx, |cx| {
        entity.read(cx);
    });
    record(&mut cx, |cx| {
        crate::fast::dependencies::render_next(cx, entity.entity_id());
        entity.update(cx, |_, _| {});
    });
    assert!(!changed(&mut cx, &reader, false));
}

/// A global read is recorded, and any write to it (set, update, mutable
/// access) makes the recording stale.
#[test]
fn global_reads_are_recorded_and_writes_change_them() {
    let mut cx = TestAppContext::single();
    cx.update(|cx| cx.set_global(Setting(0)));
    let dependencies = record(&mut cx, |cx| {
        cx.global::<Setting>();
    });
    assert_eq!(dependencies.globals.len(), 1);
    assert!(!changed(&mut cx, &dependencies, true));
    cx.update(|cx| cx.global_mut::<Setting>().0 += 1);
    assert!(changed(&mut cx, &dependencies, false));

    let dependencies = record(&mut cx, |cx| {
        cx.try_global::<Setting>();
    });
    cx.update(|cx| cx.update_global::<Setting, _>(|setting, _| setting.0 += 1));
    assert!(changed(&mut cx, &dependencies, false));
}

/// Asking only whether a global is set depends on its presence: writing to
/// it leaves that as it was, inserting or removing it does not.
#[test]
fn global_presence_changes_only_when_it_comes_or_goes() {
    let mut cx = TestAppContext::single();
    cx.update(|cx| cx.set_global(Setting(0)));
    let dependencies = record(&mut cx, |cx| {
        cx.has_global::<Setting>();
    });
    cx.update(|cx| cx.set_global(Setting(1)));
    cx.update(|cx| cx.global_mut::<Setting>().0 += 1);
    assert!(
        !changed(&mut cx, &dependencies, false),
        "a write changed presence"
    );

    cx.update(|cx| {
        cx.remove_global::<Setting>();
    });
    assert!(changed(&mut cx, &dependencies, false));

    let dependencies = record(&mut cx, |cx| {
        cx.has_global::<Setting>();
    });
    cx.update(|cx| cx.set_global(Setting(2)));
    assert!(changed(&mut cx, &dependencies, false));
}

/// A scroll handle's version moves when a request changes where its owner
/// will scroll to, and not when it repeats the request or asks for where
/// the handle already is.
#[test]
fn scroll_handle_requests_move_its_version_only_when_they_change_something() {
    let mut cx = TestAppContext::single();
    let handle = ScrollHandle::new();
    let read = |cx: &mut TestAppContext| {
        record(cx, |cx| {
            crate::fast::dependencies::note_state_read(cx, &handle.fast_version())
        })
    };
    let dependencies = read(&mut cx);
    assert_eq!(dependencies.states.len(), 1);

    handle.scroll_to_item(3);
    assert!(changed(&mut cx, &dependencies, false));
    let dependencies = read(&mut cx);
    handle.scroll_to_item(3);
    assert!(
        !changed(&mut cx, &dependencies, false),
        "a repeated request moved it"
    );

    // A request for the offset the handle is at changes nothing.
    handle.request_offset(point(px(0.), px(0.)), ScrollMutationSource::Programmatic);
    assert!(!changed(&mut cx, &dependencies, false));
    handle.request_offset(point(px(0.), px(-40.)), ScrollMutationSource::Programmatic);
    assert!(changed(&mut cx, &dependencies, false));

    let dependencies = read(&mut cx);
    handle.scroll_to_bottom();
    assert!(changed(&mut cx, &dependencies, false));
    let dependencies = read(&mut cx);
    handle.scroll_to_bottom();
    assert!(!changed(&mut cx, &dependencies, false));
}

/// A list state's version moves when its items or scroll position change
/// from outside, and not for a scroll to where it already is.
#[test]
fn list_state_changes_move_its_version_only_when_they_change_something() {
    let mut cx = TestAppContext::single();
    let list = ListState::new(10, ListAlignment::Top, px(0.));
    let read = |cx: &mut TestAppContext| {
        record(cx, |cx| {
            crate::fast::dependencies::note_state_read(cx, &list.fast_version())
        })
    };
    let dependencies = read(&mut cx);
    let offset = ListOffset {
        item_ix: 4,
        offset_in_item: px(2.),
    };
    list.scroll_to(offset);
    assert!(changed(&mut cx, &dependencies, false));
    let dependencies = read(&mut cx);
    list.scroll_to(offset);
    assert!(
        !changed(&mut cx, &dependencies, false),
        "scrolling to where it is moved it"
    );
    list.scroll_by(px(0.));
    assert!(!changed(&mut cx, &dependencies, false));

    list.splice(2..3, 4);
    assert!(changed(&mut cx, &dependencies, false));
    let dependencies = read(&mut cx);
    list.scroll_to_end();
    assert!(changed(&mut cx, &dependencies, false));
    let dependencies = read(&mut cx);
    list.scroll_to_end();
    assert!(!changed(&mut cx, &dependencies, false));
}

struct Plain;

impl Render for Plain {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Reading the pointer, the modifier keys or the pressed button while drawn
/// depends on them: an event that changes one makes the recording stale, one
/// that leaves them as they were does not.
#[test]
fn ambient_input_reads_are_recorded_and_events_stamp_them() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| Plain);
    let record_window = |cx: &mut TestAppContext, read: fn(&Window)| {
        cx.update_window(window.into(), |_, window, cx| {
            let recording = cx.begin_recording_dependencies();
            read(window);
            cx.finish_recording_dependencies(recording)
        })
        .unwrap()
    };
    let dispatch = |cx: &mut TestAppContext, event: crate::PlatformInput| {
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_event(event, cx);
        })
        .unwrap();
    };
    let move_to = |x: f32| {
        crate::MouseMoveEvent {
            position: point(px(x), px(10.)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }
        .to_platform_input()
    };

    dispatch(&mut cx, move_to(10.));
    let pointer = record_window(&mut cx, |window| {
        window.mouse_position();
    });
    let keys = record_window(&mut cx, |window| {
        window.modifiers();
    });
    let button = record_window(&mut cx, |window| {
        window.pressed_mouse_button();
    });
    dispatch(&mut cx, move_to(10.));
    assert!(
        !changed(&mut cx, &pointer, false),
        "a move to the same place changed it"
    );
    dispatch(&mut cx, move_to(30.));
    assert!(changed(&mut cx, &pointer, false));
    assert!(!changed(&mut cx, &keys, false));
    assert!(!changed(&mut cx, &button, false));

    dispatch(
        &mut cx,
        crate::ModifiersChangedEvent {
            modifiers: Modifiers::shift(),
            capslock: Default::default(),
        }
        .to_platform_input(),
    );
    assert!(changed(&mut cx, &keys, false));

    dispatch(
        &mut cx,
        crate::MouseDownEvent {
            button: MouseButton::Left,
            position: point(px(30.), px(10.)),
            modifiers: Modifiers::shift(),
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
    );
    assert!(changed(&mut cx, &button, false));
}

/// Hovers read while a recording is open are recorded with what they read.
#[test]
fn hover_reads_are_recorded() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| Plain);
    cx.update_window(window.into(), |_, window, _| {
        let start = crate::fast::dependencies::begin_recording(window);
        let hovered = crate::HitboxId::placeholder().is_hovered(window);
        let reads = crate::fast::dependencies::finish_recording(window, start);
        assert_eq!(reads, vec![(crate::HitboxId::placeholder(), hovered)]);
    })
    .unwrap();
}

/// A text input the platform asks for its selection every frame.
struct Field {
    asked: usize,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl EntityInputHandler for Field {
    fn text_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        self.asked += 1;
        None
    }

    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        _: &str,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        _: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }

    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: crate::Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<crate::Bounds<Pixels>> {
        None
    }

    fn character_index_for_point(
        &mut self,
        _: crate::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// The platform's questions to a text input (its selection, its text) are
/// answered through an update of the input's view, which does not count as
/// changing it; a view that read it is not built again for them.
#[test]
fn text_input_queries_do_not_change_the_input() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| Field { asked: 0 });
    let field = window.root(&mut cx).unwrap();
    let dependencies = record(&mut cx, |cx| {
        field.read(cx);
    });
    cx.update_window(window.into(), |_, window, cx| {
        let mut handler = crate::ElementInputHandler::new(Default::default(), field.clone());
        handler.selected_text_range(false, window, cx);
        handler.text_for_range(0..0, &mut None, window, cx);
    })
    .unwrap();
    assert_eq!(field.read_with(&cx, |field, _| field.asked), 1);
    assert!(!changed(&mut cx, &dependencies, true));
    // The same update outside a query is a change for a view drawn inside a
    // notified one.
    field.update(&mut cx, |field, _| field.asked += 1);
    assert!(changed(&mut cx, &dependencies, true));
}
