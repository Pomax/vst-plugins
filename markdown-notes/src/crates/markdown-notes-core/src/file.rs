//! Disk operations: open a `.md` file, Save, Save As.
//!
//! The dialogs live in the GUI layer; everything here is plain path-in /
//! path-out so the test runner can exercise the same code without a UI.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::edit::{Editor, DEFAULT_TITLE};
use crate::frontmatter;

/// The markdown extension: the one the open and save dialogs offer, and the
/// one a dropped file is a document by.
pub const MARKDOWN_EXTENSIONS: &[&str] = &["md"];

impl Editor {
    /// Load `path` into the editor, replacing every section.
    ///
    /// The file is one document; its top-level headings become the sections.
    /// Frontmatter at the top of it is not document: its `title` field is the
    /// document's name, and the block is kept to be written back on save.
    pub fn open_path(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)?;
        self.open_text(&contents);
        self.file = Some(path.to_path_buf());
        Ok(())
    }

    /// Load `contents`, the bytes of a markdown file, into the editor, the
    /// way [`Editor::open_path`] loads a file, with no file to save back to:
    /// Save asks where, as it does for a document that was never saved.
    pub fn open_text(&mut self, contents: &str) {
        // Normalise CRLF so caret arithmetic stays byte-exact, and drop the
        // byte order mark Windows editors put in front of the first line.
        let contents = contents.replace("\r\n", "\n");
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(contents.as_str());
        let (block, body) = frontmatter::split_off(contents);
        self.set_document_text(body);
        if let Some(block) = block {
            if let Some(title) = block.title() {
                self.title = title;
            }
            self.frontmatter = block;
        }
        self.file = None;
        self.dirty = false;
        self.clear_history();
    }

    /// Write to the current file. Fails if the document has no path yet —
    /// the caller should fall back to Save As.
    pub fn save(&mut self) -> io::Result<PathBuf> {
        let Some(path) = self.file.clone() else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no file associated with this document",
            ));
        };
        self.write_to(&path)?;
        Ok(path)
    }

    /// Write to `path` and adopt it as the current file.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> io::Result<PathBuf> {
        let path = with_default_extension(path.as_ref());
        self.write_to(&path)?;
        self.file = Some(path.clone());
        Ok(path)
    }

    fn write_to(&mut self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, self.file_text().as_bytes())?;
        self.dirty = false;
        Ok(())
    }

    /// The bytes the file holds: the frontmatter, when there is anything to
    /// put in it, and the document. The title goes in it once the document
    /// has one; the placeholder is not a name and is not written.
    fn file_text(&self) -> String {
        let body = self.document_text();
        let mut block = self.frontmatter.clone();
        if self.title != DEFAULT_TITLE {
            block.set_title(&self.title);
        }
        if block.is_empty() {
            body
        } else {
            frontmatter::join(&block, &body)
        }
    }

    /// True when there are unsaved changes relative to disk or project state.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

}

