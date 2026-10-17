//! ARIA's defaults for what an element leaves unset: a slider without an orientation is
//! horizontal (VoiceOver calls a slider without one a circular slider), and an orientation the
//! element sets is kept.
//!
//! `cargo test -p gpux --test a11y_aria_defaults`

use accesskit::{Orientation, Role as AkRole};
use gpux::{
    Context, IntoElement, Render, Role, TestAppContext, Window, WindowHandle, div, prelude::*, px,
};

struct Sliders;

impl Render for Sliders {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("root")
            .size_full()
            .child(
                div()
                    .id("plain")
                    .role(Role::Slider)
                    .aria_label("Plain")
                    .aria_numeric_value(5.)
                    .w(px(100.))
                    .h(px(20.)),
            )
            .child(
                div()
                    .id("vertical")
                    .role(Role::Slider)
                    .aria_label("Vertical")
                    .aria_orientation(Orientation::Vertical)
                    .aria_numeric_value(5.)
                    .w(px(20.))
                    .h(px(100.)),
            )
    }
}

fn orientations(cx: &mut TestAppContext) -> Vec<(String, Option<Orientation>)> {
    let window: WindowHandle<Sliders> = cx.add_window(|_, _| Sliders);
    let _guard = cx
        .update_window(window.into(), |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        let tree = window.a11y_tree().expect("an inspected tree");
        tree.nodes
            .iter()
            .filter(|(_, node)| node.role() == AkRole::Slider)
            .map(|(_, node)| {
                (
                    node.label().unwrap_or_default().to_string(),
                    node.orientation(),
                )
            })
            .collect()
    })
    .unwrap()
}

#[gpux::test]
fn a_slider_without_an_orientation_is_horizontal(cx: &mut TestAppContext) {
    let sliders = orientations(cx);
    assert!(
        sliders.contains(&("Plain".to_string(), Some(Orientation::Horizontal))),
        "{sliders:?}"
    );
}

#[gpux::test]
fn a_slider_keeps_the_orientation_it_sets(cx: &mut TestAppContext) {
    let sliders = orientations(cx);
    assert!(
        sliders.contains(&("Vertical".to_string(), Some(Orientation::Vertical))),
        "{sliders:?}"
    );
}
