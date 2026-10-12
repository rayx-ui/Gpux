//! What the scroll-layer groups of lane C share: the views they scroll, the two windows they
//! compare (one with every fast-mode mechanism on, one with all of them off and the
//! device-pixel snapping forced on), and the comparison of their frames, byte for byte.
#![allow(dead_code)]

use gpux::{
    AnyWindowHandle, BoxShadow, Context, Entity, FastMode, FastStats, IntoElement, ListAlignment,
    ListState, Modifiers, ParentElement, PathBuilder, Render, ScrollDelta, ScrollWheelEvent,
    SharedString, Styled, TestAppContext, TestDispatcher, TestWindowOptions, TouchPhase,
    VisualTestContext, Window, canvas, div, linear_color_stop, linear_gradient, list, point,
    prelude::*, px, rgb, rgba, size, uniform_list,
};
use gpux_conformance::test_host::lane_c_renderer;
use image::RgbaImage;

pub const WINDOW: (f32, f32) = (420., 360.);

/// The panel's padding around the scroll container.
pub const PADDING: f32 = 16.;

/// What the scrolled content is made of.
#[derive(Clone, Copy, Debug)]
pub enum Content {
    /// Rows of text on alternating backgrounds, with borders.
    Text,
    /// Rounded cards with shadows, gradients and borders, 24 px apart.
    Cards,
    /// Cards 6 px apart under a large two-part shadow, whose blur reaches well over the
    /// neighbours.
    TightCards,
    /// White cards 2 px apart under a heavy shadow: 16 px of blur at half black.
    HeavyShadows,
    /// Rows with backgrounds of their own, each holding a card under a heavy shadow that
    /// reaches over the rows around it.
    ShadowedRows,
    /// Sections with backgrounds of their own, each holding three shadowed cards: the first
    /// and the last card's shadows reach into the sections around.
    Sections,
    /// Rows holding a shadowed card that clips content taller than itself, as a preview of
    /// a long text does.
    ClippedCards,
    /// Bordered rows with backgrounds, each reaching 8 px up over the row before, holding a
    /// shadowed card.
    OverlappingRows,
    /// Narrow shadowed rows in a container with no background of its own: behind it lie
    /// the panel on the left and a nest of backgrounds on the right.
    NoBackground,
    /// Rows of one height in a `uniform_list`, the background of every other one reaching
    /// 8 px up over the row before, each holding a shadowed card.
    AlternatingList,
    /// Rows with a border all round and a background, each holding a bordered, shadowed
    /// card: four clips a border, however far the content is scrolled.
    BorderedRows,
    /// Rows 30 px apart, each with a background and a shadowed card whose blur reaches
    /// into the gaps, under a bar laid over the container's bottom edge and painted after
    /// it: where the bar lies over a gap, nothing it lies over is as high as the shadow.
    UnderABar,
    /// The shadowed rows in a container laid over another just like it, with a veil painted
    /// between the two: two clips alike, and something painted between them.
    Stacked,
    /// Rows of which every third holds a scrolling container of its own, with a shadowed card
    /// and a label in it, and a shadowed bar laid over that container's bottom edge and
    /// painted after it.
    Nested,
    /// Rows of rounded bordered shadowed cards, each turned, moved or scaled.
    TransformedBorders,
    /// Rows holding a card that clips four lines of underlined text taller than itself: the
    /// card's clip cuts glyphs and underlines.
    ClippedText,
    /// Rows holding a bordered card that clips a line of text and is turned a little: glyphs
    /// painted under a transformation.
    RotatedText,
    /// Rows of text in a white strip, each line in its place.
    TextNative,
    /// The same rows, each line moved to its place by a transformation of its own (a whole
    /// number of device pixels at scale 1 and 2): the same pixels, through the filter by hand.
    TextTranslated,
    /// The turned and scaled text rows, every third row holding a scrolling container of its
    /// own with a turned card of text in it.
    NestedRotatedText,
    /// The same cards, turned only after an odd number of ticks: the turn comes and goes with
    /// the content scrolled.
    TurnedAndStraightText,
    /// The shadowed rows, each card moved a few pixels by a transformation of its own.
    TranslatedCards,
    /// The shadowed rows, each card turned a little.
    RotatedCards,
    /// Rows that each draw a path, the last thing painted where it lies.
    Paths,
    /// Rows wider than the viewport: the container scrolls on both axes.
    Wide,
    /// The text rows in a `uniform_list`, whose layer holds them row by row.
    UniformList,
    /// Text rows of three heights in a `list`.
    List,
}

