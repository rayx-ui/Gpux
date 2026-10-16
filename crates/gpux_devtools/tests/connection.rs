//! The connection to the sidecar against an in-test WebSocket server: URL precedence, the off
//! switch, backoff while nothing listens, `hello`, version and token refusals, window
//! notifications, inspection guards held only while connected, errors keeping request ids, and
//! `set_enabled` at runtime.
//!
//! `cargo test -p gpux-devtools --test connection`

mod support;

use std::time::Duration;

use gpux::TestAppContext;
use gpux_devtools::{Options, protocol};
use serde_json::json;
use support::*;

fn options(server: &Server) -> Options {
    Options::new("dev.gpux.test", "Devtools test")
        .url(server.url())
        .backoff(Duration::from_millis(20), Duration::from_millis(80))
}

fn app() -> TestAppContext {
    TestAppContext::single()
}

#[test]
fn the_url_comes_from_the_options_then_the_environment_then_the_default() {
    let explicit = Options::new("a", "A").url("ws://127.0.0.1:1234");
    assert_eq!(explicit.resolved_url(), "ws://127.0.0.1:1234");
    if std::env::var(gpux_devtools::URL_ENV).is_err() {
        assert_eq!(
            Options::new("a", "A").resolved_url(),
            protocol::default_url()
        );
    }
    assert_eq!(
        protocol::default_url(),
        format!("ws://127.0.0.1:{}", protocol::DEFAULT_PORT)
    );
    // The variable, in a process of its own.
    run_child("url", &[(gpux_devtools::URL_ENV, "ws://127.0.0.1:4321")]);
}

#[test]
fn the_switch_comes_from_the_options_then_the_environment() {
    assert!(!Options::new("a", "A").enabled(false).resolved_enabled());
    assert!(Options::new("a", "A").enabled(true).resolved_enabled());
    run_child("off", &[(gpux_devtools::ENABLED_ENV, "off")]);
}

/// Checks run in a child process with the environment `run_child` gave it.
#[test]
fn child_checks_the_environment() {
    match std::env::var("GPUX_DEVTOOLS_TEST_CHILD").as_deref() {
        Ok("url") => assert_eq!(Options::new("a", "A").resolved_url(), "ws://127.0.0.1:4321"),
        Ok("off") => {
            assert!(!Options::new("a", "A").resolved_enabled());
            // The options win over the variable.
            assert!(Options::new("a", "A").enabled(true).resolved_enabled());
        }
        _ => return,
    }
    println!("child checked");
}

// A test may block on its child process: the environment cannot be set for one test alone.
#[allow(clippy::disallowed_methods)]
fn run_child(check: &str, env: &[(&str, &str)]) {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "child_checks_the_environment", "--nocapture"])
        .env("GPUX_DEVTOOLS_TEST_CHILD", check);
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("child checked"),
        "{check}: {stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn an_app_started_off_never_connects() {
    let server = Server::bind();
    let mut cx = app();
    cx.update(|cx| gpux_devtools::install(cx, options(&server).enabled(false)));
    for _ in 0..20 {
        pump(&mut cx);
        assert!(
            server.try_accept().is_none(),
            "an app switched off connects to nothing"
        );
    }
    assert!(!cx.update(|cx| gpux_devtools::is_enabled(cx)));
}

#[test]
fn the_app_keeps_trying_until_a_sidecar_listens() {
    // Nothing listens on the port yet: the app retries with backoff.
    let port = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    let mut cx = app();
    cx.update(|cx| {
        gpux_devtools::install(
            cx,
            Options::new("dev.gpux.test", "Devtools test")
                .url(format!("ws://127.0.0.1:{port}"))
                .backoff(Duration::from_millis(20), Duration::from_millis(80)),
        )
    });
    for _ in 0..10 {
        pump(&mut cx);
    }
    assert!(!cx.update(|cx| gpux_devtools::is_connected(cx)));
    // The sidecar comes up later.
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = support::Server::from_listener(listener);
    let mut connection = server.accept(&mut cx);
    connection.handshake(&mut cx);
    until(&mut cx, "the app to be connected", |cx| {
        cx.update(|cx| gpux_devtools::is_connected(cx))
            .then_some(())
    });
}

