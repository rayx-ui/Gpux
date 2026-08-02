use gpux::{
    Context, DeferredScrollToItem, Instant, IntoElement, ListAlignment, ListOffset, ListState,
    Render, ScrollBehavior, ScrollHandle, ScrollMutationSource, ScrollStrategy,
    StatefulInteractiveElement, TestApp, UniformListScrollHandle, Window, div, list, point,
    prelude::*, px, uniform_list,
};
use std::{cell::Cell, rc::Rc, time::Duration};

struct DivScrollOwner {
    handle: ScrollHandle,
    content_size: Rc<Cell<f32>>,
}

impl Render for DivScrollOwner {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("programmatic-div")
            .w(px(100.))
            .h(px(100.))
            .overflow_scroll()
            .track_scroll(&self.handle)
            .child(
                div()
                    .w(px(self.content_size.get()))
                    .h(px(self.content_size.get())),
            )
    }
}

#[test]
fn programmatic_scroll_owners_div_uses_exact_frames_retarget_and_direct_cancellation() {
    let mut app = TestApp::new();
    let handle = ScrollHandle::new();
    let content_size = Rc::new(Cell::new(500.));
    let mut window = app.open_window({
        let handle = handle.clone();
        let content_size = content_size.clone();
        move |_, _| DivScrollOwner {
            handle,
            content_size,
        }
    });
    window.draw();

    assert!(handle.scroll_to(point(px(-200.), px(-300.)), ScrollBehavior::Instant));
    window.draw();
    assert_eq!(handle.offset(), point(px(-200.), px(-300.)));

    assert!(handle.scroll_to(
        point(px(0.), px(-400.)),
        ScrollBehavior::Smooth(Duration::from_secs(1)),
    ));
    window.draw();
    // A fresh owner assigns generation 1 to its first request; this is the second request.
    window.update(|_, _window, cx| {
        let current_view = cx.entity_id();
        assert!(handle.advance_programmatic_scroll_for_test(
            2,
            Instant::now() + Duration::from_secs(2),
            current_view,
            cx,
        ));
    });
    assert_eq!(handle.offset(), point(px(0.), px(-400.)));

    assert!(handle.scroll_to(
        point(px(-400.), px(-100.)),
        ScrollBehavior::Smooth(Duration::from_secs(1)),
    ));
    window.draw();
    assert!(handle.scroll_to(
        point(px(-100.), px(-200.)),
        ScrollBehavior::Smooth(Duration::from_secs(1)),
    ));
    window.draw();
    window.update(|_, _window, cx| {
        let current_view = cx.entity_id();
        assert!(!handle.advance_programmatic_scroll_for_test(
            3,
            Instant::now() + Duration::from_secs(2),
            current_view,
            cx,
        ));
        assert!(handle.advance_programmatic_scroll_for_test(
            5,
            Instant::now() + Duration::from_secs(2),
            current_view,
            cx,
        ));
    });
    assert_eq!(handle.offset(), point(px(-100.), px(-200.)));

    assert!(handle.scroll_to(
        point(px(-400.), px(-100.)),
        ScrollBehavior::Smooth(Duration::from_secs(1)),
    ));
    window.draw();
    assert!(handle.request_offset(point(px(-40.), px(-60.)), ScrollMutationSource::Wheel,));
    window.draw();
    window.update(|_, _window, cx| {
        let current_view = cx.entity_id();
        assert!(!handle.advance_programmatic_scroll_for_test(
            6,
            Instant::now() + Duration::from_secs(2),
            current_view,
            cx,
        ));
    });
    assert_eq!(handle.offset(), point(px(-40.), px(-60.)));

    assert!(handle.scroll_to(
        point(px(-900.), px(-900.)),
        ScrollBehavior::Smooth(Duration::ZERO),
    ));
    window.draw();
    assert_eq!(handle.offset(), point(px(-400.), px(-400.)));

    assert!(handle.scroll_to(point(px(0.), px(0.)), ScrollBehavior::Instant));
    window.draw();
    assert!(handle.scroll_to(
        point(px(-400.), px(-400.)),
        ScrollBehavior::Smooth(Duration::from_secs(1)),
    ));
    window.draw();
    content_size.set(160.);
    window.update(|_, window, cx| {
        window.refresh();
        cx.notify();
    });
    window.draw();
    window.update(|_, _window, cx| {
        let current_view = cx.entity_id();
        assert!(handle.advance_programmatic_scroll_for_test(
            10,
            Instant::now() + Duration::from_secs(2),
            current_view,
            cx,
        ));
    });
    assert_eq!(handle.offset(), point(px(-60.), px(-60.)));
}

struct UniformScrollOwner {
    handle: UniformListScrollHandle,
    flipped: bool,
}