/// A panel around a scroll container over an opaque background, which a layer's tiles bake.
pub struct Page {
    content: Content,
    /// Changes what a few rows show, for the tests that change content while it is scrolled.
    tick: usize,
    /// The state of the `list`, when the content is one.
    list: ListState,
}

/// How many rows the content has.
pub fn row_count(content: Content) -> usize {
    match content {
        Content::Cards
        | Content::TightCards
        | Content::HeavyShadows
        | Content::ShadowedRows
        | Content::ClippedCards
        | Content::OverlappingRows
        | Content::NoBackground
        | Content::AlternatingList
        | Content::BorderedRows
        | Content::UnderABar
        | Content::Stacked
        | Content::Nested
        | Content::ClippedText
        | Content::TransformedBorders
        | Content::RotatedText
        | Content::NestedRotatedText
        | Content::TextNative
        | Content::TextTranslated
        | Content::TurnedAndStraightText
        | Content::TranslatedCards
        | Content::RotatedCards => 40,
        Content::Sections => 16,
        _ => 120,
    }
}

impl Page {
    pub fn row(&self, index: usize) -> gpux::AnyElement {
        row(self.content, self.tick, index)
    }
}

/// Row `index` of `content` after `tick` changes.
pub fn row(content: Content, tick: usize, index: usize) -> gpux::AnyElement {
    let page = RowOf { content, tick };
    page.row(index)
}

/// What a row is drawn from.
pub struct RowOf {
    content: Content,
    tick: usize,
}

