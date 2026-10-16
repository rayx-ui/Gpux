//! Agent inspection for gpux apps.
//!
//! [`install`] makes an app connect out, over a WebSocket, to the `gpux-devtools-mcp` sidecar,
//! which offers the app to an agent as MCP tools. The app answers from its windows' AccessKit
//! trees: [`snapshot`](requests::snapshot) and `query` read them, `act` performs accessibility
//! actions, `input` synthesizes pointer, touch and keyboard input, `screenshot` renders a window
//! to PNG, `wait` waits for a query to match, and `stats` reports frame counters. Apps add their
//! own tools with [`Options::extension`].
//!
//! The devtools are an optional dependency: `gpux` and `gpux-platform` never depend on this
//! crate, so an app links none of it (no WebSocket client, no query engine) unless it adds the
//! dependency, usually behind a Cargo feature of its own. At runtime [`Options::enabled`],
//! `GPUX_DEVTOOLS` (`on`/`off`) and [`set_enabled`] switch it; while it is off or disconnected
//! it costs a retry timer and nothing per frame. While connected, every window holds an
//! inspection guard ([`gpux::Window::begin_a11y_inspection`]), so its tree is built without any
//! screen reader and without telling the operating system about one.
//!
//! The URL is [`Options::url`], else `GPUX_DEVTOOLS_URL`, else (in the browser) the page's
//! `gpux_devtools` query parameter (a URL, or a port on loopback), else
//! [`protocol::default_url`]. The protocol is described in [`protocol`].

pub mod protocol;
pub mod requests;
mod snapshot;
mod transport;

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

use gpux::{A11yInspection, AnyWindowHandle, App, AsyncApp, Global, Task, WindowId};
use protocol::{HELLO_ID, Hello, Message, PROTOCOL_VERSION, RpcError, ToolInfo, Welcome};
use serde_json::{Value, json};

pub use snapshot::render_snapshot;
pub use transport::Backoff;
use transport::{Control, Event, Transport};

/// The variable that switches the devtools at startup: `on`/`1` or `off`/`0`.
pub const ENABLED_ENV: &str = "GPUX_DEVTOOLS";
/// The variable naming the sidecar's URL.
pub const URL_ENV: &str = "GPUX_DEVTOOLS_URL";
/// The variable holding the shared token the sidecar was started with.
pub const TOKEN_ENV: &str = "GPUX_DEVTOOLS_TOKEN";

/// What an app's own tool does with its arguments.
pub type ExtensionHandler = Rc<dyn Fn(Value, &mut App) -> anyhow::Result<Value>>;

/// A tool an app adds to the sidecar's list, which forwards calls of it to the app.
#[derive(Clone)]
pub struct Extension {
    pub name: String,
    pub description: String,
    /// The JSON schema of the tool's arguments.
    pub input_schema: Value,
    pub handler: ExtensionHandler,
}

impl Extension {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl Fn(Value, &mut App) -> anyhow::Result<Value> + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            handler: Rc::new(handler),
        }
    }
}

/// How the app connects to the sidecar.
#[derive(Clone)]
pub struct Options {
    app_id: String,
    app_name: String,
    url: Option<String>,
    token: Option<String>,
    enabled: Option<bool>,
    backoff: Backoff,
    extensions: Vec<Extension>,
}

