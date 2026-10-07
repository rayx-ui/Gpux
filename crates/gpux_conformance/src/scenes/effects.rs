//! Frame effects: effect frames with shader effects, animated frames and transition frames.

use super::common::*;
use crate::scene::{Area, Capability, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyElement, Context, FrameTransition, IntoElement, Render, Window, animated_frame, div,
    effect_frame, glow, layer_blur, px, rgb, transition_frame,
};
use std::time::Duration;

fn shader_effects() -> AnyElement {
    let block = |name: &'static str| div().id(name).w(px(40.)).h(px(40.)).bg(rgb(RED_HEX));
    // Glow adds light, so it shows on a dark backdrop; on white it saturates away.
    root()
        .bg(rgb(BLACK_HEX))
        .flex()
        .gap(px(30.))
        .p(px(20.))
        .child(effect_frame().child(block("plain")))
        .child(
            effect_frame()
                .effect(layer_blur(px(6.)))
                .child(block("blurred")),
        )
        .child(
            effect_frame()
                .effect(glow(rgb(BLUE_HEX)).radius(px(10.)))
                .child(block("glowing")),
        )
        .into_any_element()
}

pub const SHADER_EFFECTS: SceneSpec = SceneSpec {
    id: "effects-shader",
    areas: &[Area::EffectFrame, Area::ShaderEffects],
    size: (220., 80.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F],
    golden: false,
    build: |_, cx| static_view(cx, shader_effects),
    script: || {
        vec![
            // Effect frames keep their child's layout whatever the renderer supports.
            Step::Expect(Expect::Bounds(id("plain"), (20., 20., 40., 40.), 0.01)),
            Step::Expect(Expect::Bounds(id("blurred"), (90., 20., 40., 40.), 0.01)),
            Step::Expect(Expect::Bounds(id("glowing"), (160., 20., 40., 40.), 0.01)),
            Step::probe(40., 40., RED),
            // Next to the plain block the backdrop is untouched; where the renderer draws
            // element effects, the blurred and glowing ones reach past their edges.
            Step::ProbeUniform((14., 20., 4., 40.), BLACK, 0),
            Step::When(
                Capability::ElementEffects,
                vec![
                    Step::ProbeDiffer((84., 36., 4., 8.), (14., 36., 4., 8.), 2),
                    Step::ProbeDiffer((154., 36., 4., 8.), (14., 36., 4., 8.), 2),
                ],
            ),
        ]
    },
};

/// An animated frame whose width follows a toggle, reporting its eased progress.
struct Animated {
    wide: bool,
}

impl Render for Animated {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        root()
            .child(
                div()
                    .id("toggle")
                    .absolute()
                    .top(px(0.))
                    .right(px(0.))
                    .w(px(30.))
                    .h(px(20.))
                    .bg(rgb(GREEN_HEX))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.wide = !this.wide;
                        cx.notify();
                    })),
            )
            .child(
                div().absolute().top(px(30.)).left(px(10.)).child(
                    animated_frame("bar")
                        .duration(Duration::from_millis(200))
                        .easing(|delta| delta)
                        .w(px(if self.wide { 200. } else { 100. }))
                        .h(px(20.))
                        .child_with_progress(|progress, _, _| {
                            div()
                                .size_full()
                                .bg(rgb(BLUE_HEX))
                                .child(status(format!("progress {progress:.2}")))
                        }),
                ),
            )
    }
}

pub const ANIMATED_FRAME: SceneSpec = SceneSpec {
    id: "effects-animated-frame",
    areas: &[Area::AnimatedFrame],
    size: (240., 70.),
    scales: &[1., 2.],
    // Exact animation progress needs the executor clock; lane D runs on the real clock.
    lanes: &[Lane::A, Lane::B, Lane::C],
    golden: false,
    build: |_, cx| cx.new(|_| Animated { wide: false }).into(),
    script: || {
        vec![
            Step::Expect(Expect::Exists(status_is("progress 1.00"))),
            Step::Click(id("toggle")),
            Step::Advance(Duration::from_millis(100)),
            Step::Expect(Expect::Exists(status_is("progress 0.50"))),
            Step::Advance(Duration::from_millis(100)),
            Step::Expect(Expect::Exists(status_is("progress 1.00"))),
            Step::probe(200., 40., BLUE),
        ]
    },
};

/// A transition frame that swaps its content when its key changes.
struct Transition {
    page: usize,
}

impl Render for Transition {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page;
        root()
            .child(
                div()
                    .id("next")
                    .absolute()
                    .top(px(0.))
                    .right(px(0.))
                    .w(px(30.))
                    .h(px(20.))
                    .bg(rgb(GREEN_HEX))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page += 1;
                        cx.notify();
                    })),
            )
            .child(
                div().absolute().top(px(30.)).left(px(10.)).child(
                    transition_frame("pages")
                        .target_key(format!("page-{page}"))
                        .transition(FrameTransition::crossfade().duration_ms(200))
                        .w(px(120.))
                        .h(px(40.))
                        .child(
                            div()
                                .size_full()
                                .bg(rgb(if page.is_multiple_of(2) {
                                    RED_HEX
                                } else {
                                    BLUE_HEX
                                }))
                                .child(status(format!("page {page}"))),
                        ),
                ),
            )
    }
}

pub const TRANSITION_FRAME: SceneSpec = SceneSpec {
    id: "effects-transition-frame",
    areas: &[Area::TransitionFrame],
    size: (180., 80.),
    scales: &[1., 2.],
    // Exact animation progress needs the executor clock; lane D runs on the real clock.
    lanes: &[Lane::A, Lane::B, Lane::C],
    golden: false,
    build: |_, cx| cx.new(|_| Transition { page: 0 }).into(),
    script: || {
        vec![
            Step::Expect(Expect::Exists(status_is("page 0"))),
            Step::probe(70., 50., RED),
            Step::Capture("old"),
            Step::Click(id("next")),
            Step::Expect(Expect::Exists(status_is("page 1"))),
            // Halfway through the 200 ms crossfade the frame is neither the old page...
            Step::Advance(Duration::from_millis(100)),
            Step::ChangedSince("old", (60., 40., 20., 20.)),
            Step::When(Capability::SnapshotTransitions, vec![Step::Capture("mid")]),
            // ...nor the new one: once the transition has finished, only the new page shows.
            // Renderers without snapshot transitions swap the pages at once.
            Step::Advance(Duration::from_millis(150)),
            Step::Advance(Duration::from_millis(50)),
            Step::probe(70., 50., BLUE),
            Step::When(
                Capability::SnapshotTransitions,
                vec![Step::ChangedSince("mid", (60., 40., 20., 20.))],
            ),
        ]
    },
};
