//! Text measurement under layout retention: unchanged text carries last
//! frame's measurement and leaves its leaf clean, recolored text carries it
//! with new decorations, text of the same size replays, text of another size
//! is measured afresh, and text measured at a probe width is measured again
//! at the width it is laid out at. Truncation and wrapping measure as
//! before, leaves other than text are dirtied every frame, and no closure
//! outlives its frame. Every frame matches the one the pool lays out.

use std::{cell::Cell, rc::Rc};

use super::oracle_support::{PALETTE, oracle_app};
use crate::{
    AppContext as _, AvailableSpace, Context, Element, ElementId, FastMode, FastStats,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Render, SharedString, Size,
    Style, Window, WindowHandle, div, prelude::*, px, size,
};

const LAYOUT: FastMode = FastMode {
    layout_retention: true,
    ..FastMode::OFF
};

/// One text of the view.
#[derive(Clone)]
struct Line {
    text: SharedString,
    color: usize,
    /// Truncate it with an ellipsis instead of wrapping it.
    truncate: bool,
    /// A row it shares with a box of this width, growing to fill the rest.
    beside: Option<f32>,
}

impl Line {
    fn new(text: &'static str) -> Self {
        Self {
            text: text.into(),
            color: 0,
            truncate: false,
            beside: None,
        }
    }
}

/// Texts in a column of a given width, and an optional leaf measured by a
/// closure of its own.
struct Texts {
    lines: Vec<Line>,
    width: f32,
    custom: Option<Rc<Cell<usize>>>,
}

impl Render for Texts {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let lines = self.lines.iter().enumerate().map(|(ix, line)| {
            let text = div()
                .id(("text", ix))
                .text_color(PALETTE[line.color % PALETTE.len()])
                .when(line.truncate, |this| this.truncate())
                .child(line.text.clone());
            match line.beside {
                Some(width) => div()
                    .flex()
                    .flex_row()
                    .child(div().w(px(width)).h(px(4.)))
                    .child(text.flex_1())
                    .into_any_element(),
                None => text.into_any_element(),
            }
        });
        div()
            .flex()
            .flex_col()
            .w(px(self.width))
            .children(lines)
            .when_some(self.custom.clone(), |this, calls| {
                this.child(Measured { calls })
            })
    }
}

/// A leaf measured by a closure of its own, counting its measurements.
struct Measured {
    calls: Rc<Cell<usize>>,
}

impl IntoElement for Measured {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Measured {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut crate::App,
    ) -> (LayoutId, ()) {
        let calls = self.calls.clone();
        let id = window.request_measured_layout(
            Style::default(),
            move |_: Size<Option<Pixels>>, _: Size<AvailableSpace>, _, _| {
                calls.set(calls.get() + 1);
                size(px(10.), px(10.))
            },
        );
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: crate::Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut crate::App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: crate::Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut crate::App,
    ) {
    }
}

/// A window laid out with layout retention beside one laid out by the pool,
/// given the same changes.
struct Pair {
    kept: (crate::TestAppContext, WindowHandle<Texts>),
    pooled: (crate::TestAppContext, WindowHandle<Texts>),
}

fn open(
    mode: FastMode,
    lines: Vec<Line>,
    custom: Option<Rc<Cell<usize>>>,
) -> (crate::TestAppContext, WindowHandle<Texts>) {
    let mut cx = oracle_app(0);
    cx.set_draw_on_flush(false);
    let window = cx.add_window(move |_, _| Texts {
        lines,
        width: 120.,
        custom,
    });
    cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
        .unwrap();
    (cx, window)
}

fn draw(
    (cx, window): &mut (crate::TestAppContext, WindowHandle<Texts>),
) -> (FastStats, Vec<String>) {
    cx.update_window((*window).into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        (window.fast_frame_stats(), window.describe_rendered_frame())
    })
    .unwrap()
}

fn update(
    (cx, window): &mut (crate::TestAppContext, WindowHandle<Texts>),
    update: &impl Fn(&mut Texts),
) {
    cx.update_window((*window).into(), |root, _, cx| {
        root.downcast::<Texts>().unwrap().update(cx, |texts, cx| {
            update(texts);
            cx.notify();
        });
    })
    .unwrap();
}

