//! Fast-mode attribution: every comparison scenario runs a fixed number of
//! frames in every cumulative mode, and the work counters the frames record
//! must have the shape that attributes each mode's benefit to its mechanism.
//!
//! The counts are deterministic: the test platform, the bundled text system
//! and the executor clock make every run draw the same frames. Timing is
//! measured only by the `fast_mode` bench in lane G.
//!
//! Until a mechanism lands, its mode must work exactly as the mode before it
//! does; each mechanism's sprint amends the expectations of the scenarios it
//! changes here and in their fixtures.
//!
//! `cargo test -p gpux --features test-support,bench-support --test fast_mode_attribution`

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpux::{
    FastStats, TestAppContext, TestDispatcher,
    fast_scenarios::{self, Fixture, Mode, Scenario},
};

/// Frames measured per scenario and mode.
const FRAMES: usize = 24;

/// Runs `scenario` in `mode`: draws its warm-up frames, then measures
/// [`FRAMES`] frames and returns their counters with the fixture.
fn measure(scenario: &Scenario, mode: Mode) -> (FastStats, Rc<RefCell<Box<dyn Fixture>>>) {
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        gpui_platform::bundled_text_system(),
    );
    let vcx = cx.add_empty_window();
    let fixture =
        Rc::new(RefCell::new(vcx.update(|window, cx| {
            fast_scenarios::open(scenario, mode, window, cx)
        })));
    vcx.update(|window, _| window.activate_window());
    let frame = |vcx: &mut gpux::VisualTestContext, index: usize| {
        // A step flushes its effects, which draws the dirty window; the frame
        // that follows runs what animations asked for.
        vcx.update(|window, cx| fixture.borrow_mut().step(index, window, cx));
        vcx.advance_frame(Duration::from_millis(16));
    };
    for index in 0..scenario.warm_up_frames {
        frame(vcx, index);
    }
    vcx.update(|window, _| window.reset_fast_stats());
    for index in 0..FRAMES {
        frame(vcx, scenario.warm_up_frames + index);
    }
    let stats = vcx.update(|window, _| window.fast_stats());
    (stats, fixture)
}

/// Runs `name` in every mode, checks each mode's counters against its
/// fixture's expectations, and returns them by mode.
fn attribute(name: &str) -> Vec<(Mode, FastStats)> {
    let scenario = fast_scenarios::scenario(name).unwrap_or_else(|| panic!("no scenario {name}"));
    Mode::ALL
        .into_iter()
        .map(|mode| {
            let (stats, fixture) = measure(&scenario, mode);
            if let Err(failure) = fixture.borrow().expect(mode, &stats) {
                panic!("{name} in {mode}: {failure}");
            }
            assert!(
                stats.frames >= FRAMES as u64,
                "{name} in {mode} drew {} frames for {FRAMES} steps",
                stats.frames
            );
            (mode, stats)
        })
        .collect()
}

