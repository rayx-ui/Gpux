//! The clipboard's HTML entry (CLIP-1): HTML travels beside an item's plain-text entry,
//! `ClipboardItem::text` never returns it, and the test platform keeps both.

use gpux::{ClipboardEntry, ClipboardItem, ClipboardString, TestAppContext};

#[test]
fn html_sits_beside_the_plain_text_entry() {
    let item = ClipboardItem::new_string_with_html("bold".into(), "<b>bold</b>".into());
    assert_eq!(item.text().as_deref(), Some("bold"));
    assert_eq!(item.html(), Some("<b>bold</b>"));
    assert_eq!(
        item.entries(),
        [
            ClipboardEntry::String(ClipboardString::new("bold".into())),
            ClipboardEntry::Html("<b>bold</b>".into()),
        ]
    );
}

#[test]
fn text_never_returns_html() {
    let item = ClipboardItem {
        entries: vec![ClipboardEntry::Html("<i>only markup</i>".into())],
    };
    assert_eq!(item.text(), None);
    assert_eq!(item.html(), Some("<i>only markup</i>"));
}

#[test]
fn a_plain_item_has_no_html() {
    let item = ClipboardItem::new_string("plain".into());
    assert_eq!(item.html(), None);
    assert_eq!(item.text().as_deref(), Some("plain"));
    // Metadata still reads with HTML beside the string.
    let mut with_metadata = ClipboardItem::new_string_with_metadata("t".into(), "m".into());
    with_metadata
        .entries
        .push(ClipboardEntry::Html("<p>t</p>".into()));
    assert_eq!(with_metadata.metadata().map(String::as_str), Some("m"));
}

#[gpux::test]
fn the_test_platform_keeps_both_entries(cx: &mut TestAppContext) {
    cx.write_to_clipboard(ClipboardItem::new_string_with_html(
        "text".into(),
        "<p>text</p>".into(),
    ));
    let item = cx.read_from_clipboard().unwrap();
    assert_eq!(item.text().as_deref(), Some("text"));
    assert_eq!(item.html(), Some("<p>text</p>"));
    cx.write_to_clipboard(ClipboardItem::new_string("plain".into()));
    assert_eq!(cx.read_from_clipboard().unwrap().html(), None);
}
