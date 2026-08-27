//! Section titles, and the split between one document and the sections holding
//! it.
//!
//! The sections are a view of a single document, not separate files. Saving
//! joins them back together in section order; loading splits the result again
//! on its top-level headings, so a file written by the editor reopens with the
//! sections it was written with.

/// Shown by a section whose text does not start with a heading.
pub const UNTITLED: &str = "untitled";

/// Widest a section title may be, in characters.
///
/// The width is fixed rather than proportional to the longest title so the bar
/// does not reflow as headings are typed.
pub const MAX_TITLE_CHARS: usize = "this many words".len();

/// What replaces the tail of a title too long to fit.
const ELLIPSIS: &str = "...";

/// Whether `line` opens a section: a top-level ATX heading.
fn is_h1(line: &str) -> bool {
    match line.strip_prefix('#') {
        Some(rest) => !rest.starts_with('#') && rest.starts_with(' '),
        None => false,
    }
}

/// The heading text of an ATX heading line, at any level.
fn heading_text(line: &str) -> Option<&str> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.starts_with(' ') {
        return None;
    }
    // A closing run of hashes is decoration, not part of the title.
    let text = rest.trim().trim_end_matches('#').trim_end();
    (!text.is_empty()).then_some(text)
}

/// A section's title: the heading it opens with, or [`UNTITLED`].
pub fn title_of(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .and_then(heading_text)
        .map(|t| t.to_string())
        .unwrap_or_else(|| UNTITLED.to_string())
}

/// `title` cut down to [`MAX_TITLE_CHARS`], ending in `...` when cut.
///
/// The result is never wider than the limit: the ellipsis is counted, not
/// added on top of it.
pub fn display_title(title: &str) -> String {
    if title.chars().count() <= MAX_TITLE_CHARS {
        return title.to_string();
    }
    let keep = MAX_TITLE_CHARS - ELLIPSIS.chars().count();
    let mut out: String = title.chars().take(keep).collect();
    out.push_str(ELLIPSIS);
    out
}

/// Whether `title` had to be shortened, and so wants a tooltip.
pub fn is_truncated(title: &str) -> bool {
    title.chars().count() > MAX_TITLE_CHARS
}

/// Split a document into one part per top-level heading.
///
/// Anything before the first heading becomes the first part, so a document
/// that never uses `#` opens as a single section. Always returns at least one
/// part, empty if the document is.
///
/// A document with no heading to split on is handed back byte for byte,
/// trailing newline included, so opening a file and saving it again does not
/// rewrite it.
pub fn split_document(text: &str) -> Vec<String> {
    let mut starts: Vec<usize> = Vec::new();
    let mut offset = 0;
    let mut in_fence = false;

    for line in text.split_inclusive('\n') {
        // A `#` inside a fenced block is code, not a heading.
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence && is_h1(line.trim_end_matches('\n')) {
            starts.push(offset);
        }
        offset += line.len();
    }

    // Nothing to split on, or nothing but a preamble before the first heading.
    if starts.is_empty() || (starts.len() == 1 && text[..starts[0]].trim().is_empty()) {
        return vec![text.to_string()];
    }

    let mut parts: Vec<String> = Vec::new();
    let preamble = &text[..starts[0]];
    if !preamble.trim().is_empty() {
        parts.push(preamble.trim_end().to_string());
    }
    for (i, &start) in starts.iter().enumerate() {
        match starts.get(i + 1) {
            Some(&end) => parts.push(text[start..end].trim_end().to_string()),
            // The last section ends where the document does, so what it trails
            // in is the document's own end and is kept as it was written.
            None => parts.push(text[start..].to_string()),
        }
    }
    parts
}