impl Pair {
    fn open(lines: Vec<Line>) -> Self {
        let mut pair = Self {
            kept: open(LAYOUT, lines.clone(), None),
            pooled: open(FastMode::OFF, lines, None),
        };
        pair.step(|_| {});
        pair.step(|_| {});
        pair
    }

    /// Applies `change` to both windows and draws them, checking the frames
    /// match, and returns the kept window's counters.
    fn step(&mut self, change: impl Fn(&mut Texts)) -> FastStats {
        update(&mut self.kept, &change);
        update(&mut self.pooled, &change);
        let (stats, frame) = draw(&mut self.kept);
        let (_, expected) = draw(&mut self.pooled);
        if let Some(difference) = crate::fast::verify::first_frame_difference(&frame, &expected) {
            panic!("the kept layout differs from the pool's: {difference}");
        }
        stats
    }
}

fn lines(count: usize) -> Vec<Line> {
    const TEXTS: [&str; 6] = [
        "alpha",
        "beta gamma",
        "delta",
        "epsilon zeta",
        "eta",
        "theta",
    ];
    (0..count)
        .map(|ix| Line::new(TEXTS[ix % TEXTS.len()]))
        .collect()
}

/// Unchanged text takes last frame's measurement over: no text is measured,
/// and its leaf, clean, leaves every node above it clean.
#[test]
fn unchanged_text_carries_its_measurement() {
    let mut pair = Pair::open(lines(6));
    let steady = pair.step(|_| {});
    assert_eq!(steady.leaves_carried, 6, "{steady:?}");
    assert_eq!(steady.leaves_measured, 0, "{steady:?}");
    assert_eq!(
        steady.leaves_remeasured + steady.leaves_replayed,
        0,
        "{steady:?}"
    );
}

/// Text recolored, but shaped as before, takes the measurement over with
/// its decorations rewritten.
#[test]
fn recolored_text_carries_with_new_decorations() {
    let mut pair = Pair::open(lines(4));
    let recolored = pair.step(|texts| texts.lines[2].color = 3);
    assert_eq!(recolored.leaves_carried, 4, "{recolored:?}");
    assert_eq!(recolored.leaves_measured, 0, "{recolored:?}");
}

/// Other text of the same size gives every size Taffy took from the leaf:
/// it is replayed and stays clean, with the new text painted.
#[test]
fn text_of_the_same_size_replays() {
    let mut pair = Pair::open(lines(4));
    // As many glyphs as "delta", each as wide as any other.
    let replayed = pair.step(|texts| texts.lines[2].text = "omega".into());
    assert_eq!(replayed.leaves_replayed, 1, "{replayed:?}");
    assert_eq!(replayed.leaves_remeasured, 0, "{replayed:?}");
    assert_eq!(replayed.leaves_carried, 3, "{replayed:?}");
    // Nothing above it was written or dirtied for it.
    assert_eq!((replayed.style_writes, replayed.children_writes), (0, 0));
}

/// Text of another size is measured afresh, and what replaying it needed is
/// dropped: its next frame carries it.
#[test]
fn text_of_another_size_is_measured_afresh() {
    let mut pair = Pair::open(lines(4));
    let remeasured = pair.step(|texts| texts.lines[2].text = "delta and more".into());
    assert_eq!(remeasured.leaves_remeasured, 1, "{remeasured:?}");
    assert!(remeasured.leaves_measured > 0, "{remeasured:?}");
    let next = pair.step(|_| {});
    assert_eq!(next.leaves_carried, 4, "{next:?}");
}

/// Text taking the rest of a row (`flex_1`, allowed to shrink to nothing), in
/// a column capped at a width, beside a line that changes every frame: the
/// tree of gpui-fast's fix `2e2c73a`.
struct FlexedText {
    tick: usize,
}

impl Render for FlexedText {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let content = div()
            .flex()
            .flex_col()
            .child(
                div().flex().child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child("a heading long enough to wrap onto several lines in the narrow column it is laid out in"),
                ),
            )
            // Longer every frame, so it is measured afresh and the column
            // above it laid out again.
            .child(SharedString::from(format!("frame {}", "+".repeat(self.tick))));
        div().flex().child(
            div().w_full().max_w(px(300.)).child(
                div().flex().flex_col().child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .child(div().flex().flex_col().min_w_0().w_full().child(content)),
                ),
            ),
        )
    }
}

