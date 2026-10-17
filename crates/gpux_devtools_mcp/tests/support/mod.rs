//! A scripted app: connects to the sidecar like `gpux-devtools` does and answers its requests
//! with canned results, recording what it was asked.

#![allow(dead_code)]

use gpux_devtools_mcp::{PROTOCOL_VERSION, apps::Server, mcp, tools::Session};
use serde_json::{Value, json};
use std::{
    io::ErrorKind,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tungstenite::Message;

pub const SNAPSHOT: &str =
    "- window \"Main\" [ref=0]:\n  - button \"Save\" [ref=5]\n  - textbox \"Name\" [ref=6]";

/// An answer that makes the scripted app hang up instead of answering.
pub const HANG_UP: &str = "__hang_up__";

/// What a scripted app was asked, in order: `(method, params)`.
pub type Requests = Arc<Mutex<Vec<(String, Value)>>>;

pub struct ScriptedApp {
    pub requests: Requests,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ScriptedApp {
    /// Connects as app `id` offering `tools`, and waits until the sidecar registered it.
    pub fn connect(server: &Server, id: &str, tools: Value) -> Self {
        Self::connect_with(server, id, tools, |_, _| None)
    }

    /// As [`Self::connect`], with `answer` overriding the canned result of a request (return
    /// `Some(Err(message))` to fail it, `Some(Err(HANG_UP.into()))` to disconnect instead).
    pub fn connect_with(
        server: &Server,
        id: &str,
        tools: Value,
        answer: impl Fn(&str, &Value) -> Option<Result<Value, String>> + Send + 'static,
    ) -> Self {
        let events = server.registry.subscribe();
        let url = format!("ws://{}", server.address);
        let (mut socket, _) = tungstenite::connect(url).expect("the sidecar accepts apps");
        let hello = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "hello",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "token": null,
                "appId": id,
                "appName": format!("{id} app"),
                "platform": "test",
                "windows": [ { "id": 1, "title": "Main", "bounds": { "x": 0, "y": 0, "width": 400, "height": 300 }, "scaleFactor": 2.0 } ],
                "tools": tools,
            }
        });
        socket.send(Message::text(hello.to_string())).unwrap();
        let reply = loop {
            if let Message::Text(text) = socket.read().unwrap() {
                break serde_json::from_str::<Value>(&text).unwrap();
            }
        };
        assert_eq!(reply["id"], 1, "{reply}");
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION);
        let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event, format!("connected {id}"));

        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_mut() {
            stream
                .set_read_timeout(Some(Duration::from_millis(10)))
                .unwrap();
        }
        let requests: Requests = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let requests = requests.clone();
            let stop = stop.clone();
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let message = match socket.read() {
                        Ok(Message::Text(text)) => serde_json::from_str::<Value>(&text).unwrap(),
                        Ok(Message::Close(_)) => return,
                        Ok(_) => continue,
                        Err(tungstenite::Error::Io(error))
                            if matches!(
                                error.kind(),
                                ErrorKind::WouldBlock | ErrorKind::TimedOut
                            ) =>
                        {
                            continue;
                        }
                        Err(_) => return,
                    };
                    let method = message["method"].as_str().unwrap_or_default().to_string();
                    let params = message["params"].clone();
                    requests
                        .lock()
                        .unwrap()
                        .push((method.clone(), params.clone()));
                    let result =
                        answer(&method, &params).unwrap_or_else(|| Ok(canned(&method, &params)));
                    if result.as_ref().err().map(String::as_str) == Some(HANG_UP) {
                        let _ = socket.close(None);
                        let _ = socket.flush();
                        return;
                    }
                    let reply = match result {
                        Ok(result) => {
                            json!({ "jsonrpc": "2.0", "id": message["id"], "result": result })
                        }
                        Err(error) => {
                            json!({ "jsonrpc": "2.0", "id": message["id"], "error": { "code": -32000, "message": error } })
                        }
                    };
                    socket.send(Message::text(reply.to_string())).unwrap();
                }
                let _ = socket.close(None);
                let _ = socket.flush();
            })
        };
        Self {
            requests,
            stop,
            thread: Some(thread),
        }
    }

    /// The methods it was asked, in order.
    pub fn methods(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _)| m.clone())
            .collect()
    }

    /// The params of the last request for `method`.
    pub fn last(&self, method: &str) -> Value {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(m, _)| m == method)
            .map(|(_, params)| params.clone())
            .unwrap_or_else(|| panic!("the app was never asked {method}"))
    }

    /// Disconnects and waits until it is gone.
    pub fn disconnect(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for ScriptedApp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn canned(method: &str, params: &Value) -> Value {
    match method {
        "snapshot" => json!({ "window": 1, "title": "Main", "snapshot": SNAPSHOT }),
        "query" => json!({ "nodes": [ { "ref": "5", "role": "button", "name": "Save" } ] }),
        "act" => json!({ "handled": true }),
        "input" => json!({ "performed": true }),
        // A 1×1 PNG's first bytes are enough for the sidecar, which passes the data through.
        "screenshot" => json!({ "png": "iVBORw0KGgo=", "width": 1, "height": 1 }),
        "wait" => json!({ "matched": true, "elapsedMs": 3 }),
        "stats" => json!({ "frames": 10, "nodes": 3 }),
        "extension.call" => json!({ "called": params["name"], "arguments": params["arguments"] }),
        _ => json!(null),
    }
}

/// An MCP client in process.
pub struct Client<'a> {
    server: &'a Server,
    session: Session,
    next_id: u64,
}

impl<'a> Client<'a> {
    pub fn new(server: &'a Server) -> Self {
        Self {
            server,
            session: Session::default(),
            next_id: 1,
        }
    }

    /// Sends a request and returns the whole response.
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line =
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        let response = mcp::handle_line(&self.server.registry, &mut self.session, &line)
            .expect("a request is answered");
        assert_eq!(
            response["id"], id,
            "the answer keeps the request id: {response}"
        );
        response
    }

    /// Calls a tool and returns its result, panicking on an error.
    pub fn call(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        assert!(response.get("error").is_none(), "{name} failed: {response}");
        response["result"].clone()
    }

    /// Calls a tool and returns its error message, panicking when it succeeds.
    pub fn call_err(&mut self, name: &str, arguments: Value) -> String {
        let response = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        response["error"]["message"]
            .as_str()
            .unwrap_or_else(|| panic!("{name} succeeded: {response}"))
            .to_string()
    }

    /// The text of a tool result's first content item.
    pub fn text(result: &Value) -> String {
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}
