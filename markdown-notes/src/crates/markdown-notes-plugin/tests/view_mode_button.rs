//! The button that shows the markdown source.
//!
//! It says the same thing whichever view is on: a button whose label changes
//! is a different button, and which of the two views you are in is what the
//! highlight says. Moving to another section comes back to the formatted view,
//! so a section is opened the way it is meant to be read.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use markdown_notes_core::{Editor, Theme, ViewMode};

const SIZE: (f32, f32) = (900.0, 400.0);

const LABEL: &str = "Markdown source";

/// Two sections, so one can be left for the other.
const SAMPLE: &str = "# One\n\nfirst\n\n# Two\n\nsecond";

fn open() -> (Harness<'static>, Arc<Mutex<Editor>>) {
    let editor = Arc::new(Mutex::new(Editor::new()));
    let kept = Arc::clone(&editor);
    if let Ok(mut e) = editor.lock() {
        e.set_document_text(SAMPLE);
        e.theme = Theme::Light;
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(editor, false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(SIZE.0, SIZE.1))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    (harness, kept)
}

fn button(harness: &Harness<'static>, label: &str) -> egui::Rect {
    harness.get_by_label(label).rect()
}

fn click(harness: &mut Harness<'static>, at: egui::Pos2) {
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.run_steps(2);
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(2);
    }
    // Away from the button, so nothing is left hovered and what is drawn is
    // the button's resting appearance.
    harness.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(10.0, SIZE.1 - 10.0)));
    harness.run_steps(2);
}

fn mode_now(editor: &Arc<Mutex<Editor>>) -> ViewMode {
    editor.lock().map(|e| e.mode).expect("the editor was poisoned")
}

/// The colours inside a rectangle, as a set.
fn colours(image: &image::RgbaImage, rect: egui::Rect) -> Vec<[u8; 4]> {
    let mut seen: Vec<[u8; 4]> = Vec::new();
    for y in rect.top() as u32..rect.bottom() as u32 {
        for x in rect.left() as u32..rect.right() as u32 {
            let p = image.get_pixel(x.min(image.width() - 1), y.min(image.height() - 1)).0;
            if !seen.contains(&p) {
                seen.push(p);
            }
        }
    }
    seen
}

#[test]
fn the_button_keeps_its_label_in_both_views() {
    let (mut harness, editor) = open();
    let at = button(&harness, LABEL).center();

    click(&mut harness, at);
    assert_eq!(mode_now(&editor), ViewMode::Raw, "the click did not change the view");
    assert_eq!(
        button(&harness, LABEL).center(),
        at,
        "the button is somewhere else, so its label changed with the view"
    );

    click(&mut harness, at);
    assert_eq!(mode_now(&editor), ViewMode::Wysiwyg);
    assert_eq!(button(&harness, LABEL).center(), at);
}

#[test]
fn the_button_is_highlighted_while_the_source_is_showing() {
    let (mut harness, _editor) = open();
    let rect = button(&harness, LABEL);
    let formatted = colours(&harness.render().expect("rendering failed"), rect);

    click(&mut harness, rect.center());
    let source = colours(&harness.render().expect("rendering failed"), rect);

    assert_ne!(
        source, formatted,
        "the button looks the same in both views, so nothing says which one is on"
    );
}

#[test]
fn opening_another_section_comes_back_to_the_formatted_view() {
    let (mut harness, editor) = open();
    let at = button(&harness, LABEL).center();
    click(&mut harness, at);
    assert_eq!(mode_now(&editor), ViewMode::Raw, "the source view was not turned on");

    let other = button(&harness, "Two").center();
    click(&mut harness, other);

    assert_eq!(
        mode_now(&editor),
        ViewMode::Wysiwyg,
        "the other section opened in the source view"
    );
}
