//! Byte-offset text utilities.
//!
//! The document keeps markdown source in a `String` and addresses positions as
//! byte offsets. Every helper here is UTF-8 safe: offsets handed back always sit
//! on a char boundary.

/// Zero-based index of the line containing `pos`.
pub fn line_index(text: &str, pos: usize) -> usize {
    text[..pos].bytes().filter(|b| *b == b'\n').count()
}

/// Byte range of line `index`, excluding the trailing newline.
pub fn line_range(text: &str, index: usize) -> Option<(usize, usize)> {
    let mut start = 0usize;
    for (i, line) in text.split('\n').enumerate() {
        let end = start + line.len();
        if i == index {
            return Some((start, end));
        }
        start = end + 1;
    }
    None
}

/// Number of lines. A trailing newline yields a final empty line, matching editors.
pub fn line_count(text: &str) -> usize {
    text.bytes().filter(|b| *b == b'\n').count() + 1
}

/// Next char boundary after `pos`, or `pos` if already at the end.
pub fn next_boundary(text: &str, pos: usize) -> usize {
    if pos >= text.len() {
        return text.len();
    }
    let mut i = pos + 1;
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Clamp an arbitrary offset onto the nearest char boundary at or below it.
pub fn clamp_boundary(text: &str, pos: usize) -> usize {
    let mut i = pos.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// A character a word is made of: a letter, a digit or an underscore.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The word at byte offset `at`, the way a double click picks one: the run
/// of word characters `at` is in, or the run that ends at `at` when it sits
/// just after one. Anything else, a space or a punctuation mark, is the one
/// character at `at`, and the end of the text is empty.
pub fn word_at(text: &str, at: usize) -> std::ops::Range<usize> {
    let at = clamp_boundary(text, at);
    let on = text[at..].chars().next();
    let before = text[..at].chars().next_back();
    match (on, before) {
        (Some(c), _) if is_word(c) => {}
        (_, Some(c)) if is_word(c) => {}
        (Some(c), _) => return at..at + c.len_utf8(),
        (None, _) => return at..at,
    }
    let start = text[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(at, |(i, _)| i);
    let end = text[at..]
        .char_indices()
        .find(|(_, c)| !is_word(*c))
        .map_or(text.len(), |(i, _)| at + i);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_at_an_offset_is_the_run_of_letters_around_it() {
        let t = "first line";
        assert_eq!(word_at(t, 0), 0..5);
        assert_eq!(word_at(t, 2), 0..5);
        assert_eq!(word_at(t, 4), 0..5);
        assert_eq!(word_at(t, 6), 6..10);
    }

    #[test]
    fn the_offset_just_after_a_word_is_still_that_word() {
        let t = "first line";
        assert_eq!(word_at(t, 5), 0..5, "the space after first");
        assert_eq!(word_at(t, 10), 6..10, "the end of the text");
        assert_eq!(word_at("one\ntwo", 3), 0..3, "the end of a line");
    }

    #[test]
    fn a_space_or_a_mark_between_words_is_a_word_of_its_own() {
        assert_eq!(word_at("a,, b", 2), 2..3, "the second comma");
        assert_eq!(word_at("a, b", 2), 2..3, "the space");
        assert_eq!(word_at("a  b", 2), 2..3, "a space after a space");
        assert_eq!(word_at("a, b", 1), 0..1, "just after a word is the word");
    }

    #[test]
    fn digits_and_underscores_are_word_characters() {
        assert_eq!(word_at("v2_final x", 3), 0..8);
    }

    #[test]
    fn words_are_found_through_multibyte_text() {
        let t = "héllo wörld";
        assert_eq!(word_at(t, 3), 0..6);
        assert_eq!(word_at(t, 8), 7..13);
        assert_eq!(word_at(t, t.len()), 7..13);
    }

    #[test]
    fn empty_text_has_an_empty_word() {
        assert_eq!(word_at("", 0), 0..0);
        assert_eq!(word_at("ab", 5), 0..2, "past the end is the end");
    }

    #[test]
    fn line_helpers() {
        let t = "alpha\nbeta\ngamma";
        assert_eq!(line_index(t, 7), 1);
        assert_eq!(line_range(t, 2), Some((11, 16)));
        assert_eq!(line_count(t), 3);
    }

    #[test]
    fn trailing_newline_makes_an_empty_last_line() {
        let t = "one\n";
        assert_eq!(line_count(t), 2);
        assert_eq!(line_range(t, 1), Some((4, 4)));
    }

    #[test]
    fn boundaries_are_utf8_safe() {
        let t = "aé漢";
        assert_eq!(next_boundary(t, 0), 1);
        assert_eq!(next_boundary(t, 1), 3);
        assert_eq!(next_boundary(t, 3), 6);
        assert_eq!(clamp_boundary(t, 2), 1);
    }
}
