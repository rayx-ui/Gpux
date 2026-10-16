//! The apps connected to the sidecar: a WebSocket server on loopback, one thread per
//! connection, and the registry the MCP tools ask.

use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::ErrorKind,
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::Duration,
};
use tungstenite::{
    Message, WebSocket,
    protocol::{CloseFrame, frame::coding::CloseCode},
};

use crate::{PROTOCOL_VERSION, codes};

/// How long a request to an app may take before it fails, beyond the time a `wait` asks for.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A window an app reported.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowInfo {
    pub id: u64,
    pub title: String,
    pub bounds: Value,
    pub scale_factor: f64,
}

impl WindowInfo {
    fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            id: value.get("id")?.as_u64()?,
            title: value
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            bounds: value.get("bounds").cloned().unwrap_or(Value::Null),
            scale_factor: value
                .get("scaleFactor")
                .and_then(Value::as_f64)
                .unwrap_or(1.0),
        })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "bounds": self.bounds,
            "scaleFactor": self.scale_factor,
        })
    }
}

/// A tool an app offers as an extension of the sidecar's own (`hello`'s `tools`).
#[derive(Clone, Debug)]
pub struct ExtensionTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// What a connected app said about itself in `hello`, kept current by its notifications.
#[derive(Clone, Debug)]
pub struct AppInfo {
    /// The sidecar's number for the connection, unique while the sidecar runs.
    pub connection: u64,
    pub app_id: String,
    pub name: String,
    pub platform: String,
    pub windows: Vec<WindowInfo>,
    pub tools: Vec<ExtensionTool>,
}

impl AppInfo {
    pub fn to_json(&self) -> Value {
        json!({
            "connection": self.connection,
            "id": self.app_id,
            "name": self.name,
            "platform": self.platform,
            "windows": self.windows.iter().map(WindowInfo::to_json).collect::<Vec<_>>(),
            "tools": self.tools.iter().map(|tool| tool.name.clone()).collect::<Vec<_>>(),
        })
    }
}

/// A request to an app failed.
#[derive(Clone, Debug, PartialEq)]
pub enum RequestError {
    /// The app answered with a JSON-RPC error.
    App { code: i64, message: String },
    /// The app disconnected before it answered.
    Disconnected(String),
    /// The app did not answer in time; the request is not sent again.
    Timeout(String),
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestError::App { code, message } => write!(f, "{message} (error {code})"),
            RequestError::Disconnected(app) => write!(f, "the app {app} disconnected"),
            RequestError::Timeout(app) => write!(f, "the app {app} did not answer in time"),
        }
    }
}

type Pending = Arc<Mutex<HashMap<u64, Sender<Result<Value, RequestError>>>>>;

struct Connection {
    info: AppInfo,
    outgoing: Sender<Message>,
    pending: Pending,
    next_id: Arc<AtomicU64>,
}

/// The apps connected to the sidecar.
#[derive(Clone, Default)]
pub struct Registry {
    inner: Arc<Mutex<HashMap<u64, Connection>>>,
    next_connection: Arc<AtomicU64>,
    /// Notifications of apps connecting and disconnecting, for tests and logs.
    events: Arc<Mutex<Vec<Sender<String>>>>,
}

impl Registry {
    /// Every connected app, in the order they connected.
    pub fn apps(&self) -> Vec<AppInfo> {
        let inner = self.inner.lock().unwrap();
        let mut apps: Vec<AppInfo> = inner.values().map(|c| c.info.clone()).collect();
        apps.sort_by_key(|app| app.connection);
        apps
    }

    /// Subscribes to the registry's events (`connected <app>`, `disconnected <app>`,
    /// `refused <reason>`).
    pub fn subscribe(&self) -> Receiver<String> {
        let (sender, receiver) = mpsc::channel();
        self.events.lock().unwrap().push(sender);
        receiver
    }

    fn notify(&self, event: String) {
        self.events
            .lock()
            .unwrap()
            .retain(|sender| sender.send(event.clone()).is_ok());
    }

