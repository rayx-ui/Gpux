//! The `devtools` group on Android: the host asks the testkit for a gpux-devtools sidecar, which
//! the testkit runs on the computer and `adb reverse`s to the device's loopback
//! ([`gpux_tests_conformance::devtools`]'s default port), connects the app to it, and waits for the
//! testkit's probe (its `hello`, a snapshot, and a touch tap on the button) to show its effect in
//! the window's tree. The testkit closes the sidecar when its probe ends, and reports the
//! probe's own failures; the host stays connected until then.

use crate::android::{REQUEST_REPEAT, request};
use crate::checks::{fail, pass};
use gpux::{AnyWindowHandle, AppContext as _, AsyncApp, WindowOptions, semantic_query::Query};
use gpux_tests_conformance::{app_host, devtools, report::ResultLine};
use std::{
    str::FromStr,
    time::{Duration, Instant},
};

/// How long the host waits for the probe's tap.
const PATIENCE: Duration = Duration::from_secs(90);

pub(crate) async fn tap(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "devtools-android";
    match run(cx).await {
        Ok(detail) => pass(ID, detail),
        Err(error) => fail(ID, format!("{error:#}")),
    }
}

async fn run(cx: &mut AsyncApp) -> anyhow::Result<String> {
    let window: AnyWindowHandle = cx
        .open_window(WindowOptions::default(), |window, cx| {
            gpux_tests_conformance::evaluate::keep_inspecting(window);
            cx.new(|_| devtools::ProbeWindow::new())
        })?
        .into();
    app_host::wait_frames(cx, window, 2).await?;
    cx.update(|cx| devtools::connect(cx, devtools::loopback_url()));
    let tapped = Query::from_str(devtools::TAPPED)?;
    let start = Instant::now();
    let mut asked = None::<Instant>;
    let result = loop {
        if asked.is_none_or(|asked| asked.elapsed() > REQUEST_REPEAT * 6) {
            // The testkit starts one sidecar however often it is asked.
            request("devtools");
            asked = Some(Instant::now());
        }
        let shown = window
            .update(cx, |_, window, _| {
                gpux_tests_conformance::evaluate::snapshot(window)
                    .ok()
                    .is_some_and(|snapshot| tapped.resolve(&snapshot).is_ok())
            })
            .unwrap_or(false);
        let connected = cx.update(|cx| gpux_devtools::is_connected(cx));
        if shown && !connected {
            break Ok(format!(
                "connected through adb reverse ({}); the probe's tap showed `{}`",
                devtools::loopback_url(),
                devtools::TAPPED
            ));
        }
        if start.elapsed() > PATIENCE {
            break Err(anyhow::anyhow!(
                "no tap and sidecar closed within {PATIENCE:?} (tap shown: {shown}, connected: \
                 {connected}); see the testkit's probe"
            ));
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    };
    cx.update(devtools::disconnect);
    gpux_tests_conformance::evaluate::stop_inspecting(window);
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
    result
}
