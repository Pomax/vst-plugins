//! Disk operations: open a `.md` file, Save, Save As.
//!
//! The dialogs live in the GUI layer; everything here is plain path-in /
//! path-out so the test runner can exercise the same code without a UI.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::edit::Editor;

/// Extensions offered in the open/save dialogs.
pub const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd", "txt"];

impl Editor {
    /// Load `path` into the editor, replacing every section.
    ///
    /// The file is one document; its top-level headings become the sections.
    pub fn open_path(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)?;
        // Normalise CRLF so caret arithmetic stays byte-exact.
        let contents = contents.replace("\r\n", "\n");
        self.set_document_text(&contents);
        self.file = Some(path.to_path_buf());
        self.dirty = false;
        self.clear_history();
        Ok(())
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
        fs::write(path, self.document_text().as_bytes())?;
        self.dirty = false;
        Ok(())
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
    use crate::edit::{Key, Mods};

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
    fn the_note_name_is_not_written_to_the_file() {
        let dir = temp_dir();
        let target = dir.join("named.md");

        let mut e = Editor::with_text("# One");
        e.title = "Ghostlight".to_string();
        e.save_as(&target).unwrap();

        let written = fs::read_to_string(&target).unwrap();
        assert!(!written.contains("Ghostlight"), "{written:?}");
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
