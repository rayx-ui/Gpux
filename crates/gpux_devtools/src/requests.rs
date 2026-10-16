//! What the app answers the sidecar's requests with, against its windows' AccessKit trees.
//!
//! Every request names its window by `window` (an id from `hello`), or means the active window
//! (else the first). Nodes are named by `ref`: their AccessKit node id, as snapshots print it.
//! The functions here are the protocol's handlers; [`handle`] dispatches by method.

use std::time::Duration;

use base64::Engine as _;
use gpux::{
    AnyWindowHandle, App, AsyncApp, Bounds, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, PlatformInput, Point, ScrollDelta,
    ScrollWheelEvent, Task, TouchEvent, TouchId, TouchPhase, Window,
    accesskit::{Action, ActionData, ActionRequest, NodeId, TreeId},
    point, px,
    semantic_query::{Query, QueryView, SemanticNode, SemanticSnapshot},
};
use serde_json::{Value, json};

use crate::protocol::{Rect, RpcError, WindowInfo, codes};

type Outcome = Result<Value, RpcError>;

/// Answers the request `method` with `params`.
pub fn handle(cx: &mut App, method: &str, params: Value) -> Task<Outcome> {
    match method {
        "snapshot" => Task::ready(snapshot(cx, &params)),
        "query" => Task::ready(query(cx, &params)),
        "act" => Task::ready(act(cx, &params)),
        "input" => input(cx, params),
        "screenshot" => Task::ready(screenshot(cx, &params)),
        "wait" => wait(cx, params),
        "stats" => Task::ready(stats(cx, &params)),
        "windows" => Task::ready(Ok(json!(window_infos(cx)))),
        _ => Task::ready(Err(RpcError::new(
            codes::METHOD_NOT_FOUND,
            format!("the app answers no `{method}`"),
        ))),
    }
}

/// Every open window, as `hello` lists them.
pub fn window_infos(cx: &mut App) -> Vec<WindowInfo> {
    cx.windows()
        .into_iter()
        .filter_map(|window| window_info(cx, window))
        .collect()
}

/// `window` as `hello` and `windowOpened` describe it.
pub fn window_info(cx: &mut App, window: AnyWindowHandle) -> Option<WindowInfo> {
    window
        .update(cx, |_, window, _| {
            let bounds = window.bounds();
            let title = window.window_title();
            WindowInfo {
                id: window.window_handle().window_id().as_u64(),
                title: (!title.is_empty()).then_some(title),
                bounds: Rect {
                    x: bounds.origin.x.as_f32(),
                    y: bounds.origin.y.as_f32(),
                    width: bounds.size.width.as_f32(),
                    height: bounds.size.height.as_f32(),
                },
                scale_factor: window.scale_factor(),
            }
        })
        .ok()
}

/// The window `params` names, else the active one, else the first.
fn window(cx: &App, params: &Value) -> Result<AnyWindowHandle, RpcError> {
    let windows = cx.windows();
    if let Some(id) = params.get("window").and_then(Value::as_u64) {
        return windows
            .into_iter()
            .find(|window| window.window_id().as_u64() == id)
            .ok_or_else(|| RpcError::failed(format!("no window {id}")));
    }
    cx.active_window()
        .filter(|active| windows.contains(active))
        .or_else(|| windows.first().copied())
        .ok_or_else(|| RpcError::failed("the app has no window"))
}

fn tree(window: &Window) -> Result<SemanticSnapshot, RpcError> {
    SemanticSnapshot::of(window).ok_or_else(|| {
        RpcError::failed(
            "the window has no accessibility tree (accessibility is disabled, or nothing \
             inspects it)",
        )
    })
}

fn with_tree<R>(
    cx: &mut App,
    params: &Value,
    f: impl FnOnce(&SemanticSnapshot, &mut Window, &mut App) -> Result<R, RpcError>,
) -> Result<R, RpcError> {
    let window = window(cx, params)?;
    window
        .update(cx, |_, window, cx| {
            let snapshot = tree(window)?;
            f(&snapshot, window, cx)
        })
        .map_err(|error| RpcError::failed(format!("{error:#}")))?
}

/// The node `ref` names, or an error naming the ref.
fn node<'a>(
    snapshot: &'a SemanticSnapshot,
    reference: &Value,
) -> Result<&'a SemanticNode, RpcError> {
    let text = match reference {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return Err(RpcError::invalid_params("`ref` is a node id")),
    };
    let id: u64 = text
        .trim_start_matches('#')
        .parse()
        .map_err(|_| RpcError::invalid_params(format!("`{text}` is not a ref")))?;
    snapshot
        .node(NodeId(id))
        .ok_or_else(|| RpcError::failed(format!("ref {text} is gone from the window's tree")))
}

