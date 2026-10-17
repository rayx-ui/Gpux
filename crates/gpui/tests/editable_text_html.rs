//! The HTML codec (CLIP-2): `AttributedText` to an HTML subset and back. Its own output round
//! trips exactly; the common subset browsers, Google Docs, Word and Pages write is accepted;
//! scripts, styles, heads and comments are dropped, entities decoded, and input above 4 MiB
//! refused. Inline objects export as their HTML fallback, or their escaped text fallback.

use gpux::{
    AttributeSpan, AttributedText, FontStyle, FontWeight, InlineObject, MAX_HTML_LEN,
    ParagraphAlignment, ParagraphFormat, SharedString, StrikethroughStyle, TextAttributes,
    TextListItem, TextListKind, UnderlineStyle, attributed_text_from_html, attributed_text_to_html,
    px, rgb,
};

const BASE: f32 = 16.;

fn parse(html: &str) -> AttributedText {
    attributed_text_from_html(html, px(BASE)).expect("within the size cap")
}

fn bold() -> TextAttributes {
    TextAttributes {
        font_weight: Some(FontWeight::BOLD),
        ..Default::default()
    }
}

/// The attributes covering `offset` in `text`.
fn at(text: &AttributedText, offset: usize) -> TextAttributes {
    text.spans
        .iter()
        .find(|span| span.range.contains(&offset))
        .map(|span| (*span.attributes).clone())
        .unwrap_or_default()
}

fn rich_document() -> AttributedText {
    let text =
        "Title here\nplain  bold italic   link\nfirst\nnested\nsecond\ndone\nlast <&> \"q\"\n\nend";
    let find = |needle: &str| {
        let start = text.find(needle).unwrap();
        start..start + needle.len()
    };
    let spans = vec![
        AttributeSpan::new(
            find("Title"),
            TextAttributes {
                font_size: Some(px(28.)),
                font_family: Some("Lilex".into()),
                ..bold()
            },
        ),
        AttributeSpan::new(find("bold"), bold()),
        AttributeSpan::new(
            find("italic"),
            TextAttributes {
                font_style: Some(FontStyle::Italic),
                color: Some(rgb(0xff0000).into()),
                background_color: Some(rgb(0x00ff00).into()),
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: None,
                    wavy: false,
                }),
                ..Default::default()
            },
        ),
        AttributeSpan::new(
            find("link"),
            TextAttributes {
                link: Some("https://gpux.dev/?a=1&b=2".into()),
                tag: Some("ref".into()),
                strikethrough: Some(StrikethroughStyle {
                    thickness: px(2.),
                    color: Some(rgb(0x0000ff).into()),
                }),
                ..Default::default()
            },
        ),
        AttributeSpan::new(
            find("nested"),
            TextAttributes {
                baseline_shift: Some(px(4.)),
                letter_spacing: Some(px(1.5)),
                font_weight: Some(FontWeight::SEMIBOLD),
                ..Default::default()
            },
        ),
        AttributeSpan::new(
            find("done"),
            TextAttributes {
                baseline_shift: Some(px(-3.)),
                underline: Some(UnderlineStyle {
                    thickness: px(2.),
                    color: Some(rgb(0xff00ff).into()),
                    wavy: true,
                }),
                ..Default::default()
            },
        ),
    ];
    let paragraphs = vec![
        ParagraphFormat {
            alignment: Some(ParagraphAlignment::Center),
            space_after: px(8.),
            ..Default::default()
        },
        ParagraphFormat {
            alignment: Some(ParagraphAlignment::Justify),
            first_line_indent: px(-12.),
            leading_indent: px(20.),
            trailing_indent: px(4.),
            space_before: px(6.),
            line_height: Some(1.5),
            letter_spacing: Some(px(0.5)),
            ..Default::default()
        },
        ParagraphFormat {
            list: Some(TextListItem {
                kind: TextListKind::Numbered { start: 3 },
                level: 0,
            }),
            ..Default::default()
        },
        ParagraphFormat {
            list: Some(TextListItem::bullet(1)),
            ..Default::default()
        },
        ParagraphFormat {
            list: Some(TextListItem::numbered(0)),
            ..Default::default()
        },
        ParagraphFormat {
            list: Some(TextListItem {
                kind: TextListKind::Checkbox { checked: true },
                level: 0,
            }),
            ..Default::default()
        },
        ParagraphFormat {
            list: Some(TextListItem {
                kind: TextListKind::Custom("->".into()),
                level: 0,
            }),
            ..Default::default()
        },
        ParagraphFormat::default(),
        ParagraphFormat::default(),
    ];
    AttributedText {
        text: text.into(),
        spans,
        paragraphs,
    }
}

