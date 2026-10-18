//! A scrolling `div` drawn by a view of its own, beside a parent that renders again between wheel
//! turns: on the frames the parent renders, the view holding the `div` is drawn from the last
//! frame, its records copied, the `div`'s layer's with them. A layer whose content read no hover
//! follows the copy, as a list's layer does, and composites the next wheel turn; one whose
//! content read hovers is painted again then, its hovers judged afresh. Every frame describes
//! itself as one drawn without layers does (what it draws, its hitboxes, listeners, tab stops,
//! dispatch tree and accessibility tree), and clicks on a copied frame and on the composited turn
//! that follows reach the rows they reach without layers.

use crate::fast::tests::layer_support::{add_window, draw, make_reference, with_window};
use crate::{
    AnyWindowHandle, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, PlatformInput,
    Render, Role, ScrollDelta, ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _,
    TestAppContext, TouchPhase, Window, div, hsla, point, px,
};
use std::{cell::RefCell, rc::Rc};

/// The rows clicked, in order.
type Clicked = Rc<RefCell<Vec<usize>>>;

/// The scrolling `div` and its rows, a view of their own.
struct Pane {
    hovers: bool,
    clicked: Clicked,
}

impl Render for Pane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (hovers, clicked) = (self.hovers, self.clicked.clone());
        // An opaque panel: what the layer's tiles are cleared with.
        div()
            .id("pane")
            .bg(crate::white())
            .w(px(200.))
            .h(px(100.))
            .overflow_y_scroll()
            .children((0..200usize).map(move |row| {
                let clicked = clicked.clone();
                // A named list item that hears clicks, reading no hover.
                let block = div()
                    .id(("row", row))
                    .role(Role::ListItem)
                    .aria_label(format!("row {row}"))
                    .h(px(20.))
                    .w_full()
                    .bg(hsla(row as f32 / 200., 0.5, 0.5, 1.))
                    .on_click(move |_, _, _| clicked.borrow_mut().push(row));
                if hovers {
                    block.hover(|style| style.opacity(0.5))
                } else {
                    block
                }
            }))
    }
}

/// The parent: the pane, and a counter of its own that it renders again for.
struct Parent {
    pane: Entity<Pane>,
    ticks: u32,
}

impl Render for Parent {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .bg(crate::white())
            .child(self.pane.clone())
            .child(div().w(px(4. + (self.ticks % 5) as f32)).h(px(10.)))
    }
}

fn open(
    cx: &mut TestAppContext,
    hovers: bool,
    reference: bool,
) -> (AnyWindowHandle, Entity<Parent>, Clicked) {
    let clicked = Clicked::default();
    let parent = Rc::new(RefCell::new(None));
    let window: AnyWindowHandle = add_window(cx, {
        let (parent, clicked) = (parent.clone(), clicked.clone());
        move |_, cx| {
            *parent.borrow_mut() = Some(cx.entity());
            Parent {
                pane: cx.new(|_| Pane { hovers, clicked }),
                ticks: 0,
            }
        }
    })
    .into();
    if reference {
        with_window(cx, window, |window, _| make_reference(window));
    }
    draw(cx, window);
    draw(cx, window);
    let parent = parent.borrow().clone().unwrap();
    (window, parent, clicked)
}

fn wheel(cx: &mut TestAppContext, window: AnyWindowHandle) {
    with_window(cx, window, |window, cx| {
        window.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                // Beside the rows, where no hover changes.
                position: point(px(190.), px(50.)),
                delta: ScrollDelta::Pixels(point(px(0.), px(-15.))),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    });
    draw(cx, window);
}

/// A click at `y` over the pane's rows, then the pointer moving beside them, then a frame.
fn click(cx: &mut TestAppContext, window: AnyWindowHandle, y: f32) {
    let at = point(px(60.), px(y));
    for event in [
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        }),
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Default::default(),
            click_count: 1,
        }),
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(190.), px(y)),
            pressed_button: None,
            modifiers: Default::default(),
        }),
    ] {
        with_window(cx, window, |window, cx| {
            window.dispatch_event(event, cx);
        });
    }
    draw(cx, window);
}

