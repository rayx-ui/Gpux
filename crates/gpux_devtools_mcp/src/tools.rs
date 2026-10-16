//! The MCP tools: `gpux_*` tools that return JSON, the Playwright-compatible `browser_*` tools
//! that answer with Playwright MCP's sections (`### Result`, `### Ran Playwright code`,
//! `### Page`, `### Snapshot`), and the extension tools the connected apps advertise.

use serde_json::{Value, json};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use crate::apps::{AppInfo, REQUEST_TIMEOUT, Registry};

/// What a stdio session remembers between calls: the app and window `browser_tabs` selected,
/// and where the pointer is for the `browser_mouse_*` tools.
#[derive(Default)]
pub struct Session {
    /// The connection and window the `browser_*` tools act on, once selected.
    current: Option<(u64, u64)>,
    mouse: (f64, f64),
}

fn tool(name: &str, title: &str, description: &str, properties: Value, read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "additionalProperties": false },
        "annotations": {
            "title": title,
            "readOnlyHint": read_only,
            "destructiveHint": !read_only,
            "openWorldHint": false
        }
    })
}

/// The arguments every `gpux_*` tool takes to pick an app and a window.
fn target_properties(mut properties: Value) -> Value {
    let object = properties
        .as_object_mut()
        .expect("properties are an object");
    object.insert(
        "app".into(),
        json!({ "type": ["string", "number", "null"], "description": "The app's id or connection number; optional when one app is connected." }),
    );
    object.insert(
        "window".into(),
        json!({ "type": ["number", "null"], "description": "The window id; the app's first window when omitted." }),
    );
    properties
}

const BUTTON: &str = r#"{ "type": ["string", "null"], "enum": ["left", "right", "middle", null] }"#;

fn button() -> Value {
    serde_json::from_str(BUTTON).unwrap()
}

