use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation as _;

pub(crate) fn previous_grapheme_boundary(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .rev()
        .find_map(|(index, _)| (index < offset).then_some(index))
        .unwrap_or(0)
}

pub(crate) fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .find_map(|(index, _)| (index > offset).then_some(index))
        .unwrap_or(text.len())
}

pub(crate) fn grapheme_boundary_at_or_before(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset == text.len() {
        return text.len();
    }
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .take_while(|index| *index <= offset)
        .last()
        .unwrap_or(0)
}

pub(crate) fn previous_word_boundary(text: &str, offset: usize) -> usize {
    let offset = grapheme_boundary_at_or_before(text, offset);
    text.split_word_bound_indices()
        .filter(|(_, segment)| !segment.chars().all(char::is_whitespace))
        .map(|(index, _)| index)
        .take_while(|index| *index < offset)
        .last()
        .unwrap_or(0)
}

pub(crate) fn next_word_boundary(text: &str, offset: usize) -> usize {
    let offset = grapheme_boundary_at_or_before(text, offset);
    text.split_word_bound_indices()
        .filter(|(_, segment)| !segment.chars().all(char::is_whitespace))
        .map(|(index, _)| index)
        .find(|index| *index > offset)
        .unwrap_or(text.len())
}

pub(crate) fn word_range_at(text: &str, offset: usize) -> Range<usize> {
    let offset = grapheme_boundary_at_or_before(text, offset);
    text.split_word_bound_indices()
        .map(|(start, segment)| start..start + segment.len())
        .find(|range| offset < range.end)
        .unwrap_or(text.len()..text.len())
}

pub(crate) fn logical_line_range_at(text: &str, offset: usize) -> Range<usize> {
    let offset = grapheme_boundary_at_or_before(text, offset);
    let start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index);
    start..end
}

pub(crate) fn previous_logical_line_start(text: &str, offset: usize) -> usize {
    let offset = grapheme_boundary_at_or_before(text, offset);
    let current_start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    if current_start == 0 {
        return 0;
    }
    text[..current_start - 1]
        .rfind('\n')
        .map_or(0, |index| index + 1)
}

pub(crate) fn next_logical_line_end(text: &str, offset: usize) -> usize {
    let offset = grapheme_boundary_at_or_before(text, offset);
    let Some(current_end) = text[offset..].find('\n').map(|index| offset + index) else {
        return text.len();
    };
    let next_start = current_end + 1;
    text[next_start..]
        .find('\n')
        .map_or(text.len(), |index| next_start + index)
}

pub(crate) fn normalize_single_line(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                normalized.push(' ');
            }
            '\n' => normalized.push(' '),
            _ => normalized.push(character),
        }
    }
    normalized
}

pub(crate) fn normalize_multiline(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' {
            if characters.peek() == Some(&'\n') {
                characters.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(character);
        }
    }
    normalized
}

/// Normalizes line breaks as [`normalize_single_line`] or [`normalize_multiline`] do and maps
/// every input byte offset to its output offset; an offset inside a character or a CRLF pair
/// maps to where that character's replacement starts.
pub(crate) fn normalize_with_offsets(text: &str, single_line: bool) -> (String, Vec<usize>) {
    let mut normalized = String::with_capacity(text.len());
    let mut offsets = vec![0; text.len() + 1];
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        let start = normalized.len();
        let mut end = index + character.len_utf8();
        match character {
            '\r' => {
                if let Some((_, '\n')) = characters.peek() {
                    characters.next();
                    end += 1;
                }
                normalized.push(if single_line { ' ' } else { '\n' });
            }
            '\n' if single_line => normalized.push(' '),
            _ => normalized.push(character),
        }
        offsets[index..end].fill(start);
    }
    offsets[text.len()] = normalized.len();
    (normalized, offsets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_boundaries_preserve_graphemes_and_unicode_segments() {
        let text = "one, cafe\u{301} 👩‍💻 العربية";
        let combining_end = "one, cafe\u{301}".len();
        assert_eq!(previous_grapheme_boundary(text, combining_end), 8);
        assert_eq!(next_grapheme_boundary(text, 8), combining_end);
        assert_eq!(next_word_boundary(text, 0), 3);
        assert_eq!(next_word_boundary(text, 3), 5);
        assert_eq!(previous_word_boundary(text, text.len()), 24);
    }

    #[test]
    fn normalization_floors_offsets_inside_graphemes() {
        let text = "a👩‍💻e\u{301}";
        assert_eq!(grapheme_boundary_at_or_before(text, 2), 1);
        assert_eq!(grapheme_boundary_at_or_before(text, text.len()), text.len());
    }

    #[test]
    fn logical_line_boundaries_cross_empty_and_unicode_lines() {
        let text = "first\n\ncafe\u{301} 👩\u{200d}💻\nlast";
        let empty_start = "first\n".len();
        let unicode_start = empty_start + 1;
        let unicode_end = text[unicode_start..].find('\n').unwrap() + unicode_start;
        let unicode_middle = unicode_start + "cafe\u{301}".len();

        assert_eq!(
            previous_logical_line_start(text, unicode_middle),
            empty_start
        );
        assert_eq!(previous_logical_line_start(text, empty_start), 0);
        assert_eq!(previous_logical_line_start(text, 0), 0);
        assert_eq!(next_logical_line_end(text, 0), empty_start);
        assert_eq!(next_logical_line_end(text, empty_start), unicode_end);
        assert_eq!(next_logical_line_end(text, unicode_end), text.len());
        assert_eq!(next_logical_line_end(text, text.len()), text.len());
    }
}
