//! The iOS entry point: `ios/Sources/main.m` calls [`gpux_bench_register`] and then
//! `gpui_ios_run_demo`, which starts gpui_mobile's platform and runs the registered callback.
//!
//! The launch arguments name the run, as on desktop (`xcrun simctl launch --console-pty <device>
//! dev.gpux.benchmark --scenario <id> --fast on ...`). The window is the screen. The report is
//! printed as one `gpux-bench-result: <json>` line on stdout, which the console carries back to
//! `run.py`, and the app exits.

use std::io::Write as _;

use gpux::{App, WindowOptions};

use crate::driver::{self, Config};

fn start(cx: &mut App) {
    let (config, _) = match Config::from_args(std::env::args().skip(1), "ios") {
        Ok(parsed) => parsed,
        Err(error) => {
            println!("gpux-bench-error: {error}");
            std::io::stdout().flush().ok();
            std::process::exit(2);
        }
    };
    crate::register_fonts(cx);
    driver::open(
        cx,
        config,
        WindowOptions::default(),
        Box::new(|report, _, _| {
            if let Some(error) = report["error"].as_str() {
                println!("gpux-bench-error: {error}");
                std::io::stdout().flush().ok();
                std::process::exit(2);
            }
            println!("gpux-bench-result: {report}");
            std::io::stdout().flush().ok();
            std::process::exit(0);
        }),
    );
}

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn gpux_bench_register() {
    gpui_mobile::ios::ffi::set_app_callback(Box::new(|cx| {
        Box::pin(async move {
            cx.update(start);
        })
    }));
}
