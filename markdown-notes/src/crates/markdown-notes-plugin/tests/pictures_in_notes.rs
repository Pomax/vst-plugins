//! Pictures in a note, as the window draws them: a picture in the note is
//! drawn in the document at its size, is text while the caret is on its
//! reference, has its images tab after the sections, and survives the state
//! being saved and loaded.
//!
//! How a picture gets into the note is a user's doing, and the real-window
//! tests `dropping-files` and `pictures-in-a-note` do it: a file dragged out
//! of the system's file manager and dropped on the window, and a paste. Here
//! the picture is put in through the editor, and what is measured is the
//! drawing.

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

    /// The picture put into the note at the caret, as a drop puts it.
    fn put_in(&mut self, alt: &str) {
        self.editor.lock().unwrap().insert_image(alt, &picture_bytes());
        self.step();
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
fn the_picture_is_drawn_in_the_document() {
    let mut window = open("# Notes\n\nbefore");
    window.editor.lock().unwrap().set_caret("# Notes\n\nbefore".len());
    assert_eq!(window.picture_pixels(), 0, "the picture's colour is in the window before any drop");

    window.put_in("cat");
    // The caret is on the line after the reference, so the reference is a
    // picture and not text.
    let text = window.text();
    window.editor.lock().unwrap().set_caret(text.len());
    window.step();

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
    window.put_in("cat");

    let text = window.text();
    window.editor.lock().unwrap().set_caret(text.find("![cat]").unwrap() + 2);
    window.step();

    assert!(window.pictures().is_empty(), "the picture is drawn while its reference is being edited");
    assert_eq!(window.picture_pixels(), 0);
}

#[test]
fn the_images_tab_comes_after_the_sections() {
    let mut window = open("# Notes\n\nbefore");
    assert!(window.images_tab().is_none(), "an images tab with no pictures");

    window.put_in("cat");
    let tab = window.images_tab().expect("no images tab with a picture in the note");
    let section = window.harness.get_by_label("Notes").rect();
    assert!(tab.left() > section.right(), "the images tab is not after the sections");
}

#[test]
fn the_pictures_survive_being_saved_and_loaded() {
    let mut window = open("# Notes\n\nbefore");
    window.editor.lock().unwrap().set_caret("# Notes\n\nbefore".len());
    window.put_in("cat");

    let saved = window.editor.lock().unwrap().state_bytes();
    let mut back = Editor::new();
    back.load_state_bytes(&saved);

    assert_eq!(back.text(), "# Notes\n\nbefore\n\n![cat][1]");
    assert_eq!(back.section_text(0), "# Notes\n\nbefore\n\n![cat][1]");
    assert_eq!(back.images().len(), 1);
    assert_eq!(back.images()[0].bytes().unwrap(), picture_bytes());
}
