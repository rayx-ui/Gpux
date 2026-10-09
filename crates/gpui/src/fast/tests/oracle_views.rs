//! View retention against frames drawn from scratch: the randomized oracle
//! in views mode (layout and view retention on, splice off) matches the
//! reference window frame by frame across every change kind and drew views
//! again from last frame along the way, and a window nothing changes in
//! draws every view from last frame and still matches.

use super::oracle_support::{Change, OracleWindow, run_lockstep, seeds_and_steps};
use crate::FastMode;

const VIEWS: FastMode = FastMode {
    layout_retention: true,
    view_retention: true,
    ..FastMode::OFF
};

/// The randomized oracle in views mode: every frame equals the one drawn
/// from scratch, and over the run views were drawn from last frame, built
/// at the layout they kept, and built.
#[test]
fn views_mode_matches_frames_drawn_from_scratch() {
    let (seeds, steps) = seeds_and_steps(16, 60);
    let mut built_at_retained_layout = 0;
    for seed in seeds {
        let stats = run_lockstep(seed, steps, VIEWS).incremental;
        assert!(stats.views_reused > 0, "seed {seed}: {stats:?}");
        assert!(stats.views_built > 0, "seed {seed}: {stats:?}");
        assert!(stats.nodes_claimed > 0, "seed {seed}: {stats:?}");
        built_at_retained_layout += stats.views_built_at_retained_layout;
    }
    assert!(built_at_retained_layout > 0);
}

/// Frames drawn with nothing changed and the pointer outside the window:
/// every view is drawn from last frame, none is rendered, and the frame
/// equals the one drawn from scratch. (A view that read a hover is built
/// while the pointer is over it, with or without view retention.)
#[test]
fn a_still_window_draws_every_view_from_last_frame() {
    let mut incremental = OracleWindow::open(0, VIEWS);
    let mut reference = OracleWindow::open(0, FastMode::OFF);
    for step in 0..4 {
        let change = if step == 0 {
            Change::Hover { x: 4000., y: 4000. }
        } else {
            Change::Redraw
        };
        incremental.apply(&change);
        reference.apply(&change);
        let (expected, _) = reference.draw(true);
        let (actual, stats) = incremental.draw(false);
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            panic!("still frame {step}: {difference}");
        }
        if step > 0 {
            assert_eq!(stats.views_built, 0, "still frame {step}: {stats:?}");
            assert!(stats.views_reused > 0, "still frame {step}: {stats:?}");
        }
    }
}
