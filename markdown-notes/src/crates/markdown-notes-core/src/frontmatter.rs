//! The YAML block at the top of a markdown file, between a line of `---` at
//! the very start and the next line of `---`.
//!
//! The editor reads one field, `title`, and writes it back. The other lines
//! belong to the file: they are kept as they were found and written back
//! ahead of the title.

/// The lines between the two `---` lines, as they were in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    lines: Vec<String>,
}

const FENCE: &str = "---";
const TITLE: &str = "title";

/// Take the block off the front of `text`, if it starts with one, and give
/// back the rest of the text. One blank line between the block and the text
/// is the block's, see [`join`].
///
/// A `---` anywhere but the first line is a horizontal rule, and an opening
/// line with no closing line is not a block: in both cases all of `text` is
/// document. Spaces after the dashes are allowed on either fence.
pub fn split_off(text: &str) -> (Option<Frontmatter>, &str) {
    let Some(after_open) = open_fence(text) else {
        return (None, text);
    };
    let mut lines = Vec::new();
    let mut rest = after_open;
    loop {
        let (line, after) = match rest.split_once('\n') {
            Some((line, after)) => (line, after),
            None => (rest, ""),
        };
        if is_fence(line) || line.trim_end() == "..." {
            let body = after.strip_prefix('\n').unwrap_or(after);
            return (Some(Frontmatter { lines }), body);
        }
        if !rest.contains('\n') {
            return (None, text);
        }
        lines.push(line.to_string());
        rest = after;
    }
}

/// Whether `line` is a line of three dashes, spaces after them allowed.
fn is_fence(line: &str) -> bool {
    line.trim_end() == FENCE
}

/// What follows the opening fence, when `text` starts with one.
fn open_fence(text: &str) -> Option<&str> {
    match text.split_once('\n') {
        Some((first, rest)) if is_fence(first) => Some(rest),
        None if is_fence(text) => Some(""),
        _ => None,
    }
}

/// The block written ahead of `body`, with one blank line between them.
pub fn join(block: &Frontmatter, body: &str) -> String {
    let mut out = String::with_capacity(body.len() + 16);
    out.push_str(FENCE);
    out.push('\n');
    for line in &block.lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(FENCE);
    out.push_str("\n\n");
    out.push_str(body);
    out
}

impl Frontmatter {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The `title` field, unquoted, if there is one.
    pub fn title(&self) -> Option<String> {
        self.lines.iter().find_map(|line| field(line, TITLE)).map(unquote)
    }

    /// Set the `title` field, replacing the one that is there or adding one
    /// after the other fields.
    pub fn set_title(&mut self, title: &str) {
        let line = format!("{TITLE}: {}", quote(title));
        match self.lines.iter().position(|l| field(l, TITLE).is_some()) {
            Some(at) => self.lines[at] = line,
            None => self.lines.push(line),
        }
    }
}

/// The value of `line` when it is the field `name`, untrimmed of quotes.
fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(name)?;
    let rest = rest.trim_start();
    let value = rest.strip_prefix(':')?;
    Some(value.trim())
}

/// A YAML scalar as text: a double-quoted or single-quoted string unescaped,
/// anything else as written.
fn unquote(value: &str) -> String {
    if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    value.to_string()
}

