# Devtools: agents inspecting gpux apps

An agent (an MCP harness such as Claude Code) inspects and drives a running gpux app through two
crates:

- [`gpux-devtools`](../crates/gpux_devtools), linked into the app, connects out over a WebSocket
  to a sidecar and answers it from the app's windows' AccessKit trees;
- [`gpux-devtools-mcp`](../crates/gpux_devtools_mcp), the sidecar, listens for apps on loopback
  and serves the agent MCP tools over stdio.

```text
agent ──stdio (MCP)── gpux-devtools-mcp ──ws://127.0.0.1:9230── app (gpux-devtools)
```

What the agent sees is the tree screen readers read ([accessibility.md](accessibility.md)): the
same roles, names, states, values, bounds and actions. Annotate an element once and screen
readers, tests (`gpux::semantic_query`) and agents all see it.

## Adding it to an app

The devtools are an optional dependency. `gpux` and `gpux-platform` never depend on
`gpux-devtools`, so an app that does not add it links none of it: no WebSocket client, no query
engine, no per-frame work. Put it behind a feature of the app:

```toml
[dependencies]
gpux-devtools = { version = "0.3", optional = true }

[features]
devtools = ["dep:gpux-devtools"]
```

and install it once the application runs:

```rust
#[cfg(feature = "devtools")]
gpux_devtools::install(cx, gpux_devtools::Options::new("dev.example.notes", "Notes"));
```

`Options::new(app_id, app_name)` names the app in `hello`: the id is stable (reverse-DNS style)
and is what the agent uses to pick an app when several are connected. Further options:

