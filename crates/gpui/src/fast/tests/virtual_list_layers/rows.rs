//! What each scroll frame of a layered `virtual_list` renders and keeps:
//! 60 wheel frames of 15 px over 10,000 rows, against a `uniform_list` of
//! the same rows.

use super::{FrameWork, Kind, Page, mean, open, scroll_frames, varied_heights};
use crate::TestAppContext;

const FRAMES: usize = 60;
const DY: f32 = -15.;

fn scroll(cx: &mut TestAppContext, page: Page) -> Vec<FrameWork> {
    let log = page.log.clone();
    let window = open(cx, page, false);
    scroll_frames(cx, window, &log, FRAMES, DY)
}

/// After promotion, every frame is shown from the list's layer: composited,
/// rendering the rows the scroll brings within reach, at most two a frame
/// on average, and keeping the others; or, once the rows it added reach
/// the limit that keeps what they read from growing, painted afresh, as a
/// `uniform_list`'s layer is.
#[crate::test]
fn a_scrolled_virtual_list_renders_only_the_rows_a_scroll_reaches(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let work = scroll(cx, Page::new(Kind::Virtual, varied_heights()));
    let mut repainted = 0;
    for (frame, work) in work.iter().enumerate() {
        let composited = work.counter("layers_composited");
        repainted += work.counter("layers_repainted");
        assert_eq!(
            composited + work.counter("layers_repainted"),
            1,
            "frame {frame} is shown from the list's layer"
        );
        if composited == 1 {
            assert!(
                work.counter("list_rows_kept") > 0,
                "frame {frame} keeps the rows the layer holds"
            );
        }
    }
    assert!(
        repainted <= 1,
        "{repainted} of {FRAMES} frames painted afresh"
    );
    let rendered = mean(&work, "list_rows_rendered");
    assert!(
        rendered <= 2.,
        "{rendered} rows rendered a frame on average, more than 2"
    );
}

/// The row closure runs only for the rows the layer renders: no frame
/// renders the first row again to measure the list's width.
#[crate::test]
fn the_row_closure_runs_only_for_rendered_rows(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let work = scroll(cx, Page::new(Kind::Virtual, varied_heights()));
    for (frame, work) in work.iter().enumerate() {
        assert_eq!(
            work.closure_rows as u64,
            work.counter("list_rows_rendered"),
            "frame {frame}: the closure rendered {:?}",
            work.calls
        );
        assert!(
            !work.calls.contains(&(0..1)),
            "frame {frame} measured row 0: {:?}",
            work.calls
        );
    }
}

/// A virtual list of rows all as tall paints within one primitive a frame
/// of a `uniform_list` of the same rows.
#[crate::test]
fn a_virtual_list_paints_what_a_uniform_list_of_its_rows_paints(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let virtual_work = scroll(cx, Page::new(Kind::Virtual, vec![20.; 10_000]));
    let uniform_work = scroll(cx, Page::new(Kind::Uniform, vec![20.; 10_000]));
    let virtual_primitives = mean(&virtual_work, "primitives_painted");
    let uniform_primitives = mean(&uniform_work, "primitives_painted");
    assert!(
        (virtual_primitives - uniform_primitives).abs() <= 1.,
        "a virtual list paints {virtual_primitives} primitives a frame, a uniform list \
         {uniform_primitives}"
    );
    assert_eq!(
        mean(&virtual_work, "layers_composited"),
        mean(&uniform_work, "layers_composited")
    );
}

/// Rendering the rows leases the view holding the list as part of drawing
/// it: the frames build no more views than a `uniform_list`'s, which renders
/// its rows the same way.
#[crate::test]
fn rendering_rows_does_not_change_the_view_holding_the_list(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let virtual_work = scroll(cx, Page::new(Kind::Virtual, vec![20.; 10_000]));
    let uniform_work = scroll(cx, Page::new(Kind::Uniform, vec![20.; 10_000]));
    for counter in ["views_built", "views_reused"] {
        assert_eq!(
            mean(&virtual_work, counter),
            mean(&uniform_work, counter),
            "{counter} a frame"
        );
    }
    assert!(mean(&virtual_work, "views_built") <= 1.);
}

/// A frame that draws the view holding the list from last frame measures
/// nothing; the frame scrolled after it still keeps the measured size, for a
/// `virtual_list` and a `uniform_list` alike.
#[crate::test]
fn a_measurement_survives_a_frame_that_reuses_the_view(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for kind in [Kind::Virtual, Kind::Uniform] {
        let page = Page::new(kind, vec![20.; 10_000]);
        let log = page.log.clone();
        let window = open(cx, page, false);
        scroll_frames(cx, window, &log, 4, DY);
        for round in 0..3 {
            // Nothing changed: the frame draws the page from last frame.
            let built = super::stats(cx, window).views_reused;
            crate::fast::tests::layer_support::draw(cx, window);
            assert!(
                super::stats(cx, window).views_reused > built,
                "{kind:?}, round {round}: the page was drawn from last frame"
            );
            let work = one_turn(cx, window, &log);
            assert_eq!(
                work.counter("layers_composited"),
                1,
                "{kind:?}, round {round}: the frame after the reuse composites"
            );
            // A uniform list's layout renders row 0 on every frame by its own
            // design; the hook keeps the measurement its prepaint would take.
            let measures = work.calls.iter().filter(|range| **range == (0..1)).count();
            let allowed = if kind == Kind::Uniform { 1 } else { 0 };
            assert!(
                measures <= allowed,
                "{kind:?}, round {round}: the frame measured row 0 again: {:?}",
                work.calls
            );
            if kind == Kind::Virtual {
                assert_eq!(
                    work.closure_rows as u64,
                    work.counter("list_rows_rendered"),
                    "{kind:?}, round {round}: {:?}",
                    work.calls
                );
            }
        }
    }
}

/// One wheel turn and what it cost.
fn one_turn(
    cx: &mut TestAppContext,
    window: crate::AnyWindowHandle,
    log: &super::SharedLog,
) -> FrameWork {
    let before = super::stats(cx, window);
    log.borrow_mut().calls.clear();
    super::wheel(cx, window, DY);
    let after = super::stats(cx, window);
    let calls = log.borrow().calls.clone();
    FrameWork {
        counters: super::difference(&after, &before),
        closure_rows: log.borrow().rows_rendered(),
        calls,
    }
}
