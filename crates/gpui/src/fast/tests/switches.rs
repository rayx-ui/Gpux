//! The fast-mode switches: the environment, aliases, precedence and
//! implications, and switching a window at runtime.

use std::collections::HashMap;

use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use super::oracle_support::{Change, OracleWindow};
use crate::{AppContext as _, FastMode};

/// The mode the environment `vars` gives a window.
fn from(vars: &[(&str, &str)]) -> FastMode {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    FastMode::from_environment(|name| vars.get(name).cloned())
}

fn mode(
    layout_retention: bool,
    view_retention: bool,
    splice: bool,
    scroll_layers: bool,
) -> FastMode {
    FastMode {
        layout_retention,
        view_retention,
        splice,
        scroll_layers,
    }
}

/// Every mechanism lands dark: until the final sprint flips them, the
/// compiled defaults are off, and a window the environment says nothing
/// about draws as gpux does without fast mode.
#[test]
fn compiled_defaults_are_off() {
    assert_eq!(FastMode::compiled_default(), FastMode::OFF);
    assert_eq!(from(&[]), FastMode::OFF);
    assert!(FastMode::OFF.is_off());
    assert!(!FastMode::ALL.is_off());
}

#[test]
fn each_variable_sets_its_mechanism() {
    let layers = crate::fast::layers::COMPILED;
    assert_eq!(
        from(&[("GPUX_LAYOUT_RETENTION", "1")]),
        mode(true, false, false, false)
    );
    assert_eq!(
        from(&[("GPUX_LAYOUT_RETENTION", "1"), ("GPUX_VIEW_RETENTION", "1")]),
        mode(true, true, false, false)
    );
    assert_eq!(
        from(&[
            ("GPUX_LAYOUT_RETENTION", "1"),
            ("GPUX_VIEW_RETENTION", "1"),
            ("GPUX_SCROLL_LAYERS", "1"),
        ]),
        mode(true, true, false, layers)
    );
    // `GPUX_FAST_MODE=1` starts everything on, and each variable can still
    // turn its mechanism off.
    assert_eq!(
        from(&[("GPUX_FAST_MODE", "1")]),
        mode(true, true, true, layers)
    );
    assert_eq!(
        from(&[("GPUX_FAST_MODE", "1"), ("GPUX_SCROLL_LAYERS", "0")]),
        mode(true, true, true, false)
    );
    assert_eq!(
        from(&[("GPUX_FAST_MODE", "1"), ("GPUX_VIEW_RETENTION", "0")]),
        mode(true, false, false, false)
    );
    assert_eq!(
        from(&[("GPUX_FAST_MODE", "1"), ("GPUX_LAYOUT_RETENTION", "0")]),
        FastMode::OFF
    );
}

#[test]
fn master_switch_off_wins_over_everything() {
    assert_eq!(
        from(&[
            ("GPUX_FAST_MODE", "0"),
            ("GPUX_LAYOUT_RETENTION", "1"),
            ("GPUX_VIEW_RETENTION", "1"),
            ("GPUX_SCROLL_LAYERS", "1"),
            ("GPUI_VIEW_RETENTION", "1"),
            ("GPUI_SCROLL_LAYERS", "1"),
        ]),
        FastMode::OFF
    );
}

#[test]
fn gpui_fast_names_are_aliases_the_gpux_names_win_over() {
    let layers = crate::fast::layers::COMPILED;
    // An alias is read when the gpux name is unset.
    assert_eq!(
        from(&[("GPUX_LAYOUT_RETENTION", "1"), ("GPUI_VIEW_RETENTION", "1")]),
        mode(true, true, false, false)
    );
    assert_eq!(
        from(&[
            ("GPUX_LAYOUT_RETENTION", "1"),
            ("GPUX_VIEW_RETENTION", "1"),
            ("GPUI_SCROLL_LAYERS", "1"),
        ]),
        mode(true, true, false, layers)
    );
    // As in gpui-fast, any alias value but `0` turns its mechanism on.
    assert_eq!(
        from(&[
            ("GPUX_LAYOUT_RETENTION", "1"),
            ("GPUI_VIEW_RETENTION", "yes")
        ]),
        mode(true, true, false, false)
    );
    // The gpux name wins when both are set.
    assert_eq!(
        from(&[
            ("GPUX_LAYOUT_RETENTION", "1"),
            ("GPUX_VIEW_RETENTION", "0"),
            ("GPUI_VIEW_RETENTION", "1"),
        ]),
        mode(true, false, false, false)
    );
    assert_eq!(
        from(&[
            ("GPUX_FAST_MODE", "1"),
            ("GPUX_SCROLL_LAYERS", "0"),
            ("GPUI_SCROLL_LAYERS", "1"),
        ]),
        mode(true, true, true, false)
    );
    assert_eq!(
        from(&[("GPUX_FAST_MODE", "1"), ("GPUI_SCROLL_LAYERS", "0")]),
        mode(true, true, true, false)
    );
}

