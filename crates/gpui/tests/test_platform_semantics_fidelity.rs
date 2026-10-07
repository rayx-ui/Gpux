//! What tests observe matches what the window paints: devtools snapshots (and the semantic
//! locators reading them) report fixed and sticky elements at the bounds they paint and
//! hit-test at, and test windows record the accessibility tree updates a platform adapter would
//! receive.

use gpux::{
    AnyWindowHandle, Context, IntoElement, Render, Role, ScrollHandle, TestAppContext,
    TestWindowOptions, VisualTestContext, Window, div, point, prelude::*, px, rgb,
    semantic_query::Query, size,
};
use std::{cell::Cell, rc::Rc};

/// A scroll container at (20, 30), 200×150, holding sticky section headers, rows, and a fixed
/// badge that counts its clicks.
struct Positioned {
    scroll: ScrollHandle,
    badge_clicks: Rc<Cell<usize>>,
}

impl Render for Positioned {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.badge_clicks.clone();
        div().size_full().child(
            div()
                .id("scroller")
                .absolute()
                .top(px(30.))
                .left(px(20.))
                .w(px(200.))
                .h(px(150.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .children(["first", "second"].into_iter().map(|section| {
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id(gpux::SharedString::from(format!("header-{section}")))
                                .sticky()
                                .top(px(0.))
                                .h(px(30.))
                                .bg(rgb(0x00ff00)),
                        )
                        .children((0..5).map(|_| div().h(px(40.)).bg(rgb(0x808080))))
                }))
                .child(
                    div()
                        .id("badge")
                        .fixed()
                        .top(px(10.))
                        .right(px(10.))
                        .w(px(40.))
                        .h(px(20.))
                        .bg(rgb(0x0000ff))
                        .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
                ),
        )
    }
}

fn open_positioned(cx: &mut TestAppContext) -> (VisualTestContext, Rc<Cell<usize>>) {
    let clicks = Rc::new(Cell::new(0));
    let badge_clicks = clicks.clone();
    let (_, vcx) = cx.add_window_view_with_options(
        TestWindowOptions::new(size(px(240.), px(200.))).scale_factor(1.),
        move |_, _| Positioned {
            scroll: ScrollHandle::new(),
            badge_clicks,
        },
    );
    let mut vcx = vcx.clone();
    vcx.update(|window, _| window.activate_window());
    vcx.run_until_parked();
    (vcx, clicks)
}

fn bounds(vcx: &mut VisualTestContext, id: &str) -> [f32; 4] {
    let element = vcx.locate(Query::id(id)).unwrap();
    let bounds = element.bounds;
    [bounds.x, bounds.y, bounds.width, bounds.height]
}

#[gpux::test]
fn fixed_elements_report_the_bounds_they_paint_at(cx: &mut TestAppContext) {
    let (mut vcx, clicks) = open_positioned(cx);
    // The badge resolves against the scrollport: 10 px in from its top right corner.
    assert_eq!(bounds(&mut vcx, "badge"), [170., 40., 40., 20.]);

    vcx.locator(Query::id("scroller"))
        .scroll(point(px(0.), px(-100.)))
        .unwrap();
    assert_eq!(bounds(&mut vcx, "badge"), [170., 40., 40., 20.]);

    // A locator click lands where the badge paints, so the badge receives it.
    vcx.locator(Query::id("badge")).click().unwrap();
    assert_eq!(clicks.get(), 1);
}

#[gpux::test]
fn sticky_elements_report_their_stuck_bounds(cx: &mut TestAppContext) {
    let (mut vcx, _) = open_positioned(cx);
    assert_eq!(bounds(&mut vcx, "header-first"), [20., 30., 200., 30.]);

    // Scrolled by 100 px, the first header sticks to the top of the scrollport rather than
    // moving up with its section.
    vcx.locator(Query::id("scroller"))
        .scroll(point(px(0.), px(-100.)))
        .unwrap();
    assert_eq!(bounds(&mut vcx, "header-first"), [20., 30., 200., 30.]);

    // The second header (content y 230) pushes the first one out as it arrives.
    vcx.locator(Query::id("scroller"))
        .scroll(point(px(0.), px(-115.)))
        .unwrap();
    assert_eq!(bounds(&mut vcx, "header-second"), [20., 45., 200., 30.]);
    assert_eq!(bounds(&mut vcx, "header-first"), [20., 15., 200., 30.]);
}

struct Labelled;

impl Render for Labelled {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .id("save")
                .role(Role::Button)
                .aria_label("Save")
                .w(px(80.))
                .h(px(24.)),
        )
    }
}

fn labels(updates: &[accesskit::TreeUpdate]) -> Vec<String> {
    updates
        .iter()
        .flat_map(|update| update.nodes.iter())
        .filter_map(|(_, node)| node.label().map(str::to_string))
        .collect()
}

#[gpux::test]
fn test_windows_record_accessibility_tree_updates(cx: &mut TestAppContext) {
    let (_, vcx) = cx.add_window_view(|_, _| Labelled);
    let mut vcx = vcx.clone();
    let window: AnyWindowHandle = vcx.window_handle();
    vcx.run_until_parked();
    // Nothing is sent while no assistive technology is listening.
    assert!(vcx.take_accessibility_updates(window).is_empty());

    vcx.activate_accessibility(window)
        .expect("the test window has an accessibility adapter");
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    let updates = vcx.take_accessibility_updates(window);
    assert!(
        labels(&updates).iter().any(|label| label == "Save"),
        "the frame's tree update carries the button label: {:?}",
        labels(&updates)
    );
    // Taking drains the record.
    assert!(vcx.take_accessibility_updates(window).is_empty());

    vcx.deactivate_accessibility(window);
    vcx.take_accessibility_updates(window);
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    assert!(vcx.take_accessibility_updates(window).is_empty());
}
