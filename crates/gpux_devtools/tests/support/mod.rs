//! A sidecar stand-in for the devtools tests: a WebSocket server on loopback, and a pump that
//! runs the test app while the server waits for the app's frames.

#![allow(dead_code)]

use std::{
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

use gpux::{Context, IntoElement, Render, Role, TestAppContext, Window, div, prelude::*, px};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

/// How long a test waits for something that should happen.
pub const PATIENCE: Duration = Duration::from_secs(10);

/// Runs the app a step: its tasks, a slice of its clock, and a moment of real time for the
/// connection's thread.
pub fn pump(cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(50));
    cx.run_until_parked();
    std::thread::sleep(Duration::from_millis(2));
}

/// Pumps the app until `done` returns something, for at most [`PATIENCE`].
pub fn until<T>(
    cx: &mut TestAppContext,
    what: &str,
    mut done: impl FnMut(&mut TestAppContext) -> Option<T>,
) -> T {
    let start = Instant::now();
    loop {
        if let Some(value) = done(cx) {
            return value;
        }
        assert!(start.elapsed() < PATIENCE, "timed out waiting for {what}");
        pump(cx);
    }
}

/// A WebSocket server on a free loopback port.
pub struct Server {
    listener: TcpListener,
    pub port: u16,
}

impl Server {
    pub fn bind() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        Self { listener, port }
    }

    /// A server on a listener the test bound itself (non-blocking).
    pub fn from_listener(listener: TcpListener) -> Self {
        let port = listener.local_addr().unwrap().port();
        Self { listener, port }
    }

    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.port)
    }

    /// A connection, if an app connected.
    pub fn try_accept(&self) -> Option<Connection> {
        match self.listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                let socket = tungstenite::accept(stream).ok()?;
                socket
                    .get_ref()
                    .set_read_timeout(Some(Duration::from_millis(5)))
                    .unwrap();
                Some(Connection { socket })
            }
            Err(_) => None,
        }
    }

    /// Waits for the app to connect, pumping it.
    pub fn accept(&self, cx: &mut TestAppContext) -> Connection {
        until(cx, "the app to connect", |_| self.try_accept())
    }
}

/// A connected app, from the sidecar's side.
pub struct Connection {
    socket: WebSocket<TcpStream>,
}

impl Connection {
    /// The next message, if one arrived.
    pub fn try_read(&mut self) -> Option<Result<Value, ()>> {
        match self.socket.read() {
            Ok(Message::Text(text)) => Some(Ok(serde_json::from_str(&text).unwrap())),
            Ok(Message::Close(_)) => Some(Err(())),
            Ok(_) => None,
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                None
            }
            Err(_) => Some(Err(())),
        }
    }

    /// The next message, pumping the app; panics when the app closed the connection.
    pub fn read(&mut self, cx: &mut TestAppContext) -> Value {
        until(cx, "a message", |_| self.try_read())
            .unwrap_or_else(|()| panic!("the app closed the connection"))
    }

    /// Waits for the app to close the connection.
    pub fn expect_closed(&mut self, cx: &mut TestAppContext) {
        until(cx, "the app to close", |_| match self.try_read() {
            Some(Err(())) => Some(()),
            _ => None,
        });
    }

    pub fn send(&mut self, value: Value) {
        self.socket.send(Message::text(value.to_string())).unwrap();
    }

    /// Reads the app's `hello` and welcomes it.
    pub fn handshake(&mut self, cx: &mut TestAppContext) -> Value {
        let hello = self.read(cx);
        assert_eq!(hello["method"], "hello");
        self.send(json!({"jsonrpc": "2.0", "id": hello["id"], "result": {"protocolVersion": 1}}));
        hello
    }

    /// Sends a request and waits for its response.
    pub fn request(
        &mut self,
        cx: &mut TestAppContext,
        id: u64,
        method: &str,
        params: Value,
    ) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self.read(cx);
            if message.get("method").is_none() && message["id"] == json!(id) {
                return message;
            }
        }
    }

    pub fn close(mut self) {
        self.socket.close(None).ok();
        self.socket.flush().ok();
    }
}

/// A small window: a titled panel with a button that counts its clicks.
pub struct Panel {
    pub clicks: usize,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                div()
                    .id("save")
                    .role(Role::Button)
                    .aria_label("Save")
                    .w(px(80.))
                    .h(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        cx.notify();
                    })),
            )
            .child(gpux::SharedString::from(format!("clicks {}", self.clicks)))
    }
}

/// A window of controls for the request tests: a button counting clicks, a check box, a slider,
/// a disclosure, a card with a custom action, a text field, and a scroll container holding a
/// button below its fold. The status line reports what happened.
pub struct Controls {
    pub log: Vec<String>,
    pub checked: bool,
    pub volume: f64,
    pub expanded: bool,
    pub title: gpux::Entity<gpux::EditableTextState>,
    pub focus: gpux::FocusHandle,
}

