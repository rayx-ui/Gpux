//! The app side of the `devtools` lane groups: a window with a button an agent taps through
//! gpux-devtools, and the connection to the sidecar the lane runs. The lane's side drives it
//! with `gpux_devtools_mcp::probe`. The names both sides share are here in every build; the
//! window and the connection come with the `app-host` feature.

#[cfg(feature = "app-host")]
pub use app::*;

/// The app id the hosts say `hello` with.
pub const APP_ID: &str = "dev.gpux.conformance";
/// The button the probe taps.
pub const BUTTON: &str = "Tap me";
/// The query that matches once the probe's tap went through.
pub const TAPPED: &str = "text=\"taps 1\"";

/// The port the device-side hosts connect to on loopback, which the testkit forwards to its
/// sidecar (Android's `adb reverse`): the sidecar's default.
pub const LOOPBACK_PORT: u16 = 9230;

/// `ws://127.0.0.1:` [`LOOPBACK_PORT`].
pub fn loopback_url() -> String {
    format!("ws://127.0.0.1:{LOOPBACK_PORT}")
}

#[cfg(feature = "app-host")]
mod app {
    use super::{APP_ID, BUTTON};
    use gpux::{
        App, Context, IntoElement, Render, Role, SharedString, Window, div, prelude::*, px,
    };
    use std::time::Duration;

    /// A window with [`BUTTON`] and a line counting its taps.
    pub struct ProbeWindow {
        taps: usize,
    }

    impl ProbeWindow {
        pub fn new() -> Self {
            Self { taps: 0 }
        }
    }

    impl Default for ProbeWindow {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Render for ProbeWindow {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap(px(12.))
                .p(px(24.))
                .bg(gpux::rgb(0xffffff))
                .text_color(gpux::rgb(0x000000))
                .child(
                    div()
                        .id("devtools-tap")
                        .role(Role::Button)
                        .aria_label(BUTTON)
                        .w(px(160.))
                        .h(px(48.))
                        .bg(gpux::rgb(0x2060c0))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.taps += 1;
                            cx.notify();
                        })),
                )
                .child(SharedString::from(format!("taps {}", self.taps)))
        }
    }

    /// Connects the app to the sidecar at `url`, retrying quickly while it is not up yet.
    pub fn connect(cx: &mut App, url: impl Into<String>) {
        gpux_devtools::install(
            cx,
            gpux_devtools::Options::new(APP_ID, "gpux conformance")
                .url(url)
                .enabled(true)
                .backoff(Duration::from_millis(50), Duration::from_millis(500)),
        );
    }

    /// Ends the connection, so a host that runs more checks leaves no client retrying.
    pub fn disconnect(cx: &mut App) {
        gpux_devtools::set_enabled(cx, false);
    }
}