#[test]
fn serialization_writes_the_subset() {
    let html = attributed_text_to_html(&rich_document());
    for expected in [
        "<p style=\"text-align:center;margin-bottom:8px;\">",
        "<b>bold</b>",
        "<i>",
        "<u>",
        "<a href=\"https://gpux.dev/?a=1&amp;b=2\">",
        "<ol start=\"3\">",
        "<ul>",
        "<ul data-gpux-checklist><li data-checked=\"true\">",
        "<sup style=\"vertical-align:4px\">",
        "<sub style=\"vertical-align:-3px\">",
        "font-family:'Lilex';",
        "font-size:28px;",
        "color:#ff0000;",
        "background-color:#00ff00;",
        "last &lt;&amp;&gt; &quot;q&quot;",
    ] {
        assert!(html.contains(expected), "{expected} missing from {html}");
    }
    assert!(!html.contains('\n'), "breaks become paragraphs: {html}");
}

#[test]
fn its_own_output_round_trips_exactly() {
    let document = rich_document();
    let parsed = parse(&attributed_text_to_html(&document));
    assert_eq!(parsed.text, document.text);
    assert_eq!(parsed.paragraphs, document.paragraphs);
    assert_eq!(parsed.spans, document.spans);
    assert_eq!(parsed, document);

    let plain = AttributedText::plain("just  text\n\n  spaced");
    assert_eq!(parse(&attributed_text_to_html(&plain)), plain);
}

#[test]
fn the_common_inline_subset_is_accepted() {
    let parsed = parse(
        "<strong>a</strong><em>b</em><b>c</b><i>d</i><u>e</u><s>f</s><strike>g</strike>\
         <del>h</del><sup>i</sup><sub>j</sub><code>k</code><a href=\"https://x.test\">l</a>\
         <span style=\"color: rgb(255, 0, 0); font-size: 12pt; font-weight: 700\">m</span>\
         <font color=\"#00f\" face=\"Lilex, monospace\">n</font>",
    );
    assert_eq!(parsed.text, "abcdefghijklmn");
    assert_eq!(at(&parsed, 0).font_weight, Some(FontWeight::BOLD));
    assert_eq!(at(&parsed, 1).font_style, Some(FontStyle::Italic));
    assert_eq!(at(&parsed, 2).font_weight, Some(FontWeight::BOLD));
    assert_eq!(at(&parsed, 3).font_style, Some(FontStyle::Italic));
    assert!(at(&parsed, 4).underline.is_some());
    for offset in 5..8 {
        assert!(at(&parsed, offset).strikethrough.is_some(), "{offset}");
    }
    assert!(at(&parsed, 8).baseline_shift.unwrap() > px(0.));
    assert!(at(&parsed, 9).baseline_shift.unwrap() < px(0.));
    assert_eq!(at(&parsed, 10), TextAttributes::default());
    assert_eq!(at(&parsed, 11).link.as_deref(), Some("https://x.test"));
    let m = at(&parsed, 12);
    assert_eq!(m.color, Some(rgb(0xff0000).into()));
    assert_eq!(m.font_size, Some(px(16.)));
    assert_eq!(m.font_weight, Some(FontWeight::BOLD));
    let n = at(&parsed, 13);
    assert_eq!(n.color, Some(rgb(0x0000ff).into()));
    assert_eq!(n.font_family.as_deref(), Some("Lilex"));
}

