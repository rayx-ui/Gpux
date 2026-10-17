//! The sidecar's MCP side against a scripted app: initialize and the tool list, every `gpux_*`
//! and `browser_*` tool, Playwright's response sections, and `--print-manifest`; and the binary
//! over a real pipe, telling the harness when apps' tools come and go.

mod support;

use gpux_devtools_mcp::{Options, apps::Server};
use serde_json::{Value, json};
use support::{Client, SNAPSHOT, ScriptedApp};

fn server() -> Server {
    Server::start(0, None).expect("the sidecar listens on loopback")
}

#[test]
fn initialize_names_the_sidecar_which_listens_on_loopback_only() {
    let server = server();
    assert!(
        server.address.ip().is_loopback(),
        "the sidecar binds loopback only"
    );
    let mut client = Client::new(&server);
    let init = client.request("initialize", json!({}));
    assert_eq!(init["result"]["serverInfo"]["name"], "gpux-devtools-mcp");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    let unknown = client.request("nope", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
}

#[test]
fn the_tool_list_holds_the_gpux_and_playwright_tools() {
    let server = server();
    let mut client = Client::new(&server);
    let tools = client.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for name in [
        "gpux_list_apps",
        "gpux_snapshot",
        "gpux_query",
        "gpux_act",
        "gpux_input",
        "gpux_screenshot",
        "gpux_wait",
        "gpux_stats",
        "browser_snapshot",
        "browser_click",
        "browser_hover",
        "browser_drag",
        "browser_type",
        "browser_press_key",
        "browser_mouse_move_xy",
        "browser_mouse_down",
        "browser_mouse_up",
        "browser_mouse_wheel",
        "browser_mouse_click_xy",
        "browser_mouse_drag_xy",
        "browser_take_screenshot",
        "browser_wait_for",
        "browser_tabs",
        "browser_observe",
    ] {
        assert!(names.contains(&name), "{name} is listed");
    }
    assert!(
        names.contains(&"browser_set_property"),
        "RayX DevKit's tool, as SetValue"
    );
}

#[test]
fn gpux_list_apps_reports_the_apps_and_their_windows() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let mut client = Client::new(&server);

    let apps: Value =
        serde_json::from_str(&Client::text(&client.call("gpux_list_apps", json!({})))).unwrap();
    assert_eq!(apps[0]["id"], "demo");
    assert_eq!(apps[0]["windows"][0]["title"], "Main");

    app.disconnect();
}

#[test]
fn gpux_snapshot_and_query_pass_their_arguments() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let mut client = Client::new(&server);
    let snapshot = client.call(
        "gpux_snapshot",
        json!({ "view": "screen-reader", "bounds": true }),
    );
    assert!(Client::text(&snapshot).contains("button \\\"Save\\\""));
    assert_eq!(app.last("snapshot")["view"], "screen-reader");
    assert_eq!(app.last("snapshot")["bounds"], true);

    client.call("gpux_query", json!({ "query": "role=button", "window": 1 }));
    assert_eq!(app.last("query")["query"], "role=button");
    assert_eq!(app.last("query")["window"], 1);
    app.disconnect();
}

#[test]
fn gpux_act_and_input_reach_the_app() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let mut client = Client::new(&server);

    let acted = client.call(
        "gpux_act",
        json!({ "ref": "5", "action": "setValue", "value": 3 }),
    );
    assert!(Client::text(&acted).contains("\"handled\": true"));
    assert_eq!(app.last("act")["value"], 3);

    client.call(
        "gpux_input",
        json!({ "kind": "click", "ref": "5", "count": 2 }),
    );
    assert_eq!(app.last("input")["count"], 2);
    app.disconnect();
}

#[test]
fn gpux_screenshot_wait_and_stats() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let mut client = Client::new(&server);
    let screenshot = client.call("gpux_screenshot", json!({ "ref": "5" }));
    assert_eq!(screenshot["content"][1]["type"], "image");
    assert_eq!(screenshot["content"][1]["mimeType"], "image/png");

    client.call(
        "gpux_wait",
        json!({ "query": "text=Saved", "timeoutMs": 500 }),
    );
    assert_eq!(app.last("wait")["timeoutMs"], 500);
    client.call("gpux_stats", json!({}));
    assert_eq!(app.methods(), ["screenshot", "wait", "stats"]);
    app.disconnect();
}

#[test]
fn browser_tools_answer_in_playwright_sections() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let mut client = Client::new(&server);

    let snapshot = Client::text(&client.call("browser_snapshot", json!({})));
    assert!(snapshot.starts_with("### Page\n- Page URL: gpux://demo/window/1\n- Page Title: Main"));
    assert!(snapshot.contains(&format!("### Snapshot\n```yaml\n{SNAPSHOT}\n```")));
    app.disconnect();
}

