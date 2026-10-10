//! Touch scrolling under scroll layers: pans and fling momentum reach a
//! container as the scrolls a wheel sends, and composite; taps and long
//! presses see content painted where it shows. Every frame is compared with
//! a window drawn without layers.

use crate::fast::layers::policy::Decision;
use crate::fast::tests::layer_support::{add_window, decision, set_layers, with_window};
use crate::{
    AnyWindowHandle, Bounds, Context, DispatchPhase, HitboxBehavior, InputEvent as _,
    InteractiveElement as _, IntoElement, LongPressEvent, MouseDownEvent, ParentElement as _,
    Pixels, Point, Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _,
    TestAppContext, TouchEvent, TouchId, TouchPhase, Window, WindowHandle, canvas, div, point, px,
    rgb,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

/// A row's index, the bounds of the hitbox it captured when painted and
/// where the event was.
type Seen = (usize, Bounds<Pixels>, Point<Pixels>);

const ROWS: usize = 60;
const ROW_HEIGHT: f32 = 20.;
const FRAME: Duration = Duration::from_millis(16);

/// A 100 px tall scroll container of sixty 20 px rows at the top left of the
/// window. Every row notes the presses and long presses it sees, with the
/// bounds its hitbox had when it was painted.
struct TouchPage {
    handle: ScrollHandle,
    presses: Rc<RefCell<Vec<Seen>>>,
    long_presses: Rc<RefCell<Vec<Seen>>>,
}

impl Render for TouchPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (presses, long_presses) = (self.presses.clone(), self.long_presses.clone());
        div().size_full().bg(rgb(0xffffff)).child(
            div()
                .id("scroller")
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .w(px(200.))
                .h(px(100.))
                .children((0..ROWS).map(move |index| {
                    let (presses, long_presses) = (presses.clone(), long_presses.clone());
                    div()
                        .h(px(ROW_HEIGHT))
                        .bg(rgb(0x100000 + index as u32 * 0x10))
                        .child(
                            canvas(
                                |bounds, window, _| {
                                    window.insert_hitbox(bounds, HitboxBehavior::Normal)
                                },
                                move |_, hitbox, window, _| {
                                    let pressed = hitbox.clone();
                                    window.on_mouse_event(
                                        move |event: &MouseDownEvent, phase, window, _| {
                                            if phase == DispatchPhase::Bubble
                                                && pressed.is_hovered(window)
                                            {
                                                presses.borrow_mut().push((
                                                    index,
                                                    pressed.bounds,
                                                    event.position,
                                                ));
                                            }
                                        },
                                    );
                                    window.on_mouse_event(
                                        move |event: &LongPressEvent, phase, window, _| {
                                            if phase == DispatchPhase::Bubble
                                                && event.phase == TouchPhase::Started
                                                && hitbox.is_hovered(window)
                                            {
                                                long_presses.borrow_mut().push((
                                                    index,
                                                    hitbox.bounds,
                                                    event.start_position,
                                                ));
                                                window.prevent_default();
                                            }
                                        },
                                    );
                                },
                            )
                            .size_full(),
                        )
                })),
        )
    }
}

/// A [`TouchPage`] in a window with scroll layers and in one drawing every
/// frame from scratch, which snaps what layers snap: both get the same touches at the same times.
struct Pair {
    layered: WindowHandle<TouchPage>,
    plain: WindowHandle<TouchPage>,
}

impl Pair {
    fn open(cx: &mut TestAppContext) -> Self {
        let open = |cx: &mut TestAppContext, layers: bool| {
            let window = add_window(cx, |_, _| TouchPage {
                handle: ScrollHandle::new(),
                presses: Default::default(),
                long_presses: Default::default(),
            });
            with_window(cx, window.into(), |window, cx| {
                if layers {
                    set_layers(window, true);
                } else {
                    crate::fast::tests::layer_support::make_reference(window);
                }
                window.draw(cx).clear(cx);
                window.draw(cx).clear(cx);
            });
            window
        };
        Pair {
            layered: open(cx, true),
            plain: open(cx, false),
        }
    }

    fn windows(&self) -> [AnyWindowHandle; 2] {
        [self.layered.into(), self.plain.into()]
    }

    /// Sends both windows a touch at `y`, a frame after the last one, and
    /// draws the frame that follows, checking both draw the same. Returns
    /// what the layered window's container decided in it, if it was
    /// prepainted.
    fn touch(
        &self,
        cx: &mut TestAppContext,
        phase: TouchPhase,
        y: f32,
        label: &str,
    ) -> Option<Decision> {
        cx.executor().advance_clock(FRAME);
        for window in self.windows() {
            with_window(cx, window, |window, cx| {
                window.dispatch_event(
                    TouchEvent {
                        id: TouchId(1),
                        phase,
                        position: point(px(50.), px(y)),
                        predicted_position: None,
                        force: None,
                        timestamp: None,
                    }
                    .to_platform_input(),
                    cx,
                );
            });
        }
        self.frame(cx, label)
    }