/// A gpux variable that is neither `0` nor `1` is ignored, as though unset:
/// the alias, then the default, decide.
#[test]
fn malformed_values_are_ignored() {
    assert_eq!(from(&[("GPUX_LAYOUT_RETENTION", "on")]), FastMode::OFF);
    assert_eq!(
        from(&[
            ("GPUX_LAYOUT_RETENTION", "1"),
            ("GPUX_VIEW_RETENTION", "true"),
            ("GPUI_VIEW_RETENTION", "1"),
        ]),
        mode(true, true, false, false)
    );
    assert_eq!(from(&[("GPUX_FAST_MODE", "maybe")]), FastMode::OFF);
    assert_eq!(
        from(&[("GPUX_LAYOUT_RETENTION", " 1 ")]),
        mode(true, false, false, false)
    );
}

/// Splice has no variable of its own, as in gpui-fast: it is on only where
/// the master switch turns everything on, or through `set_fast_mode`.
#[test]
fn splice_has_no_variable() {
    assert_eq!(
        from(&[
            ("GPUX_LAYOUT_RETENTION", "1"),
            ("GPUX_VIEW_RETENTION", "1"),
            ("GPUX_SPLICE", "1"),
            ("GPUI_SPLICE", "1"),
        ]),
        mode(true, true, false, false)
    );
}

#[test]
fn implications_are_normalized() {
    // Every combination normalizes to one where nothing is on without what it
    // builds on, and normalizing twice changes nothing.
    for bits in 0..16u8 {
        let requested = mode(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
        for compiled in [false, true] {
            let normalized = requested.normalized_for(compiled);
            assert_eq!(normalized.normalized_for(compiled), normalized);
            assert!(!normalized.view_retention || normalized.layout_retention);
            assert!(!normalized.splice || normalized.view_retention);
            assert!(!normalized.scroll_layers || normalized.view_retention);
            assert_eq!(
                normalized.layout_retention, requested.layout_retention,
                "layout retention needs nothing"
            );
            assert_eq!(
                normalized.view_retention,
                requested.view_retention && requested.layout_retention
            );
            assert_eq!(
                normalized.splice,
                requested.splice && requested.view_retention && requested.layout_retention
            );
        }
    }
    assert_eq!(mode(false, true, true, true).normalized(), FastMode::OFF);
    assert_eq!(
        mode(true, false, true, true).normalized(),
        mode(true, false, false, false)
    );
}

/// Scroll layers stay off wherever no renderer implements their tiles.
#[test]
fn scroll_layers_are_off_where_not_compiled() {
    assert!(!FastMode::ALL.normalized_for(false).scroll_layers);
    assert!(FastMode::ALL.normalized_for(true).scroll_layers);
    assert_eq!(
        FastMode::ALL.normalized().scroll_layers,
        crate::fast::layers::COMPILED
    );
}

#[test]
fn set_fast_mode_normalizes_and_reads_back() {
    let mut oracle = OracleWindow::open(0, FastMode::OFF);
    oracle
        .cx
        .update_window(oracle.window.into(), |_, window, _| {
            assert_eq!(window.fast_mode(), FastMode::OFF);
            window.set_fast_mode(mode(false, true, true, true));
            assert_eq!(window.fast_mode(), FastMode::OFF);
            window.set_fast_mode(FastMode::ALL);
            assert_eq!(window.fast_mode(), FastMode::ALL.normalized());
            window.set_fast_mode(mode(true, false, true, false));
            assert_eq!(window.fast_mode(), mode(true, false, false, false));
        })
        .unwrap();
}

/// A mode change takes effect on the next frame, which is drawn from
/// scratch: it equals what a window drawing from scratch draws, and replays
/// nothing.
#[test]
fn a_mode_change_draws_the_next_frame_from_scratch() {
    let modes = [
        FastMode::ALL,
        FastMode::OFF,
        mode(true, false, false, false),
        mode(true, true, false, false),
        mode(true, true, true, false),
        FastMode::OFF,
    ];
    let mut incremental = OracleWindow::open(7, FastMode::OFF);
    let mut reference = OracleWindow::open(7, FastMode::OFF);
    let mut rng = StdRng::seed_from_u64(7);
    incremental.draw(false);
    reference.draw(true);
    for (step, &next) in modes.iter().enumerate() {
        // Settle into the current mode, so the change has something to undo.
        for _ in 0..3 {
            let change = Change::random(&mut rng);
            incremental.apply(&change);
            reference.apply(&change);
            incremental.draw(false);
            reference.draw(true);
        }
        let mut dirty = false;
        let (actual, stats) = incremental.draw_after(false, |window| {
            window.set_fast_mode(next);
            dirty = window.is_dirty();
        });
        let mode_changed = step == 0 || modes[step - 1].normalized() != next.normalized();
        assert!(mode_changed, "consecutive modes must differ");
        assert!(dirty, "switching to {next:?} did not ask for a frame");
        assert_eq!(
            stats.cached_views_replayed, 0,
            "the frame after switching to {next:?} replayed a cached view"
        );
        let (expected, _) = reference.draw(true);
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            panic!("after switching to {next:?}: {difference}");
        }
    }
}

