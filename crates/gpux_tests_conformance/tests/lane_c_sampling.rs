//! The two modes of the scroll-layer suites: what the sampled mode draws and compares, that the
//! full mode is the whole script, and that a sampled run still fails on a defect it compares.
//! A seeded difference is a scroll of one pixel the reference takes at one step and takes back
//! right after, so only that step's frame can show it.
//!
//! ```text
//! cargo test -p gpux-tests-conformance --test lane_c_sampling -- --test-threads=1
//! ```

use scroll_layers_support::{
    Content, MIN_STRETCH, Sampling, Step, UNCOVERED_BY_SAMPLED, compare_with, sample_runs, scrolls,
    scrolls_and_single_pixels,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

mod scroll_layers_support;

#[test]
fn full_mode_compares_every_frame_of_the_script() {
    let script = scrolls_and_single_pixels();
    assert_eq!(script.len(), 135);
    let plan = Sampling::Full.plan(&script);
    assert_eq!(plan.compared, vec![true; 135]);
    assert!(plan.uncovered().is_empty());
}

#[test]
fn sampled_mode_compares_the_documented_frames() {
    let script = scrolls_and_single_pixels();
    let plan = Sampling::Sampled.plan(&script);
    // One entry per step: the layered window is drawn after every one of the 135 either way.
    assert_eq!(plan.compared.len(), 135);
    // The 15 scripted scrolls are compared whole; the stretches of 80 and 40 one-pixel steps
    // keep their first and last 4 steps and every 4th between.
    assert!(plan.compared[..15].iter().all(|&compared| compared));
    assert_eq!(plan.compared_count(), 15 + 26 + 16);
    for (start, len) in [(15, 80), (95, 40)] {
        for p in 0..len {
            assert_eq!(
                plan.compared[start + p],
                p < 4 || p + 4 >= len || p % 4 == 0,
                "position {p} of the stretch of {len}"
            );
        }
        // The stretch's first and last frames are always compared.
        assert!(plan.compared[start] && plan.compared[start + len - 1]);
    }
}

#[test]
fn the_documented_uncovered_steps_match_the_rule() {
    let plan = Sampling::Sampled.plan(&scrolls_and_single_pixels());
    assert_eq!(plan.uncovered(), UNCOVERED_BY_SAMPLED);
    assert_eq!(UNCOVERED_BY_SAMPLED.len() + plan.compared_count(), 135);
    assert!(
        UNCOVERED_BY_SAMPLED
            .iter()
            .all(|&index| !plan.compared[index])
    );
    // Those are the steps of the two stretches that are not first, last or every 4th.
    assert!(
        UNCOVERED_BY_SAMPLED
            .iter()
            .all(|&index| (20..=90).contains(&index) || (100..=130).contains(&index))
    );
}

#[test]
fn short_runs_and_other_steps_are_compared_whole() {
    let short: Vec<Step> = (0..MIN_STRETCH - 1).map(|_| Step::Wheel(0., -1.)).collect();
    let mut script = scrolls();
    script.insert(3, Step::Tick);
    script.extend(short.iter().copied());
    script.extend((0..40).map(|_| Step::Wheel(0., -3.)));
    let plan = Sampling::Sampled.plan(&script);
    assert_eq!(plan.compared, vec![true; script.len()]);
    // A run of exactly MIN_STRETCH is a stretch, and so is one that follows a run of the other
    // direction.
    let mut stretches: Vec<Step> = (0..MIN_STRETCH).map(|_| Step::Wheel(0., 1.)).collect();
    stretches.extend((0..MIN_STRETCH).map(|_| Step::Wheel(0., -1.)));
    let plan = Sampling::Sampled.plan(&stretches);
    assert_eq!(plan.compared_count(), 2 * 10);
    assert_eq!(plan.uncovered().len(), 2 * 6);
    assert!(plan.compared[0] && plan.compared[15] && plan.compared[16] && plan.compared[31]);
}

#[test]
fn the_mode_comes_from_gpux_lane_c_full() {
    assert_eq!(Sampling::from_value(None), Sampling::Sampled);
    assert_eq!(Sampling::from_value(Some("")), Sampling::Sampled);
    assert_eq!(Sampling::from_value(Some("0")), Sampling::Sampled);
    assert_eq!(Sampling::from_value(Some("1")), Sampling::Full);
}

#[test]
fn hover_acts_are_compared_by_run() {
    // Three pointer moves around wheel runs of 1, 5 and 2.
    let wheel = [
        false, true, false, true, true, true, true, true, false, true, true, false,
    ];
    let compared = sample_runs(&wheel, Sampling::Sampled);
    let wheels: Vec<bool> = wheel
        .iter()
        .zip(&compared)
        .filter(|(is_wheel, _)| **is_wheel)
        .map(|(_, &c)| c)
        .collect();
    // [1] [0 1 2 3 4] [0 1]: the first, the last and every 3rd of each run.
    assert_eq!(wheels, [true, true, false, false, true, true, true, true]);
    assert!(
        wheel
            .iter()
            .zip(&compared)
            .all(|(is_wheel, &c)| *is_wheel || c),
        "pointer moves are always compared"
    );
    assert!(sample_runs(&wheel, Sampling::Full).iter().all(|&c| c));
}

/// The panic message of `compare_with` seeded at `seed`, or `None` when it passes.
fn seeded(sampling: Sampling, seed: usize, script: &[Step]) -> Option<String> {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        compare_with(Content::Cards, 1., script, sampling, Some(seed));
    }));
    outcome.err().map(|error| {
        error
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| error.downcast_ref::<&str>().map(|text| text.to_string()))
            .unwrap_or_default()
    })
}