/// The sidecar's own tools.
pub fn builtin() -> Vec<Value> {
    vec![
        tool(
            "gpux_list_apps",
            "List apps",
            "List the gpux apps connected to the sidecar and their windows.",
            json!({}),
            true,
        ),
        tool(
            "gpux_snapshot",
            "Accessibility snapshot",
            "A window's AccessKit tree as YAML in the shape of Playwright's aria snapshot, with [ref=<node id>] on every node.",
            target_properties(json!({
                "view": { "type": ["string", "null"], "enum": ["inspection", "screen-reader", null], "description": "inspection (default) includes generic containers; screen-reader is what assistive technology sees." },
                "ref": { "type": ["string", "null"], "description": "A ref to root the snapshot at." },
                "bounds": { "type": ["boolean", "null"], "description": "Add each node's bounds in window pixels." },
                "depth": { "type": ["number", "null"], "description": "How many levels below the root to show." }
            })),
            true,
        ),
        tool(
            "gpux_query",
            "Query nodes",
            "The nodes a semantic query matches, e.g. `role=button name=\"Save\"`.",
            target_properties(json!({
                "query": { "type": "string" },
                "view": { "type": ["string", "null"], "enum": ["inspection", "screen-reader", null] }
            })),
            true,
        ),
        tool(
            "gpux_act",
            "Perform an accessibility action",
            "Perform an AccessKit action on a ref (click, focus, setValue, increment, decrement, expand, collapse, scrollIntoView, customAction); reports whether a handler ran.",
            target_properties(json!({
                "ref": { "type": "string" },
                "action": { "type": "string", "enum": ["click", "focus", "setValue", "increment", "decrement", "expand", "collapse", "scrollIntoView", "customAction"] },
                "value": { "type": ["string", "number", "null"] },
                "customAction": { "type": ["number", "null"], "description": "The custom action's id." }
            })),
            false,
        ),
        tool(
            "gpux_input",
            "Synthesize input",
            "Synthesize platform input at a ref's bounds or a point: click, hover, drag, longPress, pinch, wheel, type, press, mouseDown, mouseUp, mouseMove.",
            target_properties(json!({
                "kind": { "type": "string", "enum": ["click", "hover", "drag", "longPress", "pinch", "wheel", "type", "press", "mouseDown", "mouseUp", "mouseMove"] },
                "ref": { "type": ["string", "null"] },
                "x": { "type": ["number", "null"] },
                "y": { "type": ["number", "null"] },
                "button": button(),
                "count": { "type": ["number", "null"] },
                "toRef": { "type": ["string", "null"] },
                "toX": { "type": ["number", "null"] },
                "toY": { "type": ["number", "null"] },
                "scale": { "type": ["number", "null"] },
                "deltaX": { "type": ["number", "null"] },
                "deltaY": { "type": ["number", "null"] },
                "text": { "type": ["string", "null"] },
                "keys": { "type": ["string", "null"], "description": "Keystrokes, e.g. `cmd-a backspace`." }
            })),
            false,
        ),
        tool(
            "gpux_screenshot",
            "Screenshot",
            "A PNG of a window, or of a ref's bounds.",
            target_properties(json!({ "ref": { "type": ["string", "null"] } })),
            true,
        ),
        tool(
            "gpux_wait",
            "Wait for a query",
            "Wait until a query starts (state=attached) or stops (state=detached) matching, frame by frame, within a timeout.",
            target_properties(json!({
                "query": { "type": "string" },
                "state": { "type": ["string", "null"], "enum": ["attached", "detached", null] },
                "timeoutMs": { "type": ["number", "null"] }
            })),
            true,
        ),
        tool(
            "gpux_stats",
            "Frame statistics",
            "Frame counts, fast-mode counters and the accessibility tree's size.",
            target_properties(json!({})),
            true,
        ),
        tool(
            "browser_snapshot",
            "Page snapshot",
            "Capture an accessibility snapshot of the current gpux window or target.",
            json!({
                "target": { "type": ["string", "null"] },
                "filename": { "type": ["string", "null"] },
                "depth": { "type": ["number", "null"] },
                "boxes": { "type": ["boolean", "null"] }
            }),
            true,
        ),
        tool(
            "browser_click",
            "Click",
            "Perform click on a gpux element.",
            json!({
                "element": { "type": ["string", "null"] },
                "target": { "type": "string" },
                "doubleClick": { "type": ["boolean", "null"] },
                "button": button()
            }),
            false,
        ),
        tool(
            "browser_hover",
            "Hover mouse",
            "Hover over a gpux element.",
            json!({
                "element": { "type": ["string", "null"] },
                "target": { "type": "string" }
            }),
            false,
        ),
        tool(
            "browser_drag",
            "Drag mouse",
            "Perform drag and drop between two gpux elements.",
            json!({
                "startElement": { "type": ["string", "null"] },
                "startTarget": { "type": "string" },
                "endElement": { "type": ["string", "null"] },
                "endTarget": { "type": "string" }
            }),
            false,
        ),
        tool(
            "browser_type",
            "Type text",
            "Type text into an editable gpux element.",
            json!({
                "element": { "type": ["string", "null"] },
                "target": { "type": "string" },
                "text": { "type": "string" },
                "submit": { "type": ["boolean", "null"] },
                "slowly": { "type": ["boolean", "null"] }
            }),
            false,
        ),
        tool(
            "browser_press_key",
            "Press a key",
            "Press a key on the keyboard.",
            json!({ "key": { "type": "string" } }),
            false,
        ),
        tool(
            "browser_mouse_move_xy",
            "Move mouse",
            "Move mouse to a given position.",
            json!({ "x": { "type": "number" }, "y": { "type": "number" } }),
            false,
        ),
        tool(
            "browser_mouse_down",
            "Press mouse down",
            "Press mouse down at the current mouse position.",
            json!({ "button": button() }),
            false,
        ),
        tool(
            "browser_mouse_up",
            "Press mouse up",
            "Release mouse at the current mouse position.",
            json!({ "button": button() }),
            false,
        ),
        tool(
            "browser_mouse_wheel",
            "Scroll mouse wheel",
            "Scroll the mouse wheel at the current mouse position.",
            json!({
                "deltaX": { "type": ["number", "null"] },
                "deltaY": { "type": ["number", "null"] }
            }),
            false,
        ),
        tool(
            "browser_mouse_click_xy",
            "Click",
            "Click mouse button at a given position.",
            json!({
                "x": { "type": "number" },
                "y": { "type": "number" },
                "button": button(),
                "clickCount": { "type": ["number", "null"] },
                "delay": { "type": ["number", "null"] }
            }),
            false,
        ),
        tool(
            "browser_mouse_drag_xy",
            "Drag mouse",
            "Drag left mouse button to a given position.",
            json!({
                "startX": { "type": "number" },
                "startY": { "type": "number" },
                "endX": { "type": "number" },
                "endY": { "type": "number" }
            }),
            false,
        ),
        tool(
            "browser_take_screenshot",
            "Take a screenshot",
            "Take a screenshot of the current gpux window or target.",
            json!({
                "element": { "type": ["string", "null"] },
                "target": { "type": ["string", "null"] },
                "type": { "type": ["string", "null"], "enum": ["png", null] },
                "filename": { "type": ["string", "null"] },
                "fullPage": { "type": ["boolean", "null"] }
            }),
            true,
        ),
        tool(
            "browser_wait_for",
            "Wait for",
            "Wait for text to appear/disappear or a specified time to pass.",
            json!({
                "time": { "type": ["number", "null"] },
                "text": { "type": ["string", "null"] },
                "textGone": { "type": ["string", "null"] }
            }),
            true,
        ),
        tool(
            "browser_tabs",
            "Manage tabs",
            "List or select gpux windows using Playwright MCP tab formatting.",
            json!({
                "action": { "type": "string", "enum": ["list", "select", "new", "close"] },
                "index": { "type": ["number", "null"] },
                "url": { "type": ["string", "null"] }
            }),
            true,
        ),
        tool(
            "browser_observe",
            "Observe",
            "Capture the current window's metadata and accessibility snapshot as one JSON object.",
            json!({
                "target": { "type": ["string", "null"] },
                "capture_screenshot": { "type": ["boolean", "null"] }
            }),
            true,
        ),
    ]
}