/// Toggling the mode 1,000 times while the view keeps changing leaves
/// nothing behind: what the mechanisms own is nothing while they are off,
/// and after each frame is exactly what drawing that frame from scratch
/// leaves, so nothing accumulates.
#[test]
fn toggling_a_thousand_times_releases_what_the_mechanisms_own() {
    let mut oracle = OracleWindow::open(3, FastMode::OFF);
    let mut rng = StdRng::seed_from_u64(3);
    let owned = |oracle: &mut OracleWindow| {
        oracle
            .cx
            .update_window(oracle.window.into(), |_, window, _| {
                window.fast_owned_state()
            })
            .unwrap()
    };
    // Grow the layout engine to the view's size before measuring.
    for _ in 0..10 {
        oracle.apply(&Change::random(&mut rng));
        oracle.draw(false);
    }
    for toggle in 0..1_000 {
        let next = if toggle % 2 == 0 {
            FastMode::ALL
        } else {
            FastMode::OFF
        };
        oracle
            .cx
            .update_window(oracle.window.into(), |_, window, _| {
                window.set_fast_mode(next)
            })
            .unwrap();
        // Churn that keeps the view the same size: no inserts or removals.
        let change = match rng.random_range(0..3) {
            0 => Change::Badge,
            1 => Change::UpdatePanel {
                panel: rng.random_range(0..6),
                value: rng.random_range(0..10),
            },
            _ => Change::Hover {
                x: rng.random_range(0.0..900.0),
                y: rng.random_range(0.0..700.0),
            },
        };
        oracle.apply(&change);
        oracle.draw(false);
        let state = owned(&mut oracle);
        if next.is_off() {
            assert_eq!(
                (
                    state.kept_layout_nodes,
                    state.retained_records,
                    state.layer_tiles
                ),
                (0, 0, 0),
                "fast mode off still owns something after toggle {toggle}"
            );
        }
        // The same frame drawn from scratch: a fresh layout engine holds
        // exactly this frame's nodes.
        oracle
            .cx
            .update_window(oracle.window.into(), |_, window, _| {
                window.forget_retained_state()
            })
            .unwrap();
        oracle.draw(false);
        let fresh = owned(&mut oracle);
        assert_eq!(
            state, fresh,
            "toggle {toggle} left more than drawing its frame from scratch does"
        );
    }
}