fn view(params: &Value) -> QueryView {
    match params.get("view").and_then(Value::as_str) {
        Some("screen-reader") => QueryView::ScreenReader,
        _ => QueryView::Inspection,
    }
}

/// `snapshot`: the window's tree as YAML, in the inspection view (every node, generic
/// containers included) or the screen-reader view (`view: "screen-reader"`), from `ref` when
/// given, `depth` levels deep when given, with bounds when `bounds` is true.
pub fn snapshot(cx: &mut App, params: &Value) -> Outcome {
    with_tree(cx, params, |snapshot, window, _| {
        let root = match params.get("ref") {
            Some(reference) => Some(node(snapshot, reference)?.node_id),
            None => None,
        };
        let bounds = params
            .get("bounds")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let depth = params
            .get("depth")
            .and_then(Value::as_u64)
            .map(|depth| depth as usize);
        let yaml = crate::snapshot::render(snapshot, view(params), root, bounds, depth);
        Ok(json!({
            "window": window.window_handle().window_id().as_u64(),
            "title": window.window_title(),
            "snapshot": yaml,
        }))
    })
}

/// A node as `query` returns it.
pub fn node_json(node: &SemanticNode) -> Value {
    json!({
        "ref": node.node_id.0.to_string(),
        "role": node.role,
        "name": node.name,
        "text": node.text,
        "value": node.value,
        "description": node.description,
        "elementId": node.element_id,
        "accessibilityId": node.accessibility_id,
        "checked": node.checked.map(|checked| format!("{checked:?}").to_lowercase()),
        "expanded": node.expanded,
        "selected": node.selected,
        "disabled": node.disabled,
        "focused": node.focused,
        "bounds": {
            "x": node.bounds.x,
            "y": node.bounds.y,
            "width": node.bounds.width,
            "height": node.bounds.height,
        },
        "actions": node.actions.iter().map(|action| format!("{action:?}")).collect::<Vec<_>>(),
    })
}

/// `query`: the nodes the query's text form (`role=button name=Save`) matches, in tree order.
pub fn query(cx: &mut App, params: &Value) -> Outcome {
    let text = params
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("query needs a `query`"))?;
    let query: Query = text
        .parse()
        .map_err(|error: gpux::semantic_query::QueryParseError| {
            RpcError::invalid_params(error.0)
        })?;
    with_tree(cx, params, |snapshot, _, _| {
        Ok(json!({
            "nodes": query.matches(snapshot).into_iter().map(node_json).collect::<Vec<_>>(),
        }))
    })
}

fn action(name: &str) -> Option<Action> {
    Some(match name {
        "click" => Action::Click,
        "focus" => Action::Focus,
        "blur" => Action::Blur,
        "setValue" | "set_value" => Action::SetValue,
        "increment" => Action::Increment,
        "decrement" => Action::Decrement,
        "expand" => Action::Expand,
        "collapse" => Action::Collapse,
        "scrollIntoView" | "scroll_into_view" => Action::ScrollIntoView,
        "showContextMenu" | "show_context_menu" => Action::ShowContextMenu,
        "customAction" | "custom_action" => Action::CustomAction,
        _ => return None,
    })
}

/// `act`: performs the AccessKit `action` on `ref` (with `value` for `setValue`, `id` for
/// `customAction`) through `Window::perform_a11y_action`, reporting whether a handler ran.
pub fn act(cx: &mut App, params: &Value) -> Outcome {
    let name = params
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("act needs an `action`"))?;
    let action =
        action(name).ok_or_else(|| RpcError::invalid_params(format!("no action `{name}`")))?;
    let data = match action {
        Action::SetValue => Some(match params.get("value") {
            Some(Value::Number(number)) => ActionData::NumericValue(number.as_f64().unwrap_or(0.)),
            Some(Value::String(text)) => ActionData::Value(text.clone().into()),
            _ => return Err(RpcError::invalid_params("setValue needs a `value`")),
        }),
        Action::CustomAction => Some(ActionData::CustomAction(
            params
                .get("id")
                .or_else(|| params.get("customAction"))
                .and_then(Value::as_i64)
                .ok_or_else(|| RpcError::invalid_params("customAction needs an `id`"))?
                as i32,
        )),
        _ => None,
    };
    let reference = params
        .get("ref")
        .ok_or_else(|| RpcError::invalid_params("act needs a `ref`"))?;
    with_tree(cx, params, |snapshot, window, cx| {
        let target = node(snapshot, reference)?.node_id;
        let handled = window.perform_a11y_action(
            ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: target,
                data,
            },
            cx,
        );
        Ok(json!({ "handled": handled }))
    })
}

