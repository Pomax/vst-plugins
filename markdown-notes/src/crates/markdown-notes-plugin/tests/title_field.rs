//! The note's name, and what clicking into it does to what is already there.
//!
//! A name nobody has set is a prompt to set one, so it goes when the first
//! character arrives. A name somebody chose is text to edit, so it stays.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use markdown_notes_core::{Editor, Theme, DEFAULT_TITLE};

/// Wide enough for the toolbar to lay out the way it does in the plugin, so
/// the name is a field across the middle of it rather than a squeezed sliver.
const SIZE: (f32, f32) = (900.0, 400.0);

const CHOSEN: &str = "Ghostlight";

fn with_title(title: Option<&str>) -> (Harness<'static>, Arc<Mutex<Editor>>) {
    let editor = Arc::new(Mutex::new(Editor::new()));
    let kept = Arc::clone(&editor);
    if let Ok(mut e) = editor.lock() {
        e.set_document_text("# Notes\n\nsomething");
        e.theme = Theme::Light;
        if let Some(title) = title {
            e.title = title.to_string();
        }
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

/// The field is found by what it holds, which is the name itself. By role as
/// well: the text drawn in the field carries the same value as the field.
fn field(harness: &Harness<'static>, showing: &str) -> egui::Rect {
    harness
        .get(egui_kittest::kittest::By::new().role(accesskit::Role::TextInput).value(showing))
        .rect()
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
}

fn type_text(harness: &mut Harness<'static>, text: &str) {
    harness.input_mut().events.push(egui::Event::Text(text.to_string()));
    harness.run_steps(3);
}

fn press(harness: &mut Harness<'static>, key: egui::Key) {
    harness.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(3);
}

fn title_now(editor: &Arc<Mutex<Editor>>) -> String {
    editor.lock().map(|e| e.title.clone()).unwrap_or_default()
}

/// Anything drawn over the toolbar's own fill: text, or the caret.
fn is_ink(p: [u8; 4]) -> bool {
    let bar = 232u8;
    p[3] == 255 && (p[0].abs_diff(bar) > 30 || p[1].abs_diff(bar) > 30 || p[2].abs_diff(bar) > 30)
}

/// The columns of `rect` that have something drawn in them.
fn inked_columns(image: &image::RgbaImage, rect: egui::Rect) -> Vec<u32> {
    let mut columns = Vec::new();
    for x in rect.left() as u32..rect.right() as u32 {
        for y in rect.top() as u32 + 1..rect.bottom() as u32 - 1 {
            if is_ink(image.get_pixel(x, y).0) {
                columns.push(x);
                break;
            }
        }
    }
    columns
}

#[test]
fn a_name_nobody_set_is_replaced_by_the_first_thing_typed() {
    let (mut harness, editor) = with_title(None);
    assert_eq!(title_now(&editor), DEFAULT_TITLE, "the name to replace was not there");

    let at = field(&harness, DEFAULT_TITLE).center();
    click(&mut harness, at);
    type_text(&mut harness, CHOSEN);

    assert_eq!(title_now(&editor), CHOSEN);
}

/// An emptied name shows nothing at all, and the caret waits in the middle of
/// the field where the first character will go.
#[test]
fn an_emptied_name_is_an_empty_field_with_the_caret_in_the_middle() {
    let (mut harness, editor) = with_title(None);

    let rect = field(&harness, DEFAULT_TITLE);
    click(&mut harness, rect.center());
    // The whole name arrives selected, so one press of Backspace clears it.
    press(&mut harness, egui::Key::Backspace);
    assert_eq!(title_now(&editor), "", "the name was not cleared");

    // The harness draws a pointer where it last was, which is in the field.
    harness.remove_cursor();
    harness.run_steps(2);
    let image = harness.render().expect("rendering failed");
    let columns = inked_columns(&image, rect);
    assert!(!columns.is_empty(), "no caret was drawn");

    // A caret is a line a pixel or two wide. Anything wider is text, and the
    // only text that can be there is a name that was supposed to be gone.
    let (left, right) = (columns[0], columns[columns.len() - 1]);
    assert!(
        right - left <= 3,
        "{} columns of the field have something drawn in them, from {left} to {right}, \
         and a caret is two or three",
        columns.len()
    );

    let caret = (left + right) as f32 / 2.0;
    assert!(
        (caret - rect.center().x).abs() <= 2.0,
        "the caret is at {caret}, and the middle of the field is {}",
        rect.center().x
    );
}

/// Enter finishes the name and hands the keyboard to the open section, so
/// naming a note and carrying on writing it is one movement.
#[test]
fn enter_hands_the_keyboard_to_the_section_that_is_open() {
    let (mut harness, editor) = with_title(None);

    let at = field(&harness, DEFAULT_TITLE).center();
    click(&mut harness, at);
    type_text(&mut harness, CHOSEN);
    press(&mut harness, egui::Key::Enter);
    type_text(&mut harness, "!");

    let (title, document) = editor
        .lock()
        .map(|e| (e.title.clone(), e.text()))
        .expect("the editor was poisoned");
    assert_eq!(title, CHOSEN, "the keystroke after Enter went into the name");
    assert!(document.contains('!'), "the section did not get the keystroke: {document:?}");
}

/// Clearing the name and clicking away leaves it cleared. Putting anything
/// back would be the field overruling somebody who just said what they wanted.
#[test]
fn a_cleared_name_stays_cleared_after_the_field_is_left() {
    let (mut harness, editor) = with_title(Some(""));

    let rect = field(&harness, "");
    click(&mut harness, rect.center());
    type_text(&mut harness, "x");
    assert_eq!(title_now(&editor), "x", "the field could not be typed into");

    press(&mut harness, egui::Key::Backspace);

    // Somewhere in the document, which is not the field.
    click(&mut harness, egui::pos2(rect.center().x, SIZE.1 - 40.0));

    assert_eq!(title_now(&editor), "");
}

#[test]
fn a_name_somebody_chose_survives_being_clicked_into() {
    let (mut harness, editor) = with_title(Some(CHOSEN));

    // The right-hand end of the field is past the end of a centred name, so the
    // caret lands after the last character rather than somewhere inside it.
    let rect = field(&harness, CHOSEN);
    let at = egui::pos2(rect.right() - 2.0, rect.center().y);
    click(&mut harness, at);
    type_text(&mut harness, "!");

    assert_eq!(title_now(&editor), format!("{CHOSEN}!"));
}
