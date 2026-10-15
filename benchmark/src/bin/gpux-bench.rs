//! `gpux-bench`: the desktop entry point (macOS, Linux, Windows).
//!
//! ```text
//! gpux-bench --list
//! gpux-bench --scenario <id> [--fast default|on|off] [--frames 600] [--warmup 120]
//!            [--label <config>] [--target <name>] [--out <file.json>]
//! ```
//!
//! Opens one real, visible window of 1280 by 800 logical pixels, runs the scenario and writes the
//! report as JSON to `--out` (or stdout), then quits. One run is one process, so the memory
//! figures belong to that scenario alone.

use gpux::{App, Bounds, WindowBounds, WindowOptions, px, size};
use gpux_benchmark::{
    WINDOW_HEIGHT, WINDOW_WIDTH,
    driver::{self, Config},
    scenarios,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--list") {
        for scenario in scenarios::all() {
            println!(
                "{:<24} {:<10} {}",
                scenario.id, scenario.group, scenario.description
            );
        }
        return;
    }
    let (config, out) = Config::from_args(args, std::env::consts::OS)
        .unwrap_or_else(|error| panic!("{error}; --list lists the scenarios"));

    gpui_platform::application().run(move |cx| async move {
        cx.update(|cx: &mut App| {
            gpux_benchmark::register_fonts(cx);
            let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            };
            driver::open(
                cx,
                config,
                options,
                Box::new(move |report, _, cx| {
                    let text =
                        serde_json::to_string_pretty(&report).expect("the report serializes");
                    match &out {
                        Some(path) => std::fs::write(path, text).expect("writing the report"),
                        None => println!("{text}"),
                    }
                    cx.quit();
                }),
            );
            cx.activate(true);
        });
    });
}
