//! `FastStats`: per-frame reset, accumulation over a run, bypass reasons, and
//! the work one frame of each kind counts.

use super::oracle_support::{Change, OracleWindow};
use crate::{AppContext as _, FastMode, FastStats, LAYER_BYPASS_REASONS, LayerBypassReason};

/// Every counter has a stable name, the work counters come before the
/// mechanism counters, and the bench line carries them all.
#[test]
fn counters_are_named_in_a_stable_order() {
    let mut stats = FastStats::default();
    stats.frames = 2;
    stats.views_built = 3;
    stats.views_reused = 4;
    stats.bypass(LayerBypassReason::Demoted);
    let counters = stats.counters();
    let names: Vec<&str> = counters.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names[0], "frames");
    assert_eq!(&names[1..=FastStats::WORK.len()], &FastStats::WORK[..]);
    for name in FastStats::MECHANISM
        .iter()
        .filter(|name| **name != "layer_bypasses")
    {
        assert!(names.contains(name), "{name} is not reported");
    }
    for reason in LayerBypassReason::ALL {
        assert!(names.contains(&format!("layer_bypasses.{}", reason.name()).as_str()));
    }
    assert_eq!(stats.get("views_built"), Some(3));
    assert_eq!(stats.get("layer_bypasses.demoted"), Some(1));
    assert_eq!(stats.get("no such counter"), None);
    assert_eq!(
        stats.bench_line("fast/still-frame/off"),
        format!(
            "gpux-fast-stats: fast/still-frame/off {}",
            counters
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    );
}

#[test]
fn bypass_reasons_index_their_own_slot() {
    assert_eq!(LayerBypassReason::ALL.len(), LAYER_BYPASS_REASONS);
    for (index, reason) in LayerBypassReason::ALL.into_iter().enumerate() {
        assert_eq!(reason as usize, index);
        let mut stats = FastStats::default();
        stats.bypass(reason);
        stats.bypass(reason);
        let mut expected = [0; LAYER_BYPASS_REASONS];
        expected[index] = 2;
        assert_eq!(stats.layer_bypasses, expected);
        assert_eq!(stats.mechanism_total(), 2);
    }
}

#[test]
fn a_run_accumulates_frames_and_a_frame_starts_from_zero() {
    let mut oracle = OracleWindow::open(5, FastMode::OFF);
    let (_, first) = oracle.draw(true);
    let (_, second) = oracle.draw(false);
    assert_eq!((first.frames, second.frames), (1, 1));
    // The second frame replays the cached views the first one built: its
    // counters are its own, not the run's.
    assert!(first.views_built > 0);
    assert!(second.cached_views_replayed > 0);
    assert!(second.views_built < first.views_built);
    let run = oracle
        .cx
        .update_window(oracle.window.into(), |_, window, _| window.fast_stats())
        .unwrap();
    let mut expected = first;
    expected.accumulate(&second);
    // The window drew frames of its own while opening; the run holds those too.
    assert!(run.frames >= 2);
    assert!(run.views_built >= expected.views_built);
    oracle
        .cx
        .update_window(oracle.window.into(), |_, window, _| {
            window.reset_fast_stats();
            assert_eq!(window.fast_stats(), FastStats::default());
        })
        .unwrap();
    let (_, third) = oracle.draw(false);
    let run = oracle
        .cx
        .update_window(oracle.window.into(), |_, window, _| window.fast_stats())
        .unwrap();
    assert_eq!(run, third);
}

/// One frame of each kind of work counts what it did: a frame building
/// everything, one replaying cached views, and one whose text changed.
#[test]
fn one_frame_counts_its_work() {
    let mut oracle = OracleWindow::open(9, FastMode::OFF);
    oracle.draw(false);

    // Built from scratch: every view renders, every node is written, every
    // primitive is painted and every draw order computed.
    let (_, built) = oracle.draw(true);
    assert!(built.views_built >= 16, "{built:?}");
    assert_eq!(built.cached_views_replayed, 0);
    assert!(built.primitives_painted > 0 && built.primitives_copied == 0);
    assert!(built.draw_orders_computed >= built.primitives_painted / 2);
    assert_eq!(built.mechanism_total(), 0);

    // Replayed: cached views copy their primitives. Of the 16 grid cells,
    // the 5 styled by their row's group hover are built every frame.
    let (_, replayed) = oracle.draw(false);
    assert!(replayed.cached_views_replayed >= 11, "{replayed:?}");
    assert!(replayed.primitives_copied > 0);
    assert!(replayed.views_built < built.views_built);

    // New text is shaped; text already shaped is not. Applying a change
    // draws the dirty window in tests, so the run since the change is read.
    let (_, unchanged) = oracle.draw(false);
    assert_eq!(unchanged.lines_shaped, 0, "{unchanged:?}");
    let run_since = |oracle: &mut OracleWindow, change: &Change| {
        oracle
            .cx
            .update_window(oracle.window.into(), |_, window, _| {
                window.reset_fast_stats()
            })
            .unwrap();
        oracle.apply(change);
        oracle.draw(false);
        oracle
            .cx
            .update_window(oracle.window.into(), |_, window, _| window.fast_stats())
            .unwrap()
    };
    // A new chip's label is text no frame has shaped.
    let retexted = run_since(&mut oracle, &Change::InsertChip { at: 0 });
    assert!(retexted.lines_shaped > 0, "{retexted:?}");
    let redrawn = run_since(&mut oracle, &Change::Redraw);
    assert_eq!(redrawn.lines_shaped, 0, "{redrawn:?}");

    // A new layout writes styles; the pooled nodes of an unchanged one
    // already hold them.
    assert!(built.style_writes > 0);
    assert!(unchanged.style_writes < built.style_writes, "{unchanged:?}");
}
