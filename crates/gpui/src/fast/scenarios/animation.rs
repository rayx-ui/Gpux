//! An animated frame, a negative control: an animation that never stops
//! changes every frame, so no mechanism can save its work.

use std::time::Duration;

use crate::{
    AnyView, App, Context, Entity, FastStats, IntoElement, Render, SharedString, Window,
    animated_frame, div, hsla, prelude::*, px,
};

use super::{Fixture, Mode, Scenario, at_least_per_frame, drawn_per_frame, expect_frames};

/// Frames between the animation's target changes.
const RETARGET_EVERY: usize = 12;

pub(super) fn scenarios() -> Vec<Scenario> {
    vec![Scenario {
        name: "animated-frame",
        description: "an animated frame retargeted every 12 frames beside 40 still rows",
        build: |_, cx| Box::new(Animating::new(cx)),
        warm_up_frames: 3,
    }]
}

pub(crate) struct Animated {
    wide: bool,
}

impl Render for Animated {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(
                animated_frame("bar")
                    .duration(Duration::from_millis(200))
                    .w(px(if self.wide { 300. } else { 100. }))
                    .h(px(20.))
                    .child_with_progress(|progress, _, _| {
                        div()
                            .size_full()
                            .bg(hsla(0.6, 0.6, 0.5, 1.))
                            .child(SharedString::from(format!("{progress:.2}")))
                    }),
            )
            .children((0..40).map(|index| {
                div()
                    .h(px(18.))
                    .child(SharedString::from(format!("still row {index}")))
            }))
    }
}

struct Animating {
    view: Entity<Animated>,
}

impl Animating {
    fn new(cx: &mut App) -> Self {
        Self {
            view: cx.new(|_| Animated { wide: false }),
        }
    }
}

impl Fixture for Animating {
    fn root(&self) -> AnyView {
        self.view.clone().into()
    }

    fn step(&mut self, frame: usize, _: &mut Window, cx: &mut App) {
        if frame.is_multiple_of(RETARGET_EVERY) {
            self.view.update(cx, |view, cx| {
                view.wide = !view.wide;
                cx.notify();
            });
        } else {
            // The animation itself asks for the frames in between.
            cx.notify(self.view.entity_id());
        }
    }

    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String> {
        expect_frames(mode, stats)?;
        at_least_per_frame(stats, "views_built", 2)?;
        // The glyphs of the still rows.
        drawn_per_frame(stats, 40 * 9)
    }
}
