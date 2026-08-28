//! Where a click in the document puts the caret, and where it takes it away.
//!
//! A click has to land on a line for there to be anywhere to type. Below the
//! last line there is no line to land on, so the caret goes, and beside a line
//! there is: the caret goes to whichever end of it the click was nearer.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use markdown_notes_core::{Editor, Rgba, Theme};

const WIDTH: f32 = 700.0;
const HEIGHT: f32 = 400.0;

/// Where the toolbars end and the document begins. The section strip is the
/// lower of the two and ends well above this.
const TOOLBARS: u32 = 90;

/// Two short lines, so most of the window below them is empty document.
const SAMPLE: &str = "first line\n\nsecond line";

/// The caret is drawn in a colour nothing else in the window uses, and drawn
/// thin, so its edges are blended into whatever is behind it: red enough, not
/// exactly red.
fn is_caret(p: [u8; 4]) -> bool {
    p[3] == 255 && p[0] > 180 && p[1] < 90 && p[2] < 90
}

fn is_text(p: [u8; 4]) -> bool {
    p[3] == 255 && !is_caret(p) && (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3 < 110
}

/// Keep the frame a failing test judged, beside the UI tests' screenshots.
fn saved(image: &image::RgbaImage, name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cache/uitests")
        .join(format!("{name}.png"));
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match image.save(&path) {
        Ok(()) => path.display().to_string(),
        Err(e) => format!("not saved: {e}"),
    }
}

fn with_editor() -> (Harness<'static>, Arc<Mutex<Editor>>) {
    let editor = Arc::new(Mutex::new(Editor::with_text(SAMPLE)));
    let kept = Arc::clone(&editor);
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
        e.colours.light.caret = Rgba::rgba(255, 0, 0, 255);
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(editor, false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(WIDTH, HEIGHT))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    (harness, kept)
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

fn caret_drawn(harness: &mut Harness<'static>) -> bool {
    let image = harness.render().expect("rendering failed");
    image.pixels().any(|p| is_caret(p.0))
}

/// The rows of each line of text in the document, top line first, told apart
/// by the blank rows between them.
fn lines(image: &image::RgbaImage) -> Vec<Vec<u32>> {
    let mut rows: Vec<u32> = image
        .enumerate_pixels()
        .filter(|(_, y, p)| *y > TOOLBARS && is_text(p.0))
        .map(|(_, y, _)| y)
        .collect();
    rows.sort_unstable();
    rows.dedup();

    let mut lines: Vec<Vec<u32>> = Vec::new();
    for y in rows {
        match lines.last_mut() {
            Some(line) if y - line[line.len() - 1] <= 1 => line.push(y),
            _ => lines.push(vec![y]),
        }
    }
    lines
}

/// Halfway down the first line of the document.
fn first_line(harness: &mut Harness<'static>) -> f32 {
    let image = harness.render().expect("rendering failed");
    let lines = lines(&image);
    assert!(
        !lines.is_empty(),
        "no text was drawn in the document\n      rendering: {}",
        saved(&image, "clicking-the-document")
    );
    let line = &lines[0];
    (line[0] + line[line.len() - 1]) as f32 / 2.0
}

/// A point level with the first line, well to the right of the text on it.
fn right_of_the_first_line(harness: &mut Harness<'static>) -> egui::Pos2 {
    egui::pos2(WIDTH - 40.0, first_line(harness))
}

/// Just left of the first letter of the document, which is still inside the
/// text area: further left than this is the panel's margin, which belongs to
/// no line and is not what "beside a line" means.
fn left_of_the_text(harness: &mut Harness<'static>) -> f32 {
    let image = harness.render().expect("rendering failed");
    let left = image
        .enumerate_pixels()
        .filter(|(_, y, p)| *y > TOOLBARS && is_text(p.0))
        .map(|(x, _, _)| x)
        .min()
        .expect("no text was drawn in the document");
    left as f32 - 2.0
}

/// A point below every line of the document.
fn below_the_text(harness: &mut Harness<'static>) -> egui::Pos2 {
    let image = harness.render().expect("rendering failed");
    let lines = lines(&image);
    let last = lines.last().expect("no text was drawn in the document");
    let bottom = last[last.len() - 1] as f32;
    assert!(
        bottom + 40.0 < HEIGHT,
        "the text reaches the bottom of the window, so there is no empty document to click in"
    );
    egui::pos2(WIDTH / 2.0, bottom + 30.0)
}

#[test]
fn clicking_below_the_last_line_takes_the_caret_away() {
    let (mut harness, editor) = with_editor();
    assert!(
        editor.lock().unwrap().has_caret(),
        "the document came up without a caret"
    );
    assert!(caret_drawn(&mut harness), "no caret was drawn to begin with");

    let at = below_the_text(&mut harness);
    click(&mut harness, at);

    assert!(
        !editor.lock().unwrap().has_caret(),
        "clicking the empty space below the text left the caret where it was"
    );
    assert!(
        !caret_drawn(&mut harness),
        "a caret is still drawn after being clicked away"
    );
}

#[test]
fn clicking_a_line_after_that_brings_the_caret_back() {
    let (mut harness, editor) = with_editor();
    let away = below_the_text(&mut harness);
    click(&mut harness, away);
    assert!(!editor.lock().unwrap().has_caret(), "the caret was not taken away");

    let back = right_of_the_first_line(&mut harness);
    click(&mut harness, back);

    let e = editor.lock().unwrap();
    assert!(e.has_caret(), "clicking a line did not put the caret back");
    assert_eq!(
        e.caret(),
        "first line".len(),
        "clicking to the right of the first line put the caret somewhere other \
         than the end of it"
    );
}

#[test]
fn clicking_to_the_right_of_a_line_puts_the_caret_at_its_end() {
    let (mut harness, editor) = with_editor();
    let at = right_of_the_first_line(&mut harness);
    click(&mut harness, at);
    let e = editor.lock().unwrap();
    assert!(e.has_caret());
    assert_eq!(e.caret(), "first line".len());
}

#[test]
fn clicking_to_the_left_of_a_line_puts_the_caret_at_its_start() {
    let (mut harness, editor) = with_editor();
    let level = first_line(&mut harness);
    let left = left_of_the_text(&mut harness);
    click(&mut harness, egui::pos2(left, level));
    let e = editor.lock().unwrap();
    assert!(e.has_caret());
    assert_eq!(e.caret(), 0);
}
