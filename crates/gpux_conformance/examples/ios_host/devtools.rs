//! The `devtools` group on the iOS Simulator: the host runs a gpux-devtools sidecar server on
//! loopback (the Simulator shares the Mac's), connects the app to it, and the sidecar's probe
//! reads `hello` and a snapshot of [`devtools::ProbeWindow`], taps its button with a touch and
//! waits for the tap's effect in the window's tree.

use crate::checks::{fail, pass};
use gpux::{AnyWindowHandle, AppContext as _, AsyncApp, WindowOptions};
use gpux_conformance::{app_host, devtools, report::ResultLine};
use std::{sync::mpsc, time::Duration};

pub(crate) async fn tap(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "devtools-ios";
    match run(cx).await {
        Ok(detail) => pass(ID, detail),
        Err(error) => fail(ID, format!("{error:#}")),
    }
}

async fn run(cx: &mut AsyncApp) -> anyhow::Result<String> {
    let server = gpux_devtools_mcp::apps::Server::start(0, None)?;
    let window: AnyWindowHandle = cx
        .open_window(WindowOptions::default(), |window, cx| {
            gpux_conformance::evaluate::keep_inspecting(window);
            cx.new(|_| devtools::ProbeWindow::new())
        })?
        .into();
    app_host::wait_frames(cx, window, 2).await?;
    let url = format!("ws://{}", server.address);
    cx.update(|cx| devtools::connect(cx, url));
    let registry = server.registry.clone();
    let (sender, result) = mpsc::channel();
    std::thread::spawn(move || {
        sender
            .send(gpux_devtools_mcp::probe::press_button_until(
                &registry,
                devtools::BUTTON,
                "tap",
                devtools::TAPPED,
                Duration::from_secs(30),
            ))
            .ok();
    });
    let probed = loop {
        match result.try_recv() {
            Ok(probed) => break probed,
            Err(mpsc::TryRecvError::Empty) => {
                cx.background_executor()
                    .timer(Duration::from_millis(20))
                    .await
            }
            Err(mpsc::TryRecvError::Disconnected) => anyhow::bail!("the probe thread died"),
        }
    };
    cx.update(devtools::disconnect);
    gpux_conformance::evaluate::stop_inspecting(window);
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
    let probed = probed.map_err(|error| anyhow::anyhow!(error))?;
    anyhow::ensure!(
        probed.app.app_id == devtools::APP_ID,
        "hello named `{}`",
        probed.app.app_id
    );
    Ok(format!(
        "hello from {} over {}; the snapshot showed the button and the tap at ref {} showed `{}`",
        probed.app.app_id,
        server.address,
        probed.reference,
        devtools::TAPPED
    ))
}