/// The line beside the row changes, the row is sized to its content once
/// more, and Taffy, placing the kept text from its cache, measures it at width
/// zero on the way: measured again at the width it is laid out at, the text
/// is painted in the lines of its box, as the pool paints it, every frame.
#[test]
fn probe_measurements_are_settled_at_the_laid_out_width() {
    let open = |mode: FastMode| {
        let mut cx = oracle_app(0);
        cx.set_draw_on_flush(false);
        let window = cx.add_window(|_, _| FlexedText { tick: 0 });
        cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
            .unwrap();
        (cx, window)
    };
    let (mut kept, mut pooled) = (open(LAYOUT), open(FastMode::OFF));
    for tick in 0..4 {
        let frames: Vec<Vec<String>> = [&mut kept, &mut pooled]
            .into_iter()
            .map(|(cx, window)| {
                cx.update_window((*window).into(), |root, window, cx| {
                    root.downcast::<FlexedText>()
                        .unwrap()
                        .update(cx, |view, cx| {
                            view.tick = tick;
                            cx.notify();
                        });
                    window.draw(cx).clear(cx);
                    window.describe_rendered_frame()
                })
                .unwrap()
            })
            .collect();
        if let Some(difference) =
            crate::fast::verify::first_frame_difference(&frames[0], &frames[1])
        {
            panic!("frame {tick}: {difference}");
        }
    }
}

/// Wrapping text whose measurement is kept in a layout the test can read.
struct Wrapped {
    layout: Rc<std::cell::RefCell<Option<crate::TextLayout>>>,
}

impl Render for Wrapped {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let text = crate::StyledText::new("a sentence long enough to wrap in its narrow box");
        *self.layout.borrow_mut() = Some(text.layout().clone());
        div().w(px(80.)).child(text)
    }
}

/// A kept text leaf asked for its size at width zero, as a flex probe asks,
/// keeps a glyph to a line; settling measures it again at the width it is
/// laid out at, and it holds its wrapped lines again.
#[test]
fn a_leaf_last_measured_at_a_probe_width_is_measured_again_where_it_is_laid_out() {
    let layout = Rc::new(std::cell::RefCell::new(None));
    let mut cx = oracle_app(0);
    cx.set_draw_on_flush(false);
    let window = cx.add_window({
        let layout = layout.clone();
        move |_, _| Wrapped { layout }
    });
    let lines = |layout: &Rc<std::cell::RefCell<Option<crate::TextLayout>>>| {
        let layout = layout.borrow();
        let inner = layout.as_ref().unwrap().0.borrow();
        inner
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .map(|line| line.wrap_boundaries().len() + 1)
            .sum::<usize>()
    };
    cx.update_window(window.into(), |_, window, cx| {
        window.set_fast_mode(LAYOUT);
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    })
    .unwrap();
    let laid_out = lines(&layout);
    assert!(laid_out > 1, "the text did not wrap");
    cx.update_window(window.into(), |_, window, cx| {
        assert_eq!(
            crate::fast::layout::probe_and_settle_text_leaves(window, cx),
            1
        );
    })
    .unwrap();
    assert_eq!(lines(&layout), laid_out);
}

/// Truncated and wrapped text measure as before through width changes.
/// Truncated text recolored is not carried, since its runs no longer line up
/// with the text it was shaped from: its new closure measures it again, to
/// the same sizes, so it is replayed with the new colors.
#[test]
fn truncation_and_wrapping_measure_as_before() {
    let mut texts = lines(4);
    texts[0].text = "a long line that will not fit".into();
    texts[0].truncate = true;
    texts[1].text = "a long line that wraps over lines".into();
    let mut pair = Pair::open(texts);
    for width in [60., 200., 90., 90., 140.] {
        pair.step(|texts| texts.width = width);
    }
    let recolored = pair.step(|texts| texts.lines[0].color = 2);
    assert_eq!(
        (recolored.leaves_carried, recolored.leaves_replayed),
        (3, 1),
        "{recolored:?}"
    );
}

/// A leaf measured by a closure other than text's is dirtied and measured
/// every frame, as the pool does: nothing says what its measurement read.
#[test]
fn leaves_other_than_text_are_dirtied_every_frame() {
    let calls = Rc::new(Cell::new(0));
    let mut kept = open(LAYOUT, lines(2), Some(calls.clone()));
    draw(&mut kept);
    for _ in 0..3 {
        let before = calls.get();
        update(&mut kept, &|_| {});
        let (stats, _) = draw(&mut kept);
        assert!(calls.get() > before, "the custom leaf was not measured");
        assert_eq!(stats.leaves_carried, 2, "{stats:?}");
    }
}

