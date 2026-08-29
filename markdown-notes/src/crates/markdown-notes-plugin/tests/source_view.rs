//! The source view shows the markdown as source: one size, one colour, one
//! monospaced face, and none of the furniture the formatted view draws.
//!
//! A heading in source is a line beginning with a hash, not a large heading; a
//! fence is three backticks, not a shaded box. What is on screen is what is in
//! the file.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use markdown_notes_core::{Editor, Rgba, Theme, ViewMode};

const SIZE: (f32, f32) = (700.0, 400.0);

/// Where the toolbars end and the document begins.
const TOOLBARS: u32 = 90;

/// A colour nothing else in the window uses, so the shading behind a fenced
/// block can be found exactly.
const FENCE_FILL: [u8; 4] = [255, 0, 255, 255];

fn render(text: &str, mode: ViewMode) -> image::RgbaImage {
    let editor = Arc::new(Mutex::new(Editor::with_text(text)));
    if let Ok(mut e) = editor.lock() {
        e.theme = Theme::Light;
        e.colours.light.code_background = Rgba::rgba(255, 0, 255, 255);
        e.mode = mode;
        // Off the first line: the caret reveals that line's markers, which is
        // a difference between the two views this is not measuring.
        e.clear_caret();
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(editor, false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(SIZE.0, SIZE.1))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    harness.render().expect("rendering failed")
}

fn is_text(p: [u8; 4]) -> bool {
    p[3] == 255 && (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3 < 110
}

/// The rows of each line of text in the document, top line first.
fn lines(image: &image::RgbaImage) -> Vec<Vec<u32>> {
    let mut rows: Vec<u32> = image
        .enumerate_pixels()
        .filter(|(_, y, p)| *y > TOOLBARS && is_text(p.0))
        .map(|(_, y, _)| y)
        .collect();
    rows.sort_unstable();
    rows.dedup();

    // Rows of slack: the dot of an `i` and a descender both sit clear of the
    // rest of the letter, and they are the same line. Lines themselves are
    // twenty rows apart at the smallest.
    let mut lines: Vec<Vec<u32>> = Vec::new();
    for y in rows {
        match lines.last_mut() {
            Some(line) if y - line[line.len() - 1] <= 8 => line.push(y),
            _ => lines.push(vec![y]),
        }
    }
    lines
}

/// The first and last row of each line, for a failure to quote.
fn spans(lines: &[Vec<u32>]) -> Vec<(u32, u32)> {
    lines.iter().map(|l| (l[0], l[l.len() - 1])).collect()
}

/// How tall a line is: top of its tallest letter to the bottom of its lowest,
/// which is not the same as how many rows have ink in them.
fn height_of(rows: &[u32]) -> u32 {
    rows[rows.len() - 1] - rows[0] + 1
}

/// How wide the text on one line is.
fn width_of(image: &image::RgbaImage, rows: &[u32]) -> u32 {
    let columns: Vec<u32> = rows
        .iter()
        .flat_map(|&y| (0..image.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| is_text(image.get_pixel(x, y).0))
        .map(|(x, _)| x)
        .collect();
    match (columns.iter().min(), columns.iter().max()) {
        (Some(left), Some(right)) => right - left,
        _ => 0,
    }
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

/// A heading is drawn large in the formatted view. In source it is a line of
/// text like any other, so it is no taller than the paragraph under it.
#[test]
fn a_heading_is_no_larger_than_body_text_in_the_source_view() {
    const DOCUMENT: &str = "# Heading\n\nbody text";

    let formatted = render(DOCUMENT, ViewMode::Wysiwyg);
    let shown = lines(&formatted);
    assert_eq!(
        shown.len(),
        2,
        "expected a heading and a paragraph, got {:?}\n      rendering: {}",
        spans(&shown),
        saved(&formatted, "source-view-heading-formatted")
    );
    let (big, ordinary) = (height_of(&shown[0]), height_of(&shown[1]));
    assert!(
        big > ordinary,
        "the heading is {big} rows tall and the body {ordinary}, so the \
         formatted view is not drawing it larger and this proves nothing"
    );

    let source = render(DOCUMENT, ViewMode::Raw);
    let shown = lines(&source);
    assert_eq!(
        shown.len(),
        2,
        "expected two lines of source, got {:?}\n      rendering: {}",
        spans(&shown),
        saved(&source, "source-view-heading")
    );
    let (heading, body) = (height_of(&shown[0]), height_of(&shown[1]));
    if heading > body {
        panic!(
            "in the source view the heading is {heading} rows tall and the body \
             {body} ({:?}), so it is still drawn as a heading\n      rendering: {}",
            spans(&shown),
            saved(&source, "source-view-heading")
        );
    }
}

/// Source is monospaced, so a line of narrow letters is as wide as a line of
/// wide ones with the same number of characters.
#[test]
fn the_source_view_is_monospaced() {
    const DOCUMENT: &str = "iiiiiiiiiiii\n\nMMMMMMMMMMMM";

    let image = render(DOCUMENT, ViewMode::Raw);
    let shown = lines(&image);
    assert_eq!(
        shown.len(),
        2,
        "expected two lines of source, got {:?}\n      rendering: {}",
        spans(&shown),
        saved(&image, "source-view-monospace")
    );
    let (narrow, wide) = (width_of(&image, &shown[0]), width_of(&image, &shown[1]));
    let difference = narrow.abs_diff(wide);
    if difference > 4 {
        panic!(
            "twelve `i` are {narrow} pixels wide and twelve `M` are {wide}, a \
             difference of {difference}, so the source is not in a monospaced \
             face\n      rendering: {}",
            saved(&image, "source-view-monospace")
        );
    }
}

/// The shading behind a fenced block is formatting. In source the fence is
/// three backticks and the lines are plain text.
#[test]
fn a_fenced_block_is_not_shaded_in_the_source_view() {
    const DOCUMENT: &str = "before\n\n```rust\nlet x = 1;\n```\n\nafter";

    let formatted = render(DOCUMENT, ViewMode::Wysiwyg);
    let shaded = formatted.pixels().filter(|p| p.0 == FENCE_FILL).count();
    assert!(
        shaded > 0,
        "nothing is shaded in the formatted view, so this proves nothing"
    );

    let source = render(DOCUMENT, ViewMode::Raw);
    let still = source.pixels().filter(|p| p.0 == FENCE_FILL).count();
    if still > 0 {
        panic!(
            "{still} pixels of the code background are painted in the source \
             view\n      rendering: {}",
            saved(&source, "source-view-fence")
        );
    }
}