impl Render for Controls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        let on = move |entry: &'static str| {
            let entity = entity.clone();
            move |data: Option<&gpux::accesskit::ActionData>, _: &mut Window, cx: &mut gpux::App| {
                let data = data.cloned();
                entity
                    .update(cx, |this: &mut Self, cx| {
                        match (entry, data) {
                            ("set", Some(gpux::accesskit::ActionData::NumericValue(value))) => {
                                this.volume = value
                            }
                            ("increment", _) => this.volume += 1.,
                            ("decrement", _) => this.volume -= 1.,
                            ("expand", _) => this.expanded = true,
                            ("collapse", _) => this.expanded = false,
                            _ => {}
                        }
                        this.log.push(entry.to_string());
                        cx.notify();
                    })
                    .ok();
            }
        };
        let log = cx.entity().downgrade();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("save")
                    .role(Role::Button)
                    .aria_label("Save")
                    .w(px(80.))
                    .h(px(20.))
                    .on_click(cx.listener(|this, event: &gpux::ClickEvent, _, cx| {
                        this.log.push(format!("click {}", event.click_count()));
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("wrap")
                    .role(Role::CheckBox)
                    .aria_label("Wrap")
                    .aria_toggled(if self.checked {
                        gpux::Toggled::True
                    } else {
                        gpux::Toggled::False
                    })
                    .track_focus(&self.focus)
                    .w(px(20.))
                    .h(px(20.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.checked = !this.checked;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("volume")
                    .role(Role::Slider)
                    .aria_label("Volume")
                    .aria_numeric_value(self.volume)
                    .w(px(100.))
                    .h(px(20.))
                    .on_a11y_action(gpux::AccessibleAction::SetValue, on("set"))
                    .on_a11y_action(gpux::AccessibleAction::Increment, on("increment"))
                    .on_a11y_action(gpux::AccessibleAction::Decrement, on("decrement")),
            )
            .child(
                div()
                    .id("details")
                    .role(Role::Button)
                    .aria_label("Details")
                    .aria_expanded(self.expanded)
                    .w(px(80.))
                    .h(px(20.))
                    .on_a11y_action(gpux::AccessibleAction::Expand, on("expand"))
                    .on_a11y_action(gpux::AccessibleAction::Collapse, on("collapse")),
            )
            .child(
                div()
                    .id("card")
                    .role(Role::Group)
                    .aria_label("Card")
                    .h(px(20.))
                    .on_a11y_custom_action(3, "Archive", move |_, cx| {
                        log.update(cx, |this, cx| {
                            this.log.push("archive".into());
                            cx.notify();
                        })
                        .ok();
                    }),
            )
            .child(
                div()
                    .w(px(200.))
                    .child(gpux::EditableText::new(&self.title).aria_label("Title")),
            )
            .child(
                div()
                    .id("shelf")
                    .h(px(40.))
                    .w(px(120.))
                    .overflow_y_scroll()
                    .child(div().h(px(200.)))
                    .child(
                        div()
                            .id("far")
                            .role(Role::Button)
                            .aria_label("Far")
                            .h(px(20.)),
                    ),
            )
            .child(gpux::SharedString::from(format!(
                "status {} checked {} volume {} expanded {}",
                self.log.join(","),
                self.checked,
                self.volume,
                self.expanded
            )))
    }
}

/// Opens [`Controls`] in a window of the test platform, inspected so the window builds its tree.
pub fn controls(cx: &mut TestAppContext) -> (gpux::WindowHandle<Controls>, gpux::A11yInspection) {
    use gpux::AppContext as _;
    let window = cx.add_window(|_, cx| Controls {
        log: Vec::new(),
        checked: false,
        volume: 5.,
        expanded: false,
        title: cx.new(gpux::EditableTextState::new),
        focus: cx.focus_handle(),
    });
    let guard = cx
        .update_window(window.into(), |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    cx.run_until_parked();
    (window, guard)
}

/// Runs the request `method` against the app, as the sidecar would ask it.
pub fn request(
    cx: &mut TestAppContext,
    method: &str,
    params: Value,
) -> Result<Value, gpux_devtools::protocol::RpcError> {
    let task = cx.update(|cx| gpux_devtools::requests::handle(cx, method, params));
    let mut task = Some(task);
    let result = std::rc::Rc::new(std::cell::RefCell::new(None));
    {
        let result = result.clone();
        let task = task.take().unwrap();
        cx.update(|cx| {
            cx.spawn(async move |_| {
                *result.borrow_mut() = Some(task.await);
            })
            .detach()
        });
    }
    until(cx, "the request's answer", |_| result.borrow_mut().take())
}

/// The ref of the node `query` matches.
pub fn reference(cx: &mut TestAppContext, query: &str) -> String {
    let nodes = request(cx, "query", json!({"query": query})).unwrap();
    nodes["nodes"][0]["ref"]
        .as_str()
        .unwrap_or_else(|| panic!("nothing matches {query}: {nodes}"))
        .to_string()
}

/// The status line's text.
pub fn status(cx: &mut TestAppContext) -> String {
    let nodes = request(cx, "query", json!({"query": "text~=status"})).unwrap();
    nodes["nodes"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}
