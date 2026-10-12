//! The compiled defaults: every mechanism is on in a window the environment
//! says nothing about, wherever the target compiles it, and the variables
//! turn each one off. Selected on its own with
//! `cargo test -p gpux --features test-support --lib fast::tests::defaults::`.

use std::collections::HashMap;

use crate::{AppContext as _, FastMode};

/// The mode the real compiled defaults and the environment `vars` give a
/// window.
fn from(vars: &[(&str, &str)]) -> FastMode {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    FastMode::from_environment(|name| vars.get(name).cloned())
}

/// What a window on this target starts in with nothing set: everything,
/// less scroll layers where the target does not compile them.
fn everything() -> FastMode {
    FastMode::ALL.normalized()
}

#[test]
fn every_compiled_default_is_on() {
    assert!(super::super::mode::DEFAULT_LAYOUT_RETENTION);
    assert!(super::super::mode::DEFAULT_VIEW_RETENTION);
    assert!(super::super::mode::DEFAULT_SPLICE);
    assert!(super::super::mode::DEFAULT_SCROLL_LAYERS);
    assert_eq!(FastMode::compiled_default(), everything());
    assert_eq!(from(&[]), everything());
    assert!(!FastMode::compiled_default().is_off());
    assert_eq!(
        FastMode::compiled_default().scroll_layers,
        crate::fast::layers::COMPILED
    );
}

/// A new window draws with fast mode on unless its renderer cannot rasterize
/// layer tiles, which keeps scroll layers off and the rest on.
#[crate::test]
fn a_new_window_starts_with_every_mechanism_on(cx: &mut crate::TestAppContext) {
    let window = cx.add_empty_window();
    let mode = window.update(|window, _| window.fast_mode());
    assert!(mode.layout_retention && mode.view_retention && mode.splice);
    assert_eq!(mode, everything());
}

#[test]
fn the_master_switch_turns_everything_off() {
    assert_eq!(from(&[("GPUX_FAST_MODE", "0")]), FastMode::OFF);
    assert_eq!(
        from(&[("GPUX_FAST_MODE", "0"), ("GPUX_SCROLL_LAYERS", "1")]),
        FastMode::OFF
    );
    assert_eq!(from(&[("GPUX_FAST_MODE", "1")]), everything());
}

#[test]
fn each_variable_turns_its_mechanism_off() {
    let all = everything();
    assert_eq!(
        from(&[("GPUX_SCROLL_LAYERS", "0")]),
        FastMode {
            scroll_layers: false,
            ..all
        }
    );
    // Splice and layers need view retention, which needs layout retention.
    assert_eq!(
        from(&[("GPUX_VIEW_RETENTION", "0")]),
        FastMode {
            layout_retention: true,
            ..FastMode::OFF
        }
    );
    assert_eq!(from(&[("GPUX_LAYOUT_RETENTION", "0")]), FastMode::OFF);
}

/// gpui-fast's names keep working where the gpux names are unset: `0`
/// turns the mechanism off, as they did there.
#[test]
fn gpui_fast_aliases_still_turn_mechanisms_off() {
    let all = everything();
    assert_eq!(
        from(&[("GPUI_SCROLL_LAYERS", "0")]),
        FastMode {
            scroll_layers: false,
            ..all
        }
    );
    assert_eq!(
        from(&[("GPUI_VIEW_RETENTION", "0")]),
        FastMode {
            layout_retention: true,
            ..FastMode::OFF
        }
    );
    // The gpux name wins over its alias.
    assert_eq!(
        from(&[("GPUX_SCROLL_LAYERS", "1"), ("GPUI_SCROLL_LAYERS", "0")]),
        all
    );
}

/// Composition stays opt-in: it is not one of the switches the defaults turn
/// on, and a window enables it only by asking.
#[crate::test]
fn composition_stays_opt_in(cx: &mut crate::TestAppContext) {
    let window = cx.add_empty_window();
    window.update(|window, _| {
        assert!(!window.fast_composition.is_enabled());
    });
}

struct Text;

impl crate::Render for Text {
    fn render(
        &mut self,
        _: &mut crate::Window,
        _: &mut crate::Context<Self>,
    ) -> impl crate::IntoElement {
        use crate::prelude::*;
        crate::div().size_full().child("some text")
    }
}

/// A scene retention replays keeps the kind of sprite text was painted as, so
/// a window follows a change of what decides it by drawing from scratch.
#[test]
fn a_change_of_the_text_policy_draws_the_next_frame_from_scratch() {
    use crate::TextRenderingMode;
    // The default test application paints no glyphs.
    let cx = &mut super::oracle_support::oracle_app(0);
    cx.update(|cx| cx.set_text_rendering_mode(TextRenderingMode::Subpixel));
    let window = cx.add_window(|_, _| Text);
    cx.set_draw_on_flush(false);
    let handle: crate::AnyWindowHandle = window.into();
    cx.test_window(handle).0.lock().subpixel_rendering_supported = true;
    let sprites = |cx: &mut crate::TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.draw(cx).clear(cx);
            let scene = &window.rendered_frame.scene;
            (scene.subpixel_sprites.len(), scene.monochrome_sprites.len())
        })
        .unwrap()
    };
    // The first frame was drawn before the window could subpixel render.
    let (subpixel, monochrome) = sprites(cx);
    assert!(subpixel > 0 && monochrome == 0, "{subpixel} {monochrome}");
    assert_eq!(
        sprites(cx),
        (subpixel, 0),
        "nothing changed, nothing drawn differently"
    );

    cx.update(|cx| cx.set_text_rendering_mode(TextRenderingMode::Grayscale));
    assert_eq!(sprites(cx), (0, subpixel));
    cx.update(|cx| cx.set_text_rendering_mode(TextRenderingMode::Subpixel));
    assert_eq!(sprites(cx), (subpixel, 0));
    cx.test_window(handle).0.lock().subpixel_rendering_supported = false;
    assert_eq!(sprites(cx), (0, subpixel));
}
