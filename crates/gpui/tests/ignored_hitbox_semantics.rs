//! Hitboxes with `HitboxBehavior::Ignore` stay out of hit testing without dropping their
//! elements' accessibility nodes: an element whose subtree ignores the pointer (RayX's
//! `IgnorePointer` and the pointer detectors built on it) is still in the tree, at its bounds in
//! window pixels, while a click there reaches what lies under it.

use gpux::{
    AnyElement, App, Bounds, Context, Element, ElementId, GlobalElementId, HitboxBehavior,
    InspectorElementId, IntoElement, LayoutId, Pixels, Render, Role, TestAppContext,
    TestWindowOptions, Window, div, prelude::*, px, size,
};
use std::{cell::Cell, rc::Rc};

/// Prepaints its child with every hitbox in it ignored, as RayX's `IgnorePointer` does.
struct IgnorePointer(AnyElement);

impl IntoElement for IgnorePointer {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for IgnorePointer {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.0.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_subtree_hitbox_behavior(HitboxBehavior::Ignore, |window| {
            self.0.prepaint(window, cx)
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0.paint(window, cx);
    }
}

struct Stack {
    below: Rc<Cell<usize>>,
    above: Rc<Cell<usize>>,
}

impl Render for Stack {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (below, above) = (self.below.clone(), self.above.clone());
        div()
            .size_full()
            .relative()
            .child(
                div()
                    .id("below")
                    .role(Role::Button)
                    .aria_label("Below")
                    .absolute()
                    .top(px(20.))
                    .left(px(20.))
                    .w(px(100.))
                    .h(px(40.))
                    .on_click(move |_, _, _| below.set(below.get() + 1)),
            )
            .child(IgnorePointer(
                div()
                    .id("detector")
                    .role(Role::Group)
                    .aria_label("Detector")
                    .absolute()
                    .top(px(10.))
                    .left(px(10.))
                    .w(px(120.))
                    .h(px(60.))
                    .on_click(move |_, _, _| above.set(above.get() + 1))
                    .into_any_element(),
            ))
    }
}

#[gpux::test]
fn an_element_ignoring_the_pointer_keeps_its_node_and_lets_clicks_through(cx: &mut TestAppContext) {
    let below = Rc::new(Cell::new(0));
    let above = Rc::new(Cell::new(0));
    let (_, vcx) = cx.add_window_view_with_options(
        TestWindowOptions::new(size(px(200.), px(120.))).scale_factor(2.),
        {
            let (below, above) = (below.clone(), above.clone());
            move |_, _| Stack { below, above }
        },
    );
    let mut vcx = vcx.clone();
    vcx.run_until_parked();

    // In the tree, at its bounds in window pixels.
    let detector = vcx.locate("role=group name=Detector").unwrap();
    assert_eq!(
        (
            detector.bounds.x,
            detector.bounds.y,
            detector.bounds.width,
            detector.bounds.height
        ),
        (10., 10., 120., 60.)
    );

    // A click on it lands on the button under it.
    vcx.locator("role=group name=Detector").click().unwrap();
    assert_eq!(below.get(), 1, "the click reached what lies under");
    assert_eq!(
        above.get(),
        0,
        "the element ignoring the pointer took no click"
    );
}
