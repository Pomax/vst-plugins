//! Pictures in a note: a file dropped on the window goes in at the caret as a
//! reference on a line of its own, its data goes into the images tab, the
//! picture is drawn in the document, and deleting the reference takes the
//! data with it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use markdown_notes_core::{Editor, Theme};
use markdown_notes_plugin::gui::{Pictures, IMAGES_TAB};

const WIDTH: f32 = 700.0;
const HEIGHT: f32 = 500.0;

/// A 40 by 30 picture, all one colour that nothing else in the window is.
const PICTURE_COLOUR: [u8; 4] = [200, 40, 160, 255];

fn picture_bytes() -> Vec<u8> {
    let mut rgba = Vec::with_capacity(40 * 30 * 4);
    for _ in 0..40 * 30 {
        rgba.extend_from_slice(&PICTURE_COLOUR);
    }
    markdown_notes_plugin::pictures::Incoming::from_rgba(40, 30, &rgba)
        .expect("the pixels did not encode")
        .bytes
}

/// The picture written to a file the test can drop, under the workspace's
/// own cache. In a directory of the test's own: the tests run at the same
/// time, and one writing a file while another reads it hands the reader half
/// a picture.
fn picture_file(test: &str, name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cache/pictures-in-notes")
        .join(test);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, picture_bytes()).unwrap();
    path
}

struct Window {
    harness: Harness<'static>,
    editor: Arc<Mutex<Editor>>,
    pictures: Pictures,
}

fn open(text: &str) -> Window {
    let editor = Arc::new(Mutex::new(Editor::with_text(text)));
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
        e.title = "pictures".to_string();
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(Arc::clone(&editor), false);
    let pictures = state.pictures();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(WIDTH, HEIGHT))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    Window { harness, editor, pictures }
}

impl Window {
    fn step(&mut self) {
        self.harness.run_steps(3);
    }

    fn drop_file(&mut self, path: PathBuf) {
        self.harness.input_mut().dropped_files.push(egui::DroppedFile {
            path: Some(path),
            ..Default::default()
        });
        self.step();
    }

    fn type_text(&mut self, text: &str) {
        self.harness.input_mut().events.push(egui::Event::Text(text.to_string()));
        self.step();
    }

    fn press(&mut self, key: egui::Key) {
        self.harness.input_mut().events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        self.step();
    }

    fn click(&mut self, at: egui::Pos2) {
        self.harness.input_mut().events.push(egui::Event::PointerMoved(at));
        self.step();
        for pressed in [true, false] {
            self.harness.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            self.step();
        }
    }

    fn text(&self) -> String {
        self.editor.lock().unwrap().text()
    }

    fn images_tab(&self) -> Option<egui::Rect> {
        self.harness.query_by_label(IMAGES_TAB).map(|tab| tab.rect())
    }

    /// Where the pictures were drawn.
    fn pictures(&self) -> Vec<egui::Rect> {
        self.pictures.lock().unwrap().iter().map(|p| p.rect).collect()
    }

    /// How many pixels of the window are the picture's colour.
    fn picture_pixels(&mut self) -> usize {
        self.harness.remove_cursor();
        self.step();
        let image = self.harness.render().expect("rendering failed");
        image.pixels().filter(|p| p.0 == PICTURE_COLOUR).count()
    }
}

#[test]
fn a_dropped_file_goes_in_at_the_caret_on_a_line_of_its_own() {
    let mut window = open("# Notes\n\nbefore");
    window.editor.lock().unwrap().set_caret("# Notes\n\nbefore".len());

    window.drop_file(picture_file("at-the-caret", "cat.png"));

    assert_eq!(window.text(), "# Notes\n\nbefore\n\n![cat][1]\n\n");
    let editor = window.editor.lock().unwrap();
    assert_eq!(editor.images().len(), 1);
    assert_eq!(editor.images()[0].mime, markdown_notes_core::images::PNG);
    assert_eq!(editor.images()[0].bytes().unwrap(), picture_bytes());
}

#[test]
fn the_picture_is_drawn_in_the_document() {
    let mut window = open("# Notes\n\nbefore");
    window.editor.lock().unwrap().set_caret("# Notes\n\nbefore".len());
    assert_eq!(window.picture_pixels(), 0, "the picture's colour is in the window before any drop");

    window.drop_file(picture_file("drawn", "cat.png"));
    // The caret is on the line after the reference, so the reference is a
    // picture and not text.
    window.type_text("after");

    let drawn = window.pictures();
    assert_eq!(drawn.len(), 1, "one picture should be drawn: {drawn:?}");
    assert!(
        window.picture_pixels() >= 40 * 30 - 40,
        "the picture is not on screen at its size"
    );
}

#[test]
fn the_reference_is_text_while_the_caret_is_on_it() {
    let mut window = open("# Notes\n\nbefore");
    window.editor.lock().unwrap().set_caret("# Notes\n\nbefore".len());
    window.drop_file(picture_file("text-on-it", "cat.png"));

    let text = window.text();
    window.editor.lock().unwrap().set_caret(text.find("![cat]").unwrap() + 2);
    window.step();

    assert!(window.pictures().is_empty(), "the picture is drawn while its reference is being edited");
    assert_eq!(window.picture_pixels(), 0);
}

