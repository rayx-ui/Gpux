//! Per-run font size, letter spacing and baseline shift in the shared text system (RUN-1):
//! runs that differ only in their metrics stay separate through every merge loop and cache key,
//! the shared post-pass spaces and shifts each run on its own, hit testing stays consistent
//! across sizes, and runs without metrics lay out exactly as before.

use gpux::{
    Context, FastMode, Font, IntoElement, LineLayout, ParentElement as _, Pixels, Render,
    RunMetrics, SharedString, Styled as _, StyledText, TestAppContext, TextLayout, TextRun,
    VisualTestContext, Window, WindowTextSystem, div, font, px,
};
use std::sync::Arc;

const PLEX: &str = "IBM Plex Sans";

fn run(len: usize, font: Font) -> TextRun {
    TextRun {
        len,
        font,
        ..Default::default()
    }
}

fn sized(len: usize, size: f32) -> TextRun {
    TextRun {
        metrics: RunMetrics {
            font_size: Some(px(size)),
            ..Default::default()
        },
        ..run(len, font(PLEX))
    }
}

#[track_caller]
fn assert_px(actual: Pixels, expected: f32) {
    let actual = f32::from(actual);
    assert!(
        (actual - expected).abs() < 1e-3,
        "expected {expected}px, measured {actual}px"
    );
}

fn window_text_system(cx: &mut TestAppContext) -> Arc<WindowTextSystem> {
    let cx = cx.add_empty_window();
    cx.update(|window, _| window.text_system().clone())
}

fn glyph_xs(layout: &LineLayout) -> Vec<f32> {
    layout
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| f32::from(glyph.position.x)))
        .collect()
}

fn glyph_ys(layout: &LineLayout) -> Vec<f32> {
    layout
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| f32::from(glyph.position.y)))
        .collect()
}

#[test]
fn default_run_inherits_every_metric() {
    let metrics = TextRun::default().metrics;
    assert!(metrics.is_inherited());
    assert_eq!(metrics.font_size_or(px(13.)), px(13.));
    assert_eq!(metrics.letter_spacing_or(px(2.)), px(2.));
    assert_eq!(metrics.baseline_shift, px(0.));
}

#[gpux::test(text = "bundled")]
fn runs_without_metrics_lay_out_as_before(cx: &mut TestAppContext) {
    let text_system = window_text_system(cx);
    let text = "Hello, gpux world!";
    // The bundled Plex values the text system measured before runs had metrics.
    let line = text_system.shape_line(text.into(), px(16.), &[run(text.len(), font(PLEX))], None);
    assert_px(line.width, 129.408);
    assert_px(line.ascent, 16.4);
    assert_px(line.descent, 4.4);
    assert_eq!(line.runs.len(), 1);
    assert_eq!(line.runs[0].font_size, px(16.));

    // A run that names the line's own size and spacing shapes identically.
    let explicit = TextRun {
        metrics: RunMetrics {
            font_size: Some(px(16.)),
            letter_spacing: Some(px(0.)),
            baseline_shift: px(0.),
        },
        ..run(text.len(), font(PLEX))
    };
    let same = text_system.layout_line(text, px(16.), px(0.), &[explicit], None);
    assert_eq!(glyph_xs(&same), glyph_xs(&line));
    assert_eq!(same.width, line.width);
    assert_eq!(same.ascent, line.ascent);
    assert_eq!(same.descent, line.descent);
}

#[gpux::test(text = "bundled")]
fn each_run_shapes_at_its_own_size(cx: &mut TestAppContext) {
    let text_system = window_text_system(cx);
    let text = "small LARGE";
    let line =
        text_system.layout_line(text, px(16.), px(0.), &[sized(6, 12.), sized(5, 28.)], None);

    let sizes: Vec<Pixels> = line.runs.iter().map(|run| run.font_size).collect();
    assert_eq!(sizes, [px(12.), px(28.)]);
    let xs = glyph_xs(&line);
    assert!(xs.windows(2).all(|pair| pair[0] < pair[1]), "{xs:?}");

    // Each run is as wide as itself shaped alone at its size.
    let alone = |part: &str, size: f32| {
        text_system
            .layout_line(part, px(size), px(0.), &[run(part.len(), font(PLEX))], None)
            .width
    };
    assert_px(
        line.width,
        f32::from(alone("small ", 12.) + alone("LARGE", 28.)),
    );
    let large_ascent = text_system.ascent(text_system.resolve_font(&font(PLEX)), px(28.));
    assert!(
        line.ascent >= large_ascent,
        "{:?} < {large_ascent:?}",
        line.ascent
    );
}