impl Options {
    /// Options for the app `app_id` (stable, such as `dev.rayx.lab`) named `app_name`.
    pub fn new(app_id: impl Into<String>, app_name: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            app_name: app_name.into(),
            url: None,
            token: None,
            enabled: None,
            backoff: Backoff::default(),
            extensions: Vec::new(),
        }
    }

    /// The sidecar's URL, over `GPUX_DEVTOOLS_URL` and the default.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// The token the sidecar requires, over `GPUX_DEVTOOLS_TOKEN`.
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Whether the devtools start on, over `GPUX_DEVTOOLS`. [`set_enabled`] changes it later.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// How long to wait between attempts while no sidecar listens.
    pub fn backoff(mut self, min: Duration, max: Duration) -> Self {
        self.backoff = Backoff { min, max };
        self
    }

    /// Adds a tool of the app's own.
    pub fn extension(mut self, extension: Extension) -> Self {
        self.extensions.push(extension);
        self
    }

    /// The URL the app connects to: [`Self::url`], else `GPUX_DEVTOOLS_URL`, else (web) the
    /// page's `gpux_devtools` parameter, else [`protocol::default_url`].
    pub fn resolved_url(&self) -> String {
        resolve_url(self)
    }

    /// Whether the devtools start on: [`Self::enabled`], else `GPUX_DEVTOOLS`, else on.
    pub fn resolved_enabled(&self) -> bool {
        resolve_enabled(self)
    }
}

/// The URL the options and the environment name, in order of precedence.
fn resolve_url(options: &Options) -> String {
    if let Some(url) = &options.url {
        return url.clone();
    }
    if let Some(url) = std::env::var(URL_ENV).ok().filter(|url| !url.is_empty()) {
        return url;
    }
    #[cfg(target_family = "wasm")]
    if let Some(url) = page_parameter() {
        return url;
    }
    protocol::default_url()
}

/// The page's `gpux_devtools` query parameter: a URL, or a port on loopback.
#[cfg(target_family = "wasm")]
fn page_parameter() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    let value = web_sys::UrlSearchParams::new_with_str(&search)
        .ok()?
        .get("gpux_devtools")?;
    if value.parse::<u16>().is_ok() {
        Some(format!("ws://127.0.0.1:{value}"))
    } else if value.starts_with("ws://") || value.starts_with("wss://") {
        Some(value)
    } else {
        // `?gpux_devtools` alone, or `=1`: the default sidecar.
        Some(protocol::default_url())
    }
}

/// Whether the options and `GPUX_DEVTOOLS` start the devtools on.
fn resolve_enabled(options: &Options) -> bool {
    if let Some(enabled) = options.enabled {
        return enabled;
    }
    match std::env::var(ENABLED_ENV).ok().as_deref() {
        Some("off" | "0" | "false" | "no") => false,
        Some(_) | None => true,
    }
}

struct Devtools {
    app_id: String,
    app_name: String,
    token: Option<String>,
    extensions: Vec<Extension>,
    enabled: bool,
    connected: bool,
    transport: Transport,
    /// The inspection guard of every window, while connected.
    guards: HashMap<WindowId, A11yInspection>,
    /// The windows the sidecar knows of.
    announced: HashSet<WindowId>,
    /// Why the sidecar refused the app, until it is enabled again.
    refused: Option<String>,
    _events: Task<()>,
    watch: Option<Task<()>>,
}

impl Global for Devtools {}

/// Installs the devtools: the app connects to the sidecar (when enabled) and answers it.
/// Installing again replaces the earlier connection.
pub fn install(cx: &mut App, options: Options) {
    let enabled = options.resolved_enabled();
    let url = options.resolved_url();
    let (sender, events) = async_channel::unbounded();
    let transport = Transport::start(url, options.backoff, enabled, sender);
    let task = cx.spawn(async move |cx| {
        while let Ok(event) = events.recv().await {
            cx.update(|cx| on_event(cx, event));
        }
    });
    let token = options.token.clone().or_else(|| {
        std::env::var(TOKEN_ENV)
            .ok()
            .filter(|token| !token.is_empty())
    });
    cx.set_global(Devtools {
        app_id: options.app_id,
        app_name: options.app_name,
        token,
        extensions: options.extensions,
        enabled,
        connected: false,
        transport,
        guards: HashMap::default(),
        announced: HashSet::default(),
        refused: None,
        _events: task,
        watch: None,
    });
}