#[test]
fn browser_pointer_tools_run_playwright_code_and_answer_with_a_snapshot() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let mut client = Client::new(&server);

    let clicked = Client::text(&client.call(
        "browser_click",
        json!({ "target": "5", "doubleClick": true }),
    ));
    assert!(clicked.starts_with("### Result\n"));
    assert!(clicked.contains(
        "### Ran Playwright code\n```js\nawait page.locator('aria-ref=5').dblclick();\n```"
    ));
    assert!(clicked.contains("### Page") && clicked.contains("### Snapshot"));
    assert_eq!(app.last("input")["count"], 2);

    client.call("browser_hover", json!({ "target": "5" }));
    assert_eq!(app.last("input")["kind"], "hover");
    client.call(
        "browser_drag",
        json!({ "startTarget": "5", "endTarget": "6" }),
    );
    assert_eq!(app.last("input")["toRef"], "6");
    app.disconnect();
}

#[test]
fn browser_type_and_press_key_translate_to_gpux_keys() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let mut client = Client::new(&server);
    let typed = Client::text(&client.call(
        "browser_type",
        json!({ "target": "6", "text": "Ada", "submit": true }),
    ));
    assert!(typed.contains(".fill(\"Ada\")") && typed.contains(".press('Enter')"));
    client.call("browser_press_key", json!({ "key": "Control+A" }));
    assert_eq!(app.last("input")["keys"], "ctrl-a");
    app.disconnect();
}

#[test]
fn browser_mouse_tools_keep_the_pointer_between_calls() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let mut client = Client::new(&server);

    client.call("browser_mouse_move_xy", json!({ "x": 10, "y": 20 }));
    client.call("browser_mouse_down", json!({}));
    assert_eq!(app.last("input")["kind"], "mouseDown");
    assert_eq!(app.last("input")["x"], 10.0);
    client.call("browser_mouse_up", json!({ "button": "right" }));
    assert_eq!(app.last("input")["button"], "right");
    client.call("browser_mouse_wheel", json!({ "deltaY": 120 }));
    assert_eq!(app.last("input")["deltaY"], 120.0);
    client.call(
        "browser_mouse_click_xy",
        json!({ "x": 1, "y": 2, "clickCount": 2 }),
    );
    assert_eq!(app.last("input")["count"], 2);
    client.call(
        "browser_mouse_drag_xy",
        json!({ "startX": 1, "startY": 2, "endX": 3, "endY": 4 }),
    );
    assert_eq!(app.last("input")["toY"], 4.0);
    app.disconnect();
}

#[test]
fn browser_screenshot_wait_tabs_and_observe() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let mut client = Client::new(&server);

    let shot = client.call("browser_take_screenshot", json!({}));
    assert_eq!(
        shot["content"][0]["text"],
        "### Result\nScreenshot captured"
    );
    assert!(
        client
            .call_err("browser_take_screenshot", json!({ "type": "jpeg" }))
            .contains("PNG")
    );

    let waited = Client::text(&client.call("browser_wait_for", json!({ "text": "Save" })));
    assert!(waited.starts_with("### Result\nWaited for Save"));
    assert!(
        client
            .call_err("browser_wait_for", json!({}))
            .contains("Either time")
    );

    let tabs = Client::text(&client.call("browser_tabs", json!({ "action": "list" })));
    assert_eq!(
        tabs,
        "### Result\n- 0: (current) [Main](gpux://demo/window/1)"
    );
    let observed: Value =
        serde_json::from_str(&Client::text(&client.call("browser_observe", json!({})))).unwrap();
    assert_eq!(observed["title"], "Main");
    app.disconnect();
}

#[test]
fn tools_fail_clearly_without_an_app() {
    let server = server();
    let mut client = Client::new(&server);
    assert!(
        client
            .call_err("browser_snapshot", json!({}))
            .contains("no gpux app is connected")
    );
    assert!(
        client
            .call_err("gpux_snapshot", json!({ "app": "ghost" }))
            .contains("ghost")
    );
    assert!(
        client
            .call_err("made_up", json!({}))
            .contains("unknown tool")
    );
}

#[test]
fn options_come_from_arguments_and_the_environment() {
    let options = Options::parse(
        ["--port", "9999", "--token=secret"].map(String::from),
        Some("ignored".into()),
    )
    .unwrap();
    assert_eq!(options.port, 9999);
    assert_eq!(options.token.as_deref(), Some("secret"));
    let options = Options::parse(Vec::<String>::new(), Some("env".into())).unwrap();
    assert_eq!(options.port, gpux_devtools_mcp::DEFAULT_PORT);
    assert_eq!(options.token.as_deref(), Some("env"));
    assert!(Options::parse(["--bogus".to_string()], None).is_err());
}

