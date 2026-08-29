//! The caret is kept on screen.
//!
//! A document taller than the window is scrolled, and what should be in view
//! is wherever the caret is: writing past the bottom edge has to bring the
//! line being written into sight, or the typing happens somewhere the writer
//! cannot see.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use markdown_notes_core::{Editor, Rgba, Theme};

const WIDTH: f32 = 700.0;
const HEIGHT: f32 = 400.0;

/// Far more lines than the window can hold.
fn tall_document() -> String {
    (0..60).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n")
}

/// The caret is drawn in a colour nothing else uses, and drawn thin, so its
/// edges blend: red enough, not exactly red.
fn is_caret(p: [u8; 4]) -> bool {
    p[3] == 255 && p[0] > 180 && p[1] < 90 && p[2] < 90
}

fn render(text: &str, caret: usize) -> image::RgbaImage {
    let editor = Arc::new(Mutex::new(Editor::with_text(text)));
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
        e.colours.light.caret = Rgba::rgba(255, 0, 0, 255);
        e.set_caret(caret);
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(editor, false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(WIDTH, HEIGHT))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    harness.render().expect("rendering failed")
}

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

fn caret_row(image: &image::RgbaImage) -> Option<u32> {
    image
        .enumerate_pixels()
        .find(|(_, _, p)| is_caret(p.0))
        .map(|(_, y, _)| y)
}

/// The same window, kept, so the view can be changed with the button and the
/// document looked at again afterwards.
fn open(text: &str, caret: usize, mode: markdown_notes_core::ViewMode) -> Harness<'static> {
    let editor = Arc::new(Mutex::new(Editor::with_text(text)));
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
        e.colours.light.caret = Rgba::rgba(255, 0, 0, 255);
        e.mode = mode;
        e.set_caret(caret);
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(editor, false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(WIDTH, HEIGHT))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    harness
}

/// Press and release, then one frame: the view that comes up is the one the
/// user sees, and it is already at the caret. A frame that has to be waited
/// for is a scroll the eye follows across the document.
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
        harness.step();
    }
}

/// Changing the view redraws every line at a different height, so where the
/// caret was on screen means nothing afterwards: the document is scrolled to
/// it again, in whichever view is being entered.
///
/// From source to formatted is the direction that says so: the formatted
/// document is the taller of the two, so the offset the source view was at
/// lands in the middle of it and the caret would be nowhere on screen.
///
/// Whether the caret is there on the *first* frame is not something this can
/// see: the harness runs a second pass per step, which is the pass that would
/// correct a scroll made too late. Only a real window shows that.
#[test]
fn changing_the_view_arrives_at_the_caret() {
    use egui_kittest::kittest::Queryable;
    use markdown_notes_core::ViewMode;

    let text = tall_document();
    let mut harness = open(&text, text.len(), ViewMode::Raw);
    assert!(
        caret_row(&harness.render().expect("rendering failed")).is_some(),
        "the caret is not in view to begin with"
    );

    let button = harness.get_by_label("Markdown source").rect().center();

    click(&mut harness, button);
    assert!(
        caret_row(&harness.render().expect("rendering failed")).is_some(),
        "the formatted view is not at the caret"
    );

    click(&mut harness, button);
    assert!(
        caret_row(&harness.render().expect("rendering failed")).is_some(),
        "the source view is not at the caret"
    );
}

#[test]
fn the_caret_at_the_end_of_a_tall_document_is_in_view() {
    let text = tall_document();
    let image = render(&text, text.len());
    if caret_row(&image).is_none() {
        panic!(
            "the caret is at the end of a document {} lines long and nothing is \
             drawn on screen, so the document was not scrolled to it\n      \
             rendering: {}",
            text.lines().count(),
            saved(&image, "caret-at-the-end")
        );
    }
}

#[test]
fn the_caret_at_the_start_of_a_tall_document_is_in_view() {
    let text = tall_document();
    let image = render(&text, 0);
    let Some(row) = caret_row(&image) else {
        panic!(
            "the caret is at the start of the document and nothing is drawn on \
             screen\n      rendering: {}",
            saved(&image, "caret-at-the-start")
        );
    };
    assert!(
        row < HEIGHT as u32 / 2,
        "the caret is at the start of the document and is drawn at row {row}, so \
         the document is scrolled somewhere else\n      rendering: {}",
        saved(&image, "caret-at-the-start")
    );
}
