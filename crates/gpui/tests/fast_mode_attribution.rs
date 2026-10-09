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
/// mechanism it adds has landed: layout retention may write other Taffy
/// nodes than the ordinal pool, and nothing else; view retention, splice and
/// scroll layers have not landed, and work as layout retention does.
fn modes_differ_only_by_landed_mechanisms(name: &str) {
    let modes = attribute(name);
    let (_, off) = modes[0];
    let (_, layout) = modes[1];
    let without_layout_writes = |stats: FastStats| FastStats {
        style_writes: 0,
        children_writes: 0,
        ..stats
    };
    assert_eq!(
        without_layout_writes(FastStats {
            nodes_claimed: 0,
            nodes_released: 0,
            nodes_lingering: 0,
            ..layout
        }),
        without_layout_writes(off),
        "{name}: layout counted other work than off beyond its Taffy writes"
    );
    for (mode, stats) in &modes[2..] {
        assert_eq!(
            *stats, layout,
            "{name}: {mode} counted other work than layout before its mechanism landed"
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