#[test]
fn blocks_and_breaks_become_paragraphs() {
    let parsed =
        parse("<p>one</p><div>two<br>three</div>\n  <p style=\"text-align:right\">four</p>");
    assert_eq!(parsed.text, "one\ntwo\nthree\nfour");
    assert_eq!(
        parsed.paragraphs[3].alignment,
        Some(ParagraphAlignment::Right)
    );
}

#[test]
fn headings_are_bold_paragraphs_on_the_size_scale() {
    let parsed = parse("<h1>a</h1><h2>b</h2><h3>c</h3><h4>d</h4><h5>e</h5><h6>f</h6><p>g</p>");
    assert_eq!(parsed.text, "a\nb\nc\nd\ne\nf\ng");
    for (index, scale) in [2., 1.5, 1.17, 1., 0.83, 0.67].into_iter().enumerate() {
        let attributes = at(&parsed, index * 2);
        assert_eq!(attributes.font_weight, Some(FontWeight::BOLD));
        assert_eq!(attributes.font_size, Some(px(BASE) * scale));
    }
    assert_eq!(at(&parsed, 12), TextAttributes::default());
}

#[test]
fn lists_nest_by_level() {
    let parsed =
        parse("<ol start=\"5\"><li>a</li><li>b<ul><li>c</li></ul></li></ol><ul><li>d</li></ul>");
    assert_eq!(parsed.text, "a\nb\nc\nd");
    let items: Vec<_> = parsed
        .paragraphs
        .iter()
        .map(|format| format.list.clone())
        .collect();
    assert_eq!(
        items,
        [
            Some(TextListItem {
                kind: TextListKind::Numbered { start: 5 },
                level: 0
            }),
            Some(TextListItem::numbered(0)),
            Some(TextListItem::bullet(1)),
            Some(TextListItem::bullet(0)),
        ]
    );
}

#[test]
fn images_become_their_alt_text() {
    let parsed = parse("before <img src=\"https://evil.test/x.png\" alt=\"a picture\"> after");
    assert_eq!(parsed.text, "before a picture after");
}

#[test]
fn scripts_styles_heads_and_comments_are_dropped() {
    let parsed = parse(
        "<html><head><title>T</title><style>p { color: red }</style></head><body>\
         <!-- a comment --><script>alert('<b>x</b>')</script><p>kept</p>\
         <!--[if gte mso 9]><xml>w</xml><![endif]--></body></html>",
    );
    assert_eq!(parsed.text, "kept");
    assert!(parsed.spans.is_empty());
}

#[test]
fn entities_are_decoded() {
    let parsed = parse("<p>&lt;a&gt; &amp; &quot;b&quot; &#169; &eacute;&nbsp;x</p>");
    assert_eq!(parsed.text, "<a> & \"b\" \u{a9} \u{e9}\u{a0}x");
}

#[test]
fn whitespace_collapses_outside_preformatted_text() {
    let parsed = parse("<p>  many   spaces\n here  </p><pre>keep  this</pre>");
    assert_eq!(parsed.text, "many spaces here \nkeep  this");
}

#[test]
fn office_markup_is_accepted() {
    // Google Docs wraps the copy in a normal-weight <b>; Word writes MSO classes and pt sizes.
    let parsed = parse(
        "<meta charset=\"utf-8\"><b style=\"font-weight:normal;\" id=\"docs-internal-guid-1\">\
         <span style=\"font-weight:700;font-style:italic\">Docs</span>\
         <span style=\"font-weight:400\"> text</span></b>\
         <p class=MsoNormal><b><span style='font-size:14.0pt;font-family:\"Lilex\"'>Word</span></b></p>",
    );
    assert_eq!(parsed.text, "Docs text\nWord");
    let docs = at(&parsed, 0);
    assert_eq!(docs.font_weight, Some(FontWeight::BOLD));
    assert_eq!(docs.font_style, Some(FontStyle::Italic));
    assert_eq!(at(&parsed, 5).font_weight, Some(FontWeight::NORMAL));
    let word = at(&parsed, 10);
    assert_eq!(word.font_weight, Some(FontWeight::BOLD));
    assert_eq!(word.font_family.as_deref(), Some("Lilex"));
    assert!((f32::from(word.font_size.unwrap()) - 14. * 4. / 3.).abs() < 0.01);
}