/// Frames drawn with every switch off make no fast-mode decision.
#[test]
fn frames_with_fast_mode_off_record_no_mechanism_counter() {
    let mut oracle = OracleWindow::open(11, FastMode::OFF);
    let mut rng = StdRng::seed_from_u64(11);
    let mut total = crate::FastStats::default();
    for _ in 0..40 {
        for _ in 0..rng.random_range(1..=3) {
            oracle.apply(&Change::random(&mut rng));
        }
        let (_, stats) = oracle.draw(false);
        assert_eq!(stats.mechanism_total(), 0, "an off frame counted {stats:?}");
        total.accumulate(&stats);
    }
    // The frames did work, which the work counters saw.
    assert_eq!(total.frames, 40);
    assert!(total.views_built > 0 && total.primitives_painted > 0);
}

/// With scroll layers off a scroll container does none of their
/// bookkeeping: its window remembers no container and counts no frame, a
/// published offset is logged for no one, and the hooks that have no window
/// to ask stay off until a window of the thread turns the switch on.
#[crate::test]
fn a_window_without_scroll_layers_keeps_none_of_their_books(cx: &mut crate::TestAppContext) {
    use crate::fast::tests::layer_support::{
        LayerPage, drag_scrollbar_by, draw, new_page, scroll, with_window,
    };
    let page = cx.add_window(|_, cx| new_page(false, cx));
    let window = page.into();
    cx.set_draw_on_flush(false);
    with_window(cx, window, |window, _| {
        assert!(!window.fast_mode().scroll_layers);
    });
    assert!(!crate::fast::layers::on_thread());
    let books = |cx: &mut crate::TestAppContext| {
        with_window(cx, window, |window, _| {
            (
                window.fast_layers.frame,
                window.fast_layers.layers.len(),
                window.fast_layers.scrolls.containers().count(),
                crate::fast::layers::invalidate::pending_publishes(),
            )
        })
    };
    draw(cx, window);
    draw(cx, window);
    let version = page
        .read_with(cx, |page: &LayerPage, _| page.handle.fast_version().get())
        .unwrap();
    for _ in 0..3 {
        cx.update_window(window, |_, window, cx| {
            window.dispatch_event(
                crate::PlatformInput::ScrollWheel(crate::ScrollWheelEvent {
                    position: crate::point(crate::px(20.), crate::px(20.)),
                    delta: crate::ScrollDelta::Pixels(crate::point(crate::px(0.), crate::px(-20.))),
                    modifiers: Default::default(),
                    touch_phase: crate::TouchPhase::Moved,
                }),
                cx,
            );
            window.draw(cx).clear(cx);
        })
        .unwrap();
    }
    assert!(drag_scrollbar_by(cx, page, -10.));
    draw(cx, window);
    let offset = page.read_with(cx, |page, _| page.handle.offset()).unwrap();
    assert_eq!(offset.y, crate::px(-70.), "the page scrolled");
    assert_eq!(books(cx), (0, 0, 0, 0));
    assert_eq!(
        page.read_with(cx, |page, _| page.handle.fast_version().get())
            .unwrap(),
        version,
        "a published offset moves no version"
    );
    assert!(!crate::fast::layers::on_thread());

    // Turned on, the window keeps them; turned off again, it drops them.
    with_window(cx, window, |window, _| {
        window.set_fast_mode(crate::fast::tests::layer_support::LAYERS)
    });
    assert_eq!(
        crate::fast::layers::on_thread(),
        crate::fast::layers::COMPILED
    );
    draw(cx, window);
    scroll(cx, window, -20.);
    scroll(cx, window, -20.);
    if crate::fast::layers::COMPILED {
        let (frames, layers, containers, _) = books(cx);
        assert!(frames > 0 && layers == 1 && containers == 1);
    }
    with_window(cx, window, |window, _| window.set_fast_mode(FastMode::OFF));
    draw(cx, window);
    let (_, layers, containers, _) = books(cx);
    assert_eq!((layers, containers), (0, 0));
}