/// When a frame is cleared, every measurement closure it gave a node is
/// taken: what a closure captured is released, and no node holds one.
#[test]
fn no_closure_outlives_its_frame() {
    let calls = Rc::new(Cell::new(0));
    let mut kept = open(LAYOUT, lines(3), Some(calls.clone()));
    for _ in 0..3 {
        update(&mut kept, &|_| {});
        draw(&mut kept);
        let (cx, window) = &mut kept;
        let held = cx
            .update_window((*window).into(), |_, window, _| {
                crate::fast::layout::closures_held(window.layout_engine.as_ref().unwrap())
            })
            .unwrap();
        assert_eq!(held, 0, "a node still holds a measurement closure");
        // The view's own handle and the one the element holds while drawn.
        assert!(Rc::strong_count(&calls) <= 2);
    }
}

/// Text carried for frames on end does not ask the line layout cache for
/// its lines, but holds them, and the cache keeps them while it does: when
/// the text is measured again (a scale change measures every text leaf
/// afresh), its lines are found there and nothing is shaped. Lines nothing
/// holds still go.
#[test]
fn carried_text_keeps_its_lines_in_the_line_layout_cache() {
    let mut kept = open(LAYOUT, lines(4), None);
    for _ in 0..4 {
        update(&mut kept, &|_| {});
        let (stats, _) = draw(&mut kept);
        assert_eq!(stats.lines_shaped, 0, "{stats:?}");
    }
    // The scale change draws a frame of its own: the run since is read.
    let (cx, window) = &mut kept;
    cx.update_window((*window).into(), |_, window, _| window.reset_fast_stats())
        .unwrap();
    cx.simulate_window_scale_factor_change((*window).into(), 1.5);
    update(&mut kept, &|_| {});
    draw(&mut kept);
    let (cx, window) = &mut kept;
    let rescaled = cx
        .update_window((*window).into(), |_, window, _| window.fast_stats())
        .unwrap();
    assert_eq!(rescaled.leaves_remeasured, 4, "{rescaled:?}");
    assert_eq!(rescaled.lines_shaped, 0, "{rescaled:?}");

    // Text that went away takes its lines with it.
    update(&mut kept, &|texts| texts.lines.truncate(1));
    draw(&mut kept);
    update(&mut kept, &|texts| {
        *texts = Texts {
            lines: lines(4),
            width: 120.,
            custom: None,
        }
    });
    let (back, _) = draw(&mut kept);
    assert!(back.lines_shaped > 0, "{back:?}");
}

/// A row whose first child, unidentified, is text or an empty box, before a
/// box placed after it.
struct Swap {
    text: bool,
}

impl Render for Swap {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .when(self.text, |this| this.child("hello wide world"))
            .when(!self.text, |this| this.child(div()))
            .child(div().size(px(10.)).bg(PALETTE[1]))
    }
}

/// Found by review: a text leaf is left clean at clear for the next text at
/// its key; an empty box claiming that key instead did not dirty it, and
/// kept the text's width in Taffy's cache. A node that was a text leaf is
/// dirtied when anything else claims it.
#[test]
fn text_replaced_by_another_element_at_its_key_is_laid_out_afresh() {
    let open = |mode: FastMode| {
        let mut cx = oracle_app(0);
        cx.set_draw_on_flush(false);
        let window = cx.add_window(|_, _| Swap { text: true });
        cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
            .unwrap();
        (cx, window)
    };
    let (mut kept, mut pooled) = (open(LAYOUT), open(FastMode::OFF));
    for (step, text) in [true, true, false, false, true, false]
        .into_iter()
        .enumerate()
    {
        let frames: Vec<Vec<String>> = [&mut kept, &mut pooled]
            .into_iter()
            .map(|(cx, window)| {
                cx.update_window((*window).into(), |root, window, cx| {
                    root.downcast::<Swap>().unwrap().update(cx, |view, cx| {
                        view.text = text;
                        cx.notify();
                    });
                    window.draw(cx).clear(cx);
                    window.describe_rendered_frame()
                })
                .unwrap()
            })
            .collect();
        if let Some(difference) =
            crate::fast::verify::first_frame_difference(&frames[0], &frames[1])
        {
            panic!("step {step}: {difference}");
        }
    }
}