/// The tools to list: the sidecar's own and every extension tool an app advertises (each name
/// once, however many apps offer it).
pub fn list(registry: &Registry) -> Vec<Value> {
    let mut tools = builtin();
    let builtin_names: HashSet<String> = tools
        .iter()
        .filter_map(|tool| tool.get("name")?.as_str().map(str::to_string))
        .collect();
    let mut seen = HashSet::new();
    for app in registry.apps() {
        for extension in &app.tools {
            if builtin_names.contains(&extension.name) || !seen.insert(extension.name.clone()) {
                continue;
            }
            let mut schema = extension.input_schema.clone();
            // The call names the app when several offer the tool.
            if let Some(properties) = schema.as_object_mut().and_then(|schema| {
                schema
                    .entry("properties")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
            }) {
                properties.insert(
                    "app".into(),
                    json!({ "type": ["string", "number", "null"], "description": "The app to call, when several offer this tool." }),
                );
            }
            tools.push(json!({
                "name": extension.name,
                "description": extension.description,
                "inputSchema": schema,
                "annotations": { "title": extension.name, "openWorldHint": false }
            }));
        }
    }
    tools
}

fn text_result(text: String) -> Value {
    json!({ "content": [ { "type": "text", "text": text } ] })
}

fn json_result(value: &Value) -> Value {
    text_result(serde_json::to_string_pretty(value).unwrap_or_default())
}

fn arg_str<'a>(arguments: &'a Value, name: &str) -> Option<&'a str> {
    arguments.get(name).and_then(Value::as_str)
}

fn arg_f64(arguments: &Value, name: &str) -> Option<f64> {
    arguments.get(name).and_then(Value::as_f64)
}

fn required<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    arg_str(arguments, name).ok_or_else(|| format!("missing argument `{name}`"))
}

/// The app an `app` argument names (its id or connection number), or the only one connected.
fn pick_app(registry: &Registry, app: Option<&Value>) -> Result<AppInfo, String> {
    let apps = registry.apps();
    let names = || {
        apps.iter()
            .map(|app| format!("{} (#{})", app.app_id, app.connection))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match app {
        Some(Value::Number(number)) => {
            let connection = number.as_u64().unwrap_or(0);
            apps.iter()
                .find(|app| app.connection == connection)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "no app is connected as #{connection}; connected: {}",
                        names()
                    )
                })
        }
        Some(Value::String(id)) => {
            let matches: Vec<&AppInfo> = apps.iter().filter(|app| &app.app_id == id).collect();
            match matches.as_slice() {
                [app] => Ok((*app).clone()),
                [] => Err(format!(
                    "the app {id} is not connected; connected: {}",
                    names()
                )),
                _ => Err(format!(
                    "several apps are connected as {id}; name one by its connection number: {}",
                    names()
                )),
            }
        }
        _ => match apps.as_slice() {
            [app] => Ok(app.clone()),
            [] => Err("no gpux app is connected; start an app with gpux-devtools enabled".into()),
            _ => Err(format!(
                "several apps are connected; name one with `app`: {}",
                names()
            )),
        },
    }
}

