//! Verify mode: a view that reads state its dependencies cannot see (an
//! `Rc<RefCell<_>>` mutated without a notify) is drawn from last frame, and
//! the shadow frame drawn from scratch names it; the frames presented are
//! the ones drawn without verify mode, and a window whose views all report
//! what they read reports nothing.

use std::{cell::RefCell, rc::Rc};

use super::{oracle_support::oracle_app, view_support::VIEWS};
use crate::{
    AppContext as _, Context, Entity, IntoElement, Render, RetentionMismatch, TestAppContext,
    Window, WindowHandle, div, prelude::*, px, size,
};

/// Shows a count it reads from a cell no notify announces.
struct Untracked {
    count: Rc<RefCell<usize>>,
}

impl Render for Untracked {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(80.))
            .h(px(20.))
            .child(format!("count {}", self.count.borrow()))
    }
}

struct Host {
    child: Entity<Untracked>,
    tick: usize,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.child.clone())
            .child(format!("tick {}", self.tick))
    }
}

/// A window drawing a host and its untracked child with view retention,
/// with verify mode on or off, collecting what it reports.
fn open(
    count: Rc<RefCell<usize>>,
    verify: bool,
) -> (
    TestAppContext,
    WindowHandle<Host>,
    Rc<RefCell<Vec<RetentionMismatch>>>,
) {
    let mut cx = oracle_app(0);
    let window = cx.add_window(move |_, cx| Host {
        child: cx.new(|_| Untracked {
            count: count.clone(),
        }),
        tick: 0,
    });
    cx.simulate_window_resize(window.into(), size(px(300.), px(200.)));
    cx.set_draw_on_flush(false);
    let reports = Rc::new(RefCell::new(Vec::new()));
    cx.update_window(window.into(), |_, window, _| {
        window.set_fast_mode(VIEWS);
        window.set_retention_verify(verify);
        // The view's text reaches the frame through the accessibility tree: the test
        // platform's glyphs do not tell `count 0` from `count 7`. Every window of these tests
        // builds it, verified or not, for its whole life.
        std::mem::forget(window.begin_a11y_inspection());
        let reports = reports.clone();
        window.on_retention_mismatch(move |mismatch| reports.borrow_mut().push(mismatch.clone()));
    })
    .unwrap();
    for _ in 0..2 {
        draw(&mut cx, window);
    }
    (cx, window, reports)
}

fn draw(cx: &mut TestAppContext, window: WindowHandle<Host>) -> Vec<String> {
    cx.update_window(window.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.describe_rendered_frame()
    })
    .unwrap()
}

/// Builds the host around its child.
fn tick(cx: &mut TestAppContext, window: WindowHandle<Host>) {
    window
        .update(cx, |host, _, cx| {
            host.tick += 1;
            cx.notify();
        })
        .unwrap();
}

/// The untracked count changes and the host is built: the child is drawn
/// from last frame showing the old count, and verify mode reports it by its
/// id and type, with the difference.
#[test]
fn verify_mode_names_a_view_that_reads_untracked_state() {
    let count = Rc::new(RefCell::new(0));
    let (mut cx, window, reports) = open(count.clone(), true);
    assert!(reports.borrow().is_empty(), "{:?}", reports.borrow());

    *count.borrow_mut() = 7;
    tick(&mut cx, window);
    draw(&mut cx, window);

    let reports = reports.borrow();
    assert_eq!(reports.len(), 1, "{reports:?}");
    let report = &reports[0];
    let child = window
        .read_with(&cx, |host, _| host.child.entity_id())
        .unwrap();
    assert!(
        report
            .global_id
            .ends_with(&format!("view-{}", child.as_u64())),
        "{report:?}"
    );
    assert!(report.view_type.contains("Untracked"), "{report:?}");
    assert!(report.difference.contains("count"), "{report:?}");
}

/// Verify mode draws its shadow frame without presenting it: the frames a
/// window presents with it on are the frames it presents with it off, and
/// a window whose views report what they read reports nothing.
#[test]
fn verify_mode_does_not_change_the_presented_frame() {
    let count = Rc::new(RefCell::new(0));
    let (mut verified, verified_window, reports) = open(count.clone(), true);
    let other_count = Rc::new(RefCell::new(0));
    let (mut plain, plain_window, _) = open(other_count.clone(), false);
    for step in 0..4 {
        if step == 2 {
            assert!(reports.borrow().is_empty(), "{:?}", reports.borrow());
            *count.borrow_mut() = 3;
            *other_count.borrow_mut() = 3;
        }
        tick(&mut verified, verified_window);
        tick(&mut plain, plain_window);
        let presented = draw(&mut verified, verified_window);
        let unverified = draw(&mut plain, plain_window);
        if let Some(difference) =
            crate::fast::verify::first_frame_difference(&presented, &unverified)
        {
            panic!("step {step}: verify mode changed the presented frame: {difference}");
        }
    }
    // Nothing differed before the untracked count changed; after, the view
    // drawn from last frame differs on every frame, since nothing it reads
    // tells it to build.
    assert_eq!(reports.borrow().len(), 2, "{:?}", reports.borrow());
}
