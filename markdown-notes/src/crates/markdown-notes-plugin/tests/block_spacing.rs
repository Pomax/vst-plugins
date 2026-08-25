//! The space between a code block and the prose around it.
//!
//! A blank line is a line the user typed, and it has to be worth a line of
//! space on top of whatever separation the blocks already get. Pixels are the
//! only way to say whether pressing Enter twice moved anything.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use markdown_notes_core::{Editor, Theme};

/// The light theme's code background, `Rgba::grey(225)`.
const CODE: [u8; 4] = [225, 225, 225, 255];

fn render(text: &str) -> image::RgbaImage {
    let editor = Arc::new(Mutex::new(Editor::with_text(text)));
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
        // Out of the way: a caret on a line reveals its markers and moves it.
        e.set_caret(0);
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(editor, false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(700.0, 400.0))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    harness.render().expect("rendering failed")
}

fn is_text(p: [u8; 4]) -> bool {
    p[3] == 255 && (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3 < 110
}

/// Rows the code block covers.
///
/// A row of it, not a stray pixel of the same colour somewhere in the window's
/// own furniture: the block runs the width of the document.
fn block_rows(image: &image::RgbaImage) -> (u32, u32) {
    let mut counts = vec![0u32; image.height() as usize];
    for (_, y, p) in image.enumerate_pixels() {
        if p.0 == CODE {
            counts[y as usize] += 1;
        }
    }
    let wide = image.width() / 2;
    let rows: Vec<u32> = counts
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > wide)
        .map(|(y, _)| y as u32)
        .collect();
    (
        *rows.first().expect("no code block was drawn"),
        *rows.last().expect("no code block was drawn"),
    )
}

/// Blank pixels between the bottom of the code block and the text under it.
fn gap_below(image: &image::RgbaImage) -> u32 {
    let (_, bottom) = block_rows(image);
    let first_text = image
        .enumerate_pixels()
        .filter(|(_, y, p)| *y > bottom && is_text(p.0))
        .map(|(_, y, _)| y)
        .min()
        .expect("nothing was drawn under the code block");
    first_text - bottom
}

/// Blank pixels between the text above the code block and its top.
fn gap_above(image: &image::RgbaImage) -> u32 {
    let (top, _) = block_rows(image);
    let last_text = image
        .enumerate_pixels()
        .filter(|(_, y, p)| *y < top && is_text(p.0))
        .map(|(_, y, _)| y)
        .max()
        .expect("nothing was drawn above the code block");
    top - last_text
}

/// A line of body text, near enough. The gap has to grow by about this much
/// for the blank line to read as a line at all.
const LINE: u32 = 12;

#[test]
fn a_blank_line_after_a_code_block_is_worth_a_line() {
    let tight = gap_below(&render("```py\ncode\n```\nOkay"));
    let spaced = gap_below(&render("```py\ncode\n```\n\nOkay"));
    assert!(
        spaced >= tight + LINE,
        "one newline leaves {tight}px under the block and two leave {spaced}px: \
         the second one bought nothing"
    );
}

#[test]
fn a_blank_line_before_a_code_block_is_worth_a_line() {
    let tight = gap_above(&render("Okay\n```py\ncode\n```"));
    let spaced = gap_above(&render("Okay\n\n```py\ncode\n```"));
    assert!(
        spaced >= tight + LINE,
        "one newline leaves {tight}px above the block and two leave {spaced}px: \
         the second one bought nothing"
    );
}

/// And the paragraph spacing everything else uses is unchanged: one blank line
/// between two paragraphs is one line of space, not two.
#[test]
fn two_paragraphs_are_not_pushed_further_apart() {
    let image = render("Okay\n\nthen\n\n```py\ncode\n```");
    let (top, _) = block_rows(&image);

    // Rows carrying text, in runs: one run per line drawn. The last two above
    // the code block are the two paragraphs; the ones before them are the
    // window's own furniture.
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for y in 0..top {
        let has_text = (0..image.width()).any(|x| is_text(image.get_pixel(x, y).0));
        match runs.last_mut() {
            Some(run) if has_text && run.1 + 2 >= y => run.1 = y,
            _ if has_text => runs.push((y, y)),
            _ => {}
        }
    }
    let [.., first, second] = runs.as_slice() else {
        panic!("expected two paragraphs above the code block, found {runs:?}");
    };

    // One blank line between them is one line of space, as it always was:
    // 25px of it here. Another line on top would be half again as much.
    let gap = second.0 - first.1;
    assert!(
        gap < LINE * 3,
        "two paragraphs one blank line apart are {gap}px apart"
    );
}
