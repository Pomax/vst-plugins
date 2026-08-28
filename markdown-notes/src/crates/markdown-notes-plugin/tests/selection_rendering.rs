//! Selected text is drawn selected, on every row it covers.
//!
//! A line wider than the window is laid out as several rows, and the band
//! behind the selection is painted per row. Whether each of those rows got one
//! is a question about pixels, so pixels answer it: the selection colour is
//! the plain fill behind the words, and a row without it was left unpainted.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use markdown_notes_core::{Editor, Theme};

const WIDTH: u32 = 700;
const HEIGHT: u32 = 400;

/// The light scheme's selection fill, `Rgba::rgb(144, 209, 255)`.
const SELECTION: [u8; 4] = [144, 209, 255, 255];

/// Where the toolbars end and the document begins.
const TOOLBARS: u32 = 120;

fn render(text: &str, selection: std::ops::Range<usize>) -> image::RgbaImage {
    let editor = Arc::new(Mutex::new(Editor::with_text(text)));
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
        e.set_caret(0);
        if !selection.is_empty() {
            e.select(selection);
        }
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

fn is_fill(p: [u8; 4]) -> bool {
    p == SELECTION
}

/// The leftmost and rightmost column of one line answering to `what`.
fn span(image: &image::RgbaImage, rows: &[u32], what: fn([u8; 4]) -> bool) -> Option<(u32, u32)> {
    let columns: Vec<u32> = rows
        .iter()
        .flat_map(|&y| (0..image.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| what(image.get_pixel(x, y).0))
        .map(|(x, _)| x)
        .collect();
    Some((*columns.iter().min()?, *columns.iter().max()?))
}

/// The rows of each line of text on the screen, top line first. A wrapped line
/// is several of these, told apart by the blank rows between them.
fn lines(image: &image::RgbaImage) -> Vec<Vec<u32>> {
    let mut lines: Vec<Vec<u32>> = Vec::new();
    for y in document_text_rows(image) {
        match lines.last_mut() {
            Some(line) if y - line[line.len() - 1] <= 1 => line.push(y),
            _ => lines.push(vec![y]),
        }
    }
    lines
}

/// What one line holds and what of it is highlighted, for a failure to quote.
fn describe(image: &image::RgbaImage, rows: &[u32]) -> String {
    match (span(image, rows, is_text), span(image, rows, is_fill)) {
        (Some((wl, wr)), Some((bl, br))) => format!("text {wl} to {wr}, highlight {bl} to {br}"),
        (Some((wl, wr)), None) => format!("text {wl} to {wr}, no highlight"),
        _ => "nothing at all".into(),
    }
}

/// Every row of the document holding any body text.
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

/// Keep the frame a failing test judged, beside the UI tests' screenshots, and
/// answer with where it went.
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

/// A paragraph far wider than the window, on one line of the document.
fn long_line() -> String {
    "the quick brown fox jumps over the lazy dog and keeps on running ".repeat(6)
}

/// A line's highlight covers all of it, give or take the couple of columns an
/// edge is blended over.
fn covered(image: &image::RgbaImage, rows: &[u32]) -> bool {
    match (span(image, rows, is_text), span(image, rows, is_fill)) {
        (Some((word, ends)), Some((band, out))) => band <= word + SLACK && out + SLACK >= ends,
        _ => false,
    }
}

const SLACK: u32 = 4;

/// How far into a line a highlight has to start or stop to count as partial:
/// wider than a word, so nothing about which glyph an edge lands on matters.
const PARTIAL: u32 = 60;

#[test]
fn selecting_everything_highlights_every_row_of_a_wrapped_line() {
    let text = long_line();
    let image = render(&text, 0..text.len());
    let lines = lines(&image);
    assert!(
        lines.len() >= 3,
        "the text came out on {} lines, so nothing wrapped and this proves nothing",
        lines.len()
    );
    for (n, rows) in lines.iter().enumerate() {
        if !covered(&image, rows) {
            let shot = saved(&image, "selecting-everything");
            panic!(
                "everything is selected, so every line should be highlighted end to \
                 end, and line {n} of {} has {}\n      rendering: {shot}",
                lines.len(),
                describe(&image, rows)
            );
        }
    }
}

#[test]
fn a_selection_starting_mid_line_leaves_the_words_before_it_alone() {
    let text = long_line();
    let image = render(&text, text.len() / 2..text.len());
    let lines = lines(&image);
    let shot = || saved(&image, "selecting-from-the-middle");

    let started = lines
        .iter()
        .position(|rows| span(&image, rows, is_fill).is_some())
        .unwrap_or_else(|| panic!("nothing was highlighted at all\n      rendering: {}", shot()));
    assert!(
        started > 0 && started + 1 < lines.len(),
        "the selection begins on line {started} of {}, which is not a line with \
         text above and below it\n      rendering: {}",
        lines.len(),
        shot()
    );

    let (word, ends) = span(&image, &lines[started], is_text).expect("a line without text");
    let (band, out) = span(&image, &lines[started], is_fill).expect("a line without highlight");
    assert!(
        band > word + PARTIAL,
        "the selection begins in the middle of line {started}, so the words in \
         front of it should be left alone, and it has {}\n      rendering: {}",
        describe(&image, &lines[started]),
        shot()
    );
    assert!(
        out + SLACK >= ends,
        "the selection carries on past line {started}, so its highlight should \
         reach the end of the line, and it has {}\n      rendering: {}",
        describe(&image, &lines[started]),
        shot()
    );

    for (n, rows) in lines.iter().enumerate() {
        let held = span(&image, rows, is_fill);
        if n < started && held.is_some() {
            panic!(
                "line {n} is in front of the selection and should not be \
                 highlighted, and it has {}\n      rendering: {}",
                describe(&image, rows),
                shot()
            );
        }
        if n > started && !covered(&image, rows) {
            panic!(
                "line {n} is inside the selection and should be highlighted end to \
                 end, and it has {}\n      rendering: {}",
                describe(&image, rows),
                shot()
            );
        }
    }
}

#[test]
fn a_selection_ending_mid_line_leaves_the_words_after_it_alone() {
    let text = long_line();
    let image = render(&text, 0..text.len() / 2);
    let lines = lines(&image);
    let shot = || saved(&image, "selecting-to-the-middle");

    let ended = lines
        .iter()
        .rposition(|rows| span(&image, rows, is_fill).is_some())
        .unwrap_or_else(|| panic!("nothing was highlighted at all\n      rendering: {}", shot()));
    assert!(
        ended > 0 && ended + 1 < lines.len(),
        "the selection ends on line {ended} of {}, which is not a line with text \
         above and below it\n      rendering: {}",
        lines.len(),
        shot()
    );

    let (word, ends) = span(&image, &lines[ended], is_text).expect("a line without text");
    let (band, out) = span(&image, &lines[ended], is_fill).expect("a line without highlight");
    assert!(
        band <= word + SLACK,
        "the selection comes from the line above, so its highlight should start at \
         the beginning of line {ended}, and it has {}\n      rendering: {}",
        describe(&image, &lines[ended]),
        shot()
    );
    assert!(
        out + PARTIAL < ends,
        "the selection ends in the middle of line {ended}, so the words after it \
         should be left alone, and it has {}\n      rendering: {}",
        describe(&image, &lines[ended]),
        shot()
    );

    for (n, rows) in lines.iter().enumerate() {
        if n < ended && !covered(&image, rows) {
            panic!(
                "line {n} is inside the selection and should be highlighted end to \
                 end, and it has {}\n      rendering: {}",
                describe(&image, rows),
                shot()
            );
        }
        if n > ended && span(&image, rows, is_fill).is_some() {
            panic!(
                "line {n} is past the end of the selection and should not be \
                 highlighted, and it has {}\n      rendering: {}",
                describe(&image, rows),
                shot()
            );
        }
    }
}

#[test]
fn a_document_with_no_selection_highlights_none_of_it() {
    let image = render(&long_line(), 0..0);
    let held: Vec<String> = lines(&image)
        .iter()
        .enumerate()
        .filter(|(_, rows)| span(&image, rows, is_fill).is_some())
        .map(|(n, rows)| format!("line {n}: {}", describe(&image, rows)))
        .collect();
    assert!(
        held.is_empty(),
        "nothing is selected and yet {}\n      rendering: {}",
        held.join(", "),
        saved(&image, "selecting-nothing")
    );
}
