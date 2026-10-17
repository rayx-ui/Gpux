//! Fast mode, scenario by scenario and mode by mode: `fast/<scenario>/<mode>`.
//!
//! Every scenario of `gpux::fast_scenarios` runs in every cumulative mode:
//! `off`, `layout`, `views`, `splice`, `layers`. Each id builds its scenario
//! in a fresh window, draws the warm-up frames, then times one scenario step
//! and the frame it causes per iteration. After the measured loop the
//! fixture checks the work counters the frames recorded, so a faster frame
//! that skipped required work fails rather than counting as a win, and one
//! `gpux-fast-stats: <id> <counter>=<n> ...` line reports them for
//! `gpux-testkit perf fast-mode`. Criterion calls an id's routine for its
//! warm-up and every sample; the last line printed for an id is its run.
//!
//! `cargo bench -p gpux-platform --features bench-support --bench fast_mode -- --test`
//! runs every id once. Timings are compared only in lane G.

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpux::{
    AppContext as _, BenchAppContext,
    fast_scenarios::{self, Mode},
};

/// Builds `name` in a fresh window in `mode`, measures it, checks its work
/// counters and prints them.
fn measure(name: &str, mode: &Mode, cx: &mut BenchAppContext) {
    let scenario =
        fast_scenarios::scenario(name).unwrap_or_else(|| panic!("no fast-mode scenario {name}"));
    let mut window = cx.add_empty_window();
    let handle = window.window_handle();
    let fixture =
        Rc::new(RefCell::new(window.update(|window, cx| {
            fast_scenarios::open(&scenario, *mode, window, cx)
        })));
    for frame in 0..scenario.warm_up_frames {
        window.update(|window, cx| fixture.borrow_mut().step(frame, window, cx));
        window.run_until_idle();
    }
    window.update(|window, _| window.reset_fast_stats());

    let mut frame = scenario.warm_up_frames;
    cx.bench_iter(|cx| {
        cx.update(|cx| {
            cx.update_window(handle, |_, window, cx| {
                fixture.borrow_mut().step(frame, window, cx)
            })
        })
        .expect("the scenario's window stays open");
        frame += 1;
    });

    let stats = window.update(|window, _| window.fast_stats());
    let id = format!("fast/{name}/{mode}");
    if let Err(failure) = fixture.borrow().expect(*mode, &stats) {
        panic!("{id}: {failure}");
    }
    println!("{}", stats.bench_line(&id));
    // Criterion calls the routine for its warm-up and every sample; a window left open holds
    // its renderer's textures for the rest of the process.
    window.update(|window, _| window.remove_window());
    drop(fixture);
    cx.settle();
}

macro_rules! scenarios {
    ($($bench:ident => $name:literal,)*) => {
        $(
            #[gpux::bench(group = "fast", input_name = $name, inputs = Mode::ALL)]
            fn $bench(mode: &Mode, cx: &mut BenchAppContext) {
                measure($name, mode, cx);
            }
        )*

        /// Every scenario has a bench.
        fn every_scenario_is_benched(_: &mut criterion::Criterion<gpux::BenchMeasurement>) {
            let names: Vec<&str> = fast_scenarios::all().iter().map(|scenario| scenario.name).collect();
            assert_eq!(names, [$($name),*], "the scenarios and the fast_mode benches differ");
        }

        // Frames take well under a millisecond to a few milliseconds, so a short measurement
        // already holds hundreds of frames; 17 scenarios in 5 modes would otherwise take most
        // of an hour.
        gpux::bench_group!(
            name = benches;
            config = criterion::Criterion::default()
                .warm_up_time(Duration::from_millis(300))
                .measurement_time(Duration::from_secs(1))
                .sample_size(20);
            targets = every_scenario_is_benched, $($bench),*
        );
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
    scroll_virtual_list => "scroll-virtual-list",
    scroll_virtual_list_horizontal => "scroll-virtual-list-horizontal",
    scroll_virtual_list_sticky => "scroll-virtual-list-sticky",
    scroll_virtual_list_synchronized => "scroll-virtual-list-synchronized",
    chat_scroll => "chat-scroll",
    sticky_scroll => "sticky-scroll",
    effect_scroll => "effect-scroll",
    animated_frame => "animated-frame",
}

gpux::bench_main!(benches);