#[test]
fn a_file_that_is_not_a_picture_is_refused() {
    let mut window = open("# Notes\n\nbefore");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.cache/pictures-in-notes");
    std::fs::create_dir_all(&dir).unwrap();
    let text = dir.join("notes.txt");
    std::fs::write(&text, b"words").unwrap();

    window.drop_file(text);

    assert_eq!(window.text(), "# Notes\n\nbefore");
    assert!(window.images_tab().is_none());
}

#[test]
fn the_images_tab_appears_last_and_shows_the_data_read_only() {
    let mut window = open("# Notes\n\nbefore");
    assert!(window.images_tab().is_none(), "an images tab with no pictures");

    window.drop_file(picture_file("the-tab", "cat.png"));
    let tab = window.images_tab().expect("no images tab after a drop");
    let section = window.harness.get_by_label("Notes").rect();
    assert!(tab.left() > section.right(), "the images tab is not after the sections");

    window.click(tab.center());
    let editor = window.editor.lock().unwrap();
    assert!(editor.viewing_images());
    assert!(editor.text().starts_with("[1]: data:image/png;base64,"));
    assert!(!editor.has_caret());
    drop(editor);

    window.type_text("x");
    assert!(window.text().starts_with("[1]: data:image/png;base64,"));
    assert_eq!(window.editor.lock().unwrap().section_text(0), "# Notes\n\nbefore\n\n![cat][1]\n\n");

    window.click(section.center());
    assert!(!window.editor.lock().unwrap().viewing_images());
    assert_eq!(window.text(), "# Notes\n\nbefore\n\n![cat][1]\n\n");
}

#[test]
fn deleting_the_reference_drops_the_data_and_the_tab() {
    let mut window = open("# Notes\n\nbefore");
    window.editor.lock().unwrap().set_caret("# Notes\n\nbefore".len());
    window.drop_file(picture_file("deleting", "cat.png"));
    window.drop_file(picture_file("deleting", "dog.png"));
    assert_eq!(window.text(), "# Notes\n\nbefore\n\n![cat][1]\n\n![dog][2]\n\n");

    let text = window.text();
    let from = text.find("![cat][1]").unwrap();
    window.editor.lock().unwrap().select(from..from + "![cat][1]".len());
    window.press(egui::Key::Backspace);

    assert_eq!(window.text(), "# Notes\n\nbefore\n\n\n\n![dog][1]\n\n");
    let numbers: Vec<usize> = window.editor.lock().unwrap().images().iter().map(|i| i.number).collect();
    assert_eq!(numbers, vec![1]);
    assert!(window.images_tab().is_some());

    let text = window.text();
    let from = text.find("![dog][1]").unwrap();
    window.editor.lock().unwrap().select(from..from + "![dog][1]".len());
    window.press(egui::Key::Backspace);

    assert!(window.editor.lock().unwrap().images().is_empty());
    assert!(window.images_tab().is_none(), "the images tab is still there with nothing in it");
}

/// Undo after deleting a reference brings the reference, its definition and
/// the images tab back, frame after frame, with nothing tidying them away
/// again.
#[test]
fn undo_brings_a_deleted_picture_and_the_images_tab_back() {
    let mut window = open("# Notes\n\nbefore");
    window.editor.lock().unwrap().set_caret("# Notes\n\nbefore".len());
    window.drop_file(picture_file("undo", "cat.png"));
    let text = window.text();
    let from = text.find("![cat][1]").unwrap();
    window.editor.lock().unwrap().select(from..from + "![cat][1]".len());
    window.press(egui::Key::Backspace);
    assert!(window.images_tab().is_none(), "the tab is still there after the deletion");

    window.harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Z,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::CTRL,
    });
    window.step();
    window.step();

    assert_eq!(window.text(), text);
    assert!(window.images_tab().is_some(), "the images tab did not come back");
    let editor = window.editor.lock().unwrap();
    assert_eq!(editor.images().len(), 1);
    assert_eq!(editor.images()[0].bytes().unwrap(), picture_bytes());
    drop(editor);

    // The undo leaves the caret on the reference, which is text while the
    // caret is on it. Away from it, the picture is drawn again.
    window.editor.lock().unwrap().set_caret(0);
    window.step();
    assert_eq!(window.pictures().len(), 1, "the picture is not drawn again");
}

#[test]
fn the_pictures_survive_being_saved_and_loaded() {
    let mut window = open("# Notes\n\nbefore");
    window.editor.lock().unwrap().set_caret("# Notes\n\nbefore".len());
    window.drop_file(picture_file("saved", "cat.png"));

    let saved = window.editor.lock().unwrap().state_bytes();
    let mut back = Editor::new();
    back.load_state_bytes(&saved);

    assert_eq!(back.text(), "# Notes\n\nbefore\n\n![cat][1]");
    assert_eq!(back.section_text(0), "# Notes\n\nbefore\n\n![cat][1]");
    assert_eq!(back.images().len(), 1);
    assert_eq!(back.images()[0].bytes().unwrap(), picture_bytes());
}