/// Where an input lands: `ref`'s center, else `x`/`y` in window pixels, else the window's
/// center.
fn position(
    snapshot: &SemanticSnapshot,
    params: &Value,
    window: &Window,
    keys: (&str, &str, &str),
) -> Result<Option<Point<Pixels>>, RpcError> {
    if let Some(reference) = params.get(keys.0) {
        let bounds = node(snapshot, reference)?.bounds.to_bounds();
        return Ok(Some(bounds.center()));
    }
    if let (Some(x), Some(y)) = (
        params.get(keys.1).and_then(Value::as_f64),
        params.get(keys.2).and_then(Value::as_f64),
    ) {
        return Ok(Some(point(px(x as f32), px(y as f32))));
    }
    let _ = window;
    Ok(None)
}

fn button(params: &Value) -> MouseButton {
    match params.get("button").and_then(Value::as_str) {
        Some("right") => MouseButton::Right,
        Some("middle") => MouseButton::Middle,
        _ => MouseButton::Left,
    }
}

fn mouse_down(
    window: &mut Window,
    cx: &mut App,
    at: Point<Pixels>,
    button: MouseButton,
    count: usize,
) {
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button,
            position: at,
            modifiers: Modifiers::default(),
            click_count: count,
            first_mouse: count == 1,
        }),
        cx,
    );
}

fn mouse_up(
    window: &mut Window,
    cx: &mut App,
    at: Point<Pixels>,
    button: MouseButton,
    count: usize,
) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button,
            position: at,
            modifiers: Modifiers::default(),
            click_count: count,
        }),
        cx,
    );
}

fn mouse_move(window: &mut Window, cx: &mut App, at: Point<Pixels>, pressed: Option<MouseButton>) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at,
            pressed_button: pressed,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn click(window: &mut Window, cx: &mut App, at: Point<Pixels>, button: MouseButton, count: usize) {
    for count in 1..=count.max(1) {
        mouse_down(window, cx, at, button, count);
        mouse_up(window, cx, at, button, count);
    }
}

fn touch(window: &mut Window, cx: &mut App, at: Point<Pixels>, phase: TouchPhase) {
    window.dispatch_event(
        PlatformInput::Touch(TouchEvent {
            id: TouchId(1),
            phase,
            position: at,
            predicted_position: None,
            force: None,
            timestamp: None,
        }),
        cx,
    );
}

/// Types `text` as input methods do, Enter for each line break.
fn type_text(window: &mut Window, cx: &mut App, text: &str) {
    let mut segment = String::new();
    for c in text.chars() {
        if c == '\n' || c == '\r' {
            if !segment.is_empty() {
                window.dispatch_text_input(&segment, cx);
                segment.clear();
            }
            if c == '\n'
                && let Ok(enter) = Keystroke::parse("enter")
            {
                window.dispatch_keystroke(enter, cx);
            }
        } else {
            segment.push(c);
        }
    }
    if !segment.is_empty() {
        window.dispatch_text_input(&segment, cx);
    }
}

/// Draws a frame, so what an input changed (the focused input handler above all) is in place
/// before the next input.
fn flush(window: &mut Window, cx: &mut App) {
    window.draw(cx).clear(cx);
}

