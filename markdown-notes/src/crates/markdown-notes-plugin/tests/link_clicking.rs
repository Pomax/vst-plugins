//! Following a link.
//!
//! A click on a link puts the caret in it, the way a click on any text does.
//! With the command key held (Ctrl on Windows and Linux, Cmd on macOS) a click
//! on a link asks the operating system to open its destination instead, and
//! the caret stays where it was.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use markdown_notes_core::{Editor, Rgba, Theme};

const WIDTH: f32 = 700.0;
const HEIGHT: f32 = 400.0;

/// Where the toolbars end and the document begins.
const TOOLBARS: u32 = 90;

const URL: &str = "https://example.com/x";

/// A line of plain text, then a line with a link in the middle of it. The
/// caret starts on the first line, so the second is drawn with its markers
/// hidden.
const SAMPLE: &str = "first line\n\nsee [the docs](https://example.com/x) for more";

/// Link text is drawn in a colour nothing else in the window uses.
fn is_link(p: [u8; 4]) -> bool {
    p[3] == 255 && p[2] > 180 && p[0] < 90 && p[1] < 90
}

fn with_editor() -> (Harness<'static>, Arc<Mutex<Editor>>) {
    let editor = Arc::new(Mutex::new(Editor::with_text(SAMPLE)));
    let kept = Arc::clone(&editor);
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
        e.colours.light.link = Rgba::rgb(0, 0, 255);
        e.set_caret(0);
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

/// The URLs the window asked the platform to open during the last frame.
fn opened(harness: &Harness<'static>) -> Vec<String> {
    harness
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|command| match command {
            egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
            _ => None,
        })
        .collect()
}

/// Click at `at` with `modifiers` held, and return every URL the window asked
/// to have opened while the click went through.
fn click(harness: &mut Harness<'static>, at: egui::Pos2, modifiers: egui::Modifiers) -> Vec<String> {
    let mut urls = Vec::new();
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.run_steps(2);
    harness.input_mut().events.push(egui::Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
        for _ in 0..2 {
            harness.step();
            urls.extend(opened(harness));
        }
    }
    harness.input_mut().events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run_steps(1);
    urls
}

/// The cursor asked for with the pointer resting at `at` and `modifiers`
/// held.
fn cursor_at(
    harness: &mut Harness<'static>,
    at: egui::Pos2,
    modifiers: egui::Modifiers,
) -> egui::CursorIcon {
    harness.input_mut().events.push(egui::Event::ModifiersChanged(modifiers));
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.run_steps(3);
    let cursor = harness.output().platform_output.cursor_icon;
    harness.input_mut().events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run_steps(1);
    cursor
}

/// The middle of the link text, as drawn.
fn middle_of_the_link(harness: &mut Harness<'static>) -> egui::Pos2 {
    let image = harness.render().expect("rendering failed");
    let (mut left, mut right, mut top, mut bottom) = (u32::MAX, 0, u32::MAX, 0);
    for (x, y, p) in image.enumerate_pixels() {
        if y > TOOLBARS && is_link(p.0) {
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
        }
    }
    assert!(left < right, "no link text was drawn in the document");
    egui::pos2((left + right) as f32 / 2.0, (top + bottom) as f32 / 2.0)
}

/// A point level with the link, well to the right of the text on its line.
fn right_of_the_link(harness: &mut Harness<'static>) -> egui::Pos2 {
    let on_the_link = middle_of_the_link(harness);
    egui::pos2(WIDTH - 40.0, on_the_link.y)
}

#[test]
fn the_pointer_points_at_a_link_while_the_command_key_is_held() {
    let (mut harness, _) = with_editor();
    let at = middle_of_the_link(&mut harness);
    assert_eq!(
        cursor_at(&mut harness, at, egui::Modifiers::COMMAND),
        egui::CursorIcon::PointingHand
    );
}

#[test]
fn the_pointer_is_an_i_beam_on_a_link_without_the_command_key() {
    let (mut harness, _) = with_editor();
    let at = middle_of_the_link(&mut harness);
    assert_eq!(
        cursor_at(&mut harness, at, egui::Modifiers::NONE),
        egui::CursorIcon::Text
    );
}

#[test]
fn the_pointer_is_an_i_beam_beside_a_link_with_the_command_key_held() {
    let (mut harness, _) = with_editor();
    let at = right_of_the_link(&mut harness);
    assert_eq!(
        cursor_at(&mut harness, at, egui::Modifiers::COMMAND),
        egui::CursorIcon::Text
    );
}

#[test]
fn command_click_on_a_link_opens_its_url() {
    let (mut harness, editor) = with_editor();
    let at = middle_of_the_link(&mut harness);

    let urls = click(&mut harness, at, egui::Modifiers::COMMAND);

    assert_eq!(urls, vec![URL.to_string()], "the link's URL was not sent to be opened");
    let e = editor.lock().unwrap();
    assert!(e.has_caret(), "following a link took the caret away");
    assert_eq!(e.caret(), 0, "following a link moved the caret");
}

#[test]
fn a_plain_click_on_a_link_puts_the_caret_in_it_and_opens_nothing() {
    let (mut harness, editor) = with_editor();
    let at = middle_of_the_link(&mut harness);

    let urls = click(&mut harness, at, egui::Modifiers::NONE);

    assert!(urls.is_empty(), "a plain click opened {urls:?}");
    let e = editor.lock().unwrap();
    let text = SAMPLE.find("the docs").expect("the sample lost its link");
    assert!(
        (text..text + "the docs".len()).contains(&e.caret()),
        "the caret went to {} rather than into the link text",
        e.caret()
    );
}

#[test]
fn command_click_beside_a_link_puts_the_caret_there_and_opens_nothing() {
    let (mut harness, editor) = with_editor();
    let at = right_of_the_link(&mut harness);

    let urls = click(&mut harness, at, egui::Modifiers::COMMAND);

    assert!(urls.is_empty(), "a click beside the link opened {urls:?}");
    let e = editor.lock().unwrap();
    assert_eq!(
        e.caret(),
        SAMPLE.len(),
        "clicking to the right of the line put the caret somewhere other than its end"
    );
}
