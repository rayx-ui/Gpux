//! A scroll layer covers its content from where the container begins: the
//! container's padding and its first child's margin scroll with the
//! children, and a layer whose painted region left them out was painted
//! again on every frame scrolled near the top, until its work budget demoted
//! it.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::layer_extent::
//! ```

use crate::fast::layers::policy::Decision;
use crate::fast::tests::layer_support::*;
use crate::{
    AnyWindowHandle, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, TestAppContext, Window, div, point, px, rgb,
};

/// Where the rows of a [`Page`] begin below its scroll container's top.
#[derive(Clone, Copy)]
enum Lead {
    /// The container's own padding.
    Padding,
    /// The first row's margin.
    Margin,
    /// Padding on every side.
    Around,
}

struct Page {
    lead: Lead,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut scroller = div()
            .id("scroller")
            .overflow_y_scroll()
            .w(px(200.))
            .h(px(100.));
        scroller = match self.lead {
            Lead::Padding => scroller.pt(px(30.)),
            Lead::Margin => scroller,
            Lead::Around => scroller.p(px(30.)),
        };
        let lead = self.lead;
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .child(scroller.children((0..60).map(move |index| {
                let row = div().h(px(20.)).bg(rgb(0x100000 + index as u32 * 0x10));
                if index == 0 && matches!(lead, Lead::Margin) {
                    row.mt(px(30.))
                } else {
                    row
                }
            })))
    }
}

/// A layered window and its reference, scrolled alike.
fn windows(cx: &mut TestAppContext, lead: Lead) -> (AnyWindowHandle, AnyWindowHandle) {
    let layered: AnyWindowHandle = add_window(cx, move |_, _| Page { lead }).into();
    let reference: AnyWindowHandle = add_window(cx, move |_, _| Page { lead }).into();
    with_window(cx, reference, |window, _| make_reference(window));
    for window in [layered, reference] {
        draw(cx, window);
        draw(cx, window);
    }
    (layered, reference)
}

fn drawn(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<String> {
    with_window(cx, window, |window, _| window.painted_primitives())
}

/// Scrolls both windows by `dy` and checks they draw alike; returns what the
/// layered one decided.
fn step(
    cx: &mut TestAppContext,
    (layered, reference): (AnyWindowHandle, AnyWindowHandle),
    dy: f32,
) -> Option<Decision> {
    let position = point(px(100.), px(50.));
    let decision = scroll_at(cx, layered, position, dy);
    scroll_at(cx, reference, position, dy);
    let (actual, expected) = (drawn(cx, layered), drawn(cx, reference));
    if actual != expected {
        let at = actual
            .iter()
            .zip(&expected)
            .position(|(actual, expected)| actual != expected)
            .unwrap_or(actual.len().min(expected.len()));
        panic!(
            "after {dy} ({decision:?}): {} primitives against {}, first difference at {at}:\n  \
             layered:   {:?}\n  reference: {:?}",
            actual.len(),
            expected.len(),
            actual.get(at),
            expected.get(at),
        );
    }
    decision
}

#[crate::test]
fn a_layer_composites_near_the_top_of_padded_content(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for lead in [Lead::Padding, Lead::Margin, Lead::Around] {
        let windows = windows(cx, lead);
        assert_eq!(step(cx, windows, -3.), Some(Decision::Bypass));
        assert_eq!(step(cx, windows, -3.), Some(Decision::Repaint));
        // Still within the lead and a quarter of the viewport of the top.
        let decisions: Vec<_> = (0..12).map(|_| step(cx, windows, -3.)).collect();
        assert!(
            decisions
                .iter()
                .all(|decision| *decision == Some(Decision::Composite)),
            "{decisions:?}"
        );
        // And back to the very top, where the lead shows again.
        let decisions: Vec<_> = (0..14).map(|_| step(cx, windows, 3.)).collect();
        assert!(
            decisions
                .iter()
                .all(|decision| *decision == Some(Decision::Composite)),
            "{decisions:?}"
        );
    }
}

/// The last rows, and the padding below them, show from the layer as they
/// do without it, with the layer painted again where the scroll leaves what
/// it painted.
#[crate::test]
fn a_layer_reaches_the_end_of_padded_content(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for lead in [Lead::Padding, Lead::Margin, Lead::Around] {
        let windows = windows(cx, lead);
        step(cx, windows, -3.);
        step(cx, windows, -3.);
        let mut composited = 0;
        // 1,200 px of content and padding: well past the end.
        for _ in 0..40 {
            if step(cx, windows, -37.) == Some(Decision::Composite) {
                composited += 1;
            }
        }
        assert!(composited >= 20, "{composited} of 40 frames composited");
        for _ in 0..10 {
            step(cx, windows, 9.);
        }
    }
}