/// Append `.md` when the user typed a name without an extension.
fn with_default_extension(path: &Path) -> PathBuf {
    match path.extension() {
        Some(_) => path.to_path_buf(),
        None => path.with_extension("md"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{Key, Mods, DEFAULT_TITLE};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "markdown-notes-core-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn open_reads_a_file_and_adopts_its_path() {
        let dir = temp_dir();
        let path = dir.join("notes.md");
        fs::write(&path, "# Hello\n\n- a\n").unwrap();

        let mut e = Editor::new();
        e.open_path(&path).unwrap();
        assert_eq!(e.text(), "# Hello\n\n- a\n");
        assert_eq!(e.file.as_deref(), Some(path.as_path()));
        assert!(!e.is_dirty());
        // At the end of the section, ready to carry on writing.
        assert!(e.has_caret());
        assert_eq!(e.caret(), e.text().len());
    }

    #[test]
    fn crlf_files_are_normalised() {
        let dir = temp_dir();
        let path = dir.join("crlf.md");
        fs::write(&path, "line one\r\nline two\r\n").unwrap();
        let mut e = Editor::new();
        e.open_path(&path).unwrap();
        assert_eq!(e.text(), "line one\nline two\n");
    }

    #[test]
    fn save_writes_back_to_the_same_path() {
        let dir = temp_dir();
        let path = dir.join("save.md");
        fs::write(&path, "original").unwrap();

        let mut e = Editor::new();
        e.open_path(&path).unwrap();
        e.set_caret(e.text().len());
        e.handle_key(Key::Char('!'), Mods::NONE);
        assert!(e.is_dirty());

        let written = e.save().unwrap();
        assert_eq!(written, path);
        assert_eq!(fs::read_to_string(&path).unwrap(), "original!");
        assert!(!e.is_dirty());
    }

    #[test]
    fn save_without_a_path_is_an_error() {
        let mut e = Editor::with_text("unsaved");
        assert!(e.save().is_err());
    }

    #[test]
    fn save_as_adopts_the_new_path_and_adds_md() {
        let dir = temp_dir();
        let target = dir.join("fresh");

        let mut e = Editor::with_text("# Fresh\n");
        let written = e.save_as(&target).unwrap();
        assert_eq!(written.extension().unwrap(), "md");
        assert_eq!(fs::read_to_string(&written).unwrap(), "# Fresh\n");
        assert_eq!(e.file.as_deref(), Some(written.as_path()));
        assert!(!e.is_dirty());
    }

    #[test]
    fn save_as_creates_missing_directories() {
        let dir = temp_dir().join("nested").join("deeper");
        let mut e = Editor::with_text("x");
        let written = e.save_as(dir.join("a.md")).unwrap();
        assert!(written.exists());
    }

    #[test]
    fn editing_marks_the_document_dirty() {
        let mut e = Editor::with_text("hi");
        e.dirty = false;
        e.set_caret(e.text().len());
        e.handle_key(Key::Char('!'), Mods::NONE);
        assert!(e.is_dirty());
    }

    #[test]
    fn save_as_writes_every_section_in_order() {
        let dir = temp_dir();
        let target = dir.join("sections.md");

        let mut e = Editor::with_text("# One\n\nfirst");
        e.new_section();
        e.set_text("# Two\n\nsecond");
        e.new_section();
        e.set_text("# Three\n\nthird");
        e.save_as(&target).unwrap();

        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "# One\n\nfirst\n\n# Two\n\nsecond\n\n# Three\n\nthird"
        );
    }

    #[test]
    fn save_as_writes_the_reordered_document_after_a_drag() {
        let dir = temp_dir();
        let target = dir.join("dragged.md");

        let mut e = Editor::with_text("# One");
        e.new_section();
        e.set_text("# Two");
        e.move_section(1, 0);
        e.save_as(&target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "# Two\n\n# One");
    }

    #[test]
    fn opening_a_file_with_headings_gives_one_section_each() {
        let dir = temp_dir();
        let path = dir.join("sections.md");
        fs::write(&path, "# One\n\nfirst\n\n# Two\n\nsecond\n").unwrap();

        let mut e = Editor::new();
        e.open_path(&path).unwrap();
        assert_eq!(e.section_count(), 2);
        assert_eq!(e.section_text(0), "# One\n\nfirst");
        // The newline the file ends with is part of the last section: what the
        // file holds is what is opened, and saving it writes the same bytes.
        assert_eq!(e.section_text(1), "# Two\n\nsecond\n");
        assert_eq!(e.section_title(0), "One");
        assert_eq!(e.section_title(1), "Two");
        assert_eq!(e.active_section(), 0);
        assert!(!e.is_dirty());
    }

    #[test]
    fn opening_replaces_the_sections_that_were_open() {
        let dir = temp_dir();
        let path = dir.join("one-section.md");
        fs::write(&path, "# Only\n\nbody\n").unwrap();

        let mut e = Editor::with_text("# Old one");
        e.new_section();
        e.set_text("# Old two");
        assert_eq!(e.section_count(), 2);

        e.open_path(&path).unwrap();
        assert_eq!(e.section_count(), 1, "the old sections are gone, not appended to");
        assert_eq!(e.section_title(0), "Only");
    }

    #[test]
    fn saving_again_writes_to_the_path_save_as_adopted() {
        let dir = temp_dir();
        let target = dir.join("adopted.md");

        let mut e = Editor::with_text("# One");
        e.save_as(&target).unwrap();
        e.new_section();
        e.set_text("# Two");
        let written = e.save().unwrap();

        assert_eq!(written, target);
        assert_eq!(fs::read_to_string(&target).unwrap(), "# One\n\n# Two");
    }

    #[test]
    fn the_title_is_written_as_frontmatter() {
        let dir = temp_dir();
        let target = dir.join("named.md");

        let mut e = Editor::with_text("# One");
        e.title = "funky cake".to_string();
        e.save_as(&target).unwrap();

        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "---\ntitle: funky cake\n---\n\n# One"
        );
    }

    /// A document that was never given a title is written as it is, with no
    /// frontmatter carrying the placeholder.
    #[test]
    fn a_document_without_a_title_is_written_without_frontmatter() {
        let dir = temp_dir();
        let target = dir.join("unnamed.md");

        let mut e = Editor::with_text("# One");
        assert_eq!(e.title, DEFAULT_TITLE);
        e.save_as(&target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "# One");
    }

    /// The block is not document: nothing of it is in any section, and the
    /// first section is the first heading.
    #[test]
    fn opening_a_file_with_a_title_in_its_frontmatter_takes_the_title() {
        let dir = temp_dir();
        let path = dir.join("titled.md");
        fs::write(&path, "---\ntitle: funky cake\n---\n\n# One\n\nbody\n").unwrap();

        let mut e = Editor::new();
        e.open_path(&path).unwrap();
        assert_eq!(e.title, "funky cake");
        assert_eq!(e.document_text(), "# One\n\nbody\n", "the frontmatter is in the document");
        assert_eq!(e.section_count(), 1, "the frontmatter became a section");
        assert_eq!(e.section_text(0), "# One\n\nbody\n");
        assert_eq!(e.section_title(0), "One");
        assert!(!e.is_dirty());
    }

    /// Files written on Windows often start with a byte order mark. The block
    /// is still at the start of the file.
    #[test]
    fn a_byte_order_mark_does_not_hide_the_frontmatter() {
        let dir = temp_dir();
        let path = dir.join("bom.md");
        fs::write(&path, "\u{feff}---\ntitle: funky cake\n---\n\n# One").unwrap();

        let mut e = Editor::new();
        e.open_path(&path).unwrap();
        assert_eq!(e.title, "funky cake");
        assert_eq!(e.section_count(), 1, "the frontmatter became a section");
        assert_eq!(e.document_text(), "# One");
    }

    /// Trailing spaces after the dashes, and Windows line ends, are still the
    /// fences.
    #[test]
    fn fences_with_trailing_whitespace_or_crlf_are_still_fences() {
        let dir = temp_dir();
        let path = dir.join("spaced.md");
        fs::write(&path, "--- \r\ntitle: funky cake\r\n---  \r\n\r\n# One\r\n").unwrap();

        let mut e = Editor::new();
        e.open_path(&path).unwrap();
        assert_eq!(e.title, "funky cake");
        assert_eq!(e.section_count(), 1, "the frontmatter became a section");
        assert_eq!(e.document_text(), "# One\n");
    }

    /// Frontmatter with no title field leaves the title as it was. Its other
    /// fields are not the editor's, but they are the file's, so they are
    /// written back ahead of the title when the document is saved.
    #[test]
    fn frontmatter_without_a_title_leaves_the_title_alone_and_is_kept() {
        let dir = temp_dir();
        let path = dir.join("author.md");
        fs::write(&path, "---\nauthor: me\n---\n\n# One").unwrap();

        let mut e = Editor::new();
        e.title = "kept".to_string();
        e.open_path(&path).unwrap();
        assert_eq!(e.title, "kept");
        assert_eq!(e.section_count(), 1, "the frontmatter became a section");
        assert_eq!(e.document_text(), "# One");

        e.save().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "---\nauthor: me\ntitle: kept\n---\n\n# One"
        );
    }

    /// Three dashes further down are a horizontal rule, not frontmatter.
    #[test]
    fn a_rule_that_is_not_at_the_start_is_not_frontmatter() {
        let dir = temp_dir();
        let path = dir.join("rule.md");
        let document = "# One\n\n---\n\ntitle: not a field\n\n---\n";
        fs::write(&path, document).unwrap();

        let mut e = Editor::new();
        e.open_path(&path).unwrap();
        assert_eq!(e.title, DEFAULT_TITLE);
        assert_eq!(e.document_text(), document);
    }

    /// An opening line with no closing line is not a block, so it is all
    /// document.
    #[test]
    fn an_unclosed_frontmatter_block_is_document() {
        let dir = temp_dir();
        let path = dir.join("unclosed.md");
        let document = "---\ntitle: not taken\n\n# One";
        fs::write(&path, document).unwrap();

        let mut e = Editor::new();
        e.open_path(&path).unwrap();
        assert_eq!(e.title, DEFAULT_TITLE);
        assert_eq!(e.document_text(), document);
    }

    #[test]
    fn a_titled_document_round_trips_through_a_save_and_an_open() {
        let dir = temp_dir();
        let path = dir.join("round-trip.md");

        let mut e = Editor::with_text("# One\n\nbody");
        e.title = "funky cake".to_string();
        e.save_as(&path).unwrap();
        let first = fs::read_to_string(&path).unwrap();

        let mut reopened = Editor::new();
        reopened.open_path(&path).unwrap();
        assert_eq!(reopened.title, "funky cake");
        assert_eq!(reopened.section_count(), 1, "the frontmatter became a section");
        assert_eq!(reopened.document_text(), "# One\n\nbody");

        reopened.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), first, "saving again rewrote the file");
    }

    /// A title YAML would read as something else, a mapping here, is quoted on
    /// the way out and unquoted on the way back in.
    #[test]
    fn a_title_with_a_colon_round_trips() {
        let dir = temp_dir();
        let path = dir.join("colon.md");

        let mut e = Editor::with_text("# One");
        e.title = "Mix: v2".to_string();
        e.save_as(&path).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "---\ntitle: \"Mix: v2\"\n---\n\n# One"
        );

        let mut reopened = Editor::new();
        reopened.open_path(&path).unwrap();
        assert_eq!(reopened.title, "Mix: v2");
    }

    /// A title somebody else quoted, in either kind of quote, comes in without
    /// the quotes.
    #[test]
    fn a_quoted_title_comes_in_unquoted() {
        let dir = temp_dir();
        for (name, line, title) in [
            ("double.md", "title: \"funky cake\"", "funky cake"),
            ("single.md", "title: 'funky cake'", "funky cake"),
            ("escaped.md", "title: \"say \\\"hi\\\"\"", "say \"hi\""),
        ] {
            let path = dir.join(name);
            fs::write(&path, format!("---\n{line}\n---\n\n# One")).unwrap();
            let mut e = Editor::new();
            e.open_path(&path).unwrap();
            assert_eq!(e.title, title, "{line}");
        }
    }

    #[test]
    fn opening_a_file_that_is_not_there_is_an_error_not_a_panic() {
        let missing = temp_dir().join("no-such-file.md");
        let mut e = Editor::with_text("# Keep me");
        assert!(e.open_path(&missing).is_err());
        assert_eq!(e.text(), "# Keep me", "a failed open leaves the document");
    }

    #[test]
    fn saving_over_a_directory_is_an_error_not_a_panic() {
        let dir = temp_dir().join("a-directory.md");
        fs::create_dir_all(&dir).unwrap();
        let mut e = Editor::with_text("# One");
        assert!(e.save_as(&dir).is_err());
    }

    #[test]
    fn saving_is_not_confused_by_an_empty_section_in_the_middle() {
        let dir = temp_dir();
        let target = dir.join("gap.md");

        let mut e = Editor::with_text("# One");
        e.new_section();
        // A new section comes with a heading, so this is emptied by hand to be
        // the gap the test is about.
        e.set_text("");
        e.new_section();
        e.set_text("# Three");
        e.save_as(&target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "# One\n\n# Three");
    }

    #[test]
    fn a_full_open_edit_save_cycle_round_trips() {
        let dir = temp_dir();
        let path = dir.join("cycle.md");
        fs::write(&path, "# Title\n").unwrap();

        let mut e = Editor::new();
        e.open_path(&path).unwrap();
        e.set_caret(e.text().len());
        for c in "- item".chars() {
            e.handle_key(Key::Char(c), Mods::NONE);
        }
        e.save().unwrap();

        let mut reopened = Editor::new();
        reopened.open_path(&path).unwrap();
        assert_eq!(reopened.text(), "# Title\n- item");
    }

    /// Empty lines typed at the end of a document are part of it: they are
    /// written out, and they are there again when it is opened.
    #[test]
    fn blank_lines_at_the_end_survive_a_save_and_an_open() {
        let dir = temp_dir();
        let path = dir.join("trailing.md");

        let mut e = Editor::new();
        e.set_document_text("# Title\n\nbody");
        e.set_caret(e.text().len());
        for _ in 0..3 {
            e.handle_key(Key::Enter, Mods::NONE);
        }
        let typed = e.document_text();
        assert_eq!(typed, "# Title\n\nbody\n\n\n\n", "the blank lines were not typed");

        e.save_as(&path).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            typed,
            "the file on disk is not what was in the editor"
        );

        let mut reopened = Editor::new();
        reopened.open_path(&path).unwrap();
        assert_eq!(reopened.document_text(), typed, "opening it lost the blank lines");
    }

    /// The same for a document of several sections: the blank lines belong to
    /// the end of the last one.
    #[test]
    fn blank_lines_at_the_end_of_the_last_section_survive_a_save_and_an_open() {
        let dir = temp_dir();
        let path = dir.join("trailing-sections.md");
        let document = "# One\n\nfirst\n\n# Two\n\nsecond\n\n\n";

        let mut e = Editor::new();
        e.set_document_text(document);
        e.save_as(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), document);

        let mut reopened = Editor::new();
        reopened.open_path(&path).unwrap();
        assert_eq!(reopened.document_text(), document);
        assert_eq!(reopened.section_count(), 2);
    }
}