#[test]
fn hello_describes_the_app_its_windows_and_its_tools() {
    let server = Server::bind();
    let mut cx = app();
    cx.add_window(|_, _| Panel { clicks: 0 });
    cx.update(|cx| {
        gpux_devtools::install(
            cx,
            options(&server).extension(gpux_devtools::Extension::new(
                "app_metadata",
                "The app's metadata.",
                json!({"type": "object"}),
                |_, _| Ok(json!({"name": "test"})),
            )),
        )
    });
    let mut connection = server.accept(&mut cx);
    let hello = connection.read(&mut cx);
    assert_eq!(hello["jsonrpc"], "2.0");
    assert_eq!(hello["method"], "hello");
    let params = &hello["params"];
    assert_eq!(params["protocolVersion"], protocol::PROTOCOL_VERSION);
    assert_eq!(params["appId"], "dev.gpux.test");
    assert_eq!(params["appName"], "Devtools test");
    assert_eq!(params["platform"], protocol::platform());
    let windows = params["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 1);
    assert!(windows[0]["id"].as_u64().is_some());
    assert!(windows[0]["bounds"]["width"].as_f64().unwrap() > 0.);
    assert!(windows[0]["scaleFactor"].as_f64().unwrap() > 0.);
    assert_eq!(params["tools"][0]["name"], "app_metadata");
    assert!(params.get("token").is_none(), "no token was set");
}

#[test]
fn a_sidecar_speaking_another_version_is_refused_and_not_retried() {
    let server = Server::bind();
    let mut cx = app();
    cx.update(|cx| gpux_devtools::install(cx, options(&server)));
    let mut connection = server.accept(&mut cx);
    let hello = connection.read(&mut cx);
    connection
        .send(json!({"jsonrpc": "2.0", "id": hello["id"], "result": {"protocolVersion": 99}}));
    connection.expect_closed(&mut cx);
    let refusal = cx.update(|cx| gpux_devtools::refusal(cx)).unwrap();
    assert!(refusal.contains("99") && refusal.contains('1'), "{refusal}");
    for _ in 0..20 {
        pump(&mut cx);
        assert!(
            server.try_accept().is_none(),
            "a refused app does not come back by itself"
        );
    }
}

#[test]
fn a_wrong_token_is_refused_with_the_sidecar_s_reason() {
    let server = Server::bind();
    let mut cx = app();
    cx.update(|cx| gpux_devtools::install(cx, options(&server).token("secret")));
    let mut connection = server.accept(&mut cx);
    let hello = connection.read(&mut cx);
    assert_eq!(hello["params"]["token"], "secret");
    connection.send(json!({
        "jsonrpc": "2.0",
        "id": hello["id"],
        "error": {"code": protocol::codes::TOKEN, "message": "the token does not match"},
    }));
    connection.expect_closed(&mut cx);
    let refusal = cx.update(|cx| gpux_devtools::refusal(cx)).unwrap();
    assert!(refusal.contains("the token does not match"), "{refusal}");
}

#[test]
fn windows_opening_and_closing_are_announced() {
    let server = Server::bind();
    let mut cx = app();
    cx.update(|cx| gpux_devtools::install(cx, options(&server)));
    let mut connection = server.accept(&mut cx);
    connection.handshake(&mut cx);
    let window = cx.add_window(|_, _| Panel { clicks: 0 });
    let opened = connection.read(&mut cx);
    assert_eq!(opened["method"], "windowOpened");
    assert!(opened.get("id").is_none(), "a notification");
    let id = opened["params"]["id"].as_u64().unwrap();
    assert_eq!(id, window.window_id().as_u64());
    cx.update(|cx| {
        window
            .update(cx, |_, window, _| window.remove_window())
            .ok()
    });
    let closed = connection.read(&mut cx);
    assert_eq!(closed["method"], "windowClosed");
    assert_eq!(closed["params"]["id"], id);
}

#[test]
fn windows_hold_inspection_guards_only_while_connected() {
    let server = Server::bind();
    let mut cx = app();
    let window = cx.add_window(|_, _| Panel { clicks: 0 });
    cx.update(|cx| gpux_devtools::install(cx, options(&server)));
    assert_eq!(cx.update(|cx| gpux_devtools::guarded_windows(cx)), 0);
    let mut connection = server.accept(&mut cx);
    connection.handshake(&mut cx);
    until(&mut cx, "a guard", |cx| {
        (cx.update(|cx| gpux_devtools::guarded_windows(cx)) == 1).then_some(())
    });
    pump(&mut cx);
    assert!(
        cx.update(|cx| window.update(cx, |_, window, _| window.a11y_tree().is_some()))
            .unwrap(),
        "the guard builds the window's tree"
    );
    connection.close();
    until(&mut cx, "the guards to go", |cx| {
        (cx.update(|cx| gpux_devtools::guarded_windows(cx)) == 0).then_some(())
    });
    pump(&mut cx);
    assert!(
        cx.update(|cx| window.update(cx, |_, window, _| window.a11y_tree().is_none()))
            .unwrap(),
        "no tree once disconnected"
    );
}

#[test]
fn errors_keep_their_request_ids() {
    let server = Server::bind();
    let mut cx = app();
    cx.add_window(|_, _| Panel { clicks: 0 });
    cx.update(|cx| gpux_devtools::install(cx, options(&server)));
    let mut connection = server.accept(&mut cx);
    connection.handshake(&mut cx);
    let unknown = connection.request(&mut cx, 7, "no.such.method", json!({}));
    assert_eq!(unknown["error"]["code"], protocol::codes::METHOD_NOT_FOUND);
    let bad = connection.request(&mut cx, 8, "act", json!({"ref": "1"}));
    assert_eq!(bad["id"], 8);
    assert_eq!(bad["error"]["code"], protocol::codes::INVALID_PARAMS);
    let gone = connection.request(
        &mut cx,
        9,
        "act",
        json!({"ref": "424242", "action": "click"}),
    );
    assert_eq!(gone["id"], 9);
    assert!(
        gone["error"]["message"]
            .as_str()
            .unwrap()
            .contains("424242")
    );
    // A request whose JSON does not parse still gets its id back where it can be read.
    connection.send(json!({"jsonrpc": "2.0", "id": 10, "method": 5}));
    let parse = connection.read(&mut cx);
    assert_eq!(parse["id"], 10);
    assert_eq!(parse["error"]["code"], protocol::codes::PARSE_ERROR);
}

#[test]
fn set_enabled_closes_and_resumes_the_connection() {
    let server = Server::bind();
    let mut cx = app();
    cx.add_window(|_, _| Panel { clicks: 0 });
    cx.update(|cx| gpux_devtools::install(cx, options(&server)));
    let mut connection = server.accept(&mut cx);
    connection.handshake(&mut cx);
    until(&mut cx, "a guard", |cx| {
        (cx.update(|cx| gpux_devtools::guarded_windows(cx)) == 1).then_some(())
    });
    cx.update(|cx| gpux_devtools::set_enabled(cx, false));
    connection.expect_closed(&mut cx);
    assert_eq!(cx.update(|cx| gpux_devtools::guarded_windows(cx)), 0);
    for _ in 0..20 {
        pump(&mut cx);
        assert!(server.try_accept().is_none(), "no retrying while off");
    }
    cx.update(|cx| gpux_devtools::set_enabled(cx, true));
    let mut again = server.accept(&mut cx);
    again.handshake(&mut cx);
    until(&mut cx, "a guard again", |cx| {
        (cx.update(|cx| gpux_devtools::guarded_windows(cx)) == 1).then_some(())
    });
}
