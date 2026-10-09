//! GPUI motion and timing state machines read time from the executor clock, so tests drive them
//! with `advance_clock` and simulated frames alone: no sleeping, and the same steps give the same
//! offsets every run.

use gpux::{
    Animation, AnimationExt as _, Context, GestureTuning, IntoElement, LongPressEvent, Pixels,
    Point, Render, ScrollBehavior, ScrollDelta, ScrollHandle, ScrollWheelEvent, TestAppContext,
    TestDispatcher, TouchEvent, TouchId, TouchPhase, VisualTestContext, Window, animated_frame,
    canvas, div, point, prelude::*, px,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

const FRAME: Duration = Duration::from_millis(16);

fn touch(phase: TouchPhase, position: Point<Pixels>) -> TouchEvent {
    TouchEvent {
        id: TouchId(1),
        phase,
        position,
        predicted_position: None,
        force: None,
        // No platform timestamp: the recognizer times the event by the executor clock.
        timestamp: None,
    }
}

/// A 100×100 scrollport over a 100×5000 column, with long presses recorded.
struct Scroller {
    handle: ScrollHandle,
    long_presses: Rc<RefCell<Vec<TouchPhase>>>,
}

impl Render for Scroller {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let long_presses = self.long_presses.clone();
        div()
            .size_full()
            .child(
                div()
                    .id("scroller")
                    .w(px(100.))
                    .h(px(100.))
                    .overflow_scroll()
                    .track_scroll(&self.handle)
                    .child(div().w(px(100.)).h(px(5000.))),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        window.on_mouse_event(move |event: &LongPressEvent, phase, _, _| {
                            if phase.bubble() {
                                long_presses.borrow_mut().push(event.phase);
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

/// Activates the window: inactive windows throttle frames to about 30 per second, which would
/// skip some of the frames these tests step.
fn activate(cx: &mut VisualTestContext) {
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
}

fn open_scroller(
    cx: &mut TestAppContext,
) -> (
    ScrollHandle,
    Rc<RefCell<Vec<TouchPhase>>>,
    &mut VisualTestContext,
) {
    let handle = ScrollHandle::new();
    let long_presses = Rc::new(RefCell::new(Vec::new()));
    let (_, cx) = cx.add_window_view({
        let handle = handle.clone();
        let long_presses = long_presses.clone();
        move |_, _| Scroller {
            handle,
            long_presses,
        }
    });
    activate(cx);
    (handle, long_presses, cx)
}

#[gpux::test]
fn long_press_fires_at_its_threshold_and_not_before(cx: &mut TestAppContext) {
    let threshold = GestureTuning::default().long_press_duration;
    let (_, long_presses, cx) = open_scroller(cx);

    cx.simulate_event(touch(TouchPhase::Started, point(px(50.), px(50.))));
    cx.executor()
        .advance_clock(threshold - Duration::from_millis(1));
    cx.run_until_parked();
    assert!(
        long_presses.borrow().is_empty(),
        "fired before its threshold"
    );

    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(*long_presses.borrow(), [TouchPhase::Started]);
}

/// Pans the scroller upward at 20 px per frame and releases it, then steps frames until the
/// fling comes to rest. Returns the offset after the pan and the resting offset.
fn fling(cx: &mut VisualTestContext, handle: &ScrollHandle) -> (Pixels, Pixels) {
    let mut y = 90.;
    cx.simulate_event(touch(TouchPhase::Started, point(px(50.), px(y))));
    for _ in 0..4 {
        cx.executor().advance_clock(FRAME);
        y -= 20.;
        cx.simulate_event(touch(TouchPhase::Moved, point(px(50.), px(y))));
    }
    cx.executor().advance_clock(FRAME);
    cx.simulate_event(touch(TouchPhase::Ended, point(px(50.), px(y))));
    let released = handle.offset().y;

    let mut resting = released;
    let mut unchanged_frames = 0;
    for _ in 0..600 {
        cx.advance_frame(FRAME);
        let offset = handle.offset().y;
        if offset == resting {
            unchanged_frames += 1;
            if unchanged_frames == 10 {
                break;
            }
        } else {
            unchanged_frames = 0;
            resting = offset;
        }
    }
    assert_eq!(unchanged_frames, 10, "the fling never came to rest");
    (released, resting)
}

#[gpux::test]
fn touch_fling_decays_to_a_deterministic_rest_offset(cx: &mut TestAppContext) {
    let (handle, _, cx) = open_scroller(cx);
    let (released, resting) = fling(cx, &handle);

    // The pan scrolled the finger's travel; momentum then carried the content further.
    assert!(released < px(0.), "the pan did not scroll: {released:?}");
    assert!(
        resting < released,
        "no momentum after release: {released:?} -> {resting:?}"
    );
    assert_eq!(
        f32::from(released),
        -80.,
        "the pan scrolled a different distance"
    );
    // The fling curve uses `f32::ln` and `f32::powf`, whose last-place precision Rust leaves
    // to the platform, so the resting offset is compared to a thousandth of a pixel across
    // operating systems. Within one process, identical steps give identical offsets exactly
    // (see the same-seed test).
    let resting = f32::from(resting);
    assert!(
        (resting - -699.387_76).abs() < 1e-3,
        "the fling came to rest at {resting}px"
    );
}

#[test]
fn identical_steps_with_the_same_seed_give_identical_fling_offsets() {
    let run = || {
        let mut cx = TestAppContext::build(TestDispatcher::new(7), None);
        let (handle, _, visual) = open_scroller(&mut cx);
        let offsets = fling(visual, &handle);
        cx.quit();
        offsets
    };
    assert_eq!(run(), run());
}

fn redraw(cx: &mut VisualTestContext) {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

#[gpux::test]
fn smooth_programmatic_scroll_reaches_its_target_at_its_duration(cx: &mut TestAppContext) {
    let (handle, _, cx) = open_scroller(cx);

    assert!(handle.scroll_to(
        point(px(0.), px(-1000.)),
        ScrollBehavior::Smooth(Duration::from_millis(300)),
    ));
    // The request is queued on the handle; the scroll owner starts it on its next draw.
    redraw(cx);
    assert_eq!(handle.offset().y, px(0.), "a smooth scroll starts in place");

    cx.advance_frame(Duration::from_millis(150));
    // Cubic ease-out at half the duration: 1 - (1 - 0.5)^3 = 0.875 of the distance.
    assert_eq!(handle.offset().y, px(-875.));

    cx.advance_frame(Duration::from_millis(149));
    assert!(
        handle.offset().y > px(-1000.),
        "arrived before its duration"
    );

    cx.advance_frame(Duration::from_millis(1));
    assert_eq!(handle.offset().y, px(-1000.));

    // Nothing moves once the target is reached.
    cx.advance_frame(Duration::from_millis(500));
    assert_eq!(handle.offset().y, px(-1000.));
}

#[gpux::test]
fn wheel_input_cancels_a_smooth_programmatic_scroll(cx: &mut TestAppContext) {
    let (handle, _, cx) = open_scroller(cx);

    assert!(handle.scroll_to(
        point(px(0.), px(-1000.)),
        ScrollBehavior::Smooth(Duration::from_millis(300)),
    ));
    // The request is queued on the handle; the scroll owner starts it on its next draw.
    redraw(cx);
    cx.advance_frame(Duration::from_millis(150));
    assert_eq!(handle.offset().y, px(-875.));

    cx.simulate_event(ScrollWheelEvent {
        position: point(px(50.), px(50.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(25.))),
        modifiers: Default::default(),
        touch_phase: TouchPhase::Moved,
    });
    assert_eq!(handle.offset().y, px(-850.));

    for _ in 0..30 {
        cx.advance_frame(FRAME);
    }
    assert_eq!(
        handle.offset().y,
        px(-850.),
        "the canceled scroll kept moving"
    );
}

/// Renders a `with_animation` child and records every progress value it is rendered with.
struct Animated {
    progress: Rc<RefCell<Vec<f32>>>,
}

impl Render for Animated {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let progress = self.progress.clone();
        div()
            .size_full()
            .child(div().w(px(10.)).h(px(10.)).with_animation(
                "fade",
                Animation::new(Duration::from_millis(1000)),
                move |element, delta| {
                    progress.borrow_mut().push(delta);
                    element
                },
            ))
    }
}

/// The distinct progress values rendered so far, in order (a view can render twice at one time).
fn distinct(progress: &Rc<RefCell<Vec<f32>>>) -> Vec<f32> {
    let mut values = progress.borrow().clone();
    values.dedup();
    values
}

fn open_animated(cx: &mut TestAppContext) -> (Rc<RefCell<Vec<f32>>>, &mut VisualTestContext) {
    let progress = Rc::new(RefCell::new(Vec::new()));
    let (_, cx) = cx.add_window_view({
        let progress = progress.clone();
        move |_, _| Animated { progress }
    });
    activate(cx);
    (progress, cx)
}

#[gpux::test]
fn with_animation_reports_exact_progress_at_fixed_times(cx: &mut TestAppContext) {
    let (progress, cx) = open_animated(cx);
    assert_eq!(distinct(&progress), [0.]);

    for _ in 0..3 {
        cx.advance_frame(Duration::from_millis(250));
    }
    assert_eq!(distinct(&progress), [0., 0.25, 0.5, 0.75]);

    cx.advance_frame(Duration::from_millis(300));
    assert_eq!(distinct(&progress), [0., 0.25, 0.5, 0.75, 1.]);

    // A finished one-shot animation requests no further frames.
    let renders = progress.borrow().len();
    cx.advance_frame(Duration::from_millis(250));
    assert_eq!(progress.borrow().len(), renders);
}

#[gpux::test]
fn reduced_motion_bypasses_animation(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (progress, cx) = open_animated(cx);
    // The one-shot animation renders its end state at once and schedules no frames.
    assert_eq!(distinct(&progress), [1.]);
    let renders = progress.borrow().len();
    cx.advance_frame(Duration::from_millis(250));
    assert_eq!(progress.borrow().len(), renders);
}

/// An `animated_frame` whose width follows `wide`, recording the eased progress it renders.
struct Frame {
    wide: bool,
    progress: Rc<RefCell<Vec<f32>>>,
}

impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let progress = self.progress.clone();
        animated_frame("frame")
            .duration(Duration::from_millis(200))
            .easing(|delta| delta)
            .w(px(if self.wide { 300. } else { 100. }))
            .h(px(10.))
            .child_with_progress(move |delta, _, _| {
                progress.borrow_mut().push(delta);
                div()
            })
    }
}

#[gpux::test]
fn animated_frame_progress_follows_the_executor_clock(cx: &mut TestAppContext) {
    let progress = Rc::new(RefCell::new(Vec::new()));
    let (frame, cx) = cx.add_window_view({
        let progress = progress.clone();
        move |_, _| Frame {
            wide: false,
            progress,
        }
    });
    activate(cx);
    assert_eq!(distinct(&progress), [1.], "the first render is complete");

    frame.update(cx, |frame, cx| {
        frame.wide = true;
        cx.notify();
    });
    cx.run_until_parked();
    cx.advance_frame(Duration::from_millis(50));
    cx.advance_frame(Duration::from_millis(50));
    cx.advance_frame(Duration::from_millis(100));
    assert_eq!(distinct(&progress), [1., 0., 0.25, 0.5, 1.]);
}

#[gpux::test]
fn inactive_windows_skip_frames_inside_their_inactive_interval(cx: &mut TestAppContext) {
    let progress = Rc::new(RefCell::new(Vec::new()));
    let (_, cx) = cx.add_window_view({
        let progress = progress.clone();
        move |_, _| Animated { progress }
    });
    cx.run_until_parked();
    assert_eq!(distinct(&progress), [0.]);

    // Test windows open inactive: a frame 16 ms after the previous one is skipped...
    cx.advance_frame(Duration::from_millis(250));
    cx.advance_frame(Duration::from_millis(16));
    assert_eq!(distinct(&progress), [0., 0.25]);
    // ...and the pending animation frame arrives once the inactive interval has passed.
    cx.advance_frame(Duration::from_millis(34));
    assert_eq!(distinct(&progress), [0., 0.25, 0.3]);

    // An active window delivers every frame.
    activate(cx);
    cx.advance_frame(Duration::from_millis(16));
    assert_eq!(distinct(&progress), [0., 0.25, 0.3, 0.316]);
}