/// Switches the devtools on or off at runtime. Off closes the connection, drops every
/// inspection guard and stops retrying; on resumes connecting.
pub fn set_enabled(cx: &mut App, enabled: bool) {
    let Some(devtools) = cx.try_global::<Devtools>() else {
        return;
    };
    if devtools.enabled == enabled {
        return;
    }
    devtools.transport.control(Control::SetEnabled(enabled));
    let devtools = cx.global_mut::<Devtools>();
    devtools.enabled = enabled;
    devtools.refused = None;
    if !enabled {
        disconnect(cx);
    }
}

/// Whether the devtools are on.
pub fn is_enabled(cx: &App) -> bool {
    cx.try_global::<Devtools>()
        .is_some_and(|devtools| devtools.enabled)
}

/// Whether the app is connected to a sidecar.
pub fn is_connected(cx: &App) -> bool {
    cx.try_global::<Devtools>()
        .is_some_and(|devtools| devtools.connected)
}

/// Why the sidecar refused the app (a protocol version it does not speak, a wrong token), until
/// the devtools are enabled again.
pub fn refusal(cx: &App) -> Option<String> {
    cx.try_global::<Devtools>()?.refused.clone()
}

/// How many windows hold the devtools' inspection guard.
pub fn guarded_windows(cx: &App) -> usize {
    cx.try_global::<Devtools>()
        .map_or(0, |devtools| devtools.guards.len())
}

fn send(cx: &App, message: Message) {
    if let Some(devtools) = cx.try_global::<Devtools>() {
        devtools.transport.control(Control::Send(message.to_text()));
    }
}

fn on_event(cx: &mut App, event: Event) {
    match event {
        Event::Connected => connected(cx),
        Event::Text(text) => received(cx, &text),
        Event::Disconnected(reason) => {
            if let Some(reason) = reason {
                log::info!("devtools: the sidecar connection ended: {reason}");
            }
            disconnect(cx);
        }
    }
}

fn connected(cx: &mut App) {
    if !cx.has_global::<Devtools>() {
        return;
    }
    cx.global_mut::<Devtools>().connected = true;
    let windows = requests::window_infos(cx);
    for window in cx.windows() {
        guard(cx, window);
    }
    let devtools = cx.global::<Devtools>();
    let hello = Hello {
        protocol_version: PROTOCOL_VERSION,
        app_id: devtools.app_id.clone(),
        app_name: devtools.app_name.clone(),
        platform: protocol::platform().into(),
        windows,
        tools: devtools
            .extensions
            .iter()
            .map(|extension| ToolInfo {
                name: extension.name.clone(),
                description: extension.description.clone(),
                input_schema: extension.input_schema.clone(),
            })
            .collect(),
        token: devtools.token.clone(),
    };
    send(
        cx,
        Message::request(
            HELLO_ID,
            "hello",
            serde_json::to_value(hello).unwrap_or_default(),
        ),
    );
    // Windows that open and close while connected are announced, and guarded.
    let watch = cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            cx.update(watch_windows);
        }
    });
    cx.global_mut::<Devtools>().watch = Some(watch);
}

/// Takes an inspection guard for `window` and marks it announced.
fn guard(cx: &mut App, window: AnyWindowHandle) {
    let Ok(guard) = window.update(cx, |_, window, _| window.begin_a11y_inspection()) else {
        return;
    };
    let devtools = cx.global_mut::<Devtools>();
    devtools.guards.insert(window.window_id(), guard);
    devtools.announced.insert(window.window_id());
}

