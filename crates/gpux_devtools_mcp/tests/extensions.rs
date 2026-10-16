//! Extension tools: what apps advertise in `hello` joins the tool list and is forwarded to the
//! app; a name two apps offer needs an app id; a disconnected app (before or during a call) and
//! a failing tool fail clearly, naming the app and keeping the request id; an app tool sharing
//! the built-in tools' prefix is still forwarded; versions and tokens are checked at `hello`.

mod support;

use gpux_devtools_mcp::{PROTOCOL_VERSION, apps::Server};
use serde_json::{Value, json};
use std::time::Duration;
use support::{Client, HANG_UP, ScriptedApp};
use tungstenite::Message;

fn metadata_tool() -> Value {
    json!([{
        "name": "rayx_metadata",
        "description": "RayX's metadata",
        "inputSchema": { "type": "object", "properties": { "detail": { "type": "string" } } }
    }])
}

fn tool_names(client: &mut Client) -> Vec<String> {
    client.request("tools/list", json!({}))["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn advertised_tools_are_listed_and_forwarded() {
    let server = Server::start(0, None).unwrap();
    let mut client = Client::new(&server);
    assert!(!tool_names(&mut client).contains(&"rayx_metadata".to_string()));

    let app = ScriptedApp::connect(&server, "lab", metadata_tool());
    assert!(tool_names(&mut client).contains(&"rayx_metadata".to_string()));
    let result = client.call("rayx_metadata", json!({ "detail": "all" }));
    let answer: Value = serde_json::from_str(&Client::text(&result)).unwrap();
    assert_eq!(answer["called"], "rayx_metadata");
    assert_eq!(answer["arguments"]["detail"], "all");
    assert_eq!(app.last("extension.call")["name"], "rayx_metadata");

    app.disconnect();
    // Gone from the list once the app is.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while tool_names(&mut client).contains(&"rayx_metadata".to_string()) {
        assert!(
            std::time::Instant::now() < deadline,
            "the tool leaves with its app"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_name_two_apps_offer_needs_an_app_id() {
    let server = Server::start(0, None).unwrap();
    let lab = ScriptedApp::connect(&server, "lab", metadata_tool());
    let suite = ScriptedApp::connect(&server, "suite", metadata_tool());
    let mut client = Client::new(&server);
    let names = tool_names(&mut client);
    assert_eq!(
        names.iter().filter(|name| *name == "rayx_metadata").count(),
        1
    );

    let error = client.call_err("rayx_metadata", json!({}));
    assert!(
        error.contains("several apps offer rayx_metadata")
            && error.contains("lab")
            && error.contains("suite"),
        "{error}"
    );
    client.call("rayx_metadata", json!({ "app": "suite" }));
    assert_eq!(suite.methods(), ["extension.call"]);
    assert!(lab.methods().is_empty());
    // The `app` argument names the app; it is not forwarded.
    assert!(
        suite.last("extension.call")["arguments"]
            .get("app")
            .is_none()
    );
}

#[test]
fn a_disconnected_app_fails_naming_it() {
    let server = Server::start(0, None).unwrap();
    let events = server.registry.subscribe();
    let app = ScriptedApp::connect(&server, "lab", metadata_tool());
    let connection = server.registry.apps()[0].connection;
    app.disconnect();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if events
            .recv_timeout(Duration::from_millis(100))
            .ok()
            .as_deref()
            == Some("disconnected lab")
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
    }
    let mut client = Client::new(&server);
    let error = client.call_err("gpux_snapshot", json!({ "app": connection }));
    assert!(error.contains(&format!("#{connection}")), "{error}");
    let error = client.call_err("gpux_snapshot", json!({ "app": "lab" }));
    assert!(error.contains("lab is not connected"), "{error}");
}

#[test]
fn a_failing_tool_keeps_its_request_id() {
    let server = Server::start(0, None).unwrap();
    let _app = ScriptedApp::connect_with(&server, "lab", metadata_tool(), |method, _| {
        (method == "extension.call").then(|| Err("the metadata store is locked".to_string()))
    });
    let mut client = Client::new(&server);
    client.request("tools/list", json!({}));
    // `Client::request` asserts the answer carries the request's id.
    let response = client.request(
        "tools/call",
        json!({ "name": "rayx_metadata", "arguments": {} }),
    );
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("the metadata store is locked")
    );
}

fn hello(server: &Server, version: u64, token: Option<&str>) -> (Value, Option<String>) {
    let (mut socket, _) = tungstenite::connect(format!("ws://{}", server.address)).unwrap();
    let hello = json!({
        "jsonrpc": "2.0", "id": 7, "method": "hello",
        "params": { "protocolVersion": version, "token": token, "appId": "x", "platform": "test", "windows": [], "tools": [] }
    });
    socket.send(Message::text(hello.to_string())).unwrap();
    let mut reply = Value::Null;
    let mut close = None;
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => reply = serde_json::from_str(&text).unwrap(),
            Ok(Message::Close(frame)) => {
                close = frame.map(|frame| frame.reason.to_string());
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
        if reply.get("result").is_some() {
            break;
        }
    }
    (reply, close)
}

#[test]
fn hello_checks_the_version_and_the_token() {
    let server = Server::start(0, Some("secret".into())).unwrap();
    let (reply, close) = hello(&server, PROTOCOL_VERSION + 1, Some("secret"));
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["error"]["code"], gpux_devtools_mcp::codes::VERSION);
    assert!(close.unwrap().contains("protocol version mismatch"));

    let (reply, close) = hello(&server, PROTOCOL_VERSION, Some("wrong"));
    assert_eq!(reply["error"]["code"], gpux_devtools_mcp::codes::TOKEN);
    assert!(close.unwrap().contains("token"));

    let (reply, _) = hello(&server, PROTOCOL_VERSION, Some("secret"));
    assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION);
}