| Option | Default | |
|---|---|---|
| `.url(url)` | `GPUX_DEVTOOLS_URL`, else (browser) the page's `gpux_devtools` parameter, else `ws://127.0.0.1:9230` | The sidecar to connect to. |
| `.token(token)` | `GPUX_DEVTOOLS_TOKEN` | Sent in `hello`; a sidecar started with `--token` refuses apps without it. |
| `.enabled(bool)` | `GPUX_DEVTOOLS`, else on | Whether to connect at all. |
| `.backoff(min, max)` | 250 ms to 5 s | How quickly it retries while no sidecar listens. |
| `.extension(Extension::new(name, description, schema, handler))` | none | An app tool of its own (see [Extension tools](#extension-tools)). |

`gpux-devtools`'s `diagnostics` feature adds fast mode's counters to `stats` (gpux keeps them only
with its own `diagnostics` feature); frame counts and the tree's size come without it.

## Switching it on and off

| When | Switch |
|---|---|
| Build | The app's own feature: without it nothing is linked. |
| Start | `Options::enabled(bool)`, else `GPUX_DEVTOOLS` (`on`/`1`, `off`/`0`); the option wins. |
| Run | `gpux_devtools::set_enabled(cx, bool)`: off closes the connection and stops retrying, on reconnects at once. |

`is_enabled`, `is_connected` and `refusal` (why a sidecar refused the app: a wrong token or
protocol version, after which the app does not retry until `set_enabled(cx, true)`) report the
state.

Switched off or with no sidecar listening, the app costs a retry timer and nothing per frame.
While connected, every window holds an inspection guard (`Window::begin_a11y_inspection`), so it
builds its AccessKit tree on every frame without a screen reader and without telling the
operating system one is running ([accessibility.md](accessibility.md#activation-sources)); the
guards go when the connection does.

## Per platform

The app always connects out, so the sidecar needs no access to the device: the device needs a
route to the sidecar's loopback port.

| Platform | Route | Notes |
|---|---|---|
| macOS, Windows, Linux | The machine's loopback. | `GPUX_DEVTOOLS_URL` names another sidecar (with its `--port`). |
| Browser | The page connects to the sidecar's loopback port itself (the browser has no environment variables). A page served from loopback reaches it as is; from another origin the browser may ask the visitor to allow local network access. | The `gpux_devtools` query parameter names the sidecar: a `ws://` URL, or a port on loopback (`?gpux_devtools=9230`). `Options::url` wins over it. |
| iOS Simulator | The Simulator shares the Mac's loopback. | Nothing: the default URL reaches the sidecar on the Mac. |
| Android emulator or device | `adb reverse tcp:9230 tcp:9230` makes the device's loopback port the computer's. | The app needs the `android.permission.INTERNET` permission, even for loopback. |
| iOS device | No reverse forwarding: point `Options::url` at the Mac's address on the network and start the sidecar where the device can reach it. | |

## The sidecar

```text
gpux-devtools-mcp [--port PORT] [--token TOKEN] [--print-manifest]
```

- `--port` (default 9230; 0 picks a free one) is the loopback port apps connect to. The sidecar
  never listens beyond loopback.
- `--token` (or `GPUX_DEVTOOLS_TOKEN`) makes it refuse apps that do not present the same token.
- `--print-manifest` prints the tools and options as JSON and exits.

Install it with `cargo install gpux-devtools-mcp` (or run it from a checkout with
`cargo run -p gpux-devtools-mcp --`), and register it with the harness. For Claude Code, in
`.mcp.json`:

```json
{
  "mcpServers": {
    "gpux": { "command": "gpux-devtools-mcp", "args": ["--port", "9230"] }
  }
}
```

The sidecar may start before or after the apps: an app retries with backoff until it connects,
and reconnects when the sidecar restarts. Several apps (and an app's several windows) can be
connected at once.

## Tools

The `gpux_*` tools speak the devtools protocol directly. Every tool that reads or drives a window
takes `app` (an app id, needed only when more than one app is connected) and `window` (a window
id, the app's last window by default).

| Tool | What it does |
|---|---|
| `gpux_list_apps` | The connected apps, their windows (id, title, bounds, scale factor) and their extension tools. |
| `gpux_snapshot` | The window's tree as YAML in Playwright's aria-snapshot shape: `- role "name" [state] [ref=N]: value`, children two spaces deeper. `view` is `inspection` (every node) or `screen-reader` (without generic containers); `ref` starts at a node, `depth` stops below it, `bounds` adds `[bounds=x,y,w,h]`. |
| `gpux_query` | The nodes a semantic query matches (`role=button name="Save"`, `id=...`, `text~=...`), with their ref, role, name, value, states and bounds. |
| `gpux_act` | An AccessKit action on a ref: `click`, `focus`, `blur`, `setValue`, `increment`, `decrement`, `expand`, `collapse`, `scrollIntoView`, `customAction` (`id`), the same requests a screen reader sends. |
| `gpux_input` | Synthesized input through the window's dispatch, as platform input arrives: `click` (`button`, `count`), `tap` (a touch), `hover`, `mouseDown`, `mouseUp`, `mouseMove`, `drag` (to `toRef` or `toX`/`toY`), `longPress` (`durationMs`), `pinch` (`scale`), `wheel` (`deltaX`/`deltaY`), `type` (`text`, clicking `ref` first; `replace` clears the field), `press` (`keys`: `cmd-a backspace`). Positions come from a ref's bounds or `x`/`y`. |
| `gpux_screenshot` | The window, or a ref's bounds, as a PNG in device pixels. |
| `gpux_wait` | Waits up to `timeoutMs` for a query to match, or with `state: "detached"` to stop matching. |
| `gpux_stats` | Frames drawn, the tree's size and, with `diagnostics`, fast mode's counters for the last frame. |

The `browser_*` tools are the Playwright MCP tools agents already know, on the same requests,
answering with Playwright MCP's sections (`### Result`, `### Ran Playwright code`, `### Page`,
`### Snapshot`): `browser_snapshot`, `browser_click`, `browser_hover`, `browser_drag`,
`browser_type`, `browser_press_key`, `browser_mouse_move_xy`, `browser_mouse_down`,
`browser_mouse_up`, `browser_mouse_wheel`, `browser_mouse_click_xy`, `browser_mouse_drag_xy`,
`browser_take_screenshot`, `browser_wait_for`, `browser_tabs` (the connected apps' windows) and
`browser_observe`. Their `ref` is a snapshot's `[ref=N]`. Setting a value is `gpux_act` with
`setValue`, the accessibility action.

### Extension tools

An app adds tools of its own in `Options::extension`:

```rust
Options::new("dev.example.notes", "Notes").extension(Extension::new(
    "notes_metadata",
    "The open note's metadata",
    serde_json::json!({ "type": "object", "properties": {} }),
    |_arguments, cx| Ok(serde_json::json!({ "notes": notes_count(cx) })),
))
```

The app advertises them in `hello`; the sidecar lists them under their own names and forwards a
call to the app, returning its result. When two connected apps advertise the same name, a call
names the app with `app`. A call to an app that disconnected fails naming it.

## Making elements visible

Agents find what the tree holds, so the rules of [accessibility.md](accessibility.md#what-the-tree-holds)
apply: give every control an id and a role, name it by its text or `aria_label`, give values to
what has one (`aria_numeric_value`, `aria_toggled`, `aria_expanded`), and handle the actions it
offers (`on_click` advertises Click; `on_a11y_action` the others). Text without an element id is
a `Label` leaf under its nearest node, so a button labelled by its own text is named by it.

```rust
div()
    .id("save")
    .role(Role::Button)
    .aria_label("Save")
    .on_click(cx.listener(|this, _, _, cx| this.save(cx)))
```

`gpux_snapshot` with `view: "screen-reader"` shows what a screen reader reads; the default view
also shows the generic containers, which inspection keeps so an agent can scope a query to them.

## The protocol

JSON-RPC 2.0 over text frames, version 1 (`gpux_devtools::protocol`). The app's first message is
`hello` (`appId`, `appName`, `protocolVersion`, `token`, `windows`, `tools`), which the sidecar
answers with its own version, or with error -32001 (token) or -32002 (version), after which the
app stops retrying. The app then sends `windowOpened`/`windowClosed` notifications and answers the
sidecar's requests (`snapshot`, `query`, `act`, `input`, `screenshot`, `wait`, `stats`, and its
extension tools by name), keeping the request's id in errors. An answer to a request of an
earlier connection is dropped, never sent on the next.

## Testing

`cargo test -p gpux-devtools` runs the client against an in-test WebSocket server (`connection`,
`snapshot`, `actions`, `observe`), and `cargo test -p gpux-devtools-mcp` the sidecar
(`stdio_protocol`, `extensions`). The `devtools` groups of lanes B, D, E and F connect the
conformance hosts to a sidecar on each platform ([testing.md](testing.md)).
