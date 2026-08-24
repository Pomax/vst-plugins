//! Where the caret is drawn.
//!
//! The document does not draw one row per line: the fences around a code block
//! take no row of their own unless the caret is on one. Anything that counts
//! rows instead of asking the layout puts the caret on the wrong line, and only
//! pixels say which row it landed on.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use notepad_core::{Editor, Rgba, Theme};

/// A code block, and a paragraph after it.
const SAMPLE: &str = "# secondary\n\ntesting as well\n\n```py\nwith code\nand more\n```\n\nOkay";

/// Render the editor with the caret at `caret`, in a caret colour nothing else
/// in the window uses.
fn render(caret: usize) -> image::RgbaImage {
    let editor = Arc::new(Mutex::new(Editor::with_text(SAMPLE)));
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
        e.colours.light.caret = Rgba::rgba(255, 0, 0, 255);
        e.set_caret(caret);
    }
    let mut state = notepad_plugin::gui::TestGui::new(editor, false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(700.0, 400.0))
        .wgpu()
        .build_ui(move |ui| notepad_plugin::gui::draw_frame_for_test(ui, &mut state));

    notepad_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    harness.render().expect("rendering failed")
}

/// Top and bottom row of every pixel matching `wanted`.
fn rows_of(image: &image::RgbaImage, wanted: impl Fn([u8; 4]) -> bool) -> Option<(u32, u32)> {
    let mut top = None;
    let mut bottom = 0;
    for (_, y, p) in image.enumerate_pixels() {
        if wanted(p.0) {
            top.get_or_insert(y);
            bottom = bottom.max(y);
        }
    }
    top.map(|t| (t, bottom))
}

fn is_caret(p: [u8; 4]) -> bool {
    p[3] == 255 && p[0] > 180 && p[1] < 90 && p[2] < 90
}

/// Text, which is dark, and not the caret, which is red.
fn is_text(p: [u8; 4]) -> bool {
    p[3] == 255 && !is_caret(p) && (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3 < 110
}

/// The caret sits on the row of the paragraph after a code block, not on the
/// blank line above it.
#[test]
fn the_caret_is_drawn_on_the_line_it_is_on() {
    let at = SAMPLE.rfind("Okay").expect("the sample lost its last line");
    let image = render(at + "Okay".len());

    let (caret_top, caret_bottom) = rows_of(&image, is_caret).expect("no caret was drawn");
    // The last line of the document is the last thing drawn in it, so the
    // bottom-most text pixels are its.
    let (_, last_text) = rows_of(&image, is_text).expect("nothing was drawn");

    assert!(
        caret_top <= last_text && caret_bottom >= last_text.saturating_sub(2),
        "caret drawn at rows {caret_top}..{caret_bottom}, \
         while the line it is on ends at row {last_text}"
    );
}

/// A fence takes no row until the caret is on it, and then it has to be a row
/// with the backticks in it: a caret alone on an empty row below the block is
/// the caret looking lost.
#[test]
fn the_closing_fence_appears_when_the_caret_is_on_it() {
    let fence = SAMPLE.rfind("```").expect("the sample lost its closing fence");
    let image = render(fence + 3);

    let (caret_top, caret_bottom) = rows_of(&image, is_caret).expect("no caret was drawn");

    // The fence belongs to the block it closes, so the row it is drawn on is
    // the block's: the light theme's code background, `Rgba::grey(225)`.
    let mut background = std::collections::HashMap::<[u8; 3], u32>::new();
    for (_, y, p) in image.enumerate_pixels() {
        if y >= caret_top && y <= caret_bottom && p.0[3] == 255 {
            *background.entry([p.0[0], p.0[1], p.0[2]]).or_default() += 1;
        }
    }
    let (colour, _) = background
        .iter()
        .max_by_key(|(_, n)| **n)
        .expect("the caret's row is empty");
    assert_eq!(
        [colour[0], colour[1], colour[2]],
        [225, 225, 225],
        "the caret is on the closing fence, drawn at rows {caret_top}..{caret_bottom}, \
         and that row is not part of the code block above it"
    );
}

/// A caret on the line after a code block stands on the page, not half inside
/// the block. The block is drawn taller than its rows so the code is not flush
/// against its edge, and that padding must not be taken out of the line below.
#[test]
fn the_caret_below_a_code_block_is_clear_of_it() {
    let at = SAMPLE.rfind("Okay").expect("the sample lost its last line");
    // One byte back: the blank line between the block and the paragraph.
    let image = render(at - 1);
    let (top, bottom) = rows_of(&image, is_caret).expect("no caret was drawn");

    let inside: Vec<u32> = image
        .enumerate_pixels()
        .filter(|(_, y, p)| *y >= top && *y <= bottom && p.0 == [225, 225, 225, 255])
        .map(|(_, y, _)| y)
        .collect();
    assert!(
        inside.is_empty(),
        "the caret is drawn at rows {top}..{bottom}, and rows {:?} of it are inside the code block",
        inside.iter().collect::<std::collections::BTreeSet<_>>()
    );
}

/// And on the blank line above, when that is where it is.
#[test]
fn the_caret_moves_up_a_row_for_the_blank_line_above() {
    let at = SAMPLE.rfind("Okay").expect("the sample lost its last line");
    let on_text = rows_of(&render(at), is_caret).expect("no caret was drawn");
    // One byte back is the blank line between the code block and the
    // paragraph: a row of its own, above the paragraph's.
    let on_blank = rows_of(&render(at - 1), is_caret).expect("no caret was drawn");

    assert!(
        on_blank.1 < on_text.1,
        "the caret on the blank line was drawn at rows {on_blank:?}, \
         the one on the line below it at {on_text:?}"
    );
}
