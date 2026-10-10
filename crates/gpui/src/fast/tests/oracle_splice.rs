//! Splice against frames drawn from scratch: the randomized oracle in splice
//! mode (layout and view retention and splice on) matches the reference
//! window frame by frame across every change kind, and drew views around
//! rebuilt nested views along the way.

use super::oracle_support::{run_lockstep, seeds_and_steps};
use crate::FastMode;

const SPLICE: FastMode = FastMode {
    layout_retention: true,
    view_retention: true,
    splice: true,
    ..FastMode::OFF
};

/// The randomized oracle in splice mode: every frame equals the one drawn
/// from scratch, and over the run views were spliced.
#[test]
fn splice_mode_matches_frames_drawn_from_scratch() {
    let (seeds, steps) = seeds_and_steps(16, 60);
    let mut spliced = 0;
    for seed in seeds {
        let stats = run_lockstep(seed, steps, SPLICE).incremental;
        assert!(stats.views_reused > 0, "seed {seed}: {stats:?}");
        spliced += stats.views_spliced;
    }
    assert!(spliced > 0, "no view was spliced");
}

/// A fixed history of changes nested two views deep: each leaf change
/// splices the panel around it, and every frame equals the frame drawn from
/// scratch.
#[test]
fn nested_leaf_changes_are_spliced_and_match_frames_drawn_from_scratch() {
    use super::oracle_support::{Change, OracleWindow};
    let mut incremental = OracleWindow::open(0, SPLICE);
    let mut reference = OracleWindow::open(0, FastMode::OFF);
    let mut spliced = 0;
    let history = [
        Change::Hover { x: 4000., y: 4000. },
        Change::Redraw,
        Change::Leaf { panel: 0 },
        Change::Leaf { panel: 2 },
        Change::Redraw,
        Change::Leaf { panel: 1 },
        Change::UpdatePanel { panel: 3, value: 5 },
        Change::Leaf { panel: 3 },
    ];
    for (step, change) in history.iter().enumerate() {
        incremental.apply(change);
        reference.apply(change);
        let (expected, _) = reference.draw(true);
        let (actual, stats) = incremental.draw(false);
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            panic!("step {step} ({change:?}): {difference}");
        }
        spliced += stats.views_spliced;
    }
    assert!(spliced > 0, "no panel was spliced around its leaf");
}