impl Render for UniformScrollOwner {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        uniform_list("programmatic-uniform", 20, |range, _, _| {
            range.map(|_| div().h(px(20.))).collect::<Vec<_>>()
        })
        .w(px(100.))
        .h(px(100.))
        .track_scroll(&self.handle)
        .y_flipped(self.flipped)
    }
}

#[test]
fn programmatic_scroll_owners_uniform_list_resolves_deferred_strict_nearest_and_reverse_requests() {
    let mut app = TestApp::new();
    let handle = UniformListScrollHandle::new();
    handle.scroll_to_item_with_behavior(
        DeferredScrollToItem {
            item_index: 10,
            strategy: ScrollStrategy::Top,
            offset: 0,
            scroll_strict: true,
        },
        ScrollBehavior::Instant,
    );
    let mut window = app.open_window({
        let handle = handle.clone();
        move |_, _| UniformScrollOwner {
            handle,
            flipped: false,
        }
    });
    window.draw();
    assert_eq!(handle.0.borrow().base_handle.offset().y, px(-200.));

    handle.scroll_to_item_with_behavior(
        DeferredScrollToItem {
            item_index: 15,
            strategy: ScrollStrategy::Nearest,
            offset: 0,
            scroll_strict: false,
        },
        ScrollBehavior::Smooth(Duration::from_secs(1)),
    );
    window.draw();
    window.draw();
    let base_handle = handle.0.borrow().base_handle.clone();
    window.update(|_, _window, cx| {
        let current_view = cx.entity_id();
        assert!(base_handle.advance_programmatic_scroll_for_test(
            2,
            Instant::now() + Duration::from_secs(2),
            current_view,
            cx,
        ));
    });
    assert_eq!(handle.0.borrow().base_handle.offset().y, px(-220.));

    window.update(|owner, window, cx| {
        owner.flipped = true;
        window.refresh();
        cx.notify();
    });
    window.draw();
    handle.scroll_to_item_with_behavior(
        DeferredScrollToItem {
            item_index: 19,
            strategy: ScrollStrategy::Top,
            offset: 0,
            scroll_strict: true,
        },
        ScrollBehavior::Instant,
    );
    window.draw();
    window.draw();
    assert_eq!(handle.0.borrow().base_handle.offset().y, px(0.));
}

struct VariableListOwner {
    state: ListState,
}

impl Render for VariableListOwner {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        list(self.state.clone(), |_, _, _| {
            div().h(px(20.)).into_any_element()
        })
        .w(px(100.))
        .h(px(100.))
    }
}

#[test]
fn programmatic_scroll_owners_list_state_uses_shared_driver_and_preserves_instant_contract() {
    let mut app = TestApp::new();
    let state = ListState::new(20, ListAlignment::Top, px(0.)).measure_all();
    let mut window = app.open_window({
        let state = state.clone();
        move |_, _| VariableListOwner { state }
    });
    window.draw();

    state.scroll_to(ListOffset {
        item_ix: 2,
        offset_in_item: px(0.),
    });
    window.draw();
    assert_eq!(state.scroll_px_offset_for_scrollbar().y, px(-40.));

    state.scroll_to_with_behavior(
        ListOffset {
            item_ix: 3,
            offset_in_item: px(0.),
        },
        ScrollBehavior::Smooth(Duration::ZERO),
    );
    window.draw();
    assert_eq!(state.scroll_px_offset_for_scrollbar().y, px(-60.));

    state.scroll_to_with_behavior(
        ListOffset {
            item_ix: 10,
            offset_in_item: px(0.),
        },
        ScrollBehavior::Smooth(Duration::from_secs(1)),
    );
    window.draw();
    window.update(|_, window, cx| {
        let current_view = cx.entity_id();
        assert!(state.advance_programmatic_scroll_for_test(
            2,
            Instant::now() + Duration::from_secs(2),
            current_view,
            window,
            cx,
        ));
    });
    assert_eq!(state.scroll_px_offset_for_scrollbar().y, px(-200.));

    state.scroll_to_with_behavior(
        ListOffset {
            item_ix: 15,
            offset_in_item: px(0.),
        },
        ScrollBehavior::Smooth(Duration::from_secs(1)),
    );
    window.draw();
    state.scroll_to(ListOffset {
        item_ix: 1,
        offset_in_item: px(0.),
    });
    window.update(|_, window, cx| {
        let current_view = cx.entity_id();
        assert!(!state.advance_programmatic_scroll_for_test(
            3,
            Instant::now() + Duration::from_secs(2),
            current_view,
            window,
            cx,
        ));
    });
    assert_eq!(state.scroll_px_offset_for_scrollbar().y, px(-20.));
}
