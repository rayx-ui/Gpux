//! Lane F's iOS host: a static library the UIKit shell in `tooling/ios` links. It runs the iOS
//! platform checks and the lane-F conformance scenes in the app's windows, printing one
//! `gpux-conformance:` result line each to standard output, which `xcrun simctl launch
//! --console-pty` relays to `gpux-testkit ios run`.
//!
//! The shell's app delegate calls `gpux_ios_host_register` and then gpui_mobile's
//! `gpui_ios_run_demo`. Launch arguments narrow the run as elsewhere (`--scene <id>`,
//! `--grep <pattern>`). Checks that need the operating system ask the testkit for it in a
//! `gpux-conformance-host:` request line:
//!
//! - `background` and `foreground`: another app comes to the front, and the host is launched
//!   again, which brings the running one back.
//!
//! Pixel steps read frames back through the window's wgpu renderer; where the Simulator offers
//! no Metal device the window has no renderer, and pixel steps report themselves skipped. The
//! run ends with `gpux-conformance-host: done`, and the host exits. Failing pixel steps save
//! their frame under the app's `Documents/frames/`, which the testkit copies.

#[cfg(target_os = "ios")]
mod checks;
#[cfg(target_os = "ios")]
mod composition;
mod platform_views;
#[cfg(target_os = "ios")]
mod scroll_layers;

#[cfg(target_os = "ios")]
mod ios {
    use crate::checks;
    use gpux::{
        AppContext as _, AsyncApp, Context, IntoElement, Render, Styled as _, Window,
        WindowOptions, div, point, px, rgb,
    };
    use gpux_conformance::{
        CATALOG, Lane,
        app_host::{self, AppHost},
        report::{Outcome, ResultLine},
    };
    use std::{io::Write as _, path::PathBuf, sync::OnceLock};

    /// Prints a result line where the testkit reads it.
    pub fn report(line: &ResultLine) {
        println!("{line}");
        std::io::stdout().flush().ok();
    }

    /// Asks the testkit for an operating-system action.
    pub fn request(request: &str) {
        println!("gpux-conformance-host: {request}");
        std::io::stdout().flush().ok();
    }

    /// The selection the launch arguments carry.
    struct Selection {
        scene: Option<String>,
        grep: Option<String>,
    }

    fn selection() -> &'static Selection {
        static SELECTION: OnceLock<Selection> = OnceLock::new();
        SELECTION.get_or_init(|| {
            let mut selection = Selection {
                scene: None,
                grep: None,
            };
            let mut args = std::env::args().skip(1);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--scene" => selection.scene = args.next(),
                    "--grep" => selection.grep = args.next(),
                    _ => {}
                }
            }
            selection
        })
    }

    /// Whether the test `id` is in the selection.
    pub fn selected(id: &str) -> bool {
        let selection = selection();
        gpux_conformance::selects(id, selection.scene.as_deref(), selection.grep.as_deref())
    }

    /// The app's documents folder, where failing pixel steps save their frames.
    fn documents() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Documents")
    }

    /// The host's own window: open for the whole run, beneath every scene's window.
    struct Anchor;

    impl Render for Anchor {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0xffffff))
        }
    }

    async fn run(cx: &mut AsyncApp) {
        if let Err(error) = cx.update(app_host::register_bundled_fonts) {
            eprintln!("ios_host: could not register the bundled fonts: {error:#}");
        }
        let _anchor = match cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Anchor)) {
            Ok(anchor) => anchor,
            Err(error) => {
                report(&ResultLine::new(
                    Outcome::Fail,
                    "ios-startup".to_string(),
                    format!("could not open the host window: {error:#}"),
                ));
                request("done");
                std::process::exit(1);
            }
        };
        for line in checks::platform_checks(cx).await {
            report(&line);
        }
        let host = AppHost::new(Lane::F, point(px(0.), px(0.)))
            .with_failure_frames(documents().join("frames"));
        for scene in CATALOG
            .iter()
            .filter(|scene| scene.runs_in(Lane::F) && selected(scene.id))
        {
            report(&host.run_scene(cx, scene).await);
        }
        request("done");
        // `simctl launch --console-pty` returns when the app exits.
        std::process::exit(0);
    }

    /// Registers the host as the app gpui_mobile runs; the shell calls it before
    /// `gpui_ios_run_demo`.
    #[unsafe(no_mangle)]
    pub extern "C" fn gpux_ios_host_register() {
        gpui_mobile::ios::ffi::set_app_callback(Box::new(|mut cx| {
            Box::pin(async move { run(&mut cx).await })
        }));
    }
}
