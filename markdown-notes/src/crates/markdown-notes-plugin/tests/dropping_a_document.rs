//! A markdown file dropped on the window is opened, the way File > Open opens
//! it: it replaces the document, its frontmatter names it, and it is the file
//! Save writes back to.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use markdown_notes_core::{Editor, Theme};

const WIDTH: f32 = 700.0;
const HEIGHT: f32 = 500.0;

/// A file dropped on the window.
///
/// egui describes a dropped file by a trait and ships no implementation of it,
/// because a browser hands one over differently from a desktop. A test drops
/// files that are really on disk.
#[derive(Debug)]
struct ADroppedFile(PathBuf);

impl egui::DroppedFile for ADroppedFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

const DOCUMENT: &str = "---\ntitle: funky cake\n---\n\n# Outline\n\nthe brief\n\n# Stakeholders\n\nthem\n";
const BODY: &str = "# Outline\n\nthe brief\n\n# Stakeholders\n\nthem\n";

/// `text` written to a file the test can drop, under the workspace's own
/// cache, in a directory of the test's own.
fn file(test: &str, name: &str, text: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cache/dropping-a-document")
        .join(test);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

struct Window {
    harness: Harness<'static>,
    editor: Arc<Mutex<Editor>>,
}

fn open(text: &str) -> Window {
    let editor = Arc::new(Mutex::new(Editor::with_text(text)));
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(Arc::clone(&editor), false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(WIDTH, HEIGHT))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    Window { harness, editor }
}

impl Window {
    fn drop_file(&mut self, path: PathBuf) {
        self.harness
            .input_mut()
            .dropped_files
            .push(Arc::new(ADroppedFile(path)));
        self.harness.run_steps(3);
    }
}

#[test]
fn a_dropped_markdown_file_replaces_the_document() {
    let path = file("replaces", "project.md", DOCUMENT);
    let mut window = open("# Old\n\nwhat was here");

    window.drop_file(path.clone());

    let e = window.editor.lock().unwrap();
    assert_eq!(e.document_text(), BODY, "the dropped file is not the document");
    assert_eq!(e.section_count(), 2);
    assert_eq!(e.section_title(0), "Outline");
    assert_eq!(e.section_title(1), "Stakeholders");
    assert_eq!(e.title, "funky cake", "the frontmatter title was not taken");
    assert_eq!(e.file.as_deref(), Some(path.as_path()), "the dropped file is not the file to save to");
    assert!(!e.is_dirty());
}

/// Every extension the open dialog offers is a markdown file when dropped.
#[test]
fn a_dropped_file_with_any_markdown_extension_is_opened() {
    for extension in markdown_notes_core::file::MARKDOWN_EXTENSIONS {
        let path = file("extensions", &format!("notes.{extension}"), "# Dropped\n");
        let mut window = open("# Old");
        window.drop_file(path);
        let e = window.editor.lock().unwrap();
        assert_eq!(e.document_text(), "# Dropped\n", ".{extension}");
    }
}

/// The extension is matched whatever its case: `.MD` from a file manager
/// that shouts.
#[test]
fn the_extension_is_matched_in_any_case() {
    let path = file("case", "NOTES.MD", "# Dropped\n");
    let mut window = open("# Old");
    window.drop_file(path);
    let e = window.editor.lock().unwrap();
    assert_eq!(e.document_text(), "# Dropped\n");
}

/// A file that is neither markdown nor a picture is left alone.
#[test]
fn a_dropped_file_of_another_kind_changes_nothing() {
    let path = file("other", "notes.rtf", "{\\rtf1 not markdown}");
    let mut window = open("# Old");
    window.drop_file(path);
    let e = window.editor.lock().unwrap();
    assert_eq!(e.document_text(), "# Old");
    assert_eq!(e.file, None);
}