    /// Sends `method` with `params` to the app on `connection` and waits up to `timeout` for
    /// its answer.
    pub fn request(
        &self,
        connection: u64,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RequestError> {
        let (receiver, label) = {
            let inner = self.inner.lock().unwrap();
            let Some(connection) = inner.get(&connection) else {
                return Err(RequestError::Disconnected(format!("#{connection}")));
            };
            let id = connection.next_id.fetch_add(1, Ordering::SeqCst);
            let (sender, receiver) = mpsc::channel();
            connection.pending.lock().unwrap().insert(id, sender);
            let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
            if connection
                .outgoing
                .send(Message::text(message.to_string()))
                .is_err()
            {
                return Err(RequestError::Disconnected(connection.info.app_id.clone()));
            }
            (receiver, connection.info.app_id.clone())
        };
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(RequestError::Timeout(label)),
            Err(RecvTimeoutError::Disconnected) => Err(RequestError::Disconnected(label)),
        }
    }
}

/// The sidecar's WebSocket server: it listens on loopback only.
pub struct Server {
    pub registry: Registry,
    pub address: SocketAddr,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// Starts listening on `127.0.0.1:port` (0 picks a free port) for apps presenting `token`
    /// when one is set.
    pub fn start(port: u16, token: Option<String>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let registry = Registry::default();
        let stop = Arc::new(AtomicBool::new(false));
        let accept_registry = registry.clone();
        let accept_stop = stop.clone();
        thread::Builder::new()
            .name("gpux-devtools-accept".into())
            .spawn(move || {
                while !accept_stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let registry = accept_registry.clone();
                            let token = token.clone();
                            let stop = accept_stop.clone();
                            let _ = thread::Builder::new()
                                .name("gpux-devtools-app".into())
                                .spawn(move || serve(stream, registry, token, stop));
                        }
                        Err(error) if error.kind() == ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => thread::sleep(Duration::from_millis(50)),
                    }
                }
            })?;
        Ok(Self {
            registry,
            address,
            stop,
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn close_with(socket: &mut WebSocket<TcpStream>, reason: &str) {
    let _ = socket.close(Some(CloseFrame {
        code: CloseCode::Policy,
        reason: reason.to_string().into(),
    }));
    let _ = socket.flush();
}

fn reply(socket: &mut WebSocket<TcpStream>, id: &Value, result: Result<Value, (i64, String)>) {
    let message = match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    };
    let _ = socket.send(Message::text(message.to_string()));
}

/// Serves one app connection: the handshake, `hello`, then requests and answers both ways.
fn serve(stream: TcpStream, registry: Registry, token: Option<String>, stop: Arc<AtomicBool>) {
    let _ = stream.set_nonblocking(false);
    let Ok(mut socket) = tungstenite::accept(stream) else {
        return;
    };
    let _ = socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(10)));

    // The first message is `hello`.
    let hello = loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match socket.read() {
            Ok(Message::Text(text)) => match serde_json::from_str::<Value>(&text) {
                Ok(value) => break value,
                Err(error) => {
                    reply(
                        &mut socket,
                        &Value::Null,
                        Err((-32700, format!("parse error: {error}"))),
                    );
                    close_with(&mut socket, "the first message must be hello");
                    return;
                }
            },
            Ok(Message::Close(_)) => return,
            Ok(_) => continue,
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                continue;
            }
            Err(_) => return,
        }
    };
    let id = hello.get("id").cloned().unwrap_or(Value::Null);
    if hello.get("method").and_then(Value::as_str) != Some("hello") {
        reply(
            &mut socket,
            &id,
            Err((-32600, "the first request must be hello".into())),
        );
        close_with(&mut socket, "the first request must be hello");
        return;
    }
    let params = hello.get("params").cloned().unwrap_or(Value::Null);
    let version = params.get("protocolVersion").and_then(Value::as_u64);
    if version != Some(PROTOCOL_VERSION) {
        let message = format!(
            "protocol version mismatch: the sidecar speaks {PROTOCOL_VERSION}, the app {}",
            version.map_or("none".to_string(), |v| v.to_string())
        );
        reply(&mut socket, &id, Err((codes::VERSION, message.clone())));
        close_with(&mut socket, &message);
        registry.notify(format!("refused {message}"));
        return;
    }
    if let Some(expected) = &token
        && params.get("token").and_then(Value::as_str) != Some(expected.as_str())
    {
        let message = "the token does not match the sidecar's".to_string();
        reply(&mut socket, &id, Err((codes::TOKEN, message.clone())));
        close_with(&mut socket, &message);
        registry.notify(format!("refused {message}"));
        return;
    }

    let connection = registry.next_connection.fetch_add(1, Ordering::SeqCst) + 1;
    let info = AppInfo {
        connection,
        app_id: params
            .get("appId")
            .and_then(Value::as_str)
            .unwrap_or("app")
            .to_string(),
        name: params
            .get("appName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        platform: params
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        windows: params
            .get("windows")
            .and_then(Value::as_array)
            .map(|windows| windows.iter().filter_map(WindowInfo::from_json).collect())
            .unwrap_or_default(),
        tools: params
            .get("tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| {
                        Some(ExtensionTool {
                            name: tool.get("name")?.as_str()?.to_string(),
                            description: tool
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            input_schema: tool
                                .get("inputSchema")
                                .cloned()
                                .unwrap_or_else(|| json!({ "type": "object" })),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };
    reply(
        &mut socket,
        &id,
        Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "server": { "name": crate::SERVER_NAME, "version": crate::SERVER_VERSION },
            "connection": connection,
        })),
    );
    let label = info.app_id.clone();
    let (outgoing, queued) = mpsc::channel::<Message>();
    let pending: Pending = Arc::default();
    registry.inner.lock().unwrap().insert(
        connection,
        Connection {
            info,
            outgoing,
            pending: pending.clone(),
            next_id: Arc::new(AtomicU64::new(1)),
        },
    );
    registry.notify(format!("connected {label}"));

    loop {
        if stop.load(Ordering::SeqCst) {
            close_with(&mut socket, "the sidecar is stopping");
            break;
        }
        while let Ok(message) = queued.try_recv() {
            if socket.send(message).is_err() {
                break;
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                let Ok(message) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                handle_app_message(&registry, connection, &pending, &mut socket, message);
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }

    registry.inner.lock().unwrap().remove(&connection);
    // Requests still waiting fail with the app's name; none is sent again.
    for (_, sender) in pending.lock().unwrap().drain() {
        let _ = sender.send(Err(RequestError::Disconnected(label.clone())));
    }
    registry.notify(format!("disconnected {label}"));
}

fn handle_app_message(
    registry: &Registry,
    connection: u64,
    pending: &Pending,
    socket: &mut WebSocket<TcpStream>,
    message: Value,
) {
    // An answer to one of the sidecar's requests.
    if message.get("method").is_none() {
        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return;
        };
        let Some(sender) = pending.lock().unwrap().remove(&id) else {
            return;
        };
        let result = match message.get("error") {
            Some(error) => Err(RequestError::App {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(-32000),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("the app failed")
                    .to_string(),
            }),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = sender.send(result);
        return;
    }
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let mut inner = registry.inner.lock().unwrap();
    let Some(entry) = inner.get_mut(&connection) else {
        return;
    };
    match method {
        "windowOpened" => {
            if let Some(window) = WindowInfo::from_json(&params) {
                entry
                    .info
                    .windows
                    .retain(|existing| existing.id != window.id);
                entry.info.windows.push(window);
            }
        }
        "windowClosed" => {
            if let Some(id) = params.get("id").and_then(Value::as_u64) {
                entry.info.windows.retain(|window| window.id != id);
            }
        }
        "windowChanged" => {
            if let Some(window) = WindowInfo::from_json(&params)
                && let Some(existing) = entry.info.windows.iter_mut().find(|w| w.id == window.id)
            {
                *existing = window;
            }
        }
        other => {
            if let Some(id) = message.get("id") {
                drop(inner);
                reply(
                    socket,
                    id,
                    Err((-32601, format!("method not found: {other}"))),
                );
            }
        }
    }
}
