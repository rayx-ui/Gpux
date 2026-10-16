//! The MCP side: JSON-RPC 2.0 over stdio, one message per line.

use serde_json::{Value, json};
use std::io::{BufRead, Write};

use crate::{SERVER_NAME, SERVER_VERSION, apps::Registry, tools};

/// The MCP protocol version the sidecar answers `initialize` with.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Serves MCP on `input` and `output` until `input` ends.
pub fn serve(
    registry: &Registry,
    input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    let mut session = tools::Session::default();
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(registry, &mut session, &line) {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
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