    /// Runs what both windows scheduled for their next frame (a fling's
    /// momentum), a frame after the last one, and draws it.
    fn momentum_frame(&self, cx: &mut TestAppContext, label: &str) -> Option<Decision> {
        cx.executor().advance_clock(FRAME);
        for window in self.windows() {
            with_window(cx, window, |window, cx| {
                window.simulate_next_frame(cx);
            });
        }
        self.frame(cx, label)
    }

    /// Draws a frame in both windows and checks they drew the same and are
    /// scrolled as far.
    fn frame(&self, cx: &mut TestAppContext, label: &str) -> Option<Decision> {
        let before = with_window(cx, self.layered.into(), |window, _| {
            window.fast_layers.frame
        });
        let mut drawn = Vec::new();
        for window in self.windows() {
            drawn.push(with_window(cx, window, |window, cx| {
                window.draw(cx).clear(cx);
                window.painted_primitives()
            }));
        }
        assert_eq!(
            self.offset(cx, self.layered),
            self.offset(cx, self.plain),
            "{label}"
        );
        if drawn[0] != drawn[1] {
            let only = |a: &[String], b: &[String]| -> Vec<String> {
                a.iter().filter(|line| !b.contains(line)).cloned().collect()
            };
            panic!(
                "{label}: only with layers {:#?}, only without {:#?}",
                only(&drawn[0], &drawn[1]),
                only(&drawn[1], &drawn[0])
            );
        }
        let decided = with_window(cx, self.layered.into(), |window, _| {
            window.fast_layers.frame == before + 1
        });
        assert!(decided, "{label}: one frame was drawn");
        decision(cx, self.layered.into())
    }

    fn offset(&self, cx: &mut TestAppContext, window: WindowHandle<TouchPage>) -> Point<Pixels> {
        window
            .read_with(cx, |page, _| page.handle.offset())
            .unwrap()
    }

    fn rebuilds(&self, cx: &mut TestAppContext) -> u64 {
        with_window(cx, self.layered.into(), |window, _| {
            window.fast_layers.input_rebuilds
        })
    }

    fn seen(
        &self,
        cx: &mut TestAppContext,
        pick: impl Fn(&TouchPage) -> Vec<Seen>,
    ) -> (Vec<Seen>, Vec<Seen>) {
        let layered = self.layered.read_with(cx, |page, _| pick(page)).unwrap();
        let plain = self.plain.read_with(cx, |page, _| pick(page)).unwrap();
        (layered, plain)
    }

    /// Pans the page up by 10 px a frame from `from`, `steps` times,
    /// leaving the touch down. Returns the decisions of the frames.
    fn pan(&self, cx: &mut TestAppContext, from: f32, steps: usize) -> Vec<Option<Decision>> {
        let mut decisions = vec![self.touch(cx, TouchPhase::Started, from, "touch down")];
        for step in 1..=steps {
            decisions.push(self.touch(
                cx,
                TouchPhase::Moved,
                from - 10. * step as f32,
                &format!("pan step {step}"),
            ));
        }
        decisions
    }

    /// Pans and lets go while moving, then runs the fling's momentum until
    /// it ends. Returns the decisions of the momentum frames.
    fn fling(&self, cx: &mut TestAppContext) -> Vec<Option<Decision>> {
        self.pan(cx, 90., 6);
        self.touch(cx, TouchPhase::Ended, 30., "touch up");
        let mut decisions = Vec::new();
        for frame in 0..600 {
            let pending = with_window(cx, self.layered.into(), |window, _| {
                !window.next_frame_callbacks.borrow().is_empty()
            });
            if !pending {
                break;
            }
            decisions.push(self.momentum_frame(cx, &format!("momentum frame {frame}")));
        }
        decisions
    }
}

fn composited(decisions: &[Option<Decision>]) -> usize {
    decisions
        .iter()
        .filter(|decision| **decision == Some(Decision::Composite))
        .count()
}

/// A pan scrolls the container through the scroll its moves are recognized
/// as, which the container notes as a wheel's: once promoted, its frames
/// composite, and no move paints the content again.
#[crate::test]
fn a_pan_composites(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let pair = Pair::open(cx);
    let decisions = pair.pan(cx, 95., 8);
    let scrolled = pair.offset(cx, pair.layered);
    assert!(
        scrolled.y < px(-50.),
        "the pan scrolled the page: {scrolled:?}"
    );
    assert!(
        composited(&decisions) >= 4,
        "the pan composited: {decisions:?}"
    );
    assert!(
        !decisions[decisions.len() - 4..].contains(&Some(Decision::Repaint)),
        "{decisions:?}"
    );
    assert_eq!(pair.rebuilds(cx), 0, "a pan's moves are scroll input");
    pair.touch(cx, TouchPhase::Ended, 15., "touch up");
}

