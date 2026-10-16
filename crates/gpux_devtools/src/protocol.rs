//! The devtools protocol: JSON-RPC 2.0 over WebSocket text frames, one connection, requests
//! flowing both ways. The sidecar (`gpux-devtools-mcp`) is the server and the app its client.
//!
//! The app opens with a `hello` request (its protocol version, id and name, platform, windows
//! and extension tools); the sidecar answers with its own protocol version, or an error when the
//! token is wrong. The app then notifies `windowOpened` and `windowClosed`, and answers the
//! sidecar's `snapshot`, `query`, `act`, `input`, `screenshot`, `wait`, `stats` and
//! `extension.call` requests. Every error carries its request's id; nothing is sent again after
//! a lost response.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The protocol version this crate speaks; `hello` carries it both ways.
pub const PROTOCOL_VERSION: u32 = 1;

/// The port the sidecar listens on by default, on loopback.
pub const DEFAULT_PORT: u16 = 9230;

/// The URL an app connects to when nothing names another: the sidecar's default, on loopback.
pub fn default_url() -> String {
    format!("ws://127.0.0.1:{DEFAULT_PORT}")
}

/// The id of the app's `hello` request.
pub const HELLO_ID: &str = "hello";

/// JSON-RPC error codes this protocol uses beyond JSON-RPC's own.
pub mod codes {
    /// The request's JSON is not a request.
    pub const PARSE_ERROR: i64 = -32700;
    /// The method is not one the app answers.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// The parameters do not fit the method.
    pub const INVALID_PARAMS: i64 = -32602;
    /// The request failed: no such window or ref, no accessibility tree, a timeout.
    pub const FAILED: i64 = -32000;
    /// The `hello` carried no token, or a wrong one.
    pub const TOKEN: i64 = -32001;
    /// The two sides speak different protocol versions.
    pub const VERSION: i64 = -32002;
}

/// A message on the connection: a request, a notification (a request without an id) or a
/// response.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// The request's id; `None` for a notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// The method of a request or notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// A request's or notification's parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// A response's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// A response's error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Message {
    /// A request.
    pub fn request(id: impl Into<Value>, method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id.into()),
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    /// A notification.
    pub fn notification(method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: None,
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    /// The response to the request `id`.
    pub fn response(id: Option<Value>, outcome: Result<Value, RpcError>) -> Self {
        let (result, error) = match outcome {
            Ok(result) => (Some(result), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            jsonrpc: "2.0".into(),
            // JSON-RPC answers a request whose id could not be read with a null id.
            id: Some(id.unwrap_or(Value::Null)),
            method: None,
            params: None,
            result,
            error,
        }
    }

    /// Whether this is a response rather than a request or notification.
    pub fn is_response(&self) -> bool {
        self.method.is_none()
    }

    /// The message as a text frame.
    pub fn to_text(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// A JSON-RPC error.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// A request that failed.
    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(codes::FAILED, message)
    }

    /// Parameters that do not fit.
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(codes::INVALID_PARAMS, message)
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

/// A window, as `hello` and `windowOpened` describe it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub id: u64,
    pub title: Option<String>,
    pub bounds: Rect,
    pub scale_factor: f32,
}

/// A rectangle in window pixels.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A tool an app adds to the sidecar's list (`extension.call` runs it).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// The app's `hello`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub protocol_version: u32,
    pub app_id: String,
    pub app_name: String,
    pub platform: String,
    pub windows: Vec<WindowInfo>,
    pub tools: Vec<ToolInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// The sidecar's answer to `hello`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Welcome {
    pub protocol_version: u32,
}

/// The platform this build runs on, as `hello` names it.
pub fn platform() -> &'static str {
    if cfg!(target_family = "wasm") {
        "web"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}
