//! Layout retention's write elision: a kept node's style is converted and
//! written only when its request changed, read through every field the Taffy
//! conversion reads, with equal fingerprints confirmed field by field; its
//! children only when the ordered child ids differ; and the root, stretched
//! to the window, is not rewritten while nothing changed.

use super::oracle_support::oracle_app;
use crate::{
    AppContext as _, Context, FastMode, FastStats, GridLocation, GridPlacement, GridTemplate,
    GridTemplateMinSize, Render, Style, Window, WindowHandle, div,
    fast::layout::{LayoutFields, layout_fingerprint},
    prelude::*,
    px, rems, size,
    taffy::ToTaffy as _,
};

const LAYOUT: FastMode = FastMode {
    layout_retention: true,
    ..FastMode::OFF
};

/// Every field the conversion to a Taffy style reads reaches both the
/// fingerprint and the field comparison: a change either missed is one a kept
/// node never receives. Each case changes one field the conversion reads.
#[test]
fn every_field_the_taffy_style_is_made_from_is_compared() {
    use crate::{
        AlignContent, AlignItems, Display, FlexDirection, FlexWrap, LayoutDirection, Overflow,
        Position, relative,
    };

    let (rem_size, scale_factor) = (px(16.), 2.);
    let base = Style::default();
    let cases: Vec<(&str, Box<dyn Fn(&mut Style)>)> = vec![
        ("display", Box::new(|s| s.display = Display::Grid)),
        (
            "direction",
            Box::new(|s| s.direction = LayoutDirection::Rtl),
        ),
        ("overflow.x", Box::new(|s| s.overflow.x = Overflow::Hidden)),
        ("overflow.y", Box::new(|s| s.overflow.y = Overflow::Scroll)),
        (
            "scrollbar_width",
            Box::new(|s| s.scrollbar_width = px(7.).into()),
        ),
        ("position", Box::new(|s| s.position = Position::Absolute)),
        ("inset", Box::new(|s| s.inset.left = px(3.).into())),
        ("size", Box::new(|s| s.size.width = px(40.).into())),
        ("size in rems", Box::new(|s| s.size.width = rems(2.).into())),
        (
            "size as a fraction",
            Box::new(|s| s.size.width = relative(0.5).into()),
        ),
        ("min_size", Box::new(|s| s.min_size.height = px(5.).into())),
        ("max_size", Box::new(|s| s.max_size.width = px(90.).into())),
        ("aspect_ratio", Box::new(|s| s.aspect_ratio = Some(1.5))),
        ("margin", Box::new(|s| s.margin.top = px(2.).into())),
        ("padding", Box::new(|s| s.padding.bottom = px(4.).into())),
        (
            "border_widths",
            Box::new(|s| s.border_widths.right = px(1.).into()),
        ),
        (
            "align_items",
            Box::new(|s| s.align_items = Some(AlignItems::Center)),
        ),
        (
            "align_self",
            Box::new(|s| s.align_self = Some(AlignItems::End)),
        ),
        (
            "align_content",
            Box::new(|s| s.align_content = Some(AlignContent::End)),
        ),
        (
            "justify_content",
            Box::new(|s| s.justify_content = Some(AlignContent::Center)),
        ),
        ("gap", Box::new(|s| s.gap.width = px(6.).into())),
        (
            "flex_direction",
            Box::new(|s| s.flex_direction = FlexDirection::Column),
        ),
        ("flex_wrap", Box::new(|s| s.flex_wrap = FlexWrap::Wrap)),
        ("flex_basis", Box::new(|s| s.flex_basis = px(12.).into())),
        ("flex_grow", Box::new(|s| s.flex_grow = 1.)),
        ("flex_shrink", Box::new(|s| s.flex_shrink = 0.)),
        (
            "grid_rows",
            Box::new(|s| {
                s.grid_rows = Some(GridTemplate {
                    repeat: 3,
                    min_size: GridTemplateMinSize::Zero,
                })
            }),
        ),
        (
            "grid_cols",
            Box::new(|s| {
                s.grid_cols = Some(GridTemplate {
                    repeat: 2,
                    min_size: GridTemplateMinSize::MinContent,
                })
            }),
        ),
        (
            "grid_location",
            Box::new(|s| {
                s.grid_location = Some(GridLocation {
                    row: GridPlacement::Line(1)..GridPlacement::Span(2),
                    column: GridPlacement::Auto..GridPlacement::Auto,
                })
            }),
        ),
    ];

    let base_fingerprint = layout_fingerprint(&base, rem_size, scale_factor);
    let base_fields = LayoutFields::of(&base, rem_size, scale_factor);
    let base_taffy = base.to_taffy(rem_size, scale_factor);
    assert!(base_fields.matches(&base, rem_size, scale_factor));
    for (field, change) in &cases {
        let mut style = base.clone();
        change(&mut style);
        assert_ne!(
            style.to_taffy(rem_size, scale_factor),
            base_taffy,
            "changing {field} should change the Taffy style, or this case tests nothing"
        );
        assert_ne!(
            layout_fingerprint(&style, rem_size, scale_factor),
            base_fingerprint,
            "changing {field} changes the Taffy style but not the fingerprint"
        );
        assert!(
            !base_fields.matches(&style, rem_size, scale_factor),
            "changing {field} changes the Taffy style but not the compared fields"
        );
    }

    // Lengths are resolved against these, so they are compared as much as
    // the style is.
    let mut in_rems = base;
    in_rems.size.width = rems(2.).into();
    let fields = LayoutFields::of(&in_rems, rem_size, scale_factor);
    for (rem, scale) in [(px(20.), scale_factor), (rem_size, 1.)] {
        assert_ne!(
            layout_fingerprint(&in_rems, rem_size, scale_factor),
            layout_fingerprint(&in_rems, rem, scale)
        );
        assert!(!fields.matches(&in_rems, rem, scale));
    }
}

