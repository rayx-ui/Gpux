//! The native transport: a background thread runs a `tungstenite` client, retrying with bounded
//! backoff while no sidecar listens, and trades text frames with the foreground over channels.
//! gpui state is never touched here.

use std::{
    io::ErrorKind,
    net::TcpStream,
    sync::mpsc::{self, RecvTimeoutError, TryRecvError},
    time::Duration,
};

use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

use super::{Backoff, Control, Event};

/// How long a read waits before the thread looks for frames to send.
const POLL: Duration = Duration::from_millis(10);

/// The connection's thread, through the channel that controls it.
pub(crate) struct Transport {
    control: mpsc::Sender<Control>,
}

impl Transport {
    /// Starts the thread, connecting to `url` while `enabled`.
    pub(crate) fn start(
        url: String,
        backoff: Backoff,
        enabled: bool,
        events: async_channel::Sender<Event>,
    ) -> Self {
        let (control, controls) = mpsc::channel();
        std::thread::Builder::new()
            .name("gpux-devtools".into())
            .spawn(move || run(url, backoff, enabled, controls, events))
            .expect("could not start the devtools thread");
        Self { control }
    }

    pub(crate) fn control(&self, control: Control) {
        self.control.send(control).ok();
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.control.send(Control::Shutdown).ok();
    }
}

enum Ended {
    /// The sidecar went away, or the connection failed.
    Lost(Option<String>),
    /// The app closed it: no retrying until it is enabled again.
    Halted,
    /// The app switched the devtools off.
    Disabled,
    Shutdown,
}

fn run(
    url: String,
    backoff: Backoff,
    mut enabled: bool,
    controls: mpsc::Receiver<Control>,
    events: async_channel::Sender<Event>,
) {
    let mut halted = false;
    let mut delay = backoff.min;
    loop {
        // Off, or closed by the app: wait to be switched on again.
        while !enabled || halted {
            match controls.recv() {
                Ok(Control::SetEnabled(on)) => {
                    enabled = on;
                    halted = false;
                    delay = backoff.min;
                }
                Ok(Control::Shutdown) | Err(_) => return,
                Ok(_) => {}
            }
        }
        match tungstenite::connect(url.as_str()) {
            Ok((mut socket, _)) => {
                delay = backoff.min;
                if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
                    stream.set_read_timeout(Some(POLL)).ok();
                    stream.set_nodelay(true).ok();
                }
                if events.send_blocking(Event::Connected).is_err() {
                    return;
                }
                match connected(&mut socket, &controls, &events) {
                    Ended::Lost(reason) => {
                        if events.send_blocking(Event::Disconnected(reason)).is_err() {
                            return;
                        }
                    }
                    Ended::Halted => {
                        halted = true;
                        events.send_blocking(Event::Disconnected(None)).ok();
                    }
                    Ended::Disabled => {
                        enabled = false;
                        events.send_blocking(Event::Disconnected(None)).ok();
                    }
                    Ended::Shutdown => return,
                }
            }
            Err(_) => {
                // Nothing listens: wait, still answering the controls, then try again.
                match controls.recv_timeout(delay) {
                    Ok(Control::SetEnabled(on)) => enabled = on,
                    Ok(Control::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
                    Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                }
                delay = (delay * 2).min(backoff.max);
            }
        }
    }
}

fn connected(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    controls: &mpsc::Receiver<Control>,
    events: &async_channel::Sender<Event>,
) -> Ended {
    loop {
        loop {
            match controls.try_recv() {
                Ok(Control::Send(text)) => {
                    if let Err(error) = socket.send(Message::text(text)) {
                        return Ended::Lost(Some(error.to_string()));
                    }
                }
                Ok(Control::Close) => {
                    socket.close(None).ok();
                    socket.flush().ok();
                    return Ended::Halted;
                }
                Ok(Control::SetEnabled(false)) => {
                    socket.close(None).ok();
                    socket.flush().ok();
                    return Ended::Disabled;
                }
                Ok(Control::SetEnabled(true)) => {}
                Ok(Control::Shutdown) | Err(TryRecvError::Disconnected) => {
                    socket.close(None).ok();
                    socket.flush().ok();
                    return Ended::Shutdown;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                if events.send_blocking(Event::Text(text.to_string())).is_err() {
                    return Ended::Shutdown;
                }
            }
            Ok(Message::Close(_)) => return Ended::Lost(None),
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Ended::Lost(None);
            }
            Err(error) => return Ended::Lost(Some(error.to_string())),
        }
    }
}
