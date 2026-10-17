//! `gpux-devtools-mcp`: one MCP sidecar for every gpux app.
//!
//! Apps that link `gpux-devtools` connect out to this sidecar over a WebSocket on loopback
//! ([`apps`]), and an MCP client (an agent harness) talks to it over stdio ([`mcp`]). The
//! sidecar turns MCP tool calls ([`tools`]) into requests to the connected apps, which answer
//! from their AccessKit trees.
//!
//! The protocol between the two is JSON-RPC 2.0 over WebSocket text frames, with requests in
//! both directions on one connection:
//!
//! - the app opens with `hello` (protocol version, optional token, app id and name, platform,
//!   windows and the extension tools it offers) and notifies `windowOpened` and `windowClosed`;
//! - the sidecar sends `snapshot`, `query`, `act`, `input`, `screenshot`, `wait`, `stats` and
//!   `extension.call`.
//!
//! A version mismatch or a wrong token closes the connection with a stated error. Errors keep
//! their request ids, and nothing is sent again after a lost response.

pub mod apps;
pub mod mcp;
pub mod probe;
pub mod tools;

/// The version of the protocol between `gpux-devtools` and this sidecar.
pub const PROTOCOL_VERSION: u64 = 1;

/// The protocol's JSON-RPC error codes beyond JSON-RPC's own (as `gpux_devtools::protocol::codes`).
pub mod codes {
    /// The `hello` carried no token, or a wrong one.
    pub const TOKEN: i64 = -32001;
    /// The two sides speak different protocol versions.
    pub const VERSION: i64 = -32002;
}

/// The loopback port the sidecar listens on unless `--port` names another; `gpux-devtools`
/// connects to `ws://127.0.0.1:<this>` by default.
pub const DEFAULT_PORT: u16 = 9230;

/// The sidecar's name in MCP `serverInfo`.
pub const SERVER_NAME: &str = "gpux-devtools-mcp";

/// The sidecar's version.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The sidecar's command-line options.
#[derive(Clone, Debug)]
pub struct Options {
    /// The loopback port to listen on (`--port`); 0 picks a free one.
    pub port: u16,
    /// The token apps must present in `hello` (`--token`, or `GPUX_DEVTOOLS_TOKEN`).
    pub token: Option<String>,
    /// Print the tools and options and exit (`--print-manifest`).
    pub print_manifest: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            token: None,
            print_manifest: false,
        }
    }
}

impl Options {
    /// Parses the options from `args` (without the program name); `token_env` is the value of
    /// `GPUX_DEVTOOLS_TOKEN`, which `--token` overrides.
    pub fn parse(
        args: impl IntoIterator<Item = String>,
        token_env: Option<String>,
    ) -> Result<Self, String> {
        let mut options = Options {
            token: token_env.filter(|token| !token.is_empty()),
            ..Options::default()
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let (name, inline) = match arg.split_once('=') {
                Some((name, value)) if name.starts_with("--") => {
                    (name.to_string(), Some(value.to_string()))
                }
                _ => (arg.clone(), None),
            };
            let mut value = |name: &str| {
                inline
                    .clone()
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{name} needs a value"))
            };
            match name.as_str() {
                "--port" => {
                    let port = value("--port")?;
                    options.port = port
                        .parse()
                        .map_err(|_| format!("--port {port:?} is not a port number"))?;
                }
                "--token" => options.token = Some(value("--token")?),
                "--print-manifest" => options.print_manifest = true,
                "--help" | "-h" => return Err(usage()),
                other => return Err(format!("unknown option {other:?}\n{}", usage())),
            }
        }
        Ok(options)
    }
}

/// The usage text.
pub fn usage() -> String {
    format!(
        "Usage: gpux-devtools-mcp [--port PORT] [--token TOKEN] [--print-manifest]\n\
         \n\
         Serves MCP over stdio and waits for gpux apps that link gpux-devtools to connect to\n\
         ws://127.0.0.1:PORT (default {DEFAULT_PORT}). GPUX_DEVTOOLS_TOKEN sets the token."
    )
}