#[gpux::test(text = "bundled")]
fn runs_differing_only_in_metrics_stay_separate(cx: &mut TestAppContext) {
    let text_system = window_text_system(cx);
    let text = "aaaabbbb";
    let runs = [run(4, font(PLEX)), sized(4, 24.)];

    let shaped = text_system.shape_line(text.into(), px(16.), &runs, None);
    assert_eq!(shaped.runs.len(), 2);
    assert_eq!(shaped.runs[1].font_size, px(24.));

    let wrapped = text_system
        .shape_text(text.into(), px(16.), px(0.), &runs, None, None)
        .unwrap();
    assert_eq!(wrapped[0].runs().len(), 2);
    assert_eq!(wrapped[0].runs()[1].font_size, px(24.));

    let by_hash =
        text_system
            .layout_line_by_hash(7, text.len(), px(16.), px(0.), &runs, None, || text.into());
    assert_eq!(by_hash.runs.len(), 2);
    assert_eq!(by_hash.runs[1].font_size, px(24.));
}

#[gpux::test(text = "bundled")]
fn cache_keys_include_run_metrics(cx: &mut TestAppContext) {
    let text_system = window_text_system(cx);
    let text = "cached";
    let plain = [run(text.len(), font(PLEX))];
    let large = [sized(text.len(), 32.)];

    let first = text_system.layout_line(text, px(16.), px(0.), &plain, None);
    let again = text_system.layout_line(text, px(16.), px(0.), &plain, None);
    assert!(Arc::ptr_eq(&first, &again), "unchanged runs hit the cache");
    let other = text_system.layout_line(text, px(16.), px(0.), &large, None);
    assert!(!Arc::ptr_eq(&first, &other));
    assert!(other.width > first.width * 1.5);

    let first =
        text_system
            .layout_line_by_hash(1, text.len(), px(16.), px(0.), &plain, None, || text.into());
    let other =
        text_system
            .layout_line_by_hash(1, text.len(), px(16.), px(0.), &large, None, || text.into());
    assert!(other.width > first.width * 1.5);
    assert!(
        text_system
            .try_layout_line_by_hash(1, text.len(), px(16.), px(0.), &large, None)
            .is_some_and(|cached| Arc::ptr_eq(&cached, &other))
    );

    let wrapped = |runs: &[TextRun]| {
        text_system
            .shape_text(text.into(), px(16.), px(0.), runs, None, None)
            .unwrap()[0]
            .width()
    };
    assert!(wrapped(&large) > wrapped(&plain) * 1.5);
}

#[gpux::test(text = "bundled")]
fn letter_spacing_applies_per_run(cx: &mut TestAppContext) {
    let text_system = window_text_system(cx);
    let text = "abcdabcd";
    let spaced = TextRun {
        metrics: RunMetrics {
            letter_spacing: Some(px(3.)),
            ..Default::default()
        },
        ..run(4, font(PLEX))
    };
    let unspaced = text_system.layout_line(text, px(16.), px(0.), &[run(8, font(PLEX))], None);
    let line = text_system.layout_line(text, px(16.), px(0.), &[spaced, run(4, font(PLEX))], None);

    // Each of the four spaced clusters is followed by 3px; the unspaced run moves by all four.
    let before = glyph_xs(&unspaced);
    let after = glyph_xs(&line);
    for (ix, (before, after)) in before.iter().zip(&after).enumerate() {
        let expected = 3. * ix.min(4) as f32;
        assert!(
            (after - before - expected).abs() < 1e-3,
            "glyph {ix}: {before} -> {after}"
        );
    }
    assert_px(line.width, f32::from(unspaced.width) + 12.);
    assert_eq!(line.letter_spacing, px(0.));

    // The line's spacing applies to runs that set none.
    let inherited = text_system.layout_line(text, px(16.), px(2.), &[run(8, font(PLEX))], None);
    assert_px(inherited.width, f32::from(unspaced.width) + 14.);
}