/// `input`: synthesizes input through the window's dispatch methods, the paths platform input
/// takes. `kind` is `click` (with `button` and `count`), `hover`, `mouseMove`, `mouseDown`,
/// `mouseUp`, `drag` (to `toRef` or `toX`/`toY`), `longPress` (`durationMs`), `pinch`
/// (`scale`), `wheel` (`deltaX`/`deltaY`), `type` (`text`, clicking `ref` first when given and
/// clearing the field first with `replace`) or `press` (`keys`: `["cmd-a", "backspace"]`, or
/// the same separated by spaces). Positions come from `ref` or `x`/`y` in window pixels.
pub fn input(cx: &mut App, params: Value) -> Task<Outcome> {
    let kind = params
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("click")
        .to_string();
    let planned = with_tree(cx, &params, |snapshot, window, cx| {
        let at = position(snapshot, &params, window, ("ref", "x", "y"))?;
        let center = window.bounds().size;
        let at_or_center = at.unwrap_or(point(center.width / 2., center.height / 2.));
        match kind.as_str() {
            "click" => {
                let count = params.get("count").and_then(Value::as_u64).unwrap_or(1) as usize;
                click(window, cx, at_or_center, button(&params), count);
            }
            "hover" | "mouseMove" => mouse_move(window, cx, at_or_center, None),
            "mouseDown" => mouse_down(window, cx, at_or_center, button(&params), 1),
            "mouseUp" => mouse_up(window, cx, at_or_center, button(&params), 1),
            "drag" => {
                let to = position(snapshot, &params, window, ("toRef", "toX", "toY"))?
                    .ok_or_else(|| RpcError::invalid_params("drag needs `toRef` or `toX`/`toY`"))?;
                let button = button(&params);
                mouse_down(window, cx, at_or_center, button, 1);
                mouse_move(window, cx, to, Some(button));
                mouse_up(window, cx, to, button, 1);
            }
            "longPress" | "long_press" => {
                touch(window, cx, at_or_center, TouchPhase::Started);
                return Ok(Some(at_or_center));
            }
            "pinch" => {
                let scale = params.get("scale").and_then(Value::as_f64).unwrap_or(1.) as f32;
                window.dispatch_event(
                    PlatformInput::Pinch(PinchEvent {
                        position: at_or_center,
                        delta: scale - 1.,
                        modifiers: Modifiers::default(),
                        phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            }
            "wheel" => {
                let delta = |key| params.get(key).and_then(Value::as_f64).unwrap_or(0.) as f32;
                window.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: at_or_center,
                        delta: ScrollDelta::Pixels(point(px(delta("deltaX")), px(delta("deltaY")))),
                        modifiers: Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            }
            "type" => {
                let text = params
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::invalid_params("type needs a `text`"))?;
                if let Some(at) = at {
                    // Focus the field, and let the frame install its input handler.
                    click(window, cx, at, MouseButton::Left, 1);
                    flush(window, cx);
                }
                if params.get("replace").and_then(Value::as_bool) == Some(true) {
                    // Fill: what the field holds goes first.
                    // The secondary modifier of the keyboard in use: a Mac's in the browser too.
                    let select_all = if Modifiers::secondary_key().platform {
                        "cmd-a"
                    } else {
                        "ctrl-a"
                    };
                    for key in [select_all, "backspace"] {
                        if let Ok(keystroke) = Keystroke::parse(key) {
                            window.dispatch_keystroke(keystroke, cx);
                        }
                    }
                }
                type_text(window, cx, text);
                flush(window, cx);
            }
            "press" => {
                let keys: Vec<String> = match params.get("keys") {
                    Some(Value::Array(keys)) => keys
                        .iter()
                        .filter_map(|key| key.as_str().map(str::to_string))
                        .collect(),
                    Some(Value::String(keys)) => {
                        keys.split_whitespace().map(str::to_string).collect()
                    }
                    _ => return Err(RpcError::invalid_params("press needs `keys`")),
                };
                for key in keys {
                    let keystroke = Keystroke::parse(&key)
                        .map_err(|error| RpcError::invalid_params(format!("`{key}`: {error}")))?;
                    window.dispatch_keystroke(keystroke, cx);
                }
            }
            other => return Err(RpcError::invalid_params(format!("no input kind `{other}`"))),
        }
        Ok(None)
    });
    let window = window(cx, &params);
    match (planned, window) {
        (Err(error), _) | (_, Err(error)) => Task::ready(Err(error)),
        (Ok(None), _) => Task::ready(Ok(json!({ "ok": true }))),
        (Ok(Some(at)), Ok(window)) => {
            // A long press holds the touch, then lifts it.
            let hold = params
                .get("durationMs")
                .and_then(Value::as_u64)
                .unwrap_or(800);
            cx.spawn(async move |cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(hold))
                    .await;
                window
                    .update(cx, |_, window, cx| touch(window, cx, at, TouchPhase::Ended))
                    .map_err(|error| RpcError::failed(format!("{error:#}")))?;
                Ok(json!({ "ok": true }))
            })
        }
    }
}