fn tick(cx: &mut TestAppContext, window: AnyWindowHandle, parent: &Entity<Parent>) {
    cx.update(|cx| {
        parent.update(cx, |parent, cx| {
            parent.ticks += 1;
            cx.notify();
        })
    });
    draw(cx, window);
}

/// What one window, layered or the reference, in an application of its own, shows after each
/// of a run of wheel turns, ticks of the parent and, with `clicks`, clicks, accessibility on:
/// each frame's description, the rows clicked, and the layers composited and repainted after
/// promotion. Without clicks a tick stands in for each.
fn session(
    hovers: bool,
    clicks: bool,
    reference: bool,
) -> (Vec<Vec<String>>, Vec<usize>, (u64, u64)) {
    let mut cx = crate::fast::tests::oracle_support::oracle_app(7);
    let cx = &mut cx;
    let (window, parent, clicked) = open(cx, hovers, reference);
    let _inspection = with_window(cx, window, |window, _| window.begin_a11y_inspection());
    // The first inspected frame is drawn from scratch; the next as usual.
    draw(cx, window);
    draw(cx, window);
    for _ in 0..4 {
        wheel(cx, window);
    }
    let stats = |cx: &mut TestAppContext| with_window(cx, window, |window, _| window.fast_stats());
    let before = stats(cx);
    let mut frames = Vec::new();
    // A wheel turn, a tick that copies the pane, a click on that frame, a wheel turn shown from
    // the layer, and a click on that one.
    for step in 0..30 {
        match step % 5 {
            0 | 3 => wheel(cx, window),
            _ if !clicks => tick(cx, window, &parent),
            1 => tick(cx, window, &parent),
            _ => click(cx, window, 15. + (step % 7) as f32 * 11.),
        }
        frames.push(with_window(cx, window, |window, _| {
            window.describe_rendered_frame()
        }));
    }
    let after = stats(cx);
    let clicked = clicked.borrow().clone();
    cx.quit();
    (
        frames,
        clicked,
        (
            after.layers_composited - before.layers_composited,
            after.layers_repainted - before.layers_repainted,
        ),
    )
}

/// Runs [`session`] with layers and as the reference, checking every frame and the rows
/// clicked, and returns the layered window's composited and repainted counts.
fn run(hovers: bool, clicks: bool) -> (u64, u64) {
    let (frames, clicked, counts) = session(hovers, clicks, false);
    let (expected, expected_clicked, _) = session(hovers, clicks, true);
    for (step, (shown, expected)) in frames.iter().zip(&expected).enumerate() {
        if let Some(difference) = crate::first_frame_difference(shown, expected) {
            panic!(
                "hovers {hovers}, step {step}: shown with layers (actual) differs from drawn \
                 without (expected): {difference}"
            );
        }
    }
    assert_eq!(
        clicked, expected_clicked,
        "hovers {hovers}: the rows clicked"
    );
    assert!(
        !clicks || expected_clicked.len() >= 10,
        "the clicks reached rows"
    );
    counts
}

#[test]
fn a_layer_without_hovers_composites_after_its_owner_is_copied() {
    if !crate::fast::layers::COMPILED {
        return;
    }
    // A click into the content paints the layer again (its input is the content's), and is
    // checked on its own.
    run(false, true);
    let (composited, repainted) = run(false, false);
    assert!(
        composited >= 11 && repainted <= 1,
        "the wheel turns after a tick were painted afresh: {composited} composited, \
         {repainted} repainted"
    );
}

#[test]
fn a_layer_whose_content_read_hovers_is_painted_after_its_owner_is_copied() {
    if !crate::fast::layers::COMPILED {
        return;
    }
    // Painted again after every copy, it is demoted before long.
    run(true, true);
    let (composited, _) = run(true, false);
    assert_eq!(
        composited, 0,
        "a layer whose content read hovers composited after a copy"
    );
}