#[gpux::test(text = "bundled")]
fn baseline_shift_moves_glyphs_and_line_extents(cx: &mut TestAppContext) {
    let text_system = window_text_system(cx);
    let text = "x2y2";
    let raised = |shift: f32| TextRun {
        metrics: RunMetrics {
            baseline_shift: px(shift),
            ..Default::default()
        },
        ..run(1, font(PLEX))
    };
    let plain = text_system.layout_line(text, px(16.), px(0.), &[run(4, font(PLEX))], None);
    let line = text_system.layout_line(
        text,
        px(16.),
        px(0.),
        &[
            run(1, font(PLEX)),
            raised(6.),
            run(1, font(PLEX)),
            raised(-4.),
        ],
        None,
    );

    assert_eq!(glyph_ys(&line), [0., -6., 0., 4.]);
    assert_eq!(glyph_xs(&line), glyph_xs(&plain));
    assert_px(line.ascent, f32::from(plain.ascent) + 6.);
    assert_px(line.descent, f32::from(plain.descent) + 4.);
}

#[gpux::test(text = "bundled")]
fn hit_testing_is_consistent_across_sizes(cx: &mut TestAppContext) {
    let text_system = window_text_system(cx);
    let text = "abcWXYZ";
    let line =
        text_system.layout_line(text, px(16.), px(0.), &[sized(3, 10.), sized(4, 30.)], None);

    for index in 0..text.len() {
        let x = line.x_for_index(index);
        let next = line.x_for_index(index + 1);
        assert!(next > x, "index {index}");
        let middle = x + (next - x) * 0.25;
        assert_eq!(line.index_for_x(middle), Some(index));
        if index + 1 < text.len() {
            // Past the last glyph's start the closest boundary is the line's end.
            assert_eq!(line.closest_index_for_x(x + px(0.1)), index);
        }
    }
    assert_eq!(line.x_for_index(text.len()), line.width);
    assert_eq!(line.index_for_x(line.width + px(1.)), None);
}

#[gpux::test]
fn test_platform_text_system_honors_run_sizes(cx: &mut TestAppContext) {
    let text_system = window_text_system(cx);
    let text = "abcd";
    let plain = text_system.layout_line(text, px(10.), px(0.), &[run(4, font(PLEX))], None);
    let line =
        text_system.layout_line(text, px(10.), px(0.), &[sized(2, 10.), sized(2, 20.)], None);

    let sizes: Vec<Pixels> = line.runs.iter().map(|run| run.font_size).collect();
    assert_eq!(sizes, [px(10.), px(20.)]);
    // The test text system's glyphs are an em of their run's size wide.
    let em = plain.width / 4.;
    assert_px(line.width, f32::from(em * 2. + em * 4.));
    assert_eq!(glyph_xs(&line)[..2], glyph_xs(&plain)[..2]);
    assert!(line.ascent > plain.ascent);
    // Found by review: a run naming the line's own size lays out as an inherited one,
    // descent sign included.
    let explicit = text_system.layout_line(text, px(10.), px(0.), &[sized(4, 10.)], None);
    assert_eq!(glyph_xs(&explicit), glyph_xs(&plain));
    assert_eq!(
        (explicit.width, explicit.ascent, explicit.descent),
        (plain.width, plain.ascent, plain.descent)
    );
}

/// Text whose runs change only their metrics between frames.
struct MetricsText {
    large: bool,
    layout: Option<TextLayout>,
}

impl Render for MetricsText {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let text: SharedString = "metrics".into();
        let runs = if self.large {
            vec![sized(text.len(), 40.)]
        } else {
            vec![run(text.len(), font(PLEX))]
        };
        let styled = StyledText::new(text).with_runs(runs);
        self.layout = Some(styled.layout().clone());
        div().w(px(400.)).child(styled)
    }
}

fn measured_width(fast: FastMode, cx: &mut TestAppContext) -> (Pixels, Pixels) {
    let (view, cx): (_, &mut VisualTestContext) = cx.add_window_view(|_, _| MetricsText {
        large: false,
        layout: None,
    });
    cx.update(|window, _| window.set_fast_mode(fast));
    cx.run_until_parked();
    let width = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.layout.as_ref().unwrap().line_layouts()[0].width()
        })
    };
    let small = width(cx);
    view.update(cx, |view, cx| {
        view.large = true;
        cx.notify();
    });
    cx.run_until_parked();
    (small, width(cx))
}

#[gpux::test(text = "bundled")]
fn fast_mode_text_carry_reshapes_on_metric_changes(cx: &mut TestAppContext) {
    let (small, large) = measured_width(FastMode::ALL, cx);
    assert!(large > small * 2., "{small:?} -> {large:?}");
    let (off_small, off_large) = measured_width(FastMode::OFF, cx);
    assert_eq!((small, large), (off_small, off_large));
}