/// An equal fingerprint is not taken on trust: the fields are compared
/// exactly, so a request whose fingerprint repeats but whose fields do not
/// compare equal (an aspect ratio that is not a number) is converted again.
#[test]
fn equal_fingerprints_are_confirmed_field_by_field() {
    let (rem_size, scale_factor) = (px(16.), 1.);
    let style = Style {
        aspect_ratio: Some(f32::NAN),
        ..Style::default()
    };
    assert_eq!(
        layout_fingerprint(&style, rem_size, scale_factor),
        layout_fingerprint(&style.clone(), rem_size, scale_factor)
    );
    assert!(!LayoutFields::of(&style, rem_size, scale_factor).matches(
        &style,
        rem_size,
        scale_factor
    ));

    let mut harness = Harness::open(LAYOUT);
    harness.update(|column| column.aspect = Some(f32::NAN));
    harness.draw();
    let before = harness.conversions();
    harness.update(|_| {});
    harness.draw();
    assert_eq!(
        harness.conversions() - before,
        1,
        "the request whose fields never compare equal was not converted again"
    );
}

/// A box with a changing width and children, and an optional aspect ratio.
struct Column {
    width: f32,
    children: usize,
    aspect: Option<f32>,
}

impl Render for Column {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut child = div().w(px(self.width)).h(px(10.));
        if let Some(aspect) = self.aspect {
            child.style().aspect_ratio = Some(aspect);
        }
        div()
            .flex()
            .flex_col()
            .child(child)
            .children((0..self.children).map(|ix| div().w(px(ix as f32 + 1.)).h(px(4.))))
    }
}

struct Harness {
    cx: crate::TestAppContext,
    window: WindowHandle<Column>,
}

impl Harness {
    fn open(mode: FastMode) -> Self {
        let mut cx = oracle_app(0);
        cx.set_draw_on_flush(false);
        let window = cx.add_window(|_, _| Column {
            width: 40.,
            children: 4,
            aspect: None,
        });
        cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
            .unwrap();
        let mut harness = Self { cx, window };
        harness.draw();
        harness.draw();
        harness
    }

    fn update(&mut self, update: impl FnOnce(&mut Column)) {
        self.cx
            .update_window(self.window.into(), |root, _, cx| {
                root.downcast::<Column>().unwrap().update(cx, |column, cx| {
                    update(column);
                    cx.notify();
                });
            })
            .unwrap();
    }

    fn draw(&mut self) -> FastStats {
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.draw(cx).clear(cx);
                window.fast_frame_stats()
            })
            .unwrap()
    }

    fn conversions(&mut self) -> u64 {
        self.cx
            .update_window(self.window.into(), |_, window, _| {
                window.layout_engine.as_ref().unwrap().retention.conversions
            })
            .unwrap()
    }
}

/// A frame asking for what the last one did converts and writes nothing; one
/// changing a style converts and writes that node alone; one changing it to
/// what converts to the same device pixels converts it and writes nothing.
#[test]
fn styles_are_converted_and_written_only_when_the_request_changed() {
    let mut harness = Harness::open(LAYOUT);
    let before = harness.conversions();
    let steady = harness.draw();
    assert_eq!(
        (steady.style_writes, steady.children_writes),
        (0, 0),
        "{steady:?}"
    );
    assert_eq!(
        harness.conversions(),
        before,
        "a steady frame converted a style"
    );

    harness.update(|column| column.width = 60.);
    let widened = harness.draw();
    assert_eq!(
        (widened.style_writes, widened.children_writes),
        (1, 0),
        "{widened:?}"
    );
    assert_eq!(harness.conversions(), before + 1);

    // 60.2 rounds to the same device pixel as 60 at scale 1.
    harness.update(|column| column.width = 60.2);
    let rounded = harness.draw();
    assert_eq!(rounded.style_writes, 0, "{rounded:?}");
    assert_eq!(harness.conversions(), before + 2);
}

/// Children are written only when the ordered child ids differ: a child's
/// style changing leaves its parent's list alone.
#[test]
fn children_are_written_only_when_they_change() {
    let mut harness = Harness::open(LAYOUT);
    harness.update(|column| column.width = 50.);
    let restyled = harness.draw();
    assert_eq!(restyled.children_writes, 0, "{restyled:?}");

    harness.update(|column| column.children = 5);
    let grown = harness.draw();
    // The new child is made; the column's children are written once.
    assert_eq!(
        (grown.style_writes, grown.children_writes),
        (1, 1),
        "{grown:?}"
    );

    harness.update(|_| {});
    let steady = harness.draw();
    assert_eq!(
        (steady.style_writes, steady.children_writes),
        (0, 0),
        "{steady:?}"
    );
}

/// The window root is stretched to the window. A frame that asks for what
/// the last one did leaves it alone; resizing the window stretches it once.
/// The pool rewrites it every frame.
#[test]
fn an_unchanged_stretched_root_is_not_rewritten() {
    let mut harness = Harness::open(LAYOUT);
    for _ in 0..3 {
        harness.update(|_| {});
        let steady = harness.draw();
        assert_eq!(steady.style_writes, 0, "{steady:?}");
    }
    harness
        .cx
        .simulate_window_resize(harness.window.into(), size(px(500.), px(400.)));
    harness.update(|_| {});
    let resized = harness.draw();
    assert_eq!(resized.style_writes, 1, "{resized:?}");
    harness.update(|_| {});
    assert_eq!(harness.draw().style_writes, 0);

    let mut pooled = Harness::open(FastMode::OFF);
    pooled.update(|_| {});
    assert!(pooled.draw().style_writes > 0);
}
