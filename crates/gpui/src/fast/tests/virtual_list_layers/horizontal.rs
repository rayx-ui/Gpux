//! A horizontal `virtual_list` that is its own scroll container and has no
//! sticky rows composites from a list layer, as a vertical one does: its
//! rows are columns, placed along x by the list's own origins, and the
//! layer holds the columns its viewport shows and the overscan to the left
//! and right of them.

use super::{
    FrameWork, Kind, Page, Step, compare_steps, compare_with_layers_off, difference, mean, open,
    stats, varied_heights, wheel_by, wheel_history,
};
use crate::TestAppContext;
use std::rc::Rc;

const FRAMES: usize = 60;
const DX: f32 = -15.;

/// Turns the wheel by [`DX`] across the list on each of [`FRAMES`] frames
/// after four turns to promote it, returning each frame's work.
fn scroll(cx: &mut TestAppContext, page: Page) -> Vec<FrameWork> {
    let log = page.log.clone();
    let window = open(cx, page, false);
    for _ in 0..4 {
        wheel_by(cx, window, DX, 0.);
    }
    (0..FRAMES)
        .map(|_| {
            let before = stats(cx, window);
            log.borrow_mut().calls.clear();
            wheel_by(cx, window, DX, 0.);
            let after = stats(cx, window);
            FrameWork {
                counters: difference(&after, &before),
                closure_rows: log.borrow().rows_rendered(),
                calls: log.borrow().calls.clone(),
            }
        })
        .collect()
}

/// After promotion every frame of 10,000 columns of varying widths is shown
/// from the list's layer: composited, rendering at most two columns a frame
/// on average and keeping the others, or painted afresh once at most.
#[crate::test]
fn a_scrolled_horizontal_list_composites_from_its_layer(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let work = scroll(cx, Page::new(Kind::Horizontal, varied_heights()));
    let mut repainted = 0;
    for (frame, work) in work.iter().enumerate() {
        let composited = work.counter("layers_composited");
        repainted += work.counter("layers_repainted");
        assert_eq!(
            composited + work.counter("layers_repainted"),
            1,
            "frame {frame} is shown from the list's layer: {:?}",
            work.counters
        );
        assert_eq!(
            work.counter("layer_bypasses.ineligible"),
            0,
            "frame {frame}: a horizontal list is eligible"
        );
        if composited == 1 {
            assert!(
                work.counter("list_rows_kept") > 0,
                "frame {frame} keeps the columns the layer holds"
            );
        }
        assert_eq!(
            work.closure_rows as u64,
            work.counter("list_rows_rendered"),
            "frame {frame}: the column closure ran for {:?}",
            work.calls
        );
    }
    assert!(
        repainted <= 1,
        "{repainted} of {FRAMES} frames painted afresh"
    );
    let rendered = mean(&work, "list_rows_rendered");
    assert!(
        rendered <= 2.,
        "{rendered} columns rendered a frame on average, more than 2"
    );
}

/// A history of horizontal wheel turns, whole and fractional, draws what a
/// window without layers draws and publishes the ranges it publishes, the
/// list composited from its layer rather than counted ineligible.
#[crate::test]
fn a_horizontal_list_draws_as_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let history: Vec<(f32, f32)> = wheel_history(4, 40)
        .into_iter()
        .map(|(_, dy)| (dy, 0.))
        .collect();
    let counters = compare_with_layers_off(
        cx,
        || Page::new(Kind::Horizontal, varied_heights()),
        &history,
        "horizontal",
    );
    assert!(counters["layers_composited"] >= 20, "{counters:?}");
    assert_eq!(
        counters
            .get("layer_bypasses.ineligible")
            .copied()
            .unwrap_or(0),
        0,
        "{counters:?}"
    );
}

/// Columns are placed by the list's own origins: a width change of a column
/// left of those shown, and of one the layer holds right of them, moves the
/// columns the layer holds where the new layout puts them; a vertical wheel
/// over the list moves nothing.
#[crate::test]
fn columns_land_where_the_lists_origins_put_them(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let grow: fn(&mut Page) = |page| {
        let mut extents = (*page.extents).clone();
        extents[3] += 7.5;
        extents[12] += 11.;
        page.extents = Rc::new(extents);
    };
    let mut steps = vec![Step::Wheel(-20., 0.); 8];
    steps.push(Step::Edit(grow));
    steps.extend(vec![Step::Wheel(-15., 0.); 8]);
    steps.push(Step::Wheel(0., -30.));
    steps.push(Step::Edit(grow));
    steps.extend(vec![Step::Wheel(10., 0.); 6]);
    let counters = compare_steps(
        cx,
        || Page::new(Kind::Horizontal, varied_heights()),
        &steps,
        1.,
        "widths",
        true,
    );
    assert!(counters["layers_composited"] >= 10, "{counters:?}");
}
