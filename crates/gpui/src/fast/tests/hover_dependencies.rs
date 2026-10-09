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

/// A cached cell styled by the hover of a group around it, which it does not
/// insert.
struct GroupCell;

impl Render for GroupCell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("group cell")
            .size_full()
            .bg(PALETTE[0])
            .group_hover("row", |style| style.bg(PALETTE[1]))
    }
}

/// A 200×50 group holding the 50×50 cached cell at its left, and an
/// optional overlay over the group's right half.
struct GroupRoot {
    cell: crate::Entity<GroupCell>,
    covered: bool,
}

impl Render for GroupRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                div().id("row").group("row").w(px(200.)).h(px(50.)).child(
                    self.cell
                        .clone()
                        .cached(crate::StyleRefinement::default().w(px(50.)).h(px(50.))),
                ),
            )
            .when(self.covered, |this| {
                this.child(
                    div()
                        .id("cover")
                        .absolute()
                        .left(px(100.))
                        .top(px(0.))
                        .w(px(100.))
                        .h(px(50.))
                        .occlude(),
                )
            })
    }
}

fn group_window() -> (crate::TestAppContext, crate::WindowHandle<GroupRoot>) {
    open(|_, cx| GroupRoot {
        cell: cx.new(|_| GroupCell),
        covered: false,
    })
}

/// Moves the pointer to `position`, then has `update` change the root and
/// draws, in one update so the frame drawn is the one the change shows in.
/// Returns the cell's background and the frame's work.
fn group_frame(
    cx: &mut crate::TestAppContext,
    window: crate::WindowHandle<GroupRoot>,
    position: crate::Point<crate::Pixels>,
    update: impl FnOnce(&mut GroupRoot),
) -> (crate::PreparedBackground, crate::FastStats) {
    use crate::InputEvent as _;
    cx.update_window(window.into(), |root, window, cx| {
        window.dispatch_event(
            crate::MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        let root = root.downcast::<GroupRoot>().unwrap();
        root.update(cx, |root, cx| {
            update(root);
            cx.notify();
        });
        window.draw(cx).clear(cx);
        (
            window.rendered_frame.scene.quads[0].background.clone(),
            window.fast_frame_stats(),
        )
    })
    .unwrap()
}

/// Found by review: a cached cell reads the hover of the group around it,
/// a hitbox it does not insert. With the pointer over the group beside the
/// cell, away from the cell's own hitboxes, an overlay appearing over the
/// pointer unhovers the group only once the frame's hitboxes are known,
/// after prepaint would have replayed the cell group-hovered.
#[test]
fn cached_view_reading_a_group_hover_drops_it_when_an_overlay_covers_the_group() {
    let (mut cx, window) = group_window();
    let (unhovered, _) = group_frame(&mut cx, window, point(px(400.), px(400.)), |_| {});
    let beside_the_cell = point(px(150.), px(10.));
    let (hovered, _) = group_frame(&mut cx, window, beside_the_cell, |_| {});
    let (hovered_again, _) = group_frame(&mut cx, window, beside_the_cell, |_| {});
    assert_ne!(hovered, unhovered);
    assert_eq!(hovered_again, hovered);
    let (covered, stats) = group_frame(&mut cx, window, beside_the_cell, |root| {
        root.covered = true;
    });
    assert_eq!(
        covered, unhovered,
        "the frame the overlay appeared in showed the cell group-hovered"
    );
    assert_eq!(stats.cached_views_replayed, 0, "{stats:?}");
}

/// Found by review: the group around a cached cell is inserted under a new
/// hitbox id each frame its owner is drawn, so after one replay the hover the
/// cell recorded named a hitbox no longer hit tested, read as unhovered
/// forever. A cell that read a hover it does not own is never replayed, so
/// the pointer entering the group beside it shows it group-hovered.
#[test]
fn cached_view_reading_a_group_hover_follows_the_group_its_owner_redraws() {
    let (mut cx, window) = group_window();
    let away = point(px(400.), px(400.));
    let (unhovered, _) = group_frame(&mut cx, window, away, |_| {});
    for _ in 0..3 {
        assert_eq!(group_frame(&mut cx, window, away, |_| {}).0, unhovered);
    }
    let (background, stats) = group_frame(&mut cx, window, point(px(150.), px(10.)), |_| {});
    assert_ne!(
        background, unhovered,
        "the cell kept its group unhovered after the pointer entered the group"
    );
    assert_eq!(stats.cached_views_replayed, 0, "{stats:?}");
}

/// A cached cell whose hovers are all its own is still replayed with the
/// pointer away from it.
#[test]
fn cached_view_reading_only_its_own_hovers_is_replayed_away_from_the_pointer() {
    use crate::InputEvent as _;
    let (mut cx, window) = open(|_, cx| HoverRoot {
        cell: cx.new(|_| HoverCell),
    });
    let replayed = (0..3)
        .map(|_| {
            cx.update_window(window.into(), |root, window, cx| {
                window.dispatch_event(
                    crate::MouseMoveEvent {
                        position: point(px(400.), px(400.)),
                        pressed_button: None,
                        modifiers: Default::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
                root.downcast::<HoverRoot>()
                    .unwrap()
                    .update(cx, |_, cx| cx.notify());
                window.draw(cx).clear(cx);
                window.fast_frame_stats().cached_views_replayed
            })
            .unwrap()
        })
        .last();
    assert_eq!(replayed, Some(1));
}

/// A cached view of interactive boxes without hover styles, and one box with
/// a hover style in a corner.
struct Panel {
    styled_corner: bool,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("panel")
            .size_full()
            .on_click(|_, _, _| {})
            .child(div().id("plain").size(px(30.)).on_click(|_, _, _| {}))
            .when(self.styled_corner, |this| {
                this.child(
                    div()
                        .id("corner")
                        .size(px(10.))
                        .bg(PALETTE[0])
                        .hover(|style| style.bg(PALETTE[1])),
                )
            })
    }
}

struct PanelRoot {
    panel: crate::Entity<Panel>,
}

impl Render for PanelRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            self.panel
                .clone()
                .cached(crate::StyleRefinement::default().w(px(100.)).h(px(100.))),
        )
    }
}

