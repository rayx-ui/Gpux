//! The deterministic test text system: `#[gpux::test(text = "bundled")]` and
//! `gpui_platform::bundled_text_system()` shape with the fonts `gpux_fonts` bundles and nothing
//! else, so the values below are the same on every desktop OS. Plain `#[gpux::test]` keeps the
//! test platform's `NoopTextSystem`.

use gpux::{
    App, Font, FontWeight, LineFragment, Pixels, SharedString, TestAppContext, TestDispatcher,
    TextRun, TruncateFrom, VisualTestContext, WindowTextSystem, font, px,
};
use std::sync::Arc;

const PLEX: &str = "IBM Plex Sans";
const LILEX: &str = "Lilex";
const PANGRAM: &str = "The quick brown fox jumps over the lazy dog";

fn run(len: usize, font: Font) -> TextRun {
    TextRun {
        len,
        font,
        color: Default::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

/// Asserts a measured length to a thousandth of a pixel, far below anything layout can observe.
#[track_caller]
fn assert_px(actual: Pixels, expected: f32) {
    let actual = f32::from(actual);
    assert!(
        (actual - expected).abs() < 1e-3,
        "expected {expected}px, measured {actual}px"
    );
}

fn window_text_system(cx: &mut TestAppContext) -> (Arc<WindowTextSystem>, &mut VisualTestContext) {
    let cx = cx.add_empty_window();
    let text_system = cx.update(|window, _| window.text_system().clone());
    (text_system, cx)
}

#[gpux::test(text = "bundled")]
fn bundled_fonts_are_the_only_fonts(cx: &mut TestAppContext) {
    assert_eq!(cx.text_system().all_font_names(), [PLEX, LILEX]);
}

#[gpux::test(text = "bundled")]
fn plex_sans_advances_and_line_metrics(cx: &mut TestAppContext) {
    let text_system = cx.text_system().clone();
    let plex = text_system.resolve_font(&font(PLEX));
    assert_px(
        text_system.advance(plex, px(16.), 'A').unwrap().width,
        10.256,
    );
    assert_px(
        text_system.advance(plex, px(16.), 'm').unwrap().width,
        13.968,
    );
    assert_px(text_system.advance(plex, px(16.), 'i').unwrap().width, 4.0);
    assert_px(text_system.ascent(plex, px(16.)), 16.4);
    assert_px(text_system.descent(plex, px(16.)), -4.4);

    let (window_text_system, _) = window_text_system(cx);
    let text = "Hello, gpux world!";
    let line =
        window_text_system.shape_line(text.into(), px(16.), &[run(text.len(), font(PLEX))], None);
    assert_px(line.width, 129.408);
    assert_px(line.ascent, 16.4);
    assert_px(line.descent, 4.4);
}

#[gpux::test(text = "bundled")]
fn lilex_is_monospaced_with_exact_metrics(cx: &mut TestAppContext) {
    let text_system = cx.text_system().clone();
    let lilex = text_system.resolve_font(&font(LILEX));
    for character in ['A', 'm', 'i', '0', ' '] {
        assert_px(
            text_system
                .advance(lilex, px(16.), character)
                .unwrap()
                .width,
            9.6,
        );
    }
    assert_px(text_system.ascent(lilex, px(16.)), 16.4);
    assert_px(text_system.descent(lilex, px(16.)), -4.4);

    let (window_text_system, _) = window_text_system(cx);
    let text = "Hello, gpux world!";
    let line =
        window_text_system.shape_line(text.into(), px(16.), &[run(text.len(), font(LILEX))], None);
    assert_px(line.width, 9.6 * text.len() as f32);
    assert_px(line.ascent, 16.4);
    assert_px(line.descent, 4.4);
}

#[gpux::test(text = "bundled")]
fn wrap_points_at_a_fixed_width(cx: &mut TestAppContext) {
    let (window_text_system, _) = window_text_system(cx);
    let lines = window_text_system
        .shape_text(
            PANGRAM.into(),
            px(16.),
            px(0.),
            &[run(PANGRAM.len(), font(PLEX))],
            Some(px(120.)),
            None,
        )
        .unwrap();
    assert_eq!(lines.len(), 1);
    let glyph_indices = lines[0]
        .wrap_boundaries()
        .iter()
        .map(|boundary| (boundary.run_ix, boundary.glyph_ix))
        .collect::<Vec<_>>();
    assert_eq!(glyph_indices, [(0, 10), (0, 20), (0, 35)]);

    let mut wrapper = window_text_system.line_wrapper(font(PLEX), px(16.), px(0.));
    let wrap_indices = wrapper
        .wrap_line(&[LineFragment::text(PANGRAM)], px(120.), Default::default())
        .map(|boundary| boundary.ix)
        .collect::<Vec<_>>();
    assert_eq!(wrap_indices, [10, 20, 35]);
    // "The quick " | "brown fox " | "jumps over the " | "lazy dog"
    assert_eq!(&PANGRAM[..10], "The quick ");
    assert_eq!(&PANGRAM[20..35], "jumps over the ");
}

#[gpux::test(text = "bundled")]
fn ellipsis_truncation(cx: &mut TestAppContext) {
    let (window_text_system, _) = window_text_system(cx);
    let mut wrapper = window_text_system.line_wrapper(font(PLEX), px(16.), px(0.));
    let runs = [run(PANGRAM.len(), font(PLEX))];
    let truncate = |wrapper: &mut gpux::LineWrapperHandle, from| {
        let (text, runs) = wrapper.truncate_line(PANGRAM.into(), px(150.), "…", &runs, from);
        let run_lengths = runs.iter().map(|run| run.len).collect::<Vec<_>>();
        (text, run_lengths)
    };

    let (text, run_lengths) = truncate(&mut wrapper, TruncateFrom::End);
    assert_eq!(text, SharedString::from("The quick brown fo…"));
    assert_eq!(run_lengths, [21]);
    let (text, run_lengths) = truncate(&mut wrapper, TruncateFrom::Middle);
    assert_eq!(text, SharedString::from("The quick br…y dog"));
    // The ellipsis joins the front run; the kept tail stays a run of its own.
    assert_eq!(run_lengths, [15, 5]);
    let (text, run_lengths) = truncate(&mut wrapper, TruncateFrom::Start);
    assert_eq!(text, SharedString::from("…s over the lazy dog"));
    assert_eq!(run_lengths, [22]);
}

#[gpux::test(text = "bundled")]
fn multi_run_styled_widths(cx: &mut TestAppContext) {
    let (window_text_system, _) = window_text_system(cx);
    let text = "regular bold code";
    let mut semibold = font(PLEX);
    semibold.weight = FontWeight::SEMIBOLD;
    let shape = |runs: &[TextRun]| {
        window_text_system
            .shape_line(text.into(), px(16.), runs, None)
            .width
    };

    let plain = shape(&[run(text.len(), font(PLEX))]);
    let styled = shape(&[
        run(8, font(PLEX)),
        run(5, semibold.clone()),
        run(4, font(LILEX)),
    ]);
    assert_px(plain, 125.216);
    assert_px(styled, 129.68);

    // Each run is shaped with its own face: the styled width is the sum of its parts.
    let parts = [
        ("regular ", font(PLEX)),
        ("bold ", semibold),
        ("code", font(LILEX)),
    ]
    .into_iter()
    .map(|(part, font)| {
        f32::from(
            window_text_system
                .shape_line(part.into(), px(16.), &[run(part.len(), font)], None)
                .width,
        )
    })
    .sum::<f32>();
    assert_px(styled, parts);
}

#[gpux::test(text = "bundled")]
fn unknown_family_falls_back_to_a_bundled_font(cx: &mut TestAppContext) {
    let text_system = cx.text_system().clone();
    let unknown = text_system.resolve_font(&font("No Such Family"));
    // GPUI's fallback stack starts with `.ZedMono`, which gpux maps to the bundled Lilex.
    for character in ['A', 'm', 'i'] {
        assert_px(
            text_system
                .advance(unknown, px(16.), character)
                .unwrap()
                .width,
            9.6,
        );
    }
    assert_eq!(text_system.all_font_names(), [PLEX, LILEX]);
}

#[gpux::test(text = "bundled")]
async fn async_tests_and_app_contexts_get_bundled_text(cx: &mut TestAppContext) {
    let names = cx.update(|cx: &mut App| cx.text_system().all_font_names());
    assert_eq!(names, [PLEX, LILEX]);
}

#[gpux::test(text = "bundled")]
fn app_parameters_get_bundled_text(cx: &mut App) {
    assert_eq!(cx.text_system().all_font_names(), [PLEX, LILEX]);
}

#[test]
fn explicit_constructor_uses_bundled_fonts() {
    let cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        gpui_platform::bundled_text_system(),
    );
    let text_system = cx.text_system().clone();
    assert_eq!(text_system.all_font_names(), [PLEX, LILEX]);
    let plex = text_system.resolve_font(&font(PLEX));
    assert_px(
        text_system.advance(plex, px(16.), 'A').unwrap().width,
        10.256,
    );
    cx.quit();
}

#[gpux::test]
fn default_context_keeps_noop_text_system(cx: &mut TestAppContext) {
    let text_system = cx.text_system().clone();
    assert!(text_system.all_font_names().is_empty());
    let any_font = text_system.resolve_font(&font(PLEX));
    // `NoopTextSystem` gives every one-UTF-16-unit glyph the same 600-unit advance.
    for character in ['A', 'm', 'i'] {
        assert_px(
            text_system
                .advance(any_font, px(16.), character)
                .unwrap()
                .width,
            9.6,
        );
    }
}

#[gpux::test(text = "noop")]
fn explicit_noop_matches_the_default(cx: &mut TestAppContext) {
    let text_system = cx.text_system().clone();
    assert!(text_system.all_font_names().is_empty());
    let any_font = text_system.resolve_font(&font(PLEX));
    assert_px(
        text_system.advance(any_font, px(16.), 'i').unwrap().width,
        9.6,
    );
}