/// The app and window a `gpux_*` call names.
fn target(registry: &Registry, arguments: &Value) -> Result<(AppInfo, Option<u64>), String> {
    let app = pick_app(registry, arguments.get("app").filter(|v| !v.is_null()))?;
    let window = arguments.get("window").and_then(Value::as_u64);
    Ok((app, window))
}

/// The app and window the `browser_*` tools act on: the one `browser_tabs` selected, else the
/// only app's first window.
fn current(registry: &Registry, session: &Session) -> Result<(AppInfo, Option<u64>), String> {
    if let Some((connection, window)) = session.current
        && let Some(app) = registry
            .apps()
            .into_iter()
            .find(|app| app.connection == connection)
    {
        return Ok((app, Some(window)));
    }
    let app = pick_app(registry, None)?;
    let window = app.windows.first().map(|window| window.id);
    Ok((app, window))
}

fn ask(
    registry: &Registry,
    app: &AppInfo,
    method: &str,
    mut params: Value,
    window: Option<u64>,
    extra: Duration,
) -> Result<Value, String> {
    if let (Some(window), Some(object)) = (window, params.as_object_mut()) {
        object.insert("window".into(), json!(window));
    }
    registry
        .request(app.connection, method, params, REQUEST_TIMEOUT + extra)
        .map_err(|error| format!("{} ({}): {error}", app.app_id, method))
}

/// Calls the tool `name`.
pub fn call(
    registry: &Registry,
    session: &mut Session,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    match name {
        "gpux_list_apps" => Ok(json_result(&Value::Array(
            registry.apps().iter().map(AppInfo::to_json).collect(),
        ))),
        "gpux_snapshot" | "gpux_query" | "gpux_act" | "gpux_input" | "gpux_screenshot"
        | "gpux_wait" | "gpux_stats" => {
            let (app, window) = target(registry, &arguments)?;
            let mut params = arguments.clone();
            if let Some(object) = params.as_object_mut() {
                object.remove("app");
                object.remove("window");
            }
            let method = name.trim_start_matches("gpux_");
            let extra =
                Duration::from_millis(arg_f64(&arguments, "timeoutMs").unwrap_or(0.0) as u64);
            let result = ask(registry, &app, method, params, window, extra)?;
            if method == "screenshot" {
                return Ok(image_result("Screenshot captured", &result));
            }
            Ok(json_result(&result))
        }
        name if name.starts_with("browser_") => browser(registry, session, name, arguments),
        _ => extension(registry, name, arguments),
    }
}