/// Each mode does the work of the mode before it, except where the
/// mechanism it adds has landed.
///
/// - Layout retention may write other Taffy nodes than the ordinal pool, and
///   measures and shapes no more text than `off` (carried and replayed text
///   is measured less); everything else is the same work.
/// - View retention renders no more views than layout retention, and copies
///   from last frame what it does not paint, so the frame shows as many
///   primitives; it claims, carries and measures no more than layout
///   retention, and writes and releases the same nodes, since a view drawn
///   from last frame keeps the nodes it laid out. Ordering is the same work.
/// - Splice builds no more views than view retention, and copies what it
///   does not paint, so the frame shows as many primitives; its layout and
///   text work is no more, and everything else (writes, releases, ordering)
///   is the same.
/// - Scroll layers do the work of splice where no frame shows a layer, and
///   count why each scroll container went without one; where frames are
///   shown from a layer's tiles, they build, shape and paint no more.
fn modes_differ_only_by_landed_mechanisms(name: &str) {
    let modes = attribute(name);
    let (_, off) = modes[0];
    let (_, layout) = modes[1];
    let (_, views) = modes[2];
    let (_, splice) = modes[3];
    assert!(
        layout.leaves_measured <= off.leaves_measured && layout.lines_shaped <= off.lines_shaped,
        "{name}: layout measured or shaped more text than off: measured {} shaped {} against {} {}",
        layout.leaves_measured,
        layout.lines_shaped,
        off.leaves_measured,
        off.lines_shaped
    );
    let shared_work = |stats: FastStats| FastStats {
        style_writes: 0,
        children_writes: 0,
        leaves_measured: 0,
        lines_shaped: 0,
        nodes_claimed: 0,
        nodes_released: 0,
        nodes_lingering: 0,
        leaves_carried: 0,
        leaves_replayed: 0,
        leaves_remeasured: 0,
        ..stats
    };
    assert_eq!(
        shared_work(layout),
        shared_work(off),
        "{name}: layout counted other work than off beyond its layout and text"
    );

    for (counter, more) in [
        ("views_built", views.views_built > layout.views_built),
        (
            "leaves_measured",
            views.leaves_measured > layout.leaves_measured,
        ),
        ("lines_shaped", views.lines_shaped > layout.lines_shaped),
        ("nodes_claimed", views.nodes_claimed > layout.nodes_claimed),
        (
            "leaves_carried",
            views.leaves_carried > layout.leaves_carried,
        ),
    ] {
        assert!(
            !more,
            "{name}: views counted more {counter} than layout: {views:?}"
        );
    }
    assert_eq!(
        views.primitives_painted + views.primitives_copied,
        layout.primitives_painted + layout.primitives_copied,
        "{name}: views shows other primitives than layout"
    );
    let unchanged_by_views = |stats: FastStats| FastStats {
        views_built: 0,
        primitives_painted: 0,
        primitives_copied: 0,
        leaves_measured: 0,
        lines_shaped: 0,
        nodes_claimed: 0,
        leaves_carried: 0,
        views_built_at_retained_layout: 0,
        views_reused: 0,
        ..stats
    };
    assert_eq!(
        unchanged_by_views(views),
        unchanged_by_views(layout),
        "{name}: views counted other work than layout beyond the views it drew from last frame"
    );
    for (counter, more) in [
        ("views_built", splice.views_built > views.views_built),
        (
            "leaves_measured",
            splice.leaves_measured > views.leaves_measured,
        ),
        ("lines_shaped", splice.lines_shaped > views.lines_shaped),
        ("nodes_claimed", splice.nodes_claimed > views.nodes_claimed),
        (
            "leaves_carried",
            splice.leaves_carried > views.leaves_carried,
        ),
    ] {
        assert!(
            !more,
            "{name}: splice counted more {counter} than views: {splice:?}"
        );
    }
    assert_eq!(
        splice.primitives_painted + splice.primitives_copied,
        views.primitives_painted + views.primitives_copied,
        "{name}: splice shows other primitives than views"
    );
    let unchanged_by_splice = |stats: FastStats| FastStats {
        views_spliced: 0,
        views_prebuilt: 0,
        views_reused: 0,
        views_built_at_retained_layout: 0,
        ..unchanged_by_views(stats)
    };
    assert_eq!(
        unchanged_by_splice(splice),
        unchanged_by_splice(views),
        "{name}: splice counted other work than views beyond the views it spliced"
    );
    let (_, layers) = modes[4];
    if layers.layers_composited + layers.layers_repainted == 0 {
        // No frame showed a layer: the same work as splice, with the scroll
        // containers counted for why they went without one. A container
        // whose content turns out to hold what no tile can (a sticky header,
        // a backdrop blur) paints it for its layer before it is drawn into
        // the frame, which orders its primitives once more; it tries again
        // every few frames.
        let but_ordering = |stats: FastStats| FastStats {
            layer_bypasses: Default::default(),
            draw_orders_computed: 0,
            draw_orders_replayed: 0,
            ..stats
        };
        assert_eq!(
            but_ordering(layers),
            but_ordering(splice),
            "{name}: layers counted other work than splice with no layer shown"
        );
        assert!(
            layers.draw_orders_computed >= splice.draw_orders_computed,
            "{name}: layers ordered fewer primitives than splice: {layers:?}"
        );
    } else {
        // Frames shown from a layer's tiles build, lay out and paint no more
        // than splice does.
        for (counter, more) in [
            ("views_built", layers.views_built > splice.views_built),
            (
                "leaves_measured",
                layers.leaves_measured > splice.leaves_measured,
            ),
            ("lines_shaped", layers.lines_shaped > splice.lines_shaped),
            (
                "primitives_painted",
                layers.primitives_painted > splice.primitives_painted,
            ),
        ] {
            assert!(
                !more,
                "{name}: layers counted more {counter} than splice: {layers:?}"
            );
        }
        assert!(
            layers.tiles_composited > 0,
            "{name}: layers showed no tile: {layers:?}"
        );
    }
}

macro_rules! scenarios {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                modes_differ_only_by_landed_mechanisms($name);
            }
        )*

        const LISTED: &[&str] = &[$($name),*];
    };
}

scenarios! {
    still_frame => "still-frame",
    one_panel => "one-panel",
    six_panels => "six-panels",
    all_panels => "all-panels",
    table_stream => "table-stream",
    settings_toggle => "settings-toggle",
    tree_expand => "tree-expand",
    form_typing => "form-typing",
    hover_sweep => "hover-sweep",
    scroll_child_view => "scroll-child-view",
    scroll_same_view => "scroll-same-view",
    scroll_uniform_list => "scroll-uniform-list",
    scroll_list => "scroll-list",
    chat_scroll => "chat-scroll",
    sticky_scroll => "sticky-scroll",
    effect_scroll => "effect-scroll",
    animated_frame => "animated-frame",
}

#[test]
fn every_scenario_is_attributed() {
    let names: Vec<&str> = fast_scenarios::all()
        .iter()
        .map(|scenario| scenario.name)
        .collect();
    assert_eq!(names, LISTED, "the scenarios and this group's tests differ");
}

/// The counts are deterministic: the same scenario in the same mode counts
/// the same work twice.
#[test]
fn counts_repeat_exactly() {
    for name in ["still-frame", "scroll-list", "form-typing"] {
        let scenario = fast_scenarios::scenario(name).unwrap();
        let (first, _) = measure(&scenario, Mode::Layers);
        let (second, _) = measure(&scenario, Mode::Layers);
        assert_eq!(
            first, second,
            "{name} counted different work on a second run"
        );
    }
}