/// Waits until the registry reports `event`.
fn wait_for(events: &std::sync::mpsc::Receiver<String>, event: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while events
        .recv_timeout(Duration::from_millis(100))
        .ok()
        .as_deref()
        != Some(event)
    {
        assert!(std::time::Instant::now() < deadline, "no `{event}`");
    }
}

#[test]
fn a_tool_of_an_app_that_disconnected_names_the_app() {
    let server = Server::start(0, None).unwrap();
    let events = server.registry.subscribe();
    let app = ScriptedApp::connect(&server, "lab", metadata_tool());
    app.disconnect();
    wait_for(&events, "disconnected lab");
    let mut client = Client::new(&server);
    let error = client.call_err("rayx_metadata", json!({}));
    assert!(
        error.contains("lab") && error.contains("disconnected"),
        "{error}"
    );
    // A name no app ever offered stays unknown.
    let error = client.call_err("rayx_nothing", json!({}));
    assert!(error.contains("unknown tool"), "{error}");
}

#[test]
fn a_call_waiting_when_its_app_disconnects_fails_naming_it() {
    let server = Server::start(0, None).unwrap();
    let _app = ScriptedApp::connect_with(&server, "lab", metadata_tool(), |method, _| {
        (method == "extension.call").then(|| Err(HANG_UP.to_string()))
    });
    let mut client = Client::new(&server);
    let error = client.call_err("rayx_metadata", json!({}));
    assert!(
        error.contains("lab") && error.contains("disconnected"),
        "{error}"
    );
}

#[test]
fn an_app_tool_sharing_the_browser_prefix_is_forwarded() {
    let server = Server::start(0, None).unwrap();
    let app = ScriptedApp::connect(
        &server,
        "lab",
        json!([{ "name": "browser_lab_extra", "description": "An app's own", "inputSchema": { "type": "object" } }]),
    );
    let mut client = Client::new(&server);
    assert!(tool_names(&mut client).contains(&"browser_lab_extra".to_string()));
    let result = client.call("browser_lab_extra", json!({ "x": 1 }));
    assert!(
        Client::text(&result).contains("browser_lab_extra"),
        "{result}"
    );
    assert_eq!(app.last("extension.call")["name"], "browser_lab_extra");
}
