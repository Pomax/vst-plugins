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

const CHOSEN: &str = "funky cake";

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

/// Far longer than the room the toolbar leaves for the name.
const LONG: &str = "This is a long title, a good deal longer than any toolbar has the room \
                    to show, and it carries on for a while after that as well";

/// What the name field holds on screen, and where it is.
fn name_shown(harness: &Harness<'static>) -> (String, egui::Rect) {
    let field = harness.get(egui_kittest::kittest::By::new().role(accesskit::Role::TextInput));
    (field.value().unwrap_or_default(), field.rect())
}

/// Keep a picture of the window beside the UI tests' own, to be looked at.
fn keep(image: &image::RgbaImage, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.cache/uitests");
    let _ = std::fs::create_dir_all(&dir);
    let _ = image.save(dir.join(name));
}

fn hover(harness: &mut Harness<'static>, at: egui::Pos2) {
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.run_steps(8);
}

/// A name longer than fits is shown as however much of it fits, plus `...`.
#[test]
fn a_name_longer_than_fits_is_shown_cut_and_ending_in_three_dots() {
    let (mut harness, editor) = with_title(Some(LONG));

    let (shown, rect) = name_shown(&harness);
    assert_ne!(shown, LONG, "the whole name is in the field");
    let start = shown.strip_suffix("...").unwrap_or_else(|| panic!("{shown:?} has no ... on it"));
    assert!(!start.is_empty(), "nothing of the name is shown");
    assert!(LONG.starts_with(start), "{start:?} is not how the name starts");
    assert_eq!(title_now(&editor), LONG, "cutting what is shown cut the name itself");

    // Nothing is drawn against either edge of the field: what is shown fits.
    harness.remove_cursor();
    harness.run_steps(2);
    let image = harness.render().expect("rendering failed");
    keep(&image, "title-cut.png");
    let columns = inked_columns(&image, rect);
    let (left, right) = (columns[0], columns[columns.len() - 1]);
    assert!(
        left > rect.left() as u32 + 1 && right < rect.right() as u32 - 2,
        "the name is drawn from {left} to {right} in a field from {} to {}",
        rect.left(),
        rect.right()
    );
}

/// Hovering over a name that was cut pops up the whole of it.
#[test]
fn hovering_over_a_cut_name_shows_the_whole_of_it() {
    let (mut harness, _editor) = with_title(Some(LONG));
    assert!(harness.query_by_label(LONG).is_none(), "the whole name is up before any hovering");

    let (_, rect) = name_shown(&harness);
    hover(&mut harness, rect.center());

    if let Ok(image) = harness.render() {
        keep(&image, "title-cut-hovered.png");
    }
    assert!(harness.query_by_label(LONG).is_some(), "no tooltip with the whole name in it");
}

/// A name that fits is all there, so hovering over it says nothing more.
#[test]
fn hovering_over_a_name_that_fits_shows_nothing() {
    let (mut harness, _editor) = with_title(Some(CHOSEN));

    let rect = field(&harness, CHOSEN);
    hover(&mut harness, rect.center());

    assert!(harness.query_by_label(CHOSEN).is_none(), "a name that fits got a tooltip");
}

/// A name that fits, then a window too narrow for it: the name is cut.
const FITS_AT_FIRST: &str = "Funky cake and friends";

fn shrink(harness: &mut Harness<'static>) {
    harness.set_size(egui::vec2(560.0, SIZE.1));
    harness.run_steps(4);
}

/// A name written and left, and then the window made narrower than it: it
/// is cut to fit from then on, the same as one that never fit.
#[test]
fn a_name_that_stops_fitting_when_the_window_shrinks_is_cut() {
    let (mut harness, editor) = with_title(Some(FITS_AT_FIRST));
    assert_eq!(name_shown(&harness).0, FITS_AT_FIRST, "the name did not fit to begin with");

    shrink(&mut harness);

    let (shown, _) = name_shown(&harness);
    assert!(shown.ends_with("..."), "{shown:?} is not cut after the window shrank");
    assert_eq!(title_now(&editor), FITS_AT_FIRST, "the name itself was cut");
}