/// The momentum of a fling scrolls the container frame by frame after the
/// touch is gone, through the same scroll: its frames composite.
#[crate::test]
fn fling_momentum_composites(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let pair = Pair::open(cx);
    pair.pan(cx, 90., 6);
    let released_at = pair.offset(cx, pair.layered);
    pair.touch(cx, TouchPhase::Ended, 30., "touch up");
    let mut decisions = Vec::new();
    for frame in 0..600 {
        let pending = with_window(cx, pair.layered.into(), |window, _| {
            !window.next_frame_callbacks.borrow().is_empty()
        });
        if !pending {
            break;
        }
        decisions.push(pair.momentum_frame(cx, &format!("momentum frame {frame}")));
    }
    let settled = pair.offset(cx, pair.layered);
    assert!(
        settled.y < released_at.y - px(20.),
        "momentum scrolled on from {released_at:?} to {settled:?}"
    );
    assert!(
        composited(&decisions) >= 5,
        "momentum composited: {decisions:?}"
    );
    assert_eq!(pair.rebuilds(cx), 0, "momentum is scroll input");
}

/// A tap after a fling lands on content that has moved since it was
/// painted: the layer is painted where it shows before the press is
/// dispatched, and the press sees the row and bounds it sees without layers.
#[crate::test]
fn a_tap_after_a_fling_sees_the_content_where_it_shows(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let pair = Pair::open(cx);
    let decisions = pair.fling(cx);
    assert!(composited(&decisions) >= 5, "{decisions:?}");
    let stale = with_window(cx, pair.layered.into(), |window, _| {
        window
            .fast_layers
            .layers
            .values()
            .any(|layer| layer.input.stale != Point::default())
    });
    assert!(stale, "the content shows scrolled since it was painted");
    assert_eq!(pair.rebuilds(cx), 0);

    // Past the multi-tap interval of the fling's touch.
    cx.executor().advance_clock(Duration::from_millis(600));
    for window in pair.windows() {
        with_window(cx, window, |window, cx| {
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                window.dispatch_event(
                    TouchEvent {
                        id: TouchId(2),
                        phase,
                        position: point(px(50.), px(45.)),
                        predicted_position: None,
                        force: None,
                        timestamp: None,
                    }
                    .to_platform_input(),
                    cx,
                );
            }
        });
    }
    assert_eq!(pair.rebuilds(cx), 1, "the tap painted the layer first");
    let (layered, plain) = pair.seen(cx, |page| page.presses.borrow().clone());
    assert_eq!(layered.len(), 1, "one row saw the press: {layered:?}");
    assert_eq!(layered, plain);
    let (_, bounds, position) = layered[0];
    assert!(bounds.contains(&position), "{bounds:?} holds {position:?}");
    for window in pair.windows() {
        with_window(cx, window, |window, cx| window.draw(cx).clear(cx));
    }
}

/// A touch down over content that has moved since it was painted paints
/// the layer first too, and the long press it becomes is seen by the rows as
/// it is without layers.
#[crate::test]
fn a_long_press_sees_the_content_where_it_shows(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let pair = Pair::open(cx);
    let decisions = pair.fling(cx);
    assert!(composited(&decisions) >= 5, "{decisions:?}");
    assert_eq!(pair.rebuilds(cx), 0);

    cx.executor().advance_clock(Duration::from_millis(600));
    for window in pair.windows() {
        with_window(cx, window, |window, cx| {
            window.dispatch_event(
                TouchEvent {
                    id: TouchId(3),
                    phase: TouchPhase::Started,
                    position: point(px(50.), px(45.)),
                    predicted_position: None,
                    force: None,
                    timestamp: None,
                }
                .to_platform_input(),
                cx,
            );
        });
    }
    assert_eq!(
        pair.rebuilds(cx),
        1,
        "the touch down, offered to the content as a touch drag, painted the layer first"
    );
    cx.executor()
        .advance_clock(crate::GestureTuning::default().long_press_duration + FRAME);
    cx.run_until_parked();
    assert_eq!(pair.rebuilds(cx), 1, "which the long press finds painted");
    let (layered, plain) = pair.seen(cx, |page| page.long_presses.borrow().clone());
    assert_eq!(layered.len(), 1, "one row saw the long press: {layered:?}");
    assert_eq!(layered, plain);
    let (_, bounds, position) = layered[0];
    assert!(bounds.contains(&position), "{bounds:?} holds {position:?}");
    pair.touch(cx, TouchPhase::Ended, 45., "touch up");
}