/// `title` as a YAML scalar: plain when YAML would read it back as the same
/// text, double-quoted otherwise.
fn quote(title: &str) -> String {
    let plain = !title.is_empty()
        && title == title.trim()
        && !title.contains([':', '#', '"', '\\', '\n', '\t'])
        && !title.starts_with(['\'', '[', ']', '{', '}', '&', '*', '!', '|', '>', '%', '@', '`', '-', '?']);
    if plain {
        return title.to_string();
    }
    let mut out = String::with_capacity(title.len() + 2);
    out.push('"');
    for c in title.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_taken_off_the_front() {
        let (block, body) = split_off("---\ntitle: x\nauthor: me\n---\n\n# One\n");
        let block = block.expect("no block");
        assert_eq!(block.title().as_deref(), Some("x"));
        assert_eq!(body, "# One\n");
    }

    #[test]
    fn the_blank_line_after_the_block_is_the_blocks() {
        let (_, body) = split_off("---\ntitle: x\n---\n# One");
        assert_eq!(body, "# One", "with no blank line");
        let (_, body) = split_off("---\ntitle: x\n---\n\n\n# One");
        assert_eq!(body, "\n# One", "a second blank line is the document's");
    }

    #[test]
    fn a_block_may_close_with_three_dots() {
        let (block, body) = split_off("---\ntitle: x\n...\nbody");
        assert!(block.is_some());
        assert_eq!(body, "body");
    }

    #[test]
    fn fences_may_have_spaces_after_the_dashes() {
        let (block, body) = split_off("---  \ntitle: x\n--- \nbody");
        assert_eq!(block.and_then(|b| b.title()).as_deref(), Some("x"));
        assert_eq!(body, "body");
    }

    #[test]
    fn a_block_closing_on_the_last_line_leaves_an_empty_document() {
        let (block, body) = split_off("---\ntitle: x\n---");
        assert_eq!(block.and_then(|b| b.title()).as_deref(), Some("x"));
        assert_eq!(body, "");
    }

    #[test]
    fn text_that_does_not_start_with_a_fence_has_no_block() {
        for text in ["# One\n\n---\n", "", "--\n---\n", " ---\ntitle: x\n---\n", "----\ntitle: x\n---\n"] {
            let (block, body) = split_off(text);
            assert!(block.is_none(), "{text:?}");
            assert_eq!(body, text);
        }
    }

    #[test]
    fn an_opening_fence_with_no_closing_fence_has_no_block() {
        for text in ["---\ntitle: x\n", "---\ntitle: x", "---\n", "---"] {
            let (block, body) = split_off(text);
            assert!(block.is_none(), "{text:?}");
            assert_eq!(body, text);
        }
    }

    #[test]
    fn join_undoes_split() {
        let text = "---\nauthor: me\ntitle: x\n---\n\n# One\n\nbody\n";
        let (block, body) = split_off(text);
        assert_eq!(join(&block.unwrap(), body), text);
    }

    #[test]
    fn the_title_field_is_found_however_it_is_spaced() {
        for line in ["title: x", "title:x", "title :  x  ", "title:\tx"] {
            let (block, _) = split_off(&format!("---\n{line}\n---\n"));
            assert_eq!(block.unwrap().title().as_deref(), Some("x"), "{line:?}");
        }
        let (block, _) = split_off("---\ntitles: x\nsubtitle: y\n---\n");
        assert_eq!(block.unwrap().title(), None);
    }

    #[test]
    fn setting_the_title_replaces_it_in_place_or_adds_it_last() {
        let (block, _) = split_off("---\ntitle: x\nauthor: me\n---\n");
        let mut block = block.unwrap();
        block.set_title("y");
        assert_eq!(join(&block, ""), "---\ntitle: y\nauthor: me\n---\n\n");

        let mut block = Frontmatter::default();
        block.set_title("y");
        assert_eq!(join(&block, ""), "---\ntitle: y\n---\n\n");
    }

    #[test]
    fn titles_are_quoted_only_when_yaml_needs_it() {
        for (title, written) in [
            ("funky cake", "funky cake"),
            ("Mix: v2", "\"Mix: v2\""),
            ("#1", "\"#1\""),
            ("say \"hi\"", "\"say \\\"hi\\\"\""),
            ("back\\slash", "\"back\\\\slash\""),
            (" padded ", "\" padded \""),
            ("", "\"\""),
            ("- dash", "\"- dash\""),
            ("it's", "it's"),
        ] {
            assert_eq!(quote(title), written, "{title:?}");
            assert_eq!(unquote(&quote(title)), title, "{title:?} does not come back");
        }
    }

    #[test]
    fn single_quoted_titles_come_back_unquoted() {
        assert_eq!(unquote("'it''s'"), "it's");
        assert_eq!(unquote("'plain'"), "plain");
    }
}
