//! Animation and decoration: everything moving, and a decorated page with one animated part.

use gpux::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div, linear_color_stop, linear_gradient, prelude::*, px, rgb,
};

use crate::{
    Built,
    util::{Rng, color, words},
};

const DOTS: usize = 1_000;

struct Dots {
    positions: Vec<(f32, f32, f32, f32)>,
}

impl Render for Dots {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .children(
                self.positions
                    .iter()
                    .enumerate()
                    .map(|(index, (x, y, _, _))| {
                        div()
                            .absolute()
                            .left(px(*x))
                            .top(px(*y))
                            .size(px(10.))
                            .rounded_full()
                            .bg(color(index as u64))
                    }),
            )
    }
}

pub fn animated_dots(window: &mut Window, cx: &mut App) -> Built {
    let mut rng = Rng::new(37);
    let viewport = window.viewport_size();
    let (width, height) = (
        f32::from(viewport.width) - 10.,
        f32::from(viewport.height) - 10.,
    );
    let positions = (0..DOTS)
        .map(|_| {
            (
                rng.unit() * width,
                rng.unit() * height,
                rng.unit() * 4. - 2.,
                rng.unit() * 4. - 2.,
            )
        })
        .collect();
    let dots = cx.new(|_| Dots { positions });
    Built {
        root: dots.clone().into(),
        step: Box::new(move |_, _, cx| {
            dots.update(cx, |dots, cx| {
                for (x, y, dx, dy) in &mut dots.positions {
                    *x += *dx;
                    *y += *dy;
                    if *x < 0. || *x > width {
                        *dx = -*dx;
                    }
                    if *y < 0. || *y > height {
                        *dy = -*dy;
                    }
                }
                cx.notify();
            });
        }),
    }
}

struct Progress {
    value: f32,
}

impl Render for Progress {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(6.))
            .w_full()
            .rounded_full()
            .bg(rgb(0x30363d))
            .child(
                div()
                    .h_full()
                    .w(gpux::relative(self.value))
                    .rounded_full()
                    .bg(linear_gradient(
                        90.,
                        [
                            linear_color_stop(rgb(0x2f6feb), 0.),
                            linear_color_stop(rgb(0xa371f7), 1.),
                        ],
                    )),
            )
    }
}

struct Gallery {
    titles: Vec<SharedString>,
    progress: Entity<Progress>,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().p_2().child(self.progress.clone()))
            .child(
                div()
                    .id("gallery")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_3()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .children(self.titles.iter().enumerate().map(|(index, title)| {
                        let hue = color(index as u64);
                        div()
                            .w(px(140.))
                            .h(px(110.))
                            .p_2()
                            .flex()
                            .flex_col()
                            .justify_between()
                            .rounded_lg()
                            .shadow_lg()
                            .border_1()
                            .border_color(hue.opacity(0.6))
                            .bg(linear_gradient(
                                135.,
                                [
                                    linear_color_stop(hue.opacity(0.85), 0.),
                                    linear_color_stop(rgb(0x1e1f24), 1.),
                                ],
                            ))
                            .opacity(if index % 5 == 0 { 0.7 } else { 1. })
                            .child(div().text_sm().child(title.clone()))
                            .child(
                                div()
                                    .flex()
                                    .gap_1()
                                    .child(div().size(px(14.)).rounded_full().bg(hue).shadow_sm())
                                    .child(
                                        div().text_xs().child(format!("{} items", index * 3 % 97)),
                                    ),
                            )
                    })),
            )
    }
}

pub fn effects_cards(_: &mut Window, cx: &mut App) -> Built {
    let mut rng = Rng::new(41);
    let titles = (0..240).map(|_| words(&mut rng, 2).into()).collect();
    let progress = cx.new(|_| Progress { value: 0. });
    let root = cx.new(|_| Gallery {
        titles,
        progress: progress.clone(),
    });
    Built {
        root: root.into(),
        step: Box::new(move |frame, _, cx| {
            progress.update(cx, |progress, cx| {
                progress.value = (frame % 240) as f32 / 240.;
                cx.notify();
            });
        }),
    }
}
