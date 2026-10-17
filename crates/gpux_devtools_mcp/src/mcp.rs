//! The MCP side: JSON-RPC 2.0 over stdio, one message per line.

use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{BufRead, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::RecvTimeoutError,
    },
    time::Duration,
};

use crate::{SERVER_NAME, SERVER_VERSION, apps::Registry, tools};

/// The MCP protocol version the sidecar answers `initialize` with.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Serves MCP on `input` and `output` until `input` ends. Whenever the extension tools the
/// connected apps offer change, it tells the harness with `notifications/tools/list_changed`,
/// so tools of apps that connect after the harness listed them reach it.
pub fn serve(
    registry: &Registry,
    input: impl BufRead,
    output: impl Write + Send + 'static,
) -> std::io::Result<()> {
    let output = Arc::new(Mutex::new(output));
    let done = Arc::new(AtomicBool::new(false));
    let watcher = {
        let events = registry.subscribe();
        let registry = registry.clone();
        let output = Arc::clone(&output);
        let done = Arc::clone(&done);
        std::thread::Builder::new()
            .name("gpux-devtools-mcp-tools".into())
            .spawn(move || {
                let mut offered = extension_names(&registry);
                while !done.load(Ordering::SeqCst) {
                    match events.recv_timeout(Duration::from_millis(100)) {
                        Ok(_) => {
                            let now = extension_names(&registry);
                            if now != offered {
                                offered = now;
                                let notification = json!({
                                    "jsonrpc": "2.0",
                                    "method": "notifications/tools/list_changed",
                                });
                                if write_line(&output, &notification).is_err() {
                                    break;
                                }
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })?
    };
    let mut session = tools::Session::default();
    let mut result = Ok(());
    for line in input.lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                result = Err(error);
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(registry, &mut session, &line)
            && let Err(error) = write_line(&output, &response)
        {
            result = Err(error);
            break;
        }
    }
    done.store(true, Ordering::SeqCst);
    watcher.join().ok();
    result
}

/// The names of the extension tools the connected apps offer.
fn extension_names(registry: &Registry) -> BTreeSet<String> {
    registry
        .apps()
        .iter()
        .flat_map(|app| app.tools.iter().map(|tool| tool.name.clone()))
        .collect()
}

/// Writes one message and a line break, as one write the watcher cannot interleave.
fn write_line(output: &Mutex<impl Write>, message: &Value) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    let mut output = output
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    output.write_all(&line)?;
    output.flush()
}

/// Answers one MCP message; `None` for notifications.
pub fn handle_line(registry: &Registry, session: &mut tools::Session, line: &str) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(message) => message,
        Err(error) => {
            return Some(error_response(
                Value::Null,
                -32700,
                format!("parse error: {error}"),
            ));
        }
    };
    // A request's id is kept on every answer, failures included.
    let id = message.get("id").cloned()?;
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Some(error_response(
            id,
            -32600,
            "a request needs a method".into(),
        ));
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": true } },
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::list(registry) })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            tools::call(registry, session, name, arguments).map_err(|message| (-32000, message))
        }
        other => Err((-32601, format!("method not found: {other}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => error_response(id, code, message),
    })
}

fn error_response(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// What `--print-manifest` prints: the sidecar, its options and its tools.
pub fn manifest() -> Value {
    json!({
        "name": SERVER_NAME,
        "version": SERVER_VERSION,
        "command": "gpux-devtools-mcp",
        "transport": "stdio",
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "apps": {
            "transport": "websocket",
            "listen": format!("ws://127.0.0.1:{}", crate::DEFAULT_PORT),
            "protocolVersion": crate::PROTOCOL_VERSION,
            "auth": { "default": "none", "argument": "--token", "environment": "GPUX_DEVTOOLS_TOKEN" },
        },
        "arguments": [
            { "name": "--port", "value": "port", "default": crate::DEFAULT_PORT },
            { "name": "--token", "value": "token" },
            { "name": "--print-manifest" },
        ],
        "tools": tools::builtin()
            .iter()
            .filter_map(|tool| tool.get("name").cloned())
            .collect::<Vec<_>>(),
    })
}
