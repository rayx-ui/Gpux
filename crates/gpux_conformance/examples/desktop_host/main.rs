//! Lane D's desktop host: runs the platform checks and the lane-D conformance scenes in real
//! windows of `gpui_platform::application()`, printing one `gpux-conformance:` result line each.
//!
//! ```text
//! cargo run -p gpux-conformance --no-default-features --features app-host --example desktop_host -- [--scene <id>] [--grep <pattern>]
//! ```
//!
//! `gpux-testkit lane d` runs it and parses the result lines. OS-level input (`SendInput` on
//! Windows, XTest through `xdotool` on X11) is required unless `GPUX_OS_INPUT=optional`, which
//! reports the OS-input check skipped when the session cannot deliver synthetic input.

mod checks;
mod os;

use gpux::{
    AppContext as _, Bounds, Context, IntoElement, ParentElement as _, Render, Styled as _,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, point, px, rgb, size,
};
use gpux_conformance::{
    CATALOG, Lane,
    app_host::{self, AppHost},
    platform_checks::PLATFORM_CHECKS,
    report::Outcome,
};
use std::{cell::Cell, rc::Rc};

/// Where scene windows open, in the top-left of the main display, clear of the parked cursor.
pub const WINDOW_ORIGIN: (f32, f32) = (80., 80.);

/// The host's own window: open for the whole run, beside the scene windows.
struct Anchor;

impl Render for Anchor {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p(px(8.))
            .bg(rgb(0xf0f0f0))
            .child("gpux desktop host")
    }
}

fn anchor_options() -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(WINDOW_ORIGIN.0 + 760.), px(WINDOW_ORIGIN.1)),
            size: size(px(240.), px(80.)),
        })),
        titlebar: Some(TitlebarOptions {
            title: Some("gpux desktop host".into()),
            ..Default::default()
        }),
        focus: false,
        ..Default::default()
    }
}

struct Args {
    scene: Option<String>,
    grep: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        scene: None,
        grep: None,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let slot = match arg.as_str() {
            "--scene" => &mut args.scene,
            "--grep" => &mut args.grep,
            other => return Err(format!("unknown argument `{other}`")),
        };
        *slot = Some(
            iter.next()
                .ok_or_else(|| format!("`{arg}` needs a value"))?,
        );
    }
    Ok(args)
}

/// The selection `gpux-testkit` planned, through the same function.
fn selected(id: &str, args: &Args) -> bool {
    gpux_conformance::selects(id, args.scene.as_deref(), args.grep.as_deref())
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("desktop_host: {error}");
            std::process::exit(2);
        }
    };
    let failed = Rc::new(Cell::new(false));
    let any_failed = failed.clone();
    gpui_platform::application().run(move |mut cx| async move {
        if let Err(error) = cx.update(app_host::register_bundled_fonts) {
            eprintln!("desktop_host: could not register the bundled fonts: {error:#}");
        }
        println!("desktop_host: {}", os::describe());
        // Platforms quit when their last window closes; this window outlives every scene's.
        let anchor = match cx.open_window(anchor_options(), |_, cx| cx.new(|_| Anchor)) {
            Ok(anchor) => anchor,
            Err(error) => {
                eprintln!("desktop_host: could not open the host window: {error:#}");
                any_failed.set(true);
                cx.update(|cx| cx.quit());
                return;
            }
        };
        let report = |line: gpux_conformance::report::ResultLine| {
            if line.outcome == Outcome::Fail {
                any_failed.set(true);
            }
            println!("{line}");
        };
        for check in PLATFORM_CHECKS
            .iter()
            .filter(|check| selected(check.id, &args))
        {
            os::park_cursor();
            report(checks::run(check.id, &mut cx).await);
        }
        let host = AppHost::new(Lane::D, point(px(WINDOW_ORIGIN.0), px(WINDOW_ORIGIN.1)))
            .with_failure_frames(gpux_conformance::compare::artifacts_dir("d"));
        for scene in CATALOG
            .iter()
            .filter(|scene| scene.runs_in(Lane::D) && selected(scene.id, &args))
        {
            os::park_cursor();
            report(host.run_scene(&mut cx, scene).await);
        }
        anchor
            .update(&mut cx, |_, window, _| window.remove_window())
            .ok();
        cx.update(|cx| cx.quit());
    });
    std::process::exit(if failed.get() { 1 } else { 0 });
}