/// The same, when the name was typed and the keyboard is still in it as the
/// window shrinks: resizing is not typing, so the name is done being edited
/// and is cut like any other.
#[test]
fn a_name_just_typed_is_cut_when_the_window_shrinks_around_it() {
    let (mut harness, editor) = with_title(None);
    let rect = field(&harness, DEFAULT_TITLE);
    click(&mut harness, rect.center());
    type_text(&mut harness, FITS_AT_FIRST);
    assert_eq!(name_shown(&harness).0, FITS_AT_FIRST, "the name did not fit to begin with");

    shrink(&mut harness);

    let (shown, _) = name_shown(&harness);
    assert!(shown.ends_with("..."), "{shown:?} is not cut after the window shrank");
    assert_eq!(title_now(&editor), FITS_AT_FIRST, "the name itself was cut");
}

/// Clicking into a cut name edits the name, not the cut of it.
#[test]
fn a_cut_name_is_whole_again_while_it_is_being_edited() {
    let (mut harness, editor) = with_title(Some(LONG));

    let (_, rect) = name_shown(&harness);
    click(&mut harness, rect.center());
    assert_eq!(name_shown(&harness).0, LONG, "the field being edited holds the cut name");

    press(&mut harness, egui::Key::End);
    type_text(&mut harness, "!");
    assert_eq!(title_now(&editor), format!("{LONG}!"));

    // Somewhere in the document: the field is left, and is cut again.
    click(&mut harness, egui::pos2(rect.center().x, SIZE.1 - 40.0));
    let (shown, _) = name_shown(&harness);
    assert!(shown.ends_with("..."), "{shown:?} is not cut after the field was left");
    assert_eq!(title_now(&editor), format!("{LONG}!"));
}

/// Where the caret is drawn in `rect`: the column inked from the top of the
/// row to the bottom of it. No letter is, since none fills both the room above
/// the small letters and the room below the line.
fn caret_column(image: &image::RgbaImage, rect: egui::Rect) -> Option<u32> {
    (rect.left() as u32..rect.right() as u32).find(|&x| {
        (rect.top() as u32 + 4..rect.bottom() as u32 - 4).all(|y| is_ink(image.get_pixel(x, y).0))
    })
}

fn caret_after(harness: &mut Harness<'static>, rect: egui::Rect, name: &str) -> Option<u32> {
    harness.remove_cursor();
    harness.run_steps(2);
    let image = harness.render().expect("rendering failed");
    keep(&image, name);
    caret_column(&image, rect)
}

/// A name typed past the end of the field keeps the caret in view, at the end
/// that is being typed at, and Home brings the start of it back with the caret
/// there.
#[test]
fn the_caret_stays_in_view_while_a_name_longer_than_fits_is_typed() {
    let (mut harness, editor) = with_title(None);

    let rect = field(&harness, DEFAULT_TITLE);
    click(&mut harness, rect.center());
    type_text(&mut harness, LONG);
    assert_eq!(title_now(&editor), LONG);

    let caret = caret_after(&mut harness, rect, "title-typed-past-the-end.png")
        .expect("no caret is in view after typing past the end of the field");
    assert!(
        caret as f32 > rect.center().x,
        "the caret is at {caret}, which is not the end being typed at, in a field from {} to {}",
        rect.left(),
        rect.right()
    );

    press(&mut harness, egui::Key::Home);
    let caret = caret_after(&mut harness, rect, "title-home.png")
        .expect("no caret is in view after Home");
    assert!(
        (caret as f32) < rect.left() + 12.0,
        "the caret is at {caret} after Home, in a field that starts at {}",
        rect.left()
    );
}

/// A name that fits stays centred in the field while it is typed.
#[test]
fn a_name_that_fits_is_centred_while_it_is_typed() {
    let (mut harness, _editor) = with_title(None);

    let rect = field(&harness, DEFAULT_TITLE);
    click(&mut harness, rect.center());
    type_text(&mut harness, CHOSEN);

    harness.remove_cursor();
    harness.run_steps(2);
    let image = harness.render().expect("rendering failed");
    let columns = inked_columns(&image, rect);
    let (left, right) = (columns[0] as f32, columns[columns.len() - 1] as f32);
    let middle = (left + right) / 2.0;
    assert!(
        (middle - rect.center().x).abs() <= 6.0,
        "the name is drawn from {left} to {right}, and the middle of the field is {}",
        rect.center().x
    );
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
