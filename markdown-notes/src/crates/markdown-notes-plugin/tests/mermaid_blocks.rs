//! Mermaid blocks: a picture while the caret is elsewhere, code while it is in
//! them, and always code in the source view.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use markdown_notes_core::{Editor, Theme, ViewMode};
use markdown_notes_plugin::gui::Pictures;

const WIDTH: f32 = 700.0;
const HEIGHT: f32 = 500.0;

/// The light theme's code background, `Rgba::grey(225)`.
const CODE: [u8; 4] = [225, 225, 225, 255];

const BLOCK: &str = "```mermaid\nflowchart LR\n    A[Start] --> B[End]\n```";

fn sample() -> String {
    format!("intro\n\n{BLOCK}\n\noutro")
}

/// Offset of the first character of the block's code.
fn code_start(text: &str) -> usize {
    text.find("flowchart").expect("the sample has no flowchart in it")
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
        e.set_caret(0);
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
    /// Where each picture is, the error ones included.
    fn pictures(&self) -> Vec<egui::Rect> {
        self.pictures.lock().unwrap().iter().map(|p| p.rect).collect()
    }

    /// Which of the pictures are the error standing in for a diagram.
    fn failed(&self) -> Vec<bool> {
        self.pictures.lock().unwrap().iter().map(|p| p.failed).collect()
    }

    fn text(&self) -> String {
        self.editor.lock().unwrap().text()
    }

    fn step(&mut self) {
        self.harness.run_steps(2);
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.harness
                .input_mut()
                .events
                .push(egui::Event::Text(c.to_string()));
            self.step();
        }
    }

    fn press(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        self.harness.input_mut().events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
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

    fn image(&mut self) -> image::RgbaImage {
        self.harness.render().expect("rendering failed")
    }
}

/// How many rows of the window a code block is drawn across.
fn code_rows(image: &image::RgbaImage) -> usize {
    let mut counts = vec![0u32; image.height() as usize];
    for (_, y, p) in image.enumerate_pixels() {
        if p.0 == CODE {
            counts[y as usize] += 1;
        }
    }
    let wide = image.width() / 2;
    counts.iter().filter(|n| **n > wide).count()
}