/// The oracle's text system, whose lines come out twice as wide once fonts
/// are installed.
struct WideningTextSystem {
    inner: super::oracle_support::GlyphBoxTextSystem,
    wide: std::sync::atomic::AtomicBool,
}

impl crate::PlatformTextSystem for WideningTextSystem {
    fn add_fonts(&self, fonts: Vec<std::borrow::Cow<'static, [u8]>>) -> anyhow::Result<()> {
        self.wide.store(true, std::sync::atomic::Ordering::SeqCst);
        self.inner.add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        self.inner.all_font_names()
    }

    fn font_id(&self, descriptor: &crate::Font) -> anyhow::Result<crate::FontId> {
        self.inner.font_id(descriptor)
    }

    fn font_metrics(&self, font_id: crate::FontId) -> crate::FontMetrics {
        self.inner.font_metrics(font_id)
    }

    fn typographic_bounds(
        &self,
        font_id: crate::FontId,
        glyph_id: crate::GlyphId,
    ) -> anyhow::Result<crate::Bounds<f32>> {
        self.inner.typographic_bounds(font_id, glyph_id)
    }

    fn advance(
        &self,
        font_id: crate::FontId,
        glyph_id: crate::GlyphId,
    ) -> anyhow::Result<Size<f32>> {
        self.inner.advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: crate::FontId, ch: char) -> Option<crate::GlyphId> {
        self.inner.glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(
        &self,
        params: &crate::RenderGlyphParams,
    ) -> anyhow::Result<crate::Bounds<crate::DevicePixels>> {
        self.inner.glyph_raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        params: &crate::RenderGlyphParams,
        raster_bounds: crate::Bounds<crate::DevicePixels>,
    ) -> anyhow::Result<(Size<crate::DevicePixels>, Vec<u8>)> {
        self.inner.rasterize_glyph(params, raster_bounds)
    }

    fn layout_line(
        &self,
        text: &str,
        font_size: Pixels,
        runs: &[crate::FontRun],
    ) -> crate::LineLayout {
        let mut line = self.inner.layout_line(text, font_size, runs);
        if self.wide.load(std::sync::atomic::Ordering::SeqCst) {
            line.width = line.width * 2.;
            for run in &mut line.runs {
                for glyph in &mut run.glyphs {
                    glyph.position.x = glyph.position.x * 2.;
                }
            }
        }
        line
    }

    fn recommended_rendering_mode(
        &self,
        font_id: crate::FontId,
        font_size: Pixels,
    ) -> crate::TextRenderingMode {
        self.inner.recommended_rendering_mode(font_id, font_size)
    }
}

/// Found by review: installing fonts reshapes every text without fast mode,
/// while carried text kept the lines and size it was shaped into before.
/// Text measured under an earlier font generation is measured afresh.
#[test]
fn text_is_measured_afresh_after_fonts_are_installed() {
    let open = |mode: FastMode| {
        let mut cx = crate::TestAppContext::build_with_text_system(
            crate::TestDispatcher::new(0),
            None,
            std::sync::Arc::new(WideningTextSystem {
                inner: super::oracle_support::GlyphBoxTextSystem(crate::NoopTextSystem),
                wide: Default::default(),
            }),
        );
        cx.set_draw_on_flush(false);
        let window = cx.add_window(|_, _| Swap { text: true });
        cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
            .unwrap();
        (cx, window)
    };
    let (mut kept, mut pooled) = (open(LAYOUT), open(FastMode::OFF));
    for step in 0..4 {
        let frames: Vec<Vec<String>> = [&mut kept, &mut pooled]
            .into_iter()
            .map(|(cx, window)| {
                if step == 2 {
                    cx.update(|cx| {
                        cx.text_system()
                            .add_fonts(vec![std::borrow::Cow::Borrowed(&[])])
                            .unwrap()
                    });
                }
                cx.update_window((*window).into(), |root, window, cx| {
                    root.downcast::<Swap>()
                        .unwrap()
                        .update(cx, |_, cx| cx.notify());
                    window.draw(cx).clear(cx);
                    window.describe_rendered_frame()
                })
                .unwrap()
            })
            .collect();
        if let Some(difference) =
            crate::fast::verify::first_frame_difference(&frames[0], &frames[1])
        {
            panic!("step {step}: {difference}");
        }
    }
}