/// The manifest is what the binary prints, so this test runs it.
#[test]
#[allow(clippy::disallowed_methods)]
fn print_manifest_lists_the_tools_and_options() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_gpux-devtools-mcp"))
        .arg("--print-manifest")
        .output()
        .unwrap();
    assert!(output.status.success());
    let manifest: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(manifest["command"], "gpux-devtools-mcp");
    assert_eq!(manifest["transport"], "stdio");
    assert!(
        manifest["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "gpux_snapshot")
    );
    assert!(
        manifest["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["name"] == "--token")
    );
}

#[test]
fn the_probe_snapshots_and_taps_a_named_button() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let probed = gpux_devtools_mcp::probe::tap_button(
        &server.registry,
        "Save",
        std::time::Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(probed.reference, "5");
    assert_eq!(app.last("input")["kind"], "click");
    assert_eq!(app.last("input")["ref"], "5");
    assert_eq!(app.last("input")["window"], 1);
    app.disconnect();
}

#[test]
fn browser_set_property_sets_the_value_through_set_value() {
    let server = server();
    let app = ScriptedApp::connect(&server, "demo", json!([]));
    let mut client = Client::new(&server);
    let result = client.call(
        "browser_set_property",
        json!({ "target": "6", "property": "value", "value": "Ada" }),
    );
    let act = app.last("act");
    assert_eq!(act["ref"], "6");
    assert_eq!(act["action"], "setValue");
    assert_eq!(act["value"], "Ada");
    let text = Client::text(&result);
    for section in [
        "### Result",
        "### Ran Playwright code",
        "### Page",
        "### Snapshot",
    ] {
        assert!(text.contains(section), "{section} in {text}");
    }
    let numeric = client.call(
        "browser_set_property",
        json!({ "target": "7", "value": 0.5 }),
    );
    assert!(Client::text(&numeric).contains("Set 7 to 0.5"), "{numeric}");
    assert_eq!(app.last("act")["value"], 0.5);
}

/// The binary on a real pipe: MCP on its stdin and stdout, apps on its port.
struct Binary {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    lines: std::sync::mpsc::Receiver<Value>,
    port: u16,
}

impl Binary {
    // A test drives the real binary on its pipes; nothing async runs here.
    #[allow(clippy::disallowed_methods)]
    fn start() -> Self {
        use std::io::BufRead as _;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_gpux-devtools-mcp"))
            .args(["--port", &port.to_string()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                if sender.send(serde_json::from_str(&line).unwrap()).is_err() {
                    return;
                }
            }
        });
        // The binary says on stderr when it listens.
        let mut stderr = std::io::BufReader::new(child.stderr.take().unwrap());
        let mut line = String::new();
        stderr.read_line(&mut line).unwrap();
        assert!(line.contains("waiting for gpux apps"), "{line}");
        Self {
            child,
            stdin,
            lines,
            port,
        }
    }

    fn send(&mut self, message: Value) {
        use std::io::Write as _;
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn next(&self, what: &str) -> Value {
        self.lines
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("no {what} on stdout"))
    }

    /// Connects an app offering `tools`, as gpux-devtools does.
    fn app(
        &self,
        id: &str,
        tools: Value,
    ) -> tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>> {
        let (mut socket, _) =
            tungstenite::connect(format!("ws://127.0.0.1:{}", self.port)).unwrap();
        let hello = json!({
            "jsonrpc": "2.0", "id": 1, "method": "hello",
            "params": { "protocolVersion": gpux_devtools_mcp::PROTOCOL_VERSION, "token": null,
                        "appId": id, "appName": id, "platform": "test", "windows": [], "tools": tools }
        });
        socket
            .send(tungstenite::Message::text(hello.to_string()))
            .unwrap();
        loop {
            if let tungstenite::Message::Text(text) = socket.read().unwrap() {
                let reply: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(reply["id"], 1, "{reply}");
                return socket;
            }
        }
    }
}

impl Drop for Binary {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn the_binary_serves_stdio_and_tells_the_harness_when_app_tools_change() {
    let mut binary = Binary::start();
    binary.send(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }));
    let init = binary.next("the initialize answer");
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["capabilities"]["tools"]["listChanged"], true);
    let list = |binary: &mut Binary, id: u64| -> Vec<String> {
        binary.send(json!({ "jsonrpc": "2.0", "id": id, "method": "tools/list" }));
        let answer = binary.next("the tool list");
        assert_eq!(answer["id"], id, "{answer}");
        answer["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert!(!list(&mut binary, 2).contains(&"rayx_metadata".to_string()));

    // An app connects after the harness listed the tools: the harness hears of it.
    let mut socket = binary.app(
        "lab",
        json!([{ "name": "rayx_metadata", "description": "metadata", "inputSchema": { "type": "object" } }]),
    );
    let changed = binary.next("a list_changed notification");
    assert_eq!(
        changed["method"], "notifications/tools/list_changed",
        "{changed}"
    );
    assert!(changed.get("id").is_none(), "a notification has no id");
    assert!(list(&mut binary, 3).contains(&"rayx_metadata".to_string()));

    // An app without tools changes nothing; the app leaving changes the list again.
    let _quiet = binary.app("quiet", json!([]));
    socket.close(None).ok();
    socket.flush().ok();
    let changed = binary.next("a list_changed notification");
    assert_eq!(
        changed["method"], "notifications/tools/list_changed",
        "{changed}"
    );
    assert!(!list(&mut binary, 4).contains(&"rayx_metadata".to_string()));
}