/// How many pixels inside `rect` are not the page the picture sits on, which
/// is whatever its top left corner is.
fn ink(image: &image::RgbaImage, rect: egui::Rect) -> usize {
    let left = rect.left().ceil() as u32;
    let top = rect.top().ceil() as u32;
    let right = (rect.right().floor() as u32).min(image.width());
    let bottom = (rect.bottom().floor() as u32).min(image.height());
    let page = image.get_pixel(left, top).0;
    let mut count = 0;
    for y in top..bottom {
        for x in left..right {
            if image.get_pixel(x, y).0 != page {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn a_mermaid_block_away_from_the_caret_is_a_picture() {
    let mut window = open(&sample());
    let pictures = window.pictures();
    assert_eq!(pictures.len(), 1, "the block was not drawn as a picture");
    assert_eq!(window.failed(), [false], "the sample was drawn as an error");
    assert!(pictures[0].height() > 20.0, "the picture has no height: {:?}", pictures[0]);

    let image = window.image();
    assert_eq!(code_rows(&image), 0, "the block's code is drawn as well as its picture");
    assert!(
        ink(&image, pictures[0]) > 200,
        "nothing is drawn where the picture is meant to be"
    );
}

#[test]
fn the_picture_does_not_change_the_text() {
    let window = open(&sample());
    assert_eq!(window.pictures().len(), 1);
    assert_eq!(window.text(), sample());
    assert!(!window.editor.lock().unwrap().is_dirty());
}

#[test]
fn the_caret_in_the_block_brings_the_code_back() {
    let mut window = open(&sample());
    assert_eq!(window.pictures().len(), 1);

    let inside = code_start(&sample()) + 3;
    window.editor.lock().unwrap().set_caret(inside);
    window.step();
    assert!(window.pictures().is_empty(), "the picture stayed with the caret in its block");
    assert!(code_rows(&window.image()) > 0, "the block's code was not drawn");

    window.editor.lock().unwrap().set_caret(0);
    window.step();
    assert_eq!(window.pictures().len(), 1, "the picture did not come back");
}

#[test]
fn the_caret_on_either_fence_brings_the_code_back() {
    let text = sample();
    for at in [text.find("```").unwrap(), text.rfind("```").unwrap() + 3] {
        let mut window = open(&text);
        window.editor.lock().unwrap().set_caret(at);
        window.step();
        assert!(window.pictures().is_empty(), "caret at {at} left the picture up");
    }
}

#[test]
fn arrowing_down_into_the_block_brings_the_code_back() {
    let mut window = open(&sample());
    assert_eq!(window.pictures().len(), 1);

    window.press(egui::Key::ArrowDown, egui::Modifiers::NONE);
    assert_eq!(window.pictures().len(), 1, "the blank line above is not the block");
    window.press(egui::Key::ArrowDown, egui::Modifiers::NONE);
    assert!(window.pictures().is_empty(), "the caret is in the block and the picture is still up");
    assert!(code_rows(&window.image()) > 0, "the block's code was not drawn");
}

#[test]
fn clicking_the_picture_puts_the_caret_in_the_block() {
    let mut window = open(&sample());
    let pictures = window.pictures();
    assert_eq!(pictures.len(), 1);

    let at = egui::pos2(pictures[0].left() + 30.0, pictures[0].center().y);
    window.click(at);

    assert_eq!(window.editor.lock().unwrap().caret(), code_start(&sample()));
    assert!(window.pictures().is_empty(), "the picture stayed up after being clicked");
    assert!(code_rows(&window.image()) > 0, "the block's code was not drawn");
}

#[test]
fn typing_a_mermaid_block_writes_it_and_leaving_draws_it() {
    let mut window = open("");
    window.type_text("```mermaid");
    window.press(egui::Key::Enter, egui::Modifiers::NONE);
    window.type_text("flowchart LR");
    window.press(egui::Key::Enter, egui::Modifiers::NONE);
    window.type_text("A[Start] --> B[End]");

    assert_eq!(
        window.text(),
        "```mermaid\nflowchart LR\nA[Start] --> B[End]\n```"
    );
    assert!(window.pictures().is_empty(), "the block became a picture while it was being typed");

    window.press(egui::Key::End, egui::Modifiers::CTRL);
    window.press(egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(
        window.text(),
        "```mermaid\nflowchart LR\nA[Start] --> B[End]\n```\n"
    );
    assert_eq!(window.pictures().len(), 1, "leaving the block did not draw it");

    window.press(egui::Key::ArrowUp, egui::Modifiers::NONE);
    assert!(window.pictures().is_empty(), "coming back to the block left the picture up");
}

#[test]
fn the_source_view_shows_the_source() {
    let mut window = open(&sample());
    assert_eq!(window.pictures().len(), 1);

    window.editor.lock().unwrap().mode = ViewMode::Raw;
    window.step();
    assert!(window.pictures().is_empty(), "the source view drew a picture");
    assert!(code_rows(&window.image()) > 0, "the source view did not draw the code");
    assert_eq!(window.text(), sample());

    window.editor.lock().unwrap().mode = ViewMode::Wysiwyg;
    window.step();
    assert_eq!(window.pictures().len(), 1, "the picture did not come back");
}

const BROKEN: &str = "intro\n\n```mermaid\nthis is not a diagram\n```\n\noutro";

#[test]
fn code_mermaid_cannot_read_is_a_picture_of_an_error() {
    let mut window = open(BROKEN);
    let pictures = window.pictures();
    assert_eq!(pictures.len(), 1, "the broken block was not drawn as a picture");
    assert_eq!(window.failed(), [true], "the broken block was not drawn as an error");

    let image = window.image();
    assert_eq!(code_rows(&image), 0, "the broken code is drawn as well as the error");
    assert!(
        ink(&image, pictures[0]) > 200,
        "nothing is drawn where the error is meant to be"
    );
    assert_eq!(window.text(), BROKEN, "drawing the error changed the text");
}

#[test]
fn the_caret_in_a_broken_block_brings_the_code_back() {
    let mut window = open(BROKEN);
    assert_eq!(window.failed(), [true]);

    let inside = BROKEN.find("this").unwrap();
    window.editor.lock().unwrap().set_caret(inside);
    window.step();
    assert!(window.pictures().is_empty(), "the error stayed with the caret in its block");
    assert!(code_rows(&window.image()) > 0, "the broken code was not drawn");
}

#[test]
fn an_empty_block_is_a_picture_of_an_error() {
    let window = open("intro\n\n```mermaid\n```\n\noutro");
    assert_eq!(window.failed(), [true]);
}

#[test]
fn a_block_in_another_language_stays_code() {
    let mut window = open("intro\n\n```rust\nlet x = 1;\n```\n\noutro");
    assert!(window.pictures().is_empty());
    assert!(code_rows(&window.image()) > 0);
}

#[test]
fn a_block_nothing_closes_stays_code() {
    let mut window = open("intro\n\n```mermaid\nflowchart LR\n    A --> B");
    assert!(window.pictures().is_empty());
    assert!(code_rows(&window.image()) > 0);
}

#[test]
fn every_block_gets_its_own_picture() {
    let text = format!("{}\n\n{BLOCK}\n\nend", sample());
    let window = open(&text);
    let pictures = window.pictures();
    assert_eq!(pictures.len(), 2);
    assert!(
        pictures[1].top() >= pictures[0].bottom(),
        "the pictures overlap: {pictures:?}"
    );
}
