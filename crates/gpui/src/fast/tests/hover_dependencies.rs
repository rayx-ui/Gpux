//! Cached views and hovers: a cached view is replayed only while the hovers
//! it read are unchanged, and not while the pointer is over its hitboxes,
//! where hitboxes the frame adds later could change them. Each case was
//! found by the randomized oracle.

use super::oracle_support::{PALETTE, oracle_app, run_lockstep};
use crate::{AppContext as _, Context, FastMode, Render, Window, div, point, prelude::*, px};

fn open<V: Render>(
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) -> (crate::TestAppContext, crate::WindowHandle<V>) {
    let mut cx = oracle_app(0);
    let window = cx.add_window(build);
    cx.update_window(window.into(), |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    })
    .unwrap();
    (cx, window)
}

/// A cached view painted with a hover style, under the pointer.
struct HoverCell;

impl Render for HoverCell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("hover cell")
            .size_full()
            .bg(PALETTE[0])
            .hover(|style| style.bg(PALETTE[1]))
    }
}

struct HoverRoot {
    cell: crate::Entity<HoverCell>,
}

impl Render for HoverRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            self.cell
                .clone()
                .cached(crate::StyleRefinement::default().w(px(50.)).h(px(50.))),
        )
    }
}

/// Found by the oracle: hover styles were updated only by mouse moves, so a
/// cached view painted hovered replayed its hover after a wheel event moved
/// the pointer off it. Cached views now record the hovers they read and are
/// replayed only while each is unchanged.
#[test]
fn cached_view_drops_its_hover_when_a_wheel_event_moves_the_pointer_away() {
    use crate::InputEvent as _;
    let (mut cx, window) = open(|_, cx| HoverRoot {
        cell: cx.new(|_| HoverCell),
    });
    let background = |cx: &mut crate::TestAppContext| {
        cx.update_window(window.into(), |_, window, _| {
            window.rendered_frame.scene.quads[0].background.clone()
        })
        .unwrap()
    };
    let dispatch = |cx: &mut crate::TestAppContext, event: crate::PlatformInput| {
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_event(event, cx);
            window.draw(cx).clear(cx);
        })
        .unwrap();
    };
    dispatch(
        &mut cx,
        crate::MouseMoveEvent {
            position: point(px(10.), px(10.)),
            pressed_button: None,
            modifiers: Default::default(),
        }
        .to_platform_input(),
    );
    let hovered = background(&mut cx);
    dispatch(
        &mut cx,
        crate::ScrollWheelEvent {
            position: point(px(200.), px(200.)),
            delta: crate::ScrollDelta::Pixels(point(px(0.), px(5.))),
            modifiers: Default::default(),
            touch_phase: crate::TouchPhase::Moved,
        }
        .to_platform_input(),
    );
    assert_ne!(
        background(&mut cx),
        hovered,
        "the cached view kept its hover style after the pointer left it"
    );
    cx.update_window(window.into(), |_, window, _| {
        assert!(window.fast_frame_stats().views_built > 0);
    })
    .unwrap();
}

struct CoveredRoot {
    cell: crate::Entity<HoverCell>,
    covered: bool,
}

impl Render for CoveredRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                self.cell
                    .clone()
                    .cached(crate::StyleRefinement::default().w(px(50.)).h(px(50.))),
            )
            .when(self.covered, |this| {
                this.child(
                    div()
                        .id("cover")
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .size(px(60.))
                        .occlude(),
                )
            })
    }
}

/// Found by the oracle: an overlay that covers a hovered cached view with the
/// pointer still is known only once the frame's hitboxes are, after prepaint
/// decided whether to replay the view. A cached view that read hovers is
/// therefore built again while the pointer is over its hitboxes, so the frame
/// the overlay appears in shows the view unhovered, as a frame drawn from
/// scratch does, and nothing notifies the view.
#[test]
fn cached_view_drops_its_hover_in_the_frame_an_overlay_covers_it() {
    use crate::InputEvent as _;
    let (mut cx, window) = open(|_, cx| CoveredRoot {
        cell: cx.new(|_| HoverCell),
        covered: false,
    });
    let cell_background = |cx: &mut crate::TestAppContext| {
        cx.update_window(window.into(), |_, window, _| {
            window.rendered_frame.scene.quads[0].background.clone()
        })
        .unwrap()
    };
    cx.update_window(window.into(), |_, window, cx| {
        window.dispatch_event(
            crate::MouseMoveEvent {
                position: point(px(10.), px(10.)),
                pressed_button: None,
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.draw(cx).clear(cx);
    })
    .unwrap();
    let hovered = cell_background(&mut cx);
    // Drawing the frame again is the window's own business: nothing about the
    // view changed, so its observers must not run.
    let observed = std::rc::Rc::new(std::cell::Cell::new(0));
    let cell = window.read_with(&cx, |root, _| root.cell.clone()).unwrap();
    let _observation = cx.update({
        let observed = observed.clone();
        move |cx| cx.observe(&cell, move |_, _| observed.set(observed.get() + 1))
    });
    // Cover the view and draw in one update, so the frame drawn is the one
    // the overlay appears in.
    cx.update_window(window.into(), |root, window, cx| {
        let root = root.downcast::<CoveredRoot>().unwrap();
        root.update(cx, |root, cx| {
            root.covered = true;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    })
    .unwrap();
    assert_ne!(
        cell_background(&mut cx),
        hovered,
        "the frame the overlay appeared in showed the covered view hovered"
    );
    cx.run_until_parked();
    assert_eq!(
        observed.get(),
        0,
        "drawing the frame again notified the covered view's observers"
    );
}

/// Found by the oracle (seed 31): drawing a frame twice to correct a stale
/// hover let the discarded draw measure a list row in the list's overdraw;
/// the frame shown then skipped the row, already measured, and dropped its
/// element state. Frames are drawn once.
#[test]
fn list_rows_keep_their_element_state_when_an_overlay_covers_a_hovered_cell() {
    let run = run_lockstep(31, 30, FastMode::OFF);
    assert!(run.incremental.cached_views_replayed > 0);
}
