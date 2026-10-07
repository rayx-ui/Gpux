//! Test windows report the cursor like a platform does: the position and modifiers of the last
//! simulated pointer event. Windows whose hover follows activation (macOS) read the cursor from
//! the platform on every draw, so hover and pointer-dependent styles must survive redraws of an
//! active test window on every OS.

use gpux::{
    Context, IntoElement, Modifiers, MouseButton, Render, ScrollDelta, ScrollWheelEvent,
    TestAppContext, TestWindowOptions, Window, div, point, prelude::*, px, size,
};
use std::{cell::Cell, rc::Rc};

struct Target {
    hovered: Rc<Cell<bool>>,
}

impl Render for Target {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let hovered = self.hovered.clone();
        div().size_full().child(
            div()
                .id("target")
                .absolute()
                .top(px(20.))
                .left(px(20.))
                .w(px(60.))
                .h(px(40.))
                .on_hover(move |is_hovered, _, _| hovered.set(*is_hovered)),
        )
    }
}

#[gpux::test]
fn test_windows_report_the_last_simulated_pointer(cx: &mut TestAppContext) {
    let (_, vcx) = cx.add_window_view_with_options(
        TestWindowOptions::new(size(px(200.), px(100.))),
        |_, _| Target {
            hovered: Rc::new(Cell::new(false)),
        },
    );
    let mut vcx = vcx.clone();
    assert_eq!(vcx.platform_pointer(), (point(px(0.), px(0.)), Modifiers::none()));

    vcx.simulate_mouse_move(point(px(30.), px(40.)), None, Modifiers::shift());
    assert_eq!(vcx.platform_pointer(), (point(px(30.), px(40.)), Modifiers::shift()));

    vcx.simulate_click(point(px(120.), px(50.)), Modifiers::secondary_key());
    assert_eq!(
        vcx.platform_pointer(),
        (point(px(120.), px(50.)), Modifiers::secondary_key())
    );

    vcx.simulate_event(ScrollWheelEvent {
        position: point(px(10.), px(90.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-20.))),
        ..Default::default()
    });
    assert_eq!(vcx.platform_pointer().0, point(px(10.), px(90.)));

    // Key events do not move the cursor.
    vcx.simulate_keystrokes("a");
    assert_eq!(vcx.platform_pointer().0, point(px(10.), px(90.)));
}

#[gpux::test]
fn hover_survives_redraws_of_an_active_window(cx: &mut TestAppContext) {
    let hovered = Rc::new(Cell::new(false));
    let target_hovered = hovered.clone();
    let (_, vcx) = cx.add_window_view_with_options(
        TestWindowOptions::new(size(px(200.), px(100.))),
        move |_, _| Target {
            hovered: target_hovered,
        },
    );
    let mut vcx = vcx.clone();
    vcx.update(|window, _| window.activate_window());
    vcx.run_until_parked();

    vcx.simulate_mouse_move(point(px(50.), px(40.)), None, Modifiers::none());
    assert!(hovered.get());
    for _ in 0..3 {
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
    }
    assert!(hovered.get(), "a redraw must not move the cursor off the target");
    assert_eq!(
        vcx.update(|window, _| window.mouse_position()),
        point(px(50.), px(40.))
    );

    vcx.simulate_mouse_move(point(px(150.), px(90.)), Some(MouseButton::Left), Modifiers::none());
    assert!(!hovered.get());
}