fn watch_windows(cx: &mut App) {
    if !cx.try_global::<Devtools>().is_some_and(|d| d.connected) {
        return;
    }
    let open = cx.windows();
    let ids: HashSet<WindowId> = open.iter().map(|window| window.window_id()).collect();
    let opened: Vec<AnyWindowHandle> = open
        .into_iter()
        .filter(|window| {
            !cx.global::<Devtools>()
                .announced
                .contains(&window.window_id())
        })
        .collect();
    for window in opened {
        guard(cx, window);
        if let Some(info) = requests::window_info(cx, window) {
            send(
                cx,
                Message::notification(
                    "windowOpened",
                    serde_json::to_value(info).unwrap_or_default(),
                ),
            );
        }
    }
    let closed: Vec<WindowId> = cx
        .global::<Devtools>()
        .announced
        .iter()
        .filter(|id| !ids.contains(id))
        .copied()
        .collect();
    for id in closed {
        let devtools = cx.global_mut::<Devtools>();
        devtools.announced.remove(&id);
        devtools.guards.remove(&id);
        send(
            cx,
            Message::notification("windowClosed", json!({ "id": id.as_u64() })),
        );
    }
}

fn disconnect(cx: &mut App) {
    if !cx.has_global::<Devtools>() {
        return;
    }
    let devtools = cx.global_mut::<Devtools>();
    devtools.connected = false;
    devtools.watch = None;
    devtools.announced.clear();
    // Dropping the guards lets the windows stop building their trees.
    let guards = std::mem::take(&mut devtools.guards);
    drop(guards);
}

fn received(cx: &mut App, text: &str) {
    let message: Message = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(error) => {
            // The id is recovered from the raw JSON where it can be, as errors keep their ids.
            let id = serde_json::from_str::<Value>(text)
                .ok()
                .and_then(|value| value.get("id").cloned());
            send(
                cx,
                Message::response(
                    id,
                    Err(RpcError::new(
                        protocol::codes::PARSE_ERROR,
                        error.to_string(),
                    )),
                ),
            );
            return;
        }
    };
    if message.is_response() {
        if message.id.as_ref().and_then(Value::as_str) == Some(HELLO_ID) {
            welcomed(cx, message);
        }
        return;
    }
    let Some(method) = message.method.clone() else {
        return;
    };
    let Some(id) = message.id.clone() else {
        // The sidecar sends no notifications the app acts on.
        return;
    };
    let params = message.params.unwrap_or(Value::Null);
    let answer = if method == "extension.call" {
        Task::ready(extension_call(cx, params))
    } else {
        requests::handle(cx, &method, params)
    };
    cx.spawn(async move |cx: &mut AsyncApp| {
        let outcome = answer.await;
        cx.update(|cx| send(cx, Message::response(Some(id), outcome)));
    })
    .detach();
}

/// The sidecar's answer to `hello`: a refusal or another protocol version closes the
/// connection, stating why, and the app tries no more until it is enabled again.
fn welcomed(cx: &mut App, message: Message) {
    let refusal = match (message.error, message.result) {
        (Some(error), _) => Some(format!("the sidecar refused the app: {}", error.message)),
        (None, Some(result)) => match serde_json::from_value::<Welcome>(result) {
            Ok(welcome) if welcome.protocol_version == PROTOCOL_VERSION => None,
            Ok(welcome) => Some(format!(
                "the sidecar speaks devtools protocol {}, this app speaks {PROTOCOL_VERSION}",
                welcome.protocol_version
            )),
            Err(error) => Some(format!("the sidecar's welcome is not one: {error}")),
        },
        (None, None) => Some("the sidecar answered hello with nothing".into()),
    };
    if let Some(refusal) = refusal {
        log::warn!("devtools: {refusal}");
        let devtools = cx.global_mut::<Devtools>();
        devtools.refused = Some(refusal);
        devtools.transport.control(Control::Close);
        disconnect(cx);
    }
}

fn extension_call(cx: &mut App, params: Value) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("extension.call needs a `name`"))?
        .to_string();
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    let handler = cx
        .global::<Devtools>()
        .extensions
        .iter()
        .find(|extension| extension.name == name)
        .map(|extension| extension.handler.clone())
        .ok_or_else(|| {
            RpcError::new(
                protocol::codes::METHOD_NOT_FOUND,
                format!("no tool `{name}`"),
            )
        })?;
    handler(arguments, cx).map_err(|error| RpcError::failed(format!("{error:#}")))
}