fn extension(registry: &Registry, name: &str, arguments: Value) -> Result<Value, String> {
    let offering: Vec<AppInfo> = registry
        .apps()
        .into_iter()
        .filter(|app| app.tools.iter().any(|tool| tool.name == name))
        .collect();
    let app = match arguments.get("app").filter(|v| !v.is_null()) {
        Some(app) => {
            let app = pick_app(registry, Some(app))?;
            if !offering
                .iter()
                .any(|offer| offer.connection == app.connection)
            {
                return Err(format!("the app {} offers no tool {name}", app.app_id));
            }
            app
        }
        None => match offering.as_slice() {
            [app] => app.clone(),
            [] => return Err(format!("unknown tool: {name}")),
            _ => {
                return Err(format!(
                    "several apps offer {name}; name one with `app`: {}",
                    offering
                        .iter()
                        .map(|app| format!("{} (#{})", app.app_id, app.connection))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        },
    };
    let mut arguments = arguments;
    if let Some(object) = arguments.as_object_mut() {
        object.remove("app");
    }
    let result = ask(
        registry,
        &app,
        "extension.call",
        json!({ "name": name, "arguments": arguments }),
        None,
        Duration::ZERO,
    )?;
    // An extension may answer with MCP content itself, or with any JSON.
    if result.get("content").is_some_and(Value::is_array) {
        return Ok(result);
    }
    Ok(json_result(&result))
}

fn image_result(text: &str, screenshot: &Value) -> Value {
    let mut content = vec![json!({ "type": "text", "text": format!("### Result\n{text}") })];
    if let Some(data) = screenshot.get("png").and_then(Value::as_str) {
        content.push(json!({ "type": "image", "data": data, "mimeType": "image/png" }));
    }
    json!({ "content": content })
}

fn page_section(app: &AppInfo, snapshot: &Value) -> String {
    format!(
        "### Page\n- Page URL: gpux://{}/window/{}\n- Page Title: {}",
        app.app_id,
        snapshot
            .get("window")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        snapshot
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
    )
}

fn snapshot_text(
    app: &AppInfo,
    snapshot: &Value,
    filename: Option<&str>,
) -> Result<String, String> {
    let yaml = snapshot
        .get("snapshot")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let section = if let Some(filename) = filename {
        std::fs::write(filename, yaml).map_err(|error| format!("writing {filename}: {error}"))?;
        format!("### Snapshot\n- [Snapshot]({filename})")
    } else {
        format!("### Snapshot\n```yaml\n{yaml}\n```")
    };
    Ok(format!("{}\n\n{section}", page_section(app, snapshot)))
}

/// Performs `input` and answers with Playwright's sections and the snapshot after it.
fn browser_input(
    registry: &Registry,
    app: &AppInfo,
    window: Option<u64>,
    input: Value,
    message: &str,
    code: String,
) -> Result<Value, String> {
    ask(registry, app, "input", input, window, Duration::ZERO)?;
    let snapshot = ask(registry, app, "snapshot", json!({}), window, Duration::ZERO)?;
    Ok(text_result(format!(
        "### Result\n{message}\n\n### Ran Playwright code\n```js\n{code}\n```\n\n{}",
        snapshot_text(app, &snapshot, None)?
    )))
}

fn button_name(arguments: &Value) -> Result<&str, String> {
    match arg_str(arguments, "button").unwrap_or("left") {
        "left" | "Left" => Ok("left"),
        "right" | "Right" => Ok("right"),
        "middle" | "Middle" => Ok("middle"),
        other => Err(format!("unsupported mouse button: {other}")),
    }
}

fn browser(
    registry: &Registry,
    session: &mut Session,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let (app, window) = current(registry, session)?;
    let locator = |target: &str| format!("page.locator('aria-ref={target}')");
    match name {
        "browser_snapshot" => {
            let snapshot = ask(
                registry,
                &app,
                "snapshot",
                json!({
                    "ref": arguments.get("target"),
                    "bounds": arguments.get("boxes"),
                    "depth": arguments.get("depth"),
                }),
                window,
                Duration::ZERO,
            )?;
            Ok(text_result(snapshot_text(
                &app,
                &snapshot,
                arg_str(&arguments, "filename"),
            )?))
        }
        "browser_click" => {
            let target = required(&arguments, "target")?;
            let double = arguments
                .get("doubleClick")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let input = json!({
                "kind": "click",
                "ref": target,
                "button": button_name(&arguments)?,
                "count": if double { 2 } else { 1 },
            });
            let method = if double { "dblclick" } else { "click" };
            browser_input(
                registry,
                &app,
                window,
                input,
                &format!("Clicked {target}"),
                format!("await {}.{method}();", locator(target)),
            )
        }
        "browser_hover" => {
            let target = required(&arguments, "target")?;
            browser_input(
                registry,
                &app,
                window,
                json!({ "kind": "hover", "ref": target }),
                &format!("Hovered {target}"),
                format!("await {}.hover();", locator(target)),
            )
        }
        "browser_drag" => {
            let start = required(&arguments, "startTarget")?;
            let end = required(&arguments, "endTarget")?;
            browser_input(
                registry,
                &app,
                window,
                json!({ "kind": "drag", "ref": start, "toRef": end }),
                &format!("Dragged {start} to {end}"),
                format!("await {}.dragTo({});", locator(start), locator(end)),
            )
        }
        "browser_type" => {
            let target = required(&arguments, "target")?;
            let text = required(&arguments, "text")?;
            let slowly = arguments
                .get("slowly")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut code = if slowly {
                format!("await {}.pressSequentially({text:?});", locator(target))
            } else {
                format!("await {}.fill({text:?});", locator(target))
            };
            ask(
                registry,
                &app,
                "input",
                json!({ "kind": "type", "ref": target, "text": text, "replace": !slowly }),
                window,
                Duration::ZERO,
            )?;
            if arguments
                .get("submit")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                code.push_str(&format!("\nawait {}.press('Enter');", locator(target)));
                return browser_input(
                    registry,
                    &app,
                    window,
                    json!({ "kind": "press", "keys": "enter" }),
                    &format!("Typed into {target} and submitted"),
                    code,
                );
            }
            let snapshot = ask(
                registry,
                &app,
                "snapshot",
                json!({}),
                window,
                Duration::ZERO,
            )?;
            Ok(text_result(format!(
                "### Result\nTyped into {target}\n\n### Ran Playwright code\n```js\n{code}\n```\n\n{}",
                snapshot_text(&app, &snapshot, None)?
            )))
        }
        "browser_press_key" => {
            let key = required(&arguments, "key")?;
            browser_input(
                registry,
                &app,
                window,
                json!({ "kind": "press", "keys": playwright_key(key) }),
                &format!("Pressed {key}"),
                format!("await page.keyboard.press({key:?});"),
            )
        }
        "browser_mouse_move_xy" => {
            let (x, y) = point(&arguments, "x", "y")?;
            session.mouse = (x, y);
            browser_input(
                registry,
                &app,
                window,
                json!({ "kind": "mouseMove", "x": x, "y": y }),
                &format!("Moved the mouse to ({x}, {y})"),
                format!("await page.mouse.move({x}, {y});"),
            )
        }
        "browser_mouse_down" | "browser_mouse_up" => {
            let (x, y) = session.mouse;
            let down = name == "browser_mouse_down";
            browser_input(
                registry,
                &app,
                window,
                json!({
                    "kind": if down { "mouseDown" } else { "mouseUp" },
                    "x": x,
                    "y": y,
                    "button": button_name(&arguments)?,
                }),
                if down {
                    "Pressed the mouse"
                } else {
                    "Released the mouse"
                },
                format!("await page.mouse.{}();", if down { "down" } else { "up" }),
            )
        }
        "browser_mouse_wheel" => {
            let (x, y) = session.mouse;
            let dx = arg_f64(&arguments, "deltaX").unwrap_or(0.0);
            let dy = arg_f64(&arguments, "deltaY").unwrap_or(0.0);
            browser_input(
                registry,
                &app,
                window,
                json!({ "kind": "wheel", "x": x, "y": y, "deltaX": dx, "deltaY": dy }),
                &format!("Scrolled by ({dx}, {dy})"),
                format!("await page.mouse.wheel({dx}, {dy});"),
            )
        }
        "browser_mouse_click_xy" => {
            let (x, y) = point(&arguments, "x", "y")?;
            session.mouse = (x, y);
            let count = arg_f64(&arguments, "clickCount").unwrap_or(1.0).max(1.0) as u64;
            browser_input(
                registry,
                &app,
                window,
                json!({ "kind": "click", "x": x, "y": y, "button": button_name(&arguments)?, "count": count }),
                &format!("Clicked at ({x}, {y})"),
                format!("await page.mouse.click({x}, {y});"),
            )
        }
        "browser_mouse_drag_xy" => {
            let (sx, sy) = point(&arguments, "startX", "startY")?;
            let (ex, ey) = point(&arguments, "endX", "endY")?;
            session.mouse = (ex, ey);
            browser_input(
                registry,
                &app,
                window,
                json!({ "kind": "drag", "x": sx, "y": sy, "toX": ex, "toY": ey }),
                &format!("Dragged from ({sx}, {sy}) to ({ex}, {ey})"),
                format!(
                    "await page.mouse.move({sx}, {sy});\nawait page.mouse.down();\nawait page.mouse.move({ex}, {ey});\nawait page.mouse.up();"
                ),
            )
        }
        "browser_take_screenshot" => {
            if let Some(kind) = arg_str(&arguments, "type")
                && kind != "png"
            {
                return Err(format!(
                    "unsupported screenshot type {kind:?}: gpux takes PNGs"
                ));
            }
            let screenshot = ask(
                registry,
                &app,
                "screenshot",
                json!({ "ref": arguments.get("target") }),
                window,
                Duration::ZERO,
            )?;
            if let Some(filename) = arg_str(&arguments, "filename") {
                let data = screenshot
                    .get("png")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let bytes = decode_base64(data).ok_or("the app's screenshot is not base64")?;
                std::fs::write(filename, bytes)
                    .map_err(|error| format!("writing {filename}: {error}"))?;
                return Ok(text_result(format!(
                    "### Result\n- [Screenshot]({filename})"
                )));
            }
            Ok(image_result("Screenshot captured", &screenshot))
        }
        "browser_wait_for" => {
            let time = arg_f64(&arguments, "time");
            let text = arg_str(&arguments, "text");
            let gone = arg_str(&arguments, "textGone");
            if time.is_none() && text.is_none() && gone.is_none() {
                return Err("Either time, text or textGone must be provided".into());
            }
            if let Some(seconds) = time {
                std::thread::sleep(Duration::from_secs_f64(seconds.clamp(0.0, 30.0)));
            }
            let deadline = Instant::now() + REQUEST_TIMEOUT;
            loop {
                let snapshot = ask(
                    registry,
                    &app,
                    "snapshot",
                    json!({}),
                    window,
                    Duration::ZERO,
                )?;
                let yaml = snapshot
                    .get("snapshot")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if text.is_none_or(|text| yaml.contains(text))
                    && gone.is_none_or(|text| !yaml.contains(text))
                {
                    return Ok(text_result(format!(
                        "### Result\nWaited for {}\n\n{}",
                        text.or(gone).unwrap_or("time"),
                        snapshot_text(&app, &snapshot, None)?
                    )));
                }
                if Instant::now() >= deadline {
                    return Err("Timed out waiting for the requested state".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        "browser_tabs" => {
            let action = arg_str(&arguments, "action").unwrap_or("list");
            if !matches!(action, "list" | "select") {
                return Err("gpux windows can be listed and selected; apps open and close their own windows".into());
            }
            let windows: Vec<(AppInfo, u64, String)> = registry
                .apps()
                .into_iter()
                .flat_map(|app| {
                    app.windows
                        .iter()
                        .map(|window| (app.clone(), window.id, window.title.clone()))
                        .collect::<Vec<_>>()
                })
                .collect();
            if action == "select" {
                let index = arguments
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or("Tab index is required for select")?
                    as usize;
                let (app, window, _) = windows
                    .get(index)
                    .ok_or_else(|| format!("no tab {index}: {} are open", windows.len()))?;
                session.current = Some((app.connection, *window));
            }
            let current = session
                .current
                .or_else(|| window.map(|window| (app.connection, window)));
            let lines: Vec<String> = windows
                .iter()
                .enumerate()
                .map(|(index, (app, window, title))| {
                    let mark = if current == Some((app.connection, *window)) {
                        " (current)"
                    } else {
                        ""
                    };
                    format!(
                        "- {index}:{mark} [{title}](gpux://{}/window/{window})",
                        app.app_id
                    )
                })
                .collect();
            Ok(text_result(format!("### Result\n{}", lines.join("\n"))))
        }
        "browser_observe" => {
            let snapshot = ask(
                registry,
                &app,
                "snapshot",
                json!({ "ref": arguments.get("target") }),
                window,
                Duration::ZERO,
            )?;
            let mut observation = json!({
                "app": app.to_json(),
                "window": snapshot.get("window"),
                "title": snapshot.get("title"),
                "snapshot": snapshot.get("snapshot"),
            });
            if arguments
                .get("capture_screenshot")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let screenshot = ask(
                    registry,
                    &app,
                    "screenshot",
                    json!({}),
                    window,
                    Duration::ZERO,
                )?;
                observation["screenshot"] = screenshot;
            }
            Ok(json_result(&observation))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

fn point(arguments: &Value, x: &str, y: &str) -> Result<(f64, f64), String> {
    Ok((
        arg_f64(arguments, x).ok_or_else(|| format!("missing argument `{x}`"))?,
        arg_f64(arguments, y).ok_or_else(|| format!("missing argument `{y}`"))?,
    ))
}

/// Playwright's key names (`Enter`, `Control+A`) as gpux keystrokes (`enter`, `ctrl-a`).
pub fn playwright_key(key: &str) -> String {
    key.split('+')
        .map(|part| match part {
            "Control" => "ctrl".to_string(),
            "Meta" => "cmd".to_string(),
            "Alt" => "alt".to_string(),
            "Shift" => "shift".to_string(),
            "ArrowLeft" => "left".to_string(),
            "ArrowRight" => "right".to_string(),
            "ArrowUp" => "up".to_string(),
            "ArrowDown" => "down".to_string(),
            "Escape" => "escape".to_string(),
            " " | "Space" => "space".to_string(),
            other => other.to_ascii_lowercase(),
        })
        .collect::<Vec<_>>()
        .join("-")
}

fn decode_base64(data: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (index, byte) in ALPHABET.iter().enumerate() {
        lookup[*byte as usize] = index as u8;
    }
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in data
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace() && *byte != b'=')
    {
        let value = lookup[byte as usize];
        if value == 255 {
            return None;
        }
        buffer = (buffer << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}