/// Draws `frames` frames with the pointer at `position`, each with the root
/// notified, and returns how many replayed the panel.
fn replays_under(styled_corner: bool, position: crate::Point<crate::Pixels>, frames: usize) -> u64 {
    use crate::InputEvent as _;
    let (mut cx, window) = open(move |_, cx| PanelRoot {
        panel: cx.new(|_| Panel { styled_corner }),
    });
    (0..frames)
        .map(|_| {
            cx.update_window(window.into(), |root, window, cx| {
                window.dispatch_event(
                    crate::MouseMoveEvent {
                        position,
                        pressed_button: None,
                        modifiers: Default::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
                root.downcast::<PanelRoot>()
                    .unwrap()
                    .update(cx, |_, cx| cx.notify());
                window.draw(cx).clear(cx);
                window.fast_frame_stats().cached_views_replayed
            })
            .unwrap()
        })
        .sum()
}

/// Found by gpux's cached-view tests: every interactive box published its
/// hover by reading it, so a cached view under the pointer always counted as
/// reading a hover and was built every frame. Only hovers something paints
/// by are recorded, and only where those hitboxes are does the pointer keep a
/// view from being replayed.
#[test]
fn a_cached_view_is_replayed_under_the_pointer_unless_it_paints_that_hover() {
    let over_plain = point(px(15.), px(15.));
    // No hover style: replayed with the pointer over it.
    assert_eq!(replays_under(false, over_plain, 4), 4);
    // A hover style elsewhere in the view: still replayed over the rest.
    assert_eq!(replays_under(true, point(px(60.), px(60.)), 4), 4);
    // Over the styled box, below the plain one: built every frame, so its
    // hover is never stale.
    assert_eq!(replays_under(true, point(px(5.), px(35.)), 4), 0);
}
