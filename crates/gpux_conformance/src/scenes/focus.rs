//! Focus and actions: tab order, actions with layered keymaps, hover and active styles.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyView, App, Context, FocusHandle, InputLayerId, IntoElement, KeyBinding, KeymapLayer, Render,
    Window, actions, div, px, rgb,
};

actions!(conformance, [FocusNext, FocusPrevious, Increment, Reset]);

/// Three focusable fields in tab order 1, 3, 2.
struct TabOrder {
    handles: Vec<FocusHandle>,
}

impl Render for TabOrder {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        root()
            .p(px(10.))
            .flex()
            .gap(px(10.))
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
            .children(self.handles.iter().zip(["first", "third", "second"]).map(
                |(handle, name)| {
                    div()
                        .id(name)
                        .track_focus(handle)
                        .w(px(50.))
                        .h(px(30.))
                        .bg(rgb(GRAY_HEX))
                        .focus(|style| style.bg(rgb(GREEN_HEX)))
                },
            ))
    }
}

fn tab_order(_: &mut Window, cx: &mut App) -> AnyView {
    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
    ]);
    cx.new(|cx| TabOrder {
        handles: [1, 3, 2]
            .into_iter()
            .map(|index| cx.focus_handle().tab_index(index).tab_stop(true))
            .collect(),
    })
    .into()
}

pub const TAB_ORDER: SceneSpec = SceneSpec {
    id: "focus-tab-order",
    areas: &[Area::FocusOrder],
    size: (220., 60.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D],
    golden: false,
    build: tab_order,
    script: || {
        vec![
            Step::Focus(id("first")),
            Step::Expect(Expect::Focused(id("first"))),
            Step::probe(35., 25., GREEN),
            Step::Keys("tab".into()),
            Step::Expect(Expect::Focused(id("second"))),
            Step::Keys("tab".into()),
            Step::Expect(Expect::Focused(id("third"))),
            Step::Keys("shift-tab".into()),
            Step::Expect(Expect::Focused(id("second"))),
            Step::probe(35., 25., GRAY),
            Step::probe(155., 25., GREEN),
        ]
    },
};

/// A counter driven by actions from a base binding and an app-layer override, next to a
/// focusable field outside its key context.
struct Counter {
    focus: FocusHandle,
    outside: FocusHandle,
    count: usize,
}

impl Render for Counter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        root()
            .child(status(format!("count {}", self.count)))
            .child(
                div()
                    .id("outside")
                    .track_focus(&self.outside)
                    .absolute()
                    .top(px(30.))
                    .left(px(120.))
                    .w(px(60.))
                    .h(px(40.))
                    .bg(rgb(GREEN_HEX)),
            )
            .child(
                div()
                    .id("counter")
                    .key_context("Counter")
                    .track_focus(&self.focus)
                    .w(px(100.))
                    .h(px(40.))
                    .bg(rgb(GRAY_HEX))
                    .on_action(cx.listener(|this, _: &Increment, _, cx| {
                        this.count += 1;
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &Reset, _, cx| {
                        this.count = 0;
                        cx.notify();
                    })),
            )
    }
}

fn counter(_: &mut Window, cx: &mut App) -> AnyView {
    cx.bind_keys([
        KeyBinding::new("ctrl-i", Increment, Some("Counter")),
        KeyBinding::new("ctrl-r", Reset, Some("Counter")),
    ]);
    // A higher-priority layer adds a second binding for the same action.
    cx.set_keymap_layer(KeymapLayer::new(
        InputLayerId::new("conformance-override"),
        10,
        [KeyBinding::new("alt-up", Increment, Some("Counter"))],
    ))
    .expect("the layer id is unique");
    cx.new(|cx| Counter {
        focus: cx.focus_handle(),
        outside: cx.focus_handle(),
        count: 0,
    })
    .into()
}

pub const ACTIONS: SceneSpec = SceneSpec {
    id: "actions-keymaps",
    areas: &[Area::Actions, Area::LayeredKeymaps],
    size: (200., 80.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D],
    golden: false,
    build: counter,
    script: || {
        vec![
            Step::probe(50., 50., GRAY),
            Step::probe(150., 50., GREEN),
            Step::Capture("zero"),
            Step::Focus(id("counter")),
            Step::Keys("ctrl-i ctrl-i".into()),
            Step::Expect(Expect::Exists(status_is("count 2"))),
            Step::ChangedSince("zero", (0., 0., 100., 26.)),
            Step::Keys("alt-up".into()),
            Step::Expect(Expect::Exists(status_is("count 3"))),
            Step::Keys("ctrl-r".into()),
            Step::Expect(Expect::Exists(status_is("count 0"))),
            // Keys bound in the context do nothing outside it.
            Step::Focus(id("outside")),
            Step::Keys("ctrl-i alt-up".into()),
            Step::Expect(Expect::Exists(status_is("count 0"))),
        ]
    },
};

/// A target with hover and active styles that also reports its state.
struct Hover {
    hovered: bool,
    clicks: usize,
}

impl Render for Hover {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        root()
            .child(status(format!(
                "hovered {} clicks {}",
                self.hovered, self.clicks
            )))
            .child(
                div()
                    .id("target")
                    .absolute()
                    .top(px(30.))
                    .left(px(20.))
                    .w(px(80.))
                    .h(px(40.))
                    .bg(rgb(GRAY_HEX))
                    .hover(|style| style.bg(rgb(GREEN_HEX)))
                    .active(|style| style.bg(rgb(BLUE_HEX)))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.hovered = *hovered;
                        cx.notify();
                    }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        cx.notify();
                    })),
            )
    }
}

pub const HOVER_ACTIVE: SceneSpec = SceneSpec {
    id: "hover-active",
    areas: &[Area::HoverActive],
    size: (160., 100.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D],
    golden: false,
    build: |_, cx| {
        cx.new(|_| Hover {
            hovered: false,
            clicks: 0,
        })
        .into()
    },
    script: || {
        vec![
            Step::probe(60., 50., GRAY),
            Step::Hover(id("target")),
            Step::Expect(Expect::Exists(status_is("hovered true clicks 0"))),
            Step::probe(60., 50., GREEN),
            Step::Click(id("target")),
            Step::Expect(Expect::Exists(status_is("hovered true clicks 1"))),
            Step::MoveTo(150., 90.),
            Step::Expect(Expect::Exists(status_is("hovered false clicks 1"))),
            Step::probe(60., 50., GRAY),
        ]
    },
};
