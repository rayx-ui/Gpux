//! Lane D's desktop host: runs the platform checks and the lane-D conformance scenes in real
//! windows of `gpui_platform::application()`, printing one `gpux-conformance:` result line each.
//!
//! ```text
//! cargo run -p gpux-tests-conformance --no-default-features --features app-host --example desktop_host -- [--scene <id>] [--grep <pattern>]
//! ```
//!
//! `--show <id>` instead opens that scene in a window and leaves it for a person to use (the
//! screen-reader walkthrough of `docs/accessibility.md`); the app quits when the window closes.
//!
//! `gpux-testkit lane d` runs it and parses the result lines. OS-level input (`SendInput` on
//! Windows, XTest through `xdotool` on X11) is required unless `GPUX_OS_INPUT=optional`, which
//! reports the OS-input check skipped when the session cannot deliver synthetic input. When
//! `GPUX_EXPECT_BACKEND` names a backend (lane G names `hardware`), the `desktop-backend` result
//! checks that the windows render on it.

mod checks;
mod composition;
mod composition_linux;
mod composition_windows;
mod os;
mod screen_reader;

use gpux::{
    AppContext as _, AsyncApp, Bounds, Context, IntoElement, ParentElement as _, Render,
    Styled as _, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, div, point,
    px, rgb, size,
};
use gpux_tests_conformance::{
    CATALOG, Lane,
    app_host::{self, AppHost},
    backend::{BackendId, EXPECT_BACKEND_ENV_VAR},
    platform_checks::PLATFORM_CHECKS,
    report::{Outcome, ResultLine},
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
    show: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        scene: None,
        grep: None,
        show: None,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let slot = match arg.as_str() {
            "--scene" => &mut args.scene,
            "--grep" => &mut args.grep,
            "--show" => &mut args.show,
            other => return Err(format!("unknown argument `{other}`")),
        };
        *slot = Some(
            iter.next()
                .ok_or_else(|| format!("`{arg}` needs a value"))?,
        );
    }
    Ok(args)
}

/// Whether the windows render on the backend `expected` names: lane G's guard against a GPU
/// that fails and leaves the windows on a software rasterizer.
fn backend_check(anchor: WindowHandle<Anchor>, expected: &str, cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "desktop-backend";
    match anchor.update(cx, |_, window, _| window.gpu_specs()) {
        Ok(Some(specs)) => {
            let backend = BackendId::from_specs(&specs);
            if backend.is(expected) {
                ResultLine::new(
                    Outcome::Pass,
                    ID,
                    format!("windows render on {}", backend.label()),
                )
            } else {
                ResultLine::new(
                    Outcome::Fail,
                    ID,
                    format!(
                        "windows render on {}, but {EXPECT_BACKEND_ENV_VAR} expects {expected}",
                        backend.label()
                    ),
                )
            }
        }
        Ok(None) => ResultLine::new(
            Outcome::Skip,
            ID,
            "the platform names no adapter for its windows (macOS renders through Metal on the \
             system GPU)",
        ),
        Err(error) => ResultLine::new(Outcome::Fail, ID, format!("{error:#}")),
    }
}

/// Opens the scene `id` in a titled window and runs until it closes.
fn show(id: &str) {
    let Some(scene) = CATALOG.iter().find(|scene| scene.id == id) else {
        eprintln!("desktop_host: no conformance scene `{id}`");
        std::process::exit(2);
    };
    gpui_platform::application().run(move |cx| async move {
        if let Err(error) = cx.update(app_host::register_bundled_fonts) {
            eprintln!("desktop_host: could not register the bundled fonts: {error:#}");
        }
        #[cfg(target_os = "macos")]
        cx.update(|cx| cx.activate(true));
        // Every platform quits with the window, macOS included.
        cx.update(|cx| cx.on_window_closed(|cx, _| cx.quit()).detach());
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(WINDOW_ORIGIN.0), px(WINDOW_ORIGIN.1)),
                size: size(px(scene.size.0), px(scene.size.1)),
            })),
            titlebar: Some(TitlebarOptions {
                title: Some(format!("gpux {}", scene.id).into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        if let Err(error) = cx.open_window(options, |window, cx| {
            cx.new(|cx| app_host::SceneFrame::new(scene, window, cx))
        }) {
            eprintln!("desktop_host: could not open `{}`: {error:#}", scene.id);
            cx.update(|cx| cx.quit());
        }
    });
}

/// The selection `gpux-testkit` planned, through the same function.
fn selected(id: &str, args: &Args) -> bool {
    gpux_tests_conformance::selects(id, args.scene.as_deref(), args.grep.as_deref())
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("desktop_host: {error}");
            std::process::exit(2);
        }
    };
    if let Some(id) = args.show {
        show(&id);
        return;
    }
    let failed = Rc::new(Cell::new(false));
    let any_failed = failed.clone();
    gpui_platform::application().run(move |mut cx| async move {
        if let Err(error) = cx.update(app_host::register_bundled_fonts) {
            eprintln!("desktop_host: could not register the bundled fonts: {error:#}");
        }
        println!("desktop_host: {}", os::describe());
        // macOS runs a window's frames only while the window is at least partly visible, so the
        // host comes to the front rather than opening its windows behind the active app.
        #[cfg(target_os = "macos")]
        cx.update(|cx| cx.activate(true));
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
        let report = |line: ResultLine| {
            if line.outcome == Outcome::Fail {
                any_failed.set(true);
            }
            println!("{line}");
        };
        if let Some(expected) = std::env::var(EXPECT_BACKEND_ENV_VAR)
            .ok()
            .filter(|value| !value.is_empty())
        {
            report(backend_check(anchor, &expected, &mut cx));
        }
        for check in PLATFORM_CHECKS
            .iter()
            .filter(|check| selected(check.id, &args))
        {
            os::park_cursor();
            report(checks::run(check.id, &mut cx).await);
        }
        let host = AppHost::new(Lane::D, point(px(WINDOW_ORIGIN.0), px(WINDOW_ORIGIN.1)))
            .with_failure_frames(gpux_tests_conformance::compare::artifacts_dir("d"));
        let host = match os::cursor_sync() {
            Some(sync) => host.with_cursor_sync(sync),
            None => host,
        };
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
