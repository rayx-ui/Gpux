# GPUI Examples

Examples can be run from the Zed repository root:

```sh
cargo run -p gpux --example hello_world
```

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

## Shader effects

Shader-backed effects are ordinary `Styled`/`div()` styling and are reported
through renderer capabilities. WGPU is the first backend with support; other
backends keep the relevant capability flags false until they implement the
shader path.

```rust
use gpux::prelude::*;

div()
    .effect(backdrop_blur(px(18.)).saturation(1.15))
    .effects((
        glass().blur(px(16.)).refraction(0.04),
        glow(hsla(0.58, 0.75, 0.62, 0.45)).radius(px(24.)),
    ))
    .child("Effects sample")
```

Screen effects are played from a `Window` and can target the full window,
explicit bounds, or a named region painted by a styled element.

```rust
div()
    .id("content")
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
- `window` demonstrates creating normal, dialog, popup, and floating windows.
- `window_positioning` demonstrates window bounds and placement.
- `window_shadow` demonstrates window shadow styling.

## Specialized examples

These examples are useful when working on GPUI itself, but they may not be the
best starting point for new applications:

- `active_state_bug` is a focused active-state reproduction.
- `layer_shell` demonstrates Linux layer-shell windows.
- `list_example` demonstrates bottom-aligned list state and scrollbar behavior.
- `ownership_post` supports the ownership and data-flow documentation.
- `paths_bench` is a path rendering benchmark.
- `tree` renders a deep tree of nested elements.
