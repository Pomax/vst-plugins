//! The document behaves like a text area: long lines wrap, and a document
//! taller than the window says so with a scrollbar.
//!
//! Both are questions about where pixels land, so pixels are what answers
//! them: a line that does not wrap runs off the right edge and is clipped
//! there, and a scrollbar that is not drawn leaves the page colour where its
//! handle would be.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use markdown_notes_core::{Editor, Theme};

const WIDTH: u32 = 700;
const HEIGHT: u32 = 400;

/// The light scheme's scrollbar handle, `Rgba::grey(230)`.
const HANDLE: [u8; 4] = [230, 230, 230, 255];

/// Where the toolbars end and the document begins.
const TOOLBARS: u32 = 120;

fn render(text: &str) -> image::RgbaImage {
    let editor = Arc::new(Mutex::new(Editor::with_text(text)));
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
        // Out of the way: a caret on a line reveals its markers and moves it.
        e.set_caret(0);
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(editor, false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(WIDTH as f32, HEIGHT as f32))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    harness.render().expect("rendering failed")
}

fn is_text(p: [u8; 4]) -> bool {
    p[3] == 255 && (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3 < 110
}

/// Every row of the document holding any body text.
///
/// Below the toolbars, which hold text of their own: the paragraph is what is
/// being measured, not the buttons above it.
fn document_text_rows(image: &image::RgbaImage) -> Vec<u32> {
    let mut rows: Vec<u32> = image
        .enumerate_pixels()
        .filter(|(_, y, p)| *y > TOOLBARS && is_text(p.0))
        .map(|(_, y, _)| y)
        .collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// The rightmost column holding body text in the document.
fn rightmost_text(image: &image::RgbaImage) -> Option<u32> {
    image
        .enumerate_pixels()
        .filter(|(_, y, p)| *y > TOOLBARS && is_text(p.0))
        .map(|(x, _, _)| x)
        .max()
}

/// The tallest unbroken run of scrollbar handle down one column, over the
/// whole window.
fn handle_height(image: &image::RgbaImage) -> u32 {
    let mut tallest = 0;
    for x in 0..image.width() {
        let mut run = 0;
        for y in 0..image.height() {
            if image.get_pixel(x, y).0 == HANDLE {
                run += 1;
                tallest = tallest.max(run);
            } else {
                run = 0;
            }
        }
    }
    tallest
}

/// A paragraph far wider than the window, on one line of the document.
fn long_line() -> String {
    "the quick brown fox jumps over the lazy dog and keeps on running ".repeat(6)
}

#[test]
fn a_line_wider_than_the_window_is_wrapped_onto_more_rows() {
    let image = render(&long_line());
    let rows = document_text_rows(&image);
    assert!(!rows.is_empty(), "no text was drawn in the document");
    let height = rows[rows.len() - 1] - rows[0];
    assert!(
        height > 30,
        "one line of text occupies {height} rows, so nothing wrapped"
    );
}

#[test]
fn a_wrapped_line_stays_clear_of_the_right_edge() {
    let image = render(&long_line());
    let right = rightmost_text(&image).expect("no text was drawn in the document");
    assert!(
        right < WIDTH - 20,
        "text reaches column {right} of {WIDTH}, so the line ran off the edge \
         instead of wrapping"
    );
}

#[test]
fn a_document_taller_than_the_window_gets_a_scrollbar() {
    let lines: Vec<String> = (0..60).map(|n| format!("line {n}")).collect();
    let image = render(&lines.join("\n"));
    let handle = handle_height(&image);
    assert!(
        handle > 20,
        "the tallest run of scrollbar handle is {handle} pixels, \
         so no scrollbar was drawn for a document that does not fit"
    );
}

#[test]
fn a_document_that_fits_has_no_scrollbar() {
    let image = render("one line, and nothing else");
    let handle = handle_height(&image);
    assert!(
        handle <= 20,
        "a scrollbar handle {handle} pixels tall was drawn for a document \
         that fits in the window"
    );
}