/// Join sections back into one document, in section order.
///
/// What a section trails in is normalised away, because one blank line is the
/// separator between them. What the last one trails in is not: blank lines at
/// the end of the document were typed there, and they are room to carry on
/// writing in.
pub fn join_document(parts: &[&str]) -> String {
    let body = parts
        .iter()
        .map(|p| p.trim_end())
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let tail = parts.last().map_or("", |p| &p[p.trim_end().len()..]);
    format!("{body}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_without_a_heading_is_untitled() {
        assert_eq!(title_of(""), "untitled");
        assert_eq!(title_of("just some prose"), "untitled");
        assert_eq!(title_of("- a list\n- of things"), "untitled");
    }

    #[test]
    fn a_leading_heading_becomes_the_title() {
        assert_eq!(title_of("# Mix notes\n\nbody"), "Mix notes");
        assert_eq!(title_of("\n\n## Drum bus\n"), "Drum bus");
        assert_eq!(title_of("# Closed heading #\n"), "Closed heading");
    }

    #[test]
    fn a_heading_further_down_is_not_the_title() {
        assert_eq!(title_of("prose first\n\n# Later heading"), "untitled");
    }

    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        assert_eq!(title_of("#hashtag"), "untitled");
    }

    #[test]
    fn short_titles_are_left_alone() {
        assert_eq!(display_title("Drum bus"), "Drum bus");
        assert!(!is_truncated("Drum bus"));
    }

    #[test]
    fn a_title_of_exactly_the_limit_is_left_alone() {
        let title = "this many words";
        assert_eq!(title.chars().count(), MAX_TITLE_CHARS);
        assert_eq!(display_title(title), title);
        assert!(!is_truncated(title));
    }

    #[test]
    fn a_long_title_is_cut_to_the_limit_including_the_ellipsis() {
        let shown = display_title("a considerably longer heading than fits");
        assert_eq!(shown.chars().count(), MAX_TITLE_CHARS);
        assert!(shown.ends_with("..."));
        assert!(is_truncated("a considerably longer heading than fits"));
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        // 21 characters, 63 bytes: cutting on bytes would split one in half.
        let title = "日本語のとても長い見出しなのでこれは切られる";
        assert!(title.len() > MAX_TITLE_CHARS);
        let shown = display_title(title);
        assert_eq!(shown.chars().count(), MAX_TITLE_CHARS);
        assert!(shown.ends_with("..."));
    }

    #[test]
    fn a_document_with_no_heading_is_handed_back_untouched() {
        assert_eq!(split_document("line one\nline two\n"), vec!["line one\nline two\n"]);
        assert_eq!(split_document("# Only\n\nbody\n"), vec!["# Only\n\nbody\n"]);
    }

    #[test]
    fn an_empty_document_is_one_empty_section() {
        assert_eq!(split_document(""), vec![String::new()]);
    }

    #[test]
    fn a_document_without_headings_is_one_section() {
        assert_eq!(split_document("no headings here"), vec!["no headings here"]);
    }

    #[test]
    fn each_top_level_heading_starts_a_section() {
        let parts = split_document("# One\n\nfirst\n\n# Two\n\nsecond");
        assert_eq!(parts, vec!["# One\n\nfirst", "# Two\n\nsecond"]);
    }

    #[test]
    fn text_before_the_first_heading_is_its_own_section() {
        let parts = split_document("preamble\n\n# One\n\nfirst");
        assert_eq!(parts, vec!["preamble", "# One\n\nfirst"]);
    }

    #[test]
    fn sub_headings_do_not_split() {
        let parts = split_document("# One\n\n## Sub\n\nbody");
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn a_hash_inside_a_fence_does_not_split() {
        let parts = split_document("# One\n\n```sh\n# a comment\n```\n\nbody");
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn sections_join_in_order() {
        let joined = join_document(&["# One\n\nfirst", "# Two\n\nsecond"]);
        assert_eq!(joined, "# One\n\nfirst\n\n# Two\n\nsecond");
    }

    #[test]
    fn empty_sections_contribute_nothing_to_the_document() {
        assert_eq!(join_document(&["# One", "", "# Two"]), "# One\n\n# Two");
    }

    #[test]
    fn headed_sections_survive_a_save_and_load() {
        let sections = ["# One\n\nfirst", "# Two\n\nsecond", "# Three\n\nthird"];
        let reopened = split_document(&join_document(&sections));
        assert_eq!(reopened, sections);
    }

    /// Blank lines at the end of the document were typed on purpose: they are
    /// room to carry on writing in, and saving is not the moment to decide the
    /// document should be shorter.
    #[test]
    fn blank_lines_at_the_end_of_the_document_are_written_out() {
        assert_eq!(
            join_document(&["# One\n\nfirst", "# Two\n\nsecond\n\n\n"]),
            "# One\n\nfirst\n\n# Two\n\nsecond\n\n\n"
        );
    }

    #[test]
    fn a_last_section_of_nothing_but_blank_lines_keeps_them() {
        assert_eq!(join_document(&["# One\n\nfirst", "\n\n"]), "# One\n\nfirst\n\n");
    }

    #[test]
    fn blank_lines_at_the_end_come_back_when_the_document_is_split() {
        assert_eq!(
            split_document("# One\n\nfirst\n\n# Two\n\nsecond\n\n\n"),
            ["# One\n\nfirst", "# Two\n\nsecond\n\n\n"]
        );
    }

    #[test]
    fn a_document_of_one_section_keeps_the_blank_lines_at_its_end() {
        let sections = ["# One\n\nfirst\n\n\n"];
        assert_eq!(split_document(&join_document(&sections)), sections);
    }

    #[test]
    fn headed_sections_survive_it_with_blank_lines_at_the_end() {
        let sections = ["# One\n\nfirst", "# Two\n\nsecond\n\n\n"];
        let reopened = split_document(&join_document(&sections));
        assert_eq!(reopened, sections);
    }
}
