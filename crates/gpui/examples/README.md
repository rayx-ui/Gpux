# GPUI Examples

Examples can be run from the Zed repository root:

```sh
cargo run -p gpux --example hello_world
```

## Running in a browser

A selection of these examples is also served by the standalone web gallery:

```sh
cd crates/gpui_web/examples/hello_web
trunk serve
```

Open <http://localhost:8080/> to choose an example:

| Path | Example |
| --- | --- |
| `/hello-world` | Hello world |
| `/text` | Styled text |
| `/text-layout` | Alignment and decorations |
| `/text-wrapper` | Wrapping and truncation |
| `/input` | Text input and selection |
| `/dynamic-fonts` | Download a font on missing-glyph notifications |
| `/prime-sieve` | Background task demo |

The server supports direct links and reloads at each path. Only the selected
example's WASM module is loaded. Follow **All examples** to return to the gallery;
navigation reloads the page so the previous app and its workers are released.

Install [Trunk](https://trunk-rs.github.io/trunk/) if needed. The gallery's toolchain file selects
nightly Rust, the WASM target, and `rust-src`. Its Trunk configuration supplies the
cross-origin-isolation headers needed for shared memory and watches the GPUI
example and renderer sources for automatic rebuilds and browser reloads. Use a
browser with WebGPU or WebGL2 support.

The gallery builds separate Cargo binaries directly from the existing example
sources. To add another browser-compatible example, register its binary in the
gallery's `Cargo.toml`, add an auxiliary Rust asset in `index.html`, and add its
gallery link with the corresponding `data-module` name.

### Dynamic font loading

Open `/dynamic-fonts` to see `App::on_missing_glyphs` fetch a versioned
[Noto Sans Devanagari TTF](https://fonts.gstatic.com/s/notosansdevanagari/v30/TuGoUUFzXI5FBtUq5a8bjKYTZjtRU6Sgv3NaV_SNmI0b8QQCQmHn6B2OHjbL_08AlXQly-A.ttf)
from Google Fonts' CDN. The 219 KB font is licensed under the
[SIL Open Font License](https://github.com/google/fonts/blob/main/ofl/notosansdevanagari/OFL.txt).
The CDN permits cross-origin requests and caches the font for one year.
No sample text is sent to the host.

The example bundles only IBM Plex Sans, downloads the fallback once on demand,
calls `add_fonts`, and refreshes the windows to reshape the text. Emoji continues
to use the default Canvas fallback. The page displays loading and failure states;
use browser network throttling or block `fonts.gstatic.com` and reload to
exercise them. Reload to retry a failed request.

This is a small script-to-font mapping, not a general font discovery service.
For production, host versioned font assets on your own CDN and choose an explicit
coverage, caching, and retry policy. Use raw TTF/OTF assets rather than passing
Google Fonts CSS or WOFF2 responses directly to `add_fonts`.

### Canvas font fallback

The web text system uses browser fonts for eligible missing CJK and emoji
graphemes. It renders each complete grapheme independently, retaining the primary
font's baseline and line-spacing metrics. Contextual CJK spacing and arbitrary
OpenType features are not reproduced. Other scripts remain on Cosmic Text.
Browser font availability is assumed stable for the application's lifetime.

Lane B's `text-fallback` browser spec checks native-font preservation,
whole-grapheme rendering, emoji atlas reuse, input/deletion and missing-glyph
reports; run it with `cargo run -p gpux-testkit -- web e2e --grep text-fallback`
(see [docs/testing.md](../../../docs/testing.md)).

## Where to start

- `hello_world` shows the basic shape of a GPUI application: create an
  `Application`, open a window, create a root view, and render a `div`.
- `input` demonstrates text input, focus, selections, clipboard actions, and
  keyboard bindings.
- `uniform_list` shows how to render a simple virtualized list.
- `testing` demonstrates `#[gpux::test]`, `TestAppContext`, actions, focus, and
  window-based tests.

## Layout and styling

- `grid_layout` demonstrates CSS-grid-style layout.
- `opacity` demonstrates opacity styling.
- `pattern` shows patterned backgrounds.
- `shadow` demonstrates box shadows.
- `text` shows styled text rendering.
- `text_layout` demonstrates text alignment, decoration, weights, and wrapping.
- `text_wrapper` shows wrapping text content.

## Frame family

Use the frame-family elements when a child subtree needs a lightweight geometry,
effect, animation, or replacement boundary without becoming a full `div`.

- `frame()` owns lightweight geometry, opacity, transforms, radius, and clipping.
- `effect_frame()` adds ordered shader effects and named screen-effect regions.
- `animated_frame(id)` interpolates frame-owned properties across target changes.
- `transition_frame(id)` captures the old rendered frame region as pixels and
  plays an image-space replacement transition into the new child state.

All frame-family elements are intentionally single-child. Wrap a `div()` inside
the frame when the child needs flex/grid layout, text styling, event handling, or
multiple children.

```rust
use gpux::{frame, px};

frame()
    .w(px(320.))
    .rounded(px(18.))
    .clip()
    .child(content)
```

```rust
use gpux::{animated_frame, ease_in_out, px};
use std::time::Duration;

animated_frame("profile-card")
    .target_key(if expanded { "expanded" } else { "compact" })
    .duration(Duration::from_millis(220))
    .easing(ease_in_out)
    .h(if expanded { px(148.) } else { px(72.) })
    .rounded(if expanded { px(22.) } else { px(12.) })
    .opacity(if expanded { 1.0 } else { 0.86 })
    .child_with_progress(move |progress, _, cx| {
        profile_card_contents(expanded, progress, cx)
    })
```

```rust
use gpux::{FrameTransition, px, transition_frame};

transition_frame("settings-panel")
    .target_key(active_panel.id())
    .transition(FrameTransition::blur_crossfade().duration_ms(260))
    .w(px(520.))
    .rounded(px(18.))
    .clip()
    .child(active_panel.render())
```

`transition_frame()` is an image-space replacement primitive. It does not
automatically diff arbitrary child trees or morph shared elements across
unrelated regions.

## Shader effects

Shader-backed element effects are owned by the lightweight single-child
`effect_frame()` element and are reported through renderer capabilities. WGPU is
the first backend with support; other backends keep the relevant capability
flags false until they implement the shader path.

```rust
use gpux::{backdrop_blur, effect_frame, glass, glow, hsla, px};

effect_frame()
    .effect(backdrop_blur(px(18.)).saturation(1.15))
    .effects((
        glass().blur(px(16.)).refraction(0.04),
        glow(hsla(0.58, 0.75, 0.62, 0.45)).radius(px(24.)),
    ))
    .child("Effects sample")
```

Screen effects are played from a `Window` and can target the full window,
explicit bounds, or a named region painted by an effect frame.

```rust
use gpux::{ScreenEffectTarget, effect_frame, screen};

effect_frame()
    .screen_effect_region("content")
    .child("Region-targeted content");

window.play_screen_effect(
    screen::ripple(click_position)
        .target(ScreenEffectTarget::region("content"))
        .duration_ms(700),
);
```

Snapshot transitions retain an old renderer texture until released. Capture
before mutating app state, play a snapshot transition, then release the snapshot
after the transition duration.

```rust
let snapshot = window.capture_screen_effect_snapshot(ScreenEffectTarget::window());
window.play_screen_effect(
    screen::ripple_reveal(snapshot, click_position)
        .target(ScreenEffectTarget::region("content"))
        .duration_ms(520),
);
// After the transition duration, release the retained texture.
window.release_screen_effect_snapshot(snapshot);
```

The old `effects()` element constructor was replaced by `effect_frame()`. Use
the `.effect(...)` or `.effects(...)` methods on `effect_frame()` for shader
effect declarations.

## Interaction

- `anchor` demonstrates anchored positioning.
- `data_table` combines virtualized list rendering with table-style rows and a
  custom scrollbar.
- `drag_drop` shows draggable elements and drop targets.
- `focus_visible` demonstrates keyboard-visible focus styling.
- `mouse_pressure` demonstrates pressure-sensitive pointer input where supported.
- `popover` shows floating layers with `deferred` and `anchored`.
- `scrollable` demonstrates scrollable content.
- `tab_stop` shows keyboard tab navigation.

## Images, drawing, and animation

- `animation` demonstrates GPUI animations and animated SVG transforms.
- `gif_viewer` shows GIF rendering.
- `gradient` demonstrates linear gradients and color spaces.
- `image` shows local and remote image loading, image sizing, and asset setup.
- `image_gallery` demonstrates image caching and loading remote images.
- `image_loading` shows image loading states and asset loading.
- `painting` demonstrates custom drawing with paths and canvas.
- `svg` shows SVG rendering.

## Windows and application behavior

- `move_entity_between_windows` shows moving an entity between windows.
- `on_window_close_quit` demonstrates quitting when a window closes.
- `set_menus` shows application menu setup.
- `system_notifications` demonstrates posting, replacing, dismissing, and responding to operating-system notifications.
- `window` demonstrates creating normal, dialog, popup, and floating windows.
- `window_positioning` demonstrates window bounds and placement.
- `window_shadow` demonstrates window shadow styling.

## Specialized examples

These examples are useful when working on GPUI itself, but they may not be the
best starting point for new applications:

- `active_state_bug` is a focused active-state reproduction.
- `toggle_headless` starts without a display and preserves its app and todo
  entities while attaching to or detaching from Wayland or X11. Run it on Linux
  with `cargo run -p gpui --example toggle_headless`; its stdin commands are
  `ls`, `create [message]`, `open`, `close`, and `quit`. `open` attaches to the
  display server in the process environment and opens the window; while headless,
  `open DISPLAY=:1` (or `WAYLAND_DISPLAY=… XDG_RUNTIME_DIR=…`) targets another one.
  `close` closes the window and returns to headless mode.
- `layer_shell` demonstrates Linux layer-shell windows.
- `list_example` demonstrates bottom-aligned list state and scrollbar behavior.
- `ownership_post` supports the ownership and data-flow documentation.
- `paths_bench` is a path rendering benchmark.
- `tree` renders a deep tree of nested elements.
