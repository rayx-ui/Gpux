# Accessibility

gpux has one semantic tree: [AccessKit](https://accesskit.dev)'s. Screen readers read it through
each platform's adapter, tests query it (`gpux::semantic_query`), and agents inspect it through
the devtools. A window builds the tree only while something asks for it, and fast mode carries
it through every frame it draws from the last one, so an app that nobody inspects and no screen
reader reads does no accessibility work at all.

gpux tracks AccessKit's 0.25 family ahead of Zed (`accesskit` 0.25.1, `accesskit_consumer`
0.39.1, `accesskit_macos` 0.27.1, `accesskit_windows` 0.35.1, `accesskit_unix` 0.24.0,
`accesskit_ios` 0.2.1, `accesskit_android` 0.9.0); [upstream/accesskit-0.25.md](upstream/accesskit-0.25.md)
records the delta.

## Switches

| When | Switch | Effect |
|---|---|---|
| Build | `accessibility` feature of `gpux-platform` (default) and of each platform crate (`gpux-macos`, `gpux-windows`, `gpux-linux`, `gpux-web`, `gpux-mobile`) | Links the platform's screen-reader adapter: `accesskit_macos`, `accesskit_windows`, `accesskit_unix`, the web DOM mirror, `accesskit_ios`, `accesskit_android`. Without it no adapter is linked and no window turns the platform source on; inspection and `AccessibilityMode::Always` still build the tree. |
| Build | `semantic-query` feature of `gpux` (on with `test-support`) | The query engine over the tree (`gpux::semantic_query`, `accesskit_consumer`). |
| Run | `AccessibilityMode` (`Auto`, `Always`, `Off`) | Set by `Application::with_accessibility_mode`, changed at any time by `App::set_accessibility_mode`, read once from `GPUX_ACCESSIBILITY` (`auto`, `always`, `off`) when the `App` is created; the builder and later calls win over the variable, and an unknown value is logged and read as `auto`. |
| Run | `Application::new_inaccessible` | The hard off: no source is ever accepted, no tree is built, and an inspection guard reports `accessibility_disabled()`. |
| Run (web) | `WebScreenReader` (`OnRequest`, `Always`) and its label | `gpui_platform::set_web_screen_reader` / `set_web_screen_reader_label` (re-exported from `gpui_web`), before a window opens: see [The web](#the-web). |

`AccessibilityMode`:

- **`Auto`** (the default): the tree follows the platform's assistive technology and inspection.
- **`Always`**: every window builds its tree on every frame, with no assistive technology. The
  adapter still receives nothing until it activates.
- **`Off`**: the platform source is refused. A screen reader that was reading the window gets one
  last tree holding only the window's root, then nothing; switching back to `Auto` or `Always`
  sends it a full tree again. Inspection keeps working, so hiding an app from screen readers
  never blinds its tests or agents.

## Activation sources

A window builds its tree for a frame when at least one source is on:

- **The platform source**: the screen-reader adapter activated it (VoiceOver, Narrator/NVDA, Orca,
  TalkBack, or the web mirror's enable button).
- **The inspection source**: `Window::begin_a11y_inspection()` returns an `A11yInspection` guard;
  while one lives the window builds its tree. The first guard forces a full redraw so the next
  tree is complete; dropping the last one requests a frame so the tree stops. `Window::a11y_tree()`
  returns the last frame's `TreeUpdate`, without a second copy.
- **`AccessibilityMode::Always`**.

Tree updates reach the adapter (`PlatformWindow::a11y_tree_update`) only for frames the platform
source covered from start to end: a frame built for inspection alone sends nothing, and inspection
never makes the operating system think assistive technology is running.

## What the tree holds

- **Nodes for elements** with an id and a role (`.id(..).role(Role::Button)`), carrying their
  properties: label, description, value, numeric value and range, toggled, expanded, selected,
  disabled, required, read-only, invalid, placeholder, orientation, level, list positions, table
  indices, live region, bounds.
- **Text leaves**: text drawn without an element id (`"Save"`, `SharedString`, `StyledText`)
  becomes a `Label` leaf under the nearest node, so a button labelled only by its text gets its
  name from it as screen readers compute names. A leaf's id derives from its owner node and its
  position among the owner's text, so it is stable across frames (and with inspection on or off)
  while the order of the text does not change. `Text::new_inaccessible` opts out.
- **Inspection containers and the id map**: while inspection is on, elements with an id and no
  role become `GenericContainer` nodes, so tests and agents reach their subtrees, and the window
  maps every node to its `GlobalElementId` (`Window::a11y_element_id(s)`). Screen readers see the
  same tree with or without them (AccessKit's filters skip generic containers), and containers
  never take the tree's focus.
- **Relations** by `accessibility_id`: `aria_labelled_by`, `aria_described_by`, `aria_controls`
  and `aria_error_message` name other elements' `.accessibility_id(..)` and resolve, in the order
  given, to their node ids at the end of the frame; an id no element carries is dropped.
- **Actions** each node advertises exactly as it handles them: `Click` (a click listener),
  `Focus` (a tracked focus handle), any `on_a11y_action(Action::..)` handler, custom actions
  (`on_a11y_custom_action(id, description, ..)`), and `ScrollIntoView`, which gpux performs
  itself by moving every scroll container around the node as a scroll does.
  `Window::perform_a11y_action(request)` runs a request as an adapter's would and returns whether
  a handler ran.
- **Focus**: the focused element's node, or the node a focused container reports through
  `aria_active_descendant`.
- **Masked text**: an `EditableText` exposes the text it shows; one masked with
  `EditableTextDisplayTransform::per_grapheme(..)` is a `PasswordInput` whose value is the mask.

## Annotating elements

```rust
div()
    .id("save")
    .role(Role::Button)          // a node, with Click advertised by its listener
    .on_click(save)
    .child("Save");              // its name, from the text leaf

div()
    .id("volume")
    .role(Role::Slider)
    .aria_label("Volume")
    .aria_numeric_value(volume)
    .aria_min_numeric_value(0.)
    .aria_max_numeric_value(10.)
    .track_focus(&focus)         // focusable, so screen readers move to it
    .on_a11y_action(AccessibleAction::Increment, raise)
    .on_a11y_action(AccessibleAction::Decrement, lower);

div()
    .id("email")
    .role(Role::TextInput)
    .aria_labelled_by("email-label")   // another element's accessibility_id
    .aria_described_by("email-hint")
    .aria_required(true);

status_line.role(Role::Status).aria_live(Live::Polite);
```

Give every control a role; name it by its text or `aria_label`; make what the keyboard reaches
focusable; prefer AccessKit actions to pointer-only handlers so screen readers and agents can
operate it.

## Fast mode

Fast mode records each frame's tree as an event log that cached view replay, splice, scroll
layers (with the scroll offset applied to carried bounds) and list rows replay, so no view or
layer is bypassed while accessibility is on, and the oracle compares the tree, the focus, the
focusable nodes, the action handlers and where actions reach, frame by frame, with a window drawn
from scratch. See [fast-mode.md](fast-mode.md).

## Queries

`gpux::semantic_query` resolves queries over the AccessKit tree (`accesskit_consumer`): `role=`
(one table, [`gpux::aria_role`]; text is `text`, generic containers `generic`), `name=` (the name
screen readers compute), `text=`, `value=`, `id=` (an element id segment or an `accessibility_id`),
`checked=`, `expanded=`, `selected=`, `disabled=`, `focused=`, `nth=`, `>>` scopes and
`view=screen-reader` (the tree as screen readers walk it). Bounds are in window pixels. A
locator's first use starts inspection. `type=` is gone: the tree has no element types.

## Platforms

### macOS, Windows, Linux

`accesskit_macos` (`NSAccessibility`, VoiceOver), `accesskit_windows` (UI Automation: Narrator,
NVDA, JAWS) and `accesskit_unix` (AT-SPI: Orca) subclass or attach to each window and activate
the platform source when assistive technology connects.

### The web

AccessKit has no web adapter, and a canvas is one opaque element to a screen reader, so
`gpui_web` mirrors the tree into the DOM, beside the canvas:

- **Activation.** Browsers do not tell a page that a screen reader runs, so each window puts a
  visually hidden, focusable **"Enable accessibility"** button first in its DOM. Activating it
  (Enter, Space, or a screen reader's activation) turns the window's platform source on and builds
  the mirror from a full tree; until then there are no mirror elements and no tree is built for
  it. `set_web_screen_reader(WebScreenReader::Always)` builds the mirror from the first frame with
  no button; `set_web_screen_reader_label(..)` localizes the button.
- **What it exposes.** One element per node that is not a generic container, with its ARIA role
  (the same table as queries), `aria-label`, `aria-description`, value and range
  (`aria-valuenow`/`min`/`max`/`valuetext`), states (`aria-checked`/`pressed`, `expanded`,
  `selected`, `disabled`, `required`, `readonly`, `invalid`), relations over DOM ids
  (`aria-labelledby`, `describedby`, `controls`, `errormessage`, `activedescendant`), list and table
  positions, `aria-live`, and a transparent box at the node's bounds that never takes the pointer.
  Updates go through `accesskit_consumer`'s change handler: an update touches only the elements of
  the nodes it changed.
- **Operation.** A DOM `click` becomes `Click`; DOM focus becomes `Focus` (and `ScrollIntoView`
  for a node a container has scrolled out of view); the ARIA keys of sliders, spin buttons and
  disclosures become `Increment`/`Decrement`/`Expand`/`Collapse`; other keys reach GPUI as from
  the canvas. When GPUI's focus moves, the mirror moves DOM focus to the focused node's element
  without sending a focus change back; a focused text field is the IME textarea (labelled with the
  field's name), so typing and IMEs keep their one path.
- **`html_element` overlays.** A node whose `accessibility_id` equals an overlay's id owns the
  overlay's DOM by `aria-owns`, so it is read where the node is.

### iOS

`gpui_mobile` puts an `accesskit_ios` `SubclassingAdapter` on each window's Metal view
(`accessibilityElements`, hit testing, `didMoveToWindow`), updated on the main thread; VoiceOver
activates it. Every window has its own view and adapter, and the adapter is dropped before the
view.

### Android

`gpui_mobile` puts an `accesskit_android` `InjectingAdapter` (with its embedded `.dex`) on the
view showing the GPUI surface: the view an embedding host declared with
`dev.gpux.mobile.Composition.setHostView`, else the first `SurfaceView` under the Activity's
content view, else (a NativeActivity) the decor view, whose window surface GPUI draws. A view that
already has an accessibility delegate (the app's own) keeps it: gpux logs a warning and exposes
nothing there. TalkBack, Switch Access and `uiautomator` read the adapter's virtual node tree;
clicks, focus and slider adjustment (scroll forward and backward) come back as actions. The
adapter posts its Java work to the UI thread itself.

All of an app's GPUI windows show on the Activity's one surface, and a view takes one delegate,
so the windows share one adapter, which describes the **front (last opened) window**; a window
behind it is exposed again when the front one closes. The adapter belongs to its Activity: a
window opened under a recreated Activity gets a new adapter on the new view, and the adapter goes
with the last window, or when an embedded host clears its context.

## Known limits

- Android exposes only the front GPUI window to TalkBack (see [Android](#android)); iOS exposes
  each window through its own view.

- The web mirror offers no text navigation inside a text leaf beyond its content, and a browser
  exposes it only after the enable button (or `WebScreenReader::Always`).
- Synthetic children (`a11y_synthetic_children`) are the way to expose text runs and caret
  positions of custom text elements; gpux's own text elements expose their text as one leaf.
- `accesskit_ios` 0.2 and `accesskit_android` 0.9 cover the roles, names, values, states and
  actions above; their newer features (rotor entries, text ranges) follow AccessKit's releases.

## Testing

- Lane A (test platform): the `crates/gpui/tests/a11y_*` groups, `semantic_query_accesskit`, the
  conformance `accessibility` scene.
- Lane B (Chromium): `tools-node/specs/accessibility.spec.ts` (inspection) and
  `screen-reader.spec.ts` (the mirror).
- Lane E (Android): the host's `screen-reader-android-*` checks (`uiautomator dump`, actions
  through the adapter's node provider).
- Lane F (iOS Simulator): the host's `screen-reader-ios-*` checks (`accessibilityElements`,
  `accessibilityActivate`).
- Lane C: `lane_c_a11y_layers` (the tree through scroll layers).

## Screen-reader walkthrough

Assistive technology itself is verified by hand. On each platform, open the conformance
`accessibility` scene and, with the screen reader on, move through it: the status line, the
**Activate** button, the **Subscribe** check box, the **Volume** slider, the **Title** text field,
the **Email** field (labelled and described by the text beside it, required) and the **Fruits**
list. For each, note the role, name and state announced; then activate the button (the status
line's click count rises and is announced), toggle the check box, adjust the slider and type into
the Title field. Record what each screen reader announced, and anything missing or wrong, in
[measurements/screen-reader-walkthrough.md](measurements/screen-reader-walkthrough.md).

- **macOS desktop (VoiceOver, Cmd-F5):**

  ```bash
  cargo run -p gpux-conformance --no-default-features --features app-host --example desktop_host -- --show accessibility
  ```

- **Web (VoiceOver in Safari and in Chrome):** `gpux-testkit web build`, then
  `gpux-testkit web serve`, and open `http://localhost:8000/?scene=accessibility&inspect=0`. Tab to
  the "Enable accessibility" button and activate it, then move through the scene.
- **iOS (VoiceOver on the Simulator or a device):** `gpux-testkit ios build --simulator`, then
  install `target/gpux-ios/products/Debug-iphonesimulator/GpuxConformance.app` on a booted
  Simulator (`xcrun simctl install booted <app>`) and launch it showing the scene with
  `xcrun simctl launch booted dev.gpux.conformance --show accessibility`; turn VoiceOver on in
  Settings → Accessibility (on the Simulator, Xcode's Accessibility Inspector reads the same tree).
- **Android (TalkBack on an emulator or device):** `gpux-testkit android build --abi arm64-v8a`
  (or `x86_64`), install `target/gpux-android/gpux-conformance.apk` (`adb install -r`),
  `adb shell setprop debug.gpux.show accessibility`, launch the host
  (`adb shell monkey -p dev.gpux.conformance 1`), and turn TalkBack on in Settings →
  Accessibility. `adb shell setprop debug.gpux.show ""` returns the host to its test runs.
- **Windows (Narrator or NVDA) and Linux (Orca)**, where available: the desktop command above.