impl RowOf {
    pub fn row(&self, index: usize) -> gpux::AnyElement {
        let marked = self.tick > 0 && index % 7 == self.tick % 7;
        match self.content {
            Content::Text | Content::Wide | Content::UniformList | Content::List => div()
                .h(px(match self.content {
                    Content::List => 27. + (index % 3) as f32 * 9.,
                    _ => 27.,
                }))
                .when(matches!(self.content, Content::Wide), |row| {
                    row.w(px(900.)).flex_none()
                })
                .px_3()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(rgb(0xc9c2b4))
                .bg(if marked {
                    rgb(0xffd9a0)
                } else if index.is_multiple_of(2) {
                    rgb(0xf4f1ea)
                } else {
                    rgb(0xe9e4d8)
                })
                .text_color(rgb(0x1e2430))
                .text_size(px(13.))
                .child(SharedString::from(format!(
                    "Row {index:03} · sphinx of black quartz, judge my vow · {}",
                    self.tick
                )))
                .into_any_element(),
            Content::TightCards => div()
                .mx_3()
                .mt(px(6.))
                .h(px(64.))
                .rounded(px(8.))
                .bg(if marked { rgb(0xffe9c9) } else { rgb(0xfbfaf7) })
                .shadow(vec![
                    BoxShadow {
                        color: rgba(0x0000001a).into(),
                        offset: point(px(0.), px(10.)),
                        blur_radius: px(15.),
                        spread_radius: px(-3.),
                        inset: false,
                    },
                    BoxShadow {
                        color: rgba(0x0000001a).into(),
                        offset: point(px(0.), px(4.)),
                        blur_radius: px(6.),
                        spread_radius: px(-4.),
                        inset: false,
                    },
                ])
                .p_3()
                .text_color(rgb(0x1e2430))
                .text_size(px(14.))
                .child(SharedString::from(format!("Card {index:02}")))
                .into_any_element(),
            Content::HeavyShadows => div()
                .mx_3()
                .mt(px(2.))
                .h(px(60.))
                .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                .shadow(vec![BoxShadow {
                    color: rgba(0x00000080).into(),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(16.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .into_any_element(),
            Content::Nested if index % 3 == 1 => div()
                .relative()
                .py(px(10.))
                .bg(rgb(0xd5dde6))
                .child(
                    div()
                        .id(("inner", index))
                        .relative()
                        .ml_3()
                        .w(px(150.))
                        .h(px(70.))
                        .overflow_y_scroll()
                        .bg(rgb(0xf4f1ea))
                        // A card whose shadow's blur reaches the container's bottom edge,
                        // and a label beside it, under nothing but the blurs around.
                        .child(
                            div()
                                .absolute()
                                .left(px(10.))
                                .top(px(8.))
                                .w(px(80.))
                                .h(px(30.))
                                .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                                .shadow(vec![BoxShadow {
                                    color: rgba(0x00000080).into(),
                                    offset: point(px(0.), px(0.)),
                                    blur_radius: px(12.),
                                    spread_radius: px(0.),
                                    inset: false,
                                }]),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(100.))
                                .top(px(8.))
                                .w(px(40.))
                                .h(px(30.))
                                .bg(rgb(0x9db8d9)),
                        )
                        .child(div().h(px(140.))),
                )
                // Painted after the container, over its bottom edge: a bar under a shadow
                // of its own, which lies over the blur of the card's where the two meet.
                .child(
                    div()
                        .absolute()
                        .left(px(40.))
                        .bottom(px(2.))
                        .w(px(220.))
                        .h(px(16.))
                        .bg(rgb(0xb5432f))
                        .shadow(vec![BoxShadow {
                            color: rgba(0x00000080).into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(16.),
                            spread_radius: px(0.),
                            inset: false,
                        }]),
                )
                .into_any_element(),
            Content::TransformedBorders => {
                let card = div()
                    .mx_3()
                    .h(px(44.))
                    .bg(rgb(0xffffff))
                    .rounded(px(6.))
                    .border_2()
                    .border_color(rgb(0x555555))
                    .shadow(vec![BoxShadow {
                        color: rgba(0x00000080).into(),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(14.),
                        spread_radius: px(0.),
                        inset: false,
                    }]);
                let card = match index % 3 {
                    0 => card.rotate(0.03),
                    1 => card.translate(point(px(3.), px(5.))),
                    _ => card.scale(1.05),
                };
                div()
                    .h(px(60.))
                    .py(px(8.))
                    .bg(if index.is_multiple_of(2) {
                        rgb(0xe9e4d8)
                    } else {
                        rgb(0xd5dde6)
                    })
                    .child(card)
                    .into_any_element()
            }
            Content::ClippedText => div()
                .py(px(7.))
                .bg(if index.is_multiple_of(2) {
                    rgb(0xe9e4d8)
                } else {
                    rgb(0xd5dde6)
                })
                .child(
                    div()
                        .mx_3()
                        .h(px(45.))
                        .px_2()
                        .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                        .overflow_hidden()
                        .text_color(rgb(0x1e2430))
                        .text_size(px(13.))
                        .line_height(px(15.))
                        .underline()
                        .children((0..4).map(|line| {
                            SharedString::from(format!(
                                "Row {index:02}, line {line}: jackdaws love my big sphinx"
                            ))
                        })),
                )
                .into_any_element(),
            Content::TextNative | Content::TextTranslated => {
                let line = SharedString::from(format!(
                    "Row {index:02}: jackdaws 😀 love my big sphinx of quartz"
                ));
                let text = div()
                    .text_color(rgb(0x1e2430))
                    .text_size(px(12.))
                    .child(line);
                div()
                    .h(px(30.))
                    .px_2()
                    .bg(if index.is_multiple_of(2) {
                        rgb(0xffffff)
                    } else {
                        rgb(0xeef2f7)
                    })
                    .child(match self.content {
                        Content::TextTranslated => div()
                            .relative()
                            .left(px(-3.))
                            .top(px(-5.))
                            .child(text.translate(point(px(3.), px(5.))))
                            .into_any_element(),
                        _ => div().relative().child(text).into_any_element(),
                    })
                    .into_any_element()
            }
            Content::NestedRotatedText if index % 3 == 1 => div()
                .py(px(8.))
                .bg(rgb(0xd5dde6))
                .child(
                    div()
                        .id(("inner", index))
                        .ml_3()
                        .w(px(300.))
                        .h(px(70.))
                        .overflow_y_scroll()
                        .bg(rgb(0xf4f1ea))
                        .child(
                            div()
                                .m(px(8.))
                                .h(px(40.))
                                .px_2()
                                .bg(rgb(0xffffff))
                                .border_1()
                                .border_color(rgb(0x555555))
                                .overflow_hidden()
                                .text_color(rgb(0x1e2430))
                                .text_size(px(12.))
                                .rotate(0.03)
                                .child(SharedString::from(format!(
                                    "Inner {index:02}: jackdaws 😀 love my big sphinx"
                                ))),
                        )
                        .child(div().h(px(120.))),
                )
                .into_any_element(),
            Content::RotatedText | Content::NestedRotatedText | Content::TurnedAndStraightText => {
                div()
                    .py(px(8.))
                    .bg(if index.is_multiple_of(2) {
                        rgb(0xe9e4d8)
                    } else {
                        rgb(0xd5dde6)
                    })
                    .child(
                        div()
                            .mx_3()
                            .h(px(44.))
                            .px_2()
                            .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                            .border_1()
                            .border_color(rgb(0x555555))
                            .overflow_hidden()
                            .text_color(rgb(0x1e2430))
                            .text_size(px(12.))
                            // Every other card of `RotatedText` is scaled, not turned; the turn of
                            // `TurnedAndStraightText` comes and goes with the ticks.
                            .when(
                                matches!(
                                    self.content,
                                    Content::RotatedText | Content::NestedRotatedText
                                ) && index % 2 == 1,
                                |card| card.scale(1.05),
                            )
                            .when(
                                (matches!(
                                    self.content,
                                    Content::RotatedText | Content::NestedRotatedText
                                ) && index % 2 == 0)
                                    || self.tick % 2 == 1,
                                |card| card.rotate(0.03),
                            )
                            .child(SharedString::from(format!(
                                "Row {index:02}: jackdaws 😀 love my big sphinx of quartz"
                            ))),
                    )
                    .into_any_element()
            }
            Content::TranslatedCards | Content::RotatedCards => div()
                .py(px(10.))
                .bg(if index.is_multiple_of(2) {
                    rgb(0xe9e4d8)
                } else {
                    rgb(0xd5dde6)
                })
                .child(
                    div()
                        .mx_3()
                        .h(px(54.))
                        .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                        .when(matches!(self.content, Content::TranslatedCards), |card| {
                            card.translate(point(px(3.), px(5.)))
                        })
                        .when(matches!(self.content, Content::RotatedCards), |card| {
                            card.rotate(0.03)
                        })
                        .shadow(vec![BoxShadow {
                            color: rgba(0x00000080).into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(16.),
                            spread_radius: px(0.),
                            inset: false,
                        }]),
                )
                .into_any_element(),
            Content::ShadowedRows | Content::Stacked | Content::Nested => div()
                .py(px(10.))
                .bg(if index.is_multiple_of(2) {
                    rgb(0xe9e4d8)
                } else {
                    rgb(0xd5dde6)
                })
                .child(
                    div()
                        .mx_3()
                        .h(px(54.))
                        .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                        .shadow(vec![BoxShadow {
                            color: rgba(0x00000080).into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(16.),
                            spread_radius: px(0.),
                            inset: false,
                        }]),
                )
                .into_any_element(),
            Content::ClippedCards => div()
                .py(px(6.))
                .bg(if index.is_multiple_of(2) {
                    rgb(0xe9e4d8)
                } else {
                    rgb(0xd5dde6)
                })
                .child(
                    div()
                        .mx_3()
                        .h(px(60.))
                        .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                        .overflow_hidden()
                        .shadow(vec![BoxShadow {
                            color: rgba(0x00000080).into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(14.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .child(div().m(px(8.)).h(px(200.)).bg(rgb(0xdfe6d8))),
                )
                .into_any_element(),
            Content::OverlappingRows => div()
                .mt(px(-8.))
                .py(px(10.))
                .border_1()
                .border_color(rgb(0x8a8272))
                .bg(if index.is_multiple_of(2) {
                    rgb(0xe9e4d8)
                } else {
                    rgb(0xd5dde6)
                })
                .child(
                    div()
                        .mx_3()
                        .h(px(40.))
                        .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                        .shadow(vec![BoxShadow {
                            color: rgba(0x00000080).into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(16.),
                            spread_radius: px(0.),
                            inset: false,
                        }]),
                )
                .into_any_element(),
            Content::NoBackground => div()
                .w(px(180.))
                .mt(px(14.))
                .ml(px(if index.is_multiple_of(3) { 12. } else { 150. }))
                .h(px(44.))
                .bg(if marked { rgb(0xffe9c9) } else { rgb(0xfbfaf7) })
                .shadow(vec![BoxShadow {
                    color: rgba(0x00000080).into(),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(14.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .into_any_element(),
            Content::BorderedRows => div()
                .py(px(10.))
                .border_1()
                .border_color(rgb(0x8a8272))
                .bg(if index.is_multiple_of(2) {
                    rgb(0xe9e4d8)
                } else {
                    rgb(0xd5dde6)
                })
                .child(
                    div()
                        .mx_3()
                        .h(px(40.))
                        .border_1()
                        .border_color(rgb(0x8a8272))
                        .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                        .shadow(vec![BoxShadow {
                            color: rgba(0x00000080).into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(16.),
                            spread_radius: px(0.),
                            inset: false,
                        }]),
                )
                .into_any_element(),
            Content::UnderABar => div()
                .mt(px(30.))
                .py(px(10.))
                .bg(if index.is_multiple_of(2) {
                    rgb(0xe9e4d8)
                } else {
                    rgb(0xd5dde6)
                })
                .child(
                    div()
                        .mx_3()
                        .h(px(40.))
                        .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                        .shadow(vec![BoxShadow {
                            color: rgba(0x00000080).into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(16.),
                            spread_radius: px(0.),
                            inset: false,
                        }]),
                )
                .into_any_element(),
            Content::AlternatingList => div()
                .h(px(74.))
                .relative()
                .child(
                    div()
                        .absolute()
                        .top(px(if index.is_multiple_of(2) { 0. } else { -8. }))
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .bg(if index.is_multiple_of(2) {
                            rgb(0xe9e4d8)
                        } else {
                            rgb(0xd5dde6)
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(10.))
                        .left(px(12.))
                        .right(px(12.))
                        .h(px(54.))
                        .bg(if marked { rgb(0xffe9c9) } else { rgb(0xffffff) })
                        .shadow(vec![BoxShadow {
                            color: rgba(0x00000080).into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(16.),
                            spread_radius: px(0.),
                            inset: false,
                        }]),
                )
                .into_any_element(),
            Content::Sections => div()
                .pt(px(4.))
                .pb(px(12.))
                .bg(if index.is_multiple_of(2) {
                    rgb(0xf0ece2)
                } else {
                    rgb(0xdfe6d8)
                })
                .children((0..3).map(|card| {
                    div()
                        .mx_3()
                        .mt(px(8.))
                        .h(px(44.))
                        .rounded(px(6.))
                        .bg(if marked && card == 1 {
                            rgb(0xffe9c9)
                        } else {
                            rgb(0xfbfaf7)
                        })
                        .shadow(vec![
                            BoxShadow {
                                color: rgba(0x00000033).into(),
                                offset: point(px(0.), px(8.)),
                                blur_radius: px(14.),
                                spread_radius: px(-2.),
                                inset: false,
                            },
                            BoxShadow {
                                color: rgba(0x00000026).into(),
                                offset: point(px(0.), px(2.)),
                                blur_radius: px(5.),
                                spread_radius: px(0.),
                                inset: false,
                            },
                        ])
                }))
                .into_any_element(),
            Content::Cards => div()
                .mx_3()
                .mt(px(24.))
                .h(px(74.))
                .rounded(px(10.))
                .border_1()
                .border_color(rgba(0x1e243080))
                .shadow_md()
                .bg(linear_gradient(
                    (index * 37 % 360) as f32,
                    [
                        linear_color_stop(if marked { rgb(0x5a2a2a) } else { rgb(0x2a3a5a) }, 0.),
                        linear_color_stop(rgb(0x1b2233), 1.),
                    ],
                ))
                .p_3()
                .text_color(rgb(0xf4f1ea))
                .text_size(px(15.))
                .child(SharedString::from(format!("Card {index:02}")))
                .into_any_element(),
            Content::Paths => div()
                .h(px(48.))
                .border_b_1()
                .border_color(rgb(0xc9c2b4))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let left = bounds.origin.x + px(16. + (index % 5) as f32 * 9.5);
                            let top = bounds.origin.y + px(8.);
                            let mut builder = PathBuilder::fill();
                            builder.move_to(point(left, top + px(30.)));
                            builder.line_to(point(left + px(60.), top));
                            builder.curve_to(
                                point(left + px(180.), top + px(30.)),
                                point(left + px(120.), top - px(4.)),
                            );
                            builder.close();
                            if let Ok(path) = builder.build() {
                                window.paint_path(
                                    path,
                                    if marked { rgb(0xb5432f) } else { rgb(0x2f6fb5) },
                                );
                            }
                        },
                    )
                    .size_full(),
                )
                .into_any_element(),
        }
    }
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = row_count(self.content);
        let panel = div().size_full().bg(rgb(0x1e2430)).p(px(PADDING));
        let (content, tick) = (self.content, self.tick);
        match content {
            Content::NoBackground => {
                return panel
                    .relative()
                    .child(
                        div()
                            .absolute()
                            .top(px(PADDING))
                            .bottom(px(PADDING))
                            .left(px(210.))
                            .right(px(PADDING))
                            .bg(rgb(0x2a3a5a))
                            .p(px(6.))
                            .child(
                                div().size_full().bg(rgb(0x35507a)).p(px(6.)).child(
                                    div()
                                        .size_full()
                                        .bg(rgb(0x4a6a9a))
                                        .p(px(6.))
                                        .child(div().size_full().bg(rgb(0x6a8aba))),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .id("scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .children((0..rows).map(|index| self.row(index))),
                    );
            }
            Content::Stacked => {
                let container = |id: &'static str| {
                    div()
                        .id(id)
                        .absolute()
                        .top(px(PADDING))
                        .bottom(px(PADDING))
                        .left(px(PADDING))
                        .right(px(PADDING))
                        .bg(rgb(0xf4f1ea))
                        .overflow_y_scroll()
                        .children((0..rows).map(|index| self.row(index)))
                };
                return panel
                    .relative()
                    .child(container("under"))
                    .child(
                        div()
                            .absolute()
                            .top(px(PADDING + 8.))
                            .bottom(px(PADDING - 8.))
                            .left(px(PADDING + 30.))
                            .right(px(PADDING + 30.))
                            .bg(rgb(0x35507a)),
                    )
                    .child(container("scroll"));
            }
            Content::UniformList | Content::AlternatingList => {
                return panel.child(
                    uniform_list(
                        "scroll",
                        rows,
                        cx.processor(move |_, range: std::ops::Range<usize>, _, _| {
                            range
                                .map(|index| row(content, tick, index))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .size_full()
                    .bg(rgb(0xf4f1ea)),
                );
            }
            Content::List => {
                return panel.child(
                    list(self.list.clone(), move |index, _, _| {
                        row(content, tick, index)
                    })
                    .size_full()
                    .bg(rgb(0xf4f1ea)),
                );
            }
            _ => {}
        }
        panel
            .relative()
            .child(
                div()
                    .id("scroll")
                    .size_full()
                    .bg(rgb(0xf4f1ea))
                    .when(matches!(self.content, Content::Wide), |container| {
                        container.overflow_scroll()
                    })
                    .when(!matches!(self.content, Content::Wide), |container| {
                        container.overflow_y_scroll()
                    })
                    .children((0..rows).map(|index| self.row(index))),
            )
            .when(matches!(self.content, Content::UnderABar), |panel| {
                panel.child(
                    div()
                        .absolute()
                        .left(px(60.))
                        .bottom(px(PADDING - 6.))
                        .w(px(200.))
                        .h(px(30.))
                        .bg(rgb(0xb5432f)),
                )
            })
    }
}

/// One of the two windows a test compares.
#[derive(Clone)]
pub struct Run {
    window: AnyWindowHandle,
    page: Entity<Page>,
}

/// The application both windows of a test live in, as two windows of a program do.
pub fn app() -> TestAppContext {
    let mut cx = TestAppContext::build_with_renderer(
        TestDispatcher::new(0),
        None,
        gpui_platform::bundled_text_system(),
        lane_c_renderer,
    );
    // A frame is drawn when the test reads it back, one a step, so that the renderer is handed
    // every frame.
    cx.set_draw_on_flush(false);
    cx
}

impl Run {
    /// Opens `content` at `scale`: with every mechanism on when `layered`, as the reference
    /// scroll layers are held to otherwise.
    pub fn open(cx: &mut TestAppContext, content: Content, scale: f32, layered: bool) -> Run {
        let handle = cx.add_window_with_options(
            TestWindowOptions::new(size(px(WINDOW.0), px(WINDOW.1))).scale_factor(scale),
            move |_, _| Page {
                content,
                tick: 0,
                list: ListState::new(row_count(content), ListAlignment::Top, px(0.)),
            },
        );
        let page = handle.root(cx).expect("the window's page");
        let window: AnyWindowHandle = handle.into();
        cx.update_window(window, |_, window, _| {
            if layered {
                window.set_fast_mode(FastMode::ALL);
            } else {
                window.set_fast_mode(FastMode::OFF);
                window.force_layer_snapping(true);
            }
            window.activate_window();
        })
        .expect("the window is open");
        cx.run_until_parked();
        Run { window, page }
    }

    pub fn wheel(&self, cx: &TestAppContext, dx: f32, dy: f32) {
        VisualTestContext::from_window(self.window, cx).simulate_event(ScrollWheelEvent {
            position: point(px(WINDOW.0 / 2.), px(WINDOW.1 / 2.)),
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
    }

    pub fn tick(&self, cx: &mut TestAppContext) {
        self.page.update(cx, |page, cx| {
            page.tick += 1;
            cx.notify();
        });
    }

    /// Draws the frame the last step asked for and reads it back, with its counters.
    pub fn frame(&self, cx: &mut TestAppContext) -> (RgbaImage, FastStats) {
        let image = VisualTestContext::from_window(self.window, cx)
            .capture_screenshot()
            .expect("the window rendered");
        let stats = cx
            .update_window(self.window, |_, window, _| window.fast_frame_stats())
            .expect("the window is open");
        (image, stats)
    }
}

/// A step of a test's script.
#[derive(Clone, Copy)]
pub enum Step {
    Wheel(f32, f32),
    /// Changes what some rows show.
    Tick,
}

/// What the layered window did over a script.
#[derive(Default)]
pub struct Totals {
    pub composited: u64,
    pub repainted: u64,
    pub tiles_composited: u64,
    pub tiles_rasterized: u64,
}

/// Runs `script` in a layered window and in the reference, comparing every frame, and returns
/// what the layered window's layers did.
pub fn compare(content: Content, scale: f32, script: &[Step]) -> Totals {
    let mut cx = app();
    let layered = Run::open(&mut cx, content, scale, true);
    let reference = Run::open(&mut cx, content, scale, false);
    let mut totals = Totals::default();
    let (first, _) = layered.frame(&mut cx);
    let (expected, _) = reference.frame(&mut cx);
    assert_same(&first, &expected, 0, scale);
    for (index, step) in script.iter().enumerate() {
        for run in [&layered, &reference] {
            match *step {
                Step::Wheel(dx, dy) => run.wheel(&cx, dx, dy),
                Step::Tick => run.tick(&mut cx),
            }
        }
        let (actual, stats) = layered.frame(&mut cx);
        let (expected, reference_stats) = reference.frame(&mut cx);
        assert_eq!(
            reference_stats.layers_composited + reference_stats.layers_repainted,
            0,
            "the reference has no layers"
        );
        assert_same(&actual, &expected, index + 1, scale);
        totals.composited += stats.layers_composited;
        totals.repainted += stats.layers_repainted;
        totals.tiles_composited += stats.tiles_composited;
        totals.tiles_rasterized += stats.tiles_rasterized;
    }
    cx.quit();
    totals
}

/// Checks `actual`, drawn from tiles, against `expected`, the reference: the same bytes.
#[track_caller]
pub fn assert_same(actual: &RgbaImage, expected: &RgbaImage, step: usize, scale: f32) {
    assert_eq!(actual.dimensions(), expected.dimensions());
    let differing: Vec<(u32, u32)> = actual
        .enumerate_pixels()
        .filter(|(x, y, pixel)| *pixel != expected.get_pixel(*x, *y))
        .map(|(x, y, _)| (x, y))
        .collect();
    let Some(&(x, y)) = differing.first() else {
        return;
    };
    let (max_x, max_y) = differing
        .iter()
        .fold((0, 0), |(mx, my), (x, y)| (mx.max(*x), my.max(*y)));
    panic!(
        "at {scale}x, after step {step}: {} pixels differ from the reference, within \
         ({x}, {y})..=({max_x}, {max_y}); first {:?} against {:?}",
        differing.len(),
        actual.get_pixel(x, y),
        expected.get_pixel(x, y),
    );
}

/// Wheel steps down the content: small ones, an odd one, a fractional one, a long one, and back.
pub fn scrolls() -> Vec<Step> {
    [
        -20., -20., -20., -37., -91.5, -13., -240., 55., -7.25, -600., -31., 410., -3., -3., -3.,
    ]
    .into_iter()
    .map(|dy| Step::Wheel(0., dy))
    .collect()
}

#[track_caller]
pub fn assert_composited(totals: &Totals, at_least: u64) {
    assert!(
        totals.composited >= at_least && totals.tiles_composited >= at_least,
        "the layered window composited {} frames ({} tiles) from its layer, repainted {} and \
         rasterized {} tiles: fewer than {at_least} frames came from tiles",
        totals.composited,
        totals.tiles_composited,
        totals.repainted,
        totals.tiles_rasterized,
    );
}

/// The scripted scroll, and then one pixel at a time, down the content and back up: every step
/// of a card's shadow on its way across the viewport's top and bottom edges.
pub fn scrolls_and_single_pixels() -> Vec<Step> {
    let mut script = scrolls();
    script.extend((0..80).map(|_| Step::Wheel(0., -1.)));
    script.extend((0..40).map(|_| Step::Wheel(0., 1.)));
    script
}
