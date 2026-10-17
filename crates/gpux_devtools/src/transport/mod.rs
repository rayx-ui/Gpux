//! The connection to the sidecar: a `tungstenite` client on a background thread on native
//! targets, the browser's `WebSocket` on the web. Either delivers [`Event`]s to the foreground
//! and takes [`Control`]s from it.

#[cfg(not(target_family = "wasm"))]
mod native;
#[cfg(target_family = "wasm")]
mod web;

#[cfg(not(target_family = "wasm"))]
pub(crate) use native::Transport;
#[cfg(target_family = "wasm")]
pub(crate) use web::Transport;

use std::time::Duration;

/// What the connection tells the foreground.
pub(crate) enum Event {
    Connected,
    Text(String),
    /// The connection ended, with the reason when it failed.
    Disconnected(Option<String>),
}

/// What the foreground asks of the connection.
pub(crate) enum Control {
    Send(String),
    /// Closes the connection and stops retrying until the devtools are enabled again (a
    /// refused `hello`).
    Close,
    SetEnabled(bool),
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    Shutdown,
}

/// How long the connection waits between attempts while nothing listens: from `min`, doubling
/// up to `max`.
#[derive(Clone, Copy, Debug)]
pub struct Backoff {
    pub min: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            min: Duration::from_millis(250),
            max: Duration::from_secs(5),
        }
    }
}
