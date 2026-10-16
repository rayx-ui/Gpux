//! Shared helpers for scene definitions.

use gpux::prelude::*;
use gpux::{
    AnyElement, AnyView, App, Context, IntoElement, Render, Role, SharedString, Window, div,
    semantic_query::Query,
};

/// Probe colors in encoded sRGB.
pub const WHITE: [u8; 4] = [255, 255, 255, 255];
pub const BLACK: [u8; 4] = [0, 0, 0, 255];
pub const RED: [u8; 4] = [255, 0, 0, 255];
pub const GREEN: [u8; 4] = [0, 255, 0, 255];
pub const BLUE: [u8; 4] = [0, 0, 255, 255];
pub const GRAY: [u8; 4] = [128, 128, 128, 255];

/// Paint colors matching the probe colors.
pub const WHITE_HEX: u32 = 0xffffff;
pub const BLACK_HEX: u32 = 0x000000;
pub const RED_HEX: u32 = 0xff0000;
pub const GREEN_HEX: u32 = 0x00ff00;
pub const BLUE_HEX: u32 = 0x0000ff;
pub const GRAY_HEX: u32 = 0x808080;

/// A view that renders the same element tree every frame.
pub struct Static(pub fn() -> AnyElement);

impl Render for Static {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        (self.0)()
    }
}

/// The root view of a static scene.
pub fn static_view(cx: &mut App, render: fn() -> AnyElement) -> AnyView {
    cx.new(|_| Static(render)).into()
}

/// A white root filling the window.
pub fn root() -> gpux::Div {
    div()
        .size_full()
        .relative()
        .bg(gpux::rgb(WHITE_HEX))
        .text_color(gpux::rgb(BLACK_HEX))
        .font_family("IBM Plex Sans")
        .text_size(gpux::px(16.))
}

/// A status line exposing scene state for hosts to query: a `Role::Status` node whose value is
/// the text it shows.
pub fn status(text: impl Into<SharedString>) -> AnyElement {
    let text = text.into();
    div()
        .id("status")
        .role(Role::Status)
        .aria_value(text.clone())
        .child(text)
        .into_any_element()
}

/// The status line reading exactly `value`.
pub fn status_is(value: impl Into<String>) -> Query {
    Query::role("status").with_value(value)
}

/// The element with this id suffix.
pub fn id(id: &str) -> Query {
    Query::id(id)
}