/// Asserts that a sampled run fails with a seeded difference at `seed`, and says which frame.
fn sampled_run_fails_at(seed: usize) {
    let script = scrolls_and_single_pixels();
    assert!(Sampling::Sampled.plan(&script).compared[seed]);
    let message = seeded(Sampling::Sampled, seed, &script).expect("the seeded difference shows");
    assert!(
        message.contains(&format!("after step {}", seed + 1)) && message.contains("pixels differ"),
        "{message}"
    );
}

#[test]
fn a_sampled_run_fails_on_a_difference_inside_a_stretch_it_compares() {
    // The 4th step of the stretch down, a step in its middle that is a multiple of 4, and the
    // 2nd step of the stretch up.
    for seed in [15 + 3, 15 + 40, 95 + 1, 95 + 20] {
        sampled_run_fails_at(seed);
    }
}

#[test]
fn a_sampled_run_fails_on_a_difference_at_a_step_of_a_long_scroll() {
    // Steps 6 and 13 are two of the scripted scrolls, compared whole in both modes.
    for seed in [6, 13] {
        sampled_run_fails_at(seed);
    }
}

#[test]
fn a_difference_the_sampled_run_does_not_compare_is_found_by_the_full_run() {
    let script = scrolls_and_single_pixels();
    let plan = Sampling::Sampled.plan(&script);
    // One uncovered step in each stretch, and the last one of the list.
    for seed in [
        20,
        61,
        100,
        UNCOVERED_BY_SAMPLED[UNCOVERED_BY_SAMPLED.len() - 1],
    ] {
        assert!(!plan.compared[seed], "{seed} is documented as uncovered");
        assert!(UNCOVERED_BY_SAMPLED.contains(&seed));
        assert!(
            seeded(Sampling::Sampled, seed, &script).is_none(),
            "the sampled run does not compare step {seed}"
        );
        let message = seeded(Sampling::Full, seed, &script)
            .expect("the full run compares every step, so it shows the difference");
        assert!(
            message.contains(&format!("after step {}", seed + 1)),
            "{message}"
        );
    }
}
