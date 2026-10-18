//! Limits on pasted HTML (final-review repair of CLIP-2, CLIP-3 and LIST-1): numbers that are
//! not finite are dropped and the rest bounded, list counters never overflow and their labels stay
//! short, and input within the size cap parses and pastes in linear time, however deeply it
//! nests or however many runs it alternates.

use gpux::{
    AppContext as _, AttributedText, ClipboardItem, Context, EditableText, EditableTextLineMode,
    EditableTextState, Entity, IntoElement, ParagraphFormat, Render, TestApp, TestAppWindow,
    TextAttributes, TextListItem, TextListKind, Window, attributed_text_from_html, div, prelude::*,
    px,
};

fn parse(html: &str) -> AttributedText {
    attributed_text_from_html(html, px(16.)).expect("within the size cap")
}

fn at(text: &AttributedText, offset: usize) -> TextAttributes {
    text.spans
        .iter()
        .find(|span| span.range.contains(&offset))
        .map(|span| (*span.attributes).clone())
        .unwrap_or_default()
}

#[track_caller]
fn assert_bounded(value: f32, limit: f32) {
    assert!(
        value.is_finite() && value.abs() <= limit,
        "{value} is not within {limit}"
    );
}

#[test]
fn non_finite_numbers_are_dropped_and_large_ones_bounded() {
    let parsed = parse(
        "<span style=\"font-size:infpx;letter-spacing:NaNpx;vertical-align:-1e30px;\
         font-weight:inf;color:rgb(NaN, 1e9, 0)\">a</span>\
         <span style=\"font-size:1e30px\">b</span>\
         <span style=\"font-size:1e30%\">c</span>",
    );
    let a = at(&parsed, 0);
    assert_eq!(a.font_size, None);
    assert_bounded(f32::from(a.letter_spacing.unwrap()), 1000.);
    assert_bounded(f32::from(a.baseline_shift.unwrap()), 1000.);
    assert_eq!(a.font_weight, None);
    assert_bounded(f32::from(at(&parsed, 1).font_size.unwrap()), 1000.);
    assert_bounded(f32::from(at(&parsed, 2).font_size.unwrap()), 1000.);

    let parsed = parse(
        "<p style=\"line-height:inf;margin-left:infpx;text-indent:1e30px;margin-top:-1e30px\">\
         a</p><p style=\"line-height:1e9\">b</p>",
    );
    let first = &parsed.paragraphs[0];
    assert_eq!(first.line_height, None);
    assert_bounded(f32::from(first.leading_indent), 1000.);
    assert_bounded(f32::from(first.first_line_indent), 1000.);
    assert_bounded(f32::from(first.space_before), 1000.);
    assert_bounded(parsed.paragraphs[1].line_height.unwrap(), 10.);
}

#[test]
fn a_huge_list_start_is_bounded() {
    let parsed = parse("<ol start=\"4294967295\"><li>a</li><li>b</li></ol>");
    let Some(TextListItem {
        kind: TextListKind::Numbered { start },
        ..
    }) = parsed.paragraphs[0].list
    else {
        panic!("a numbered item: {:?}", parsed.paragraphs);
    };
    assert!(start <= 1_000_000, "{start}");
}

struct Field {
    state: Entity<EditableTextState>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(300.))
            .h(px(300.))
            .child(EditableText::new(&self.state))
    }
}

fn open(text: &str) -> (TestApp, TestAppWindow<Field>, Entity<EditableTextState>) {
    let mut app = TestApp::new();
    let text = text.to_owned();
    let mut window = app.open_window(move |_, cx| Field {
        state: cx
            .new(|cx| EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx)),
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    (app, window, state)
}

/// An owner may store any start: counting past `u32::MAX` saturates, and deep levels' roman
/// labels fall back to digits instead of growing with the number.
#[test]
fn list_counters_saturate_and_labels_stay_short() {
    for level in [0, 1, 2] {
        let (mut app, mut window, state) = open("a\nb\nc");
        app.update_entity(&state, |state, cx| {
            let end = state.text().len();
            assert!(state.set_paragraph_format(
                0..end,
                ParagraphFormat {
                    list: Some(TextListItem {
                        kind: TextListKind::Numbered { start: u32::MAX },
                        level,
                    }),
                    ..Default::default()
                },
                cx,
            ));
        });
        window.draw();
        app.read_entity(&state, |state, _| {
            let markers = state.layout_map().list_markers();
            assert_eq!(markers.len(), 3);
            for marker in markers {
                assert!(marker.text.len() <= 16, "{:?}", marker.text);
            }
        });
    }
}

#[test]
fn deep_nesting_within_the_cap_parses_in_linear_time() {
    // 3.5 MB of unclosed elements, the innermost preserving whitespace.
    let count = 500_000;
    let html = format!("<pre>{}", "<span>a".repeat(count));
    let parsed = parse(&html);
    assert_eq!(parsed.text.len(), count);
}

#[test]
fn many_alternating_runs_parse_and_paste_in_linear_time() {
    let count = 100_000;
    let html = "<b>a</b><i>b</i>".repeat(count);
    let parsed = parse(&html);
    assert_eq!(parsed.text.len(), 2 * count);
    assert_eq!(parsed.spans.len(), 2 * count);

    // No window: this measures the paste, not laying out 200,000 alternating runs.
    let mut app = TestApp::new();
    let state = app.update(|cx| {
        cx.new(|cx| EditableTextState::with_line_mode("", EditableTextLineMode::MultiLine, cx))
    });
    app.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_string_with_html(
            parsed.text.clone(),
            html.clone(),
        ))
    });
    app.update_entity(&state, |state, cx| {
        state.set_rich_paste(true);
        assert!(state.paste(cx));
        assert_eq!(state.text().len(), 2 * count);
        assert_eq!(state.attribute_spans().len(), 2 * count);
    });
}