/// `screenshot`: a PNG of the window's last frame, or of `ref`'s bounds in it, base64-encoded.
pub fn screenshot(cx: &mut App, params: &Value) -> Outcome {
    let window = window(cx, params)?;
    window
        .update(cx, |_, window, _| -> Outcome {
            let image = window.render_to_image().map_err(|error| {
                RpcError::failed(format!("the window cannot render to an image: {error:#}"))
            })?;
            let image = match params.get("ref") {
                Some(reference) => {
                    let snapshot = tree(window)?;
                    let bounds = node(&snapshot, reference)?.bounds.to_bounds();
                    crop(image, bounds, window.scale_factor())
                }
                None => image,
            };
            let mut png = Vec::new();
            image::DynamicImage::ImageRgba8(image.clone())
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .map_err(|error| RpcError::failed(error.to_string()))?;
            Ok(json!({
                "width": image.width(),
                "height": image.height(),
                "png": base64::engine::general_purpose::STANDARD.encode(png),
            }))
        })
        .map_err(|error| RpcError::failed(format!("{error:#}")))?
}

fn crop(image: image::RgbaImage, bounds: Bounds<Pixels>, scale: f32) -> image::RgbaImage {
    let x = ((bounds.origin.x.as_f32() * scale).max(0.) as u32).min(image.width());
    let y = ((bounds.origin.y.as_f32() * scale).max(0.) as u32).min(image.height());
    let width = ((bounds.size.width.as_f32() * scale) as u32)
        .min(image.width() - x)
        .max(1);
    let height = ((bounds.size.height.as_f32() * scale) as u32)
        .min(image.height() - y)
        .max(1);
    image::imageops::crop_imm(&image, x, y, width, height).to_image()
}

/// `wait`: resolves when `query` matches (or, with `state: "detached"`, no longer matches),
/// checking each frame interval, or fails after `timeoutMs` (5000 by default).
pub fn wait(cx: &mut App, params: Value) -> Task<Outcome> {
    let query: Query = match params.get("query").and_then(Value::as_str).map(str::parse) {
        Some(Ok(query)) => query,
        Some(Err(error)) => {
            let error: gpux::semantic_query::QueryParseError = error;
            return Task::ready(Err(RpcError::invalid_params(error.0)));
        }
        None => return Task::ready(Err(RpcError::invalid_params("wait needs a `query`"))),
    };
    let detached = params.get("state").and_then(Value::as_str) == Some("detached");
    let timeout = Duration::from_millis(
        params
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(5000),
    );
    let window = match window(cx, &params) {
        Ok(window) => window,
        Err(error) => return Task::ready(Err(error)),
    };
    cx.spawn(async move |cx: &mut AsyncApp| {
        // The executor's clock: the browser has no `std::time::Instant`, and tests drive it.
        let executor = cx.background_executor().clone();
        let start = executor.now();
        let elapsed = || executor.now().duration_since(start);
        loop {
            let matched = window
                .update(cx, |_, window, _| {
                    SemanticSnapshot::of(window)
                        .map(|snapshot| !query.matches(&snapshot).is_empty())
                })
                .map_err(|error| RpcError::failed(format!("{error:#}")))?;
            if matched.is_some_and(|matched| matched != detached) {
                return Ok(json!({
                    "matched": true,
                    "elapsedMs": elapsed().as_millis() as u64,
                }));
            }
            if elapsed() >= timeout {
                return Err(RpcError::failed(format!(
                    "`{query}` did not {} within {} ms",
                    if detached { "stop matching" } else { "match" },
                    timeout.as_millis()
                )));
            }
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
        }
    })
}

/// `stats`: how many frames the window drew, the size of its tree and, with this crate's
/// `diagnostics` feature, fast mode's counters for its last frame.
pub fn stats(cx: &mut App, params: &Value) -> Outcome {
    let window = window(cx, params)?;
    window
        .update(cx, |_, window, _| {
            #[cfg(feature = "diagnostics")]
            let counters = Value::Object(
                window
                    .fast_frame_stats()
                    .counters()
                    .into_iter()
                    .map(|(name, value)| (name, json!(value)))
                    .collect(),
            );
            #[cfg(not(feature = "diagnostics"))]
            let counters = Value::Null;
            json!({
                "window": window.window_handle().window_id().as_u64(),
                "fast": counters,
                "frames": window.frames_drawn(),
                "treeNodes": SemanticSnapshot::of(window).map(|snapshot| snapshot.nodes.len()),
            })
        })
        .map_err(|error| RpcError::failed(format!("{error:#}")))
}
