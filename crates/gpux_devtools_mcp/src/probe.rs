//! A short drive of a connected app, for the platform lanes: the app's `hello` reached the
//! registry, a snapshot holds a named button, and a tap at the button's ref goes through.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::apps::{AppInfo, REQUEST_TIMEOUT, Registry};

/// What the probe saw.
#[derive(Debug)]
pub struct Probed {
    pub app: AppInfo,
    /// The snapshot it took.
    pub snapshot: String,
    /// The window it drove.
    pub window: u64,
    /// The ref it tapped.
    pub reference: String,
}

/// Waits up to `patience` for an app to connect, then snapshots its last window, finds the
/// button named `button` and presses it with a synthesized click.
pub fn tap_button(registry: &Registry, button: &str, patience: Duration) -> Result<Probed, String> {
    press_button(registry, button, "click", patience)
}

/// [`tap_button`] with the input `kind` (`click`, or `tap` for a touch).
pub fn press_button(
    registry: &Registry,
    button: &str,
    kind: &str,
    patience: Duration,
) -> Result<Probed, String> {
    let start = Instant::now();
    let app = loop {
        if let Some(app) = registry.apps().into_iter().next() {
            break app;
        }
        if start.elapsed() > patience {
            return Err("no app connected".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let window = app
        .windows
        .last()
        .map(|window| window.id)
        .ok_or_else(|| format!("{} said hello with no window", app.app_id))?;
    let ask = |method: &str, mut params: Value| {
        params["window"] = json!(window);
        registry
            .request(app.connection, method, params, REQUEST_TIMEOUT)
            .map_err(|error| format!("{method}: {error}"))
    };
    // The window may still be drawing its first frames.
    let query = format!("role=button name=\"{button}\"");
    let (snapshot, reference) = loop {
        let snapshot = ask("snapshot", json!({}))?["snapshot"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let nodes = ask("query", json!({ "query": query }))?;
        if let Some(reference) = nodes["nodes"][0]["ref"].as_str() {
            break (snapshot, reference.to_string());
        }
        if start.elapsed() > patience {
            return Err(format!("no `{query}` in the snapshot:\n{snapshot}"));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !snapshot.contains(&format!("button \"{button}\"")) {
        return Err(format!(
            "the snapshot does not show the button:\n{snapshot}"
        ));
    }
    ask("input", json!({ "kind": kind, "ref": reference }))?;
    Ok(Probed {
        app,
        snapshot,
        window,
        reference,
    })
}

/// [`press_button`], then waits up to `patience` for `effect`, a semantic query, to match in
/// the window: the press's effect, as the app's tree shows it.
pub fn press_button_until(
    registry: &Registry,
    button: &str,
    kind: &str,
    effect: &str,
    patience: Duration,
) -> Result<Probed, String> {
    let probed = press_button(registry, button, kind, patience)?;
    let timeout = patience.as_millis() as u64;
    registry
        .request(
            probed.app.connection,
            "wait",
            json!({ "window": probed.window, "query": effect, "timeoutMs": timeout }),
            REQUEST_TIMEOUT + patience,
        )
        .map_err(|error| {
            // What the window shows, to tell a missed tap from a tree that did not update.
            let shown = registry
                .request(
                    probed.app.connection,
                    "snapshot",
                    json!({ "window": probed.window, "bounds": true }),
                    REQUEST_TIMEOUT,
                )
                .map(|snapshot| {
                    snapshot["snapshot"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string()
                })
                .unwrap_or_else(|error| error.to_string());
            format!(
                "after the tap at ref {}: {error}; the window shows\n{shown}",
                probed.reference
            )
        })?;
    Ok(probed)
}