#[test]
fn input_above_the_cap_is_refused() {
    let small = "<p>x</p>";
    assert!(attributed_text_from_html(small, px(BASE)).is_some());
    let large = "a".repeat(MAX_HTML_LEN + 1);
    assert!(attributed_text_from_html(&large, px(BASE)).is_none());
    assert_eq!(MAX_HTML_LEN, 4 * 1024 * 1024);
}

/// OBJ-1's HTML export: an object writes its trusted HTML fallback, else its escaped text
/// fallback.
#[test]
fn objects_export_as_their_fallbacks() {
    let object = |html: Option<&str>| InlineObject {
        key: SharedString::from("chip"),
        width: px(30.),
        height: px(10.),
        baseline_offset: px(2.),
        text_fallback: SharedString::from("<chip>"),
        html_fallback: html.map(SharedString::from),
    };
    let document = |object: InlineObject| AttributedText {
        text: "a\u{FFFC}b".into(),
        spans: vec![AttributeSpan::new(
            1..4,
            TextAttributes {
                object: Some(object),
                ..Default::default()
            },
        )],
        paragraphs: Vec::new(),
    };
    let with_html =
        attributed_text_to_html(&document(object(Some("<span class=\"chip\">@ana</span>"))));
    assert!(
        with_html.contains("a<span class=\"chip\">@ana</span>b"),
        "{with_html}"
    );
    let without = attributed_text_to_html(&document(object(None)));
    assert!(without.contains("a&lt;chip&gt;b"), "{without}");
    assert_eq!(parse(&without).text, "a<chip>b");
}

/// Found by review: a stored family cannot break out of the style attribute or inject CSS, and
/// the output still round trips.
#[test]
fn style_values_are_escaped_and_families_sanitized() {
    let pasted = parse(
        "<span style=\"font-family:'x&quot;&gt;&lt;img src=x onerror=alert(1)&gt;;color:red'\">a</span>",
    );
    let family = at(&pasted, 0).font_family.expect("a family");
    assert!(!family.contains(['"', '\'', '<', '>', ';']), "{family:?}");
    let text = AttributedText {
        text: "a".into(),
        spans: vec![AttributeSpan::new(
            0..1,
            TextAttributes {
                font_family: Some("Evil\"><img src=x onerror=alert(1)>;color:red".into()),
                color: Some(rgb(0x112233).into()),
                ..Default::default()
            },
        )],
        paragraphs: Vec::new(),
    };
    let html = attributed_text_to_html(&text);
    assert!(!html.contains("<img"), "{html}");
    assert!(
        html.contains("<span style=\"font-family:'Evilimg src=x onerror=alert(1)color:red';"),
        "{html}"
    );
    let parsed = parse(&html);
    assert_eq!(parsed.text, "a");
    assert_eq!(at(&parsed, 0).color, Some(rgb(0x112233).into()));
    assert!(
        !at(&parsed, 0)
            .font_family
            .unwrap()
            .contains(['"', ';', '<'])
    );
}

/// Found by review: a head without `</head>` ends where the body starts, and a script in the
/// head stays raw text even when it holds `</head>`.
#[test]
fn an_unclosed_head_ends_at_the_body_and_its_scripts_stay_raw() {
    let parsed =
        parse("<html><head><meta charset=utf-8><title>t</title><body><p>kept</p></body></html>");
    assert_eq!(parsed.text, "kept");
    let parsed = parse("<head><script>var a = '</head><p>leak</p>';</script></head><p>kept</p>");
    assert_eq!(parsed.text, "kept");
    let parsed = parse("<head><style>p{}</style><p>body text</p>");
    assert_eq!(parsed.text, "body text");
}
