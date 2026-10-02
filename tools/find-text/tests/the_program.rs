//! The program, run on a picture these tests draw themselves: what it prints
//! and how it exits.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use font_kit::canvas::{Canvas, Format, RasterizationOptions};
use font_kit::family_name::FamilyName;
use font_kit::font::Font;
use font_kit::hinting::HintingOptions;
use font_kit::properties::{Properties, Weight};
use font_kit::source::SystemSource;
use image::{GrayImage, Luma, Rgb, RgbImage};
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::vector::{Vector2F, Vector2I};

/// A box as the program prints one: left, top, width, height.
type Area = (i32, i32, i32, i32);

/// The height of the drawn lettering, in pixels: interface text.
const SIZE: f32 = 16.0;
const PICTURE: (u32, u32) = (480, 200);
/// How far an edge of a printed box may sit from the ink, in pixels.
const SLACK: i32 = 4;
/// What is drawn, and where the origin of each first glyph goes: its left
/// edge and its baseline.
const DRAWN: [(&str, (i32, i32)); 3] = [
    ("Save preset", (40, 60)),
    ("Save", (300, 100)),
    ("Open the notes", (150, 140)),
];

/// A button the desktop wants pressed, as its file dialog draws one: a pill
/// of the desktop's colour on a pale window, with white lettering. `BUTTON`
/// is the pill's left, top, width and height.
const WINDOW: [u8; 3] = [250, 250, 250];
const ACCENT: [u8; 3] = [233, 84, 32];
const LETTERING: [u8; 3] = [255, 255, 255];
const BUTTON: Area = (340, 78, 96, 44);
const ON_THE_BUTTON: (&str, (i32, i32)) = ("Save", (369, 106));
/// What the dialog has on the button's row, dark on the window.
const DARK: [u8; 3] = [46, 52, 54];
const BESIDE_THE_BUTTON: [(&str, (i32, i32)); 2] =
    [("Markdown", (40, 106)), ("Untitled.md", (190, 106))];

/// A strip of tabs on a dark window: a tab with a title on it, and beside it
/// a button with no word on it, only a `+`. Each is a box with an edge a
/// pixel wide: its left, top, width and height.
const STRIP: [u8; 3] = [40, 40, 40];
const BOX: [u8; 3] = [27, 27, 27];
const EDGE: [u8; 3] = [140, 140, 140];
const ON_THE_STRIP: [u8; 3] = [210, 210, 210];
const TAB: Area = (127, 88, 108, 22);
const ON_THE_TAB: (&str, (i32, i32)) = ("Drum bus", (150, 104));
const ADD: Area = (243, 88, 17, 22);
const ON_ADD: (&str, (i32, i32)) = ("+", (247, 104));

/// A picture as a note keeps one, on a dark window: its number, what kind
/// of thing follows, and the start of the picture written out in letters and
/// digits.
const NOTE: [u8; 3] = [27, 27, 27];
const ON_THE_NOTE: [u8; 3] = [140, 140, 140];
/// The height of the note's lettering, in pixels: the plugin's body text.
const NOTE_SIZE: f32 = 15.0;
const KEPT: [(&str, (i32, i32)); 3] = [
    ("[1]:", (18, 60)),
    ("data:image/png;base64,", (18, 82)),
    ("iVBORw0KGgoAAAANSUhEUgAAACgAAAAeCAYAAABe3VzdAAAA", (18, 104)),
];

/// Where these tests keep their files.
fn cache() -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(".cache").join("tests");
    std::fs::create_dir_all(&directory).expect("could not make .cache/tests");
    directory
}

/// The picture every test here reads, drawn once: its path, and where the ink
/// of each text of `DRAWN` is.
fn picture() -> &'static (String, Vec<Area>) {
    static DRAWING: OnceLock<(String, Vec<Area>)> = OnceLock::new();
    DRAWING.get_or_init(draw)
}

/// This machine's sans-serif face.
fn face(properties: &Properties) -> Font {
    SystemSource::new()
        .select_best_match(&[FamilyName::SansSerif], properties)
        .expect("this machine has no sans-serif face")
        .load()
        .expect("the sans-serif face did not load")
}

/// Letter `text` in `font` with the origin of its first glyph at `at`, its
/// left edge and its baseline. `put` is handed every pixel of the picture the
/// lettering covers, and how much of it. The answer is where the ink is.
fn ink(font: &Font, text: &str, at: (i32, i32), put: impl FnMut(u32, u32, u8)) -> Area {
    ink_sized(font, SIZE, text, at, put)
}

/// `ink`, with lettering `size` pixels high.
fn ink_sized(
    font: &Font,
    size: f32,
    text: &str,
    at: (i32, i32),
    mut put: impl FnMut(u32, u32, u8),
) -> Area {
    let per_unit = size / font.metrics().units_per_em as f32;
    let (width, height) = PICTURE;
    let (x, baseline) = at;

    let mut pen = x as f32;
    let (mut left, mut top) = (i32::MAX, i32::MAX);
    let (mut right, mut bottom) = (i32::MIN, i32::MIN);
    for character in text.chars() {
        let glyph = font
            .glyph_for_char(character)
            .expect("the face has no glyph for a drawn character");

        // Each glyph is drawn into a canvas of its own size, a pixel wider
        // all round, and put into the picture from there. Only the part
        // of a pixel the pen stands at goes to the font.
        let part = Transform2F::from_translation(Vector2F::new(pen.fract(), 0.0));
        let bounds = font
            .raster_bounds(
                glyph,
                size,
                part,
                HintingOptions::None,
                RasterizationOptions::GrayscaleAa,
            )
            .unwrap_or_else(|e| {
                panic!("{character:?} has no bounds in {}: {e:?}", font.full_name())
            });
        let origin = bounds.origin() - Vector2I::splat(1);
        let mut canvas = Canvas::new(bounds.size() + Vector2I::splat(2), Format::A8);
        font.rasterize_glyph(
            &mut canvas,
            glyph,
            size,
            Transform2F::from_translation(-origin.to_f32()) * part,
            HintingOptions::None,
            RasterizationOptions::GrayscaleAa,
        )
        .unwrap_or_else(|e| {
            panic!("{character:?} did not draw in {}: {e:?}", font.full_name())
        });
        for (index, cover) in canvas.pixels.iter().enumerate() {
            if *cover == 0 {
                continue;
            }
            let px = pen.floor() as i32 + origin.x() + (index % canvas.stride) as i32;
            let py = baseline + origin.y() + (index / canvas.stride) as i32;
            if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                continue;
            }
            put(px as u32, py as u32, *cover);
            left = left.min(px);
            top = top.min(py);
            right = right.max(px);
            bottom = bottom.max(py);
        }
        let advance = font.advance(glyph).expect("a glyph has no advance");
        pen += advance.x() * per_unit;
    }
    (left, top, right - left + 1, bottom - top + 1)
}

/// Draw `DRAWN` dark on white in this machine's sans-serif face and write it
/// under `.cache/tests`.
fn draw() -> (String, Vec<Area>) {
    let font = face(&Properties::new());
    let (width, height) = PICTURE;

    let mut picture = GrayImage::from_pixel(width, height, Luma([255]));
    let mut inked = Vec::new();
    for (text, at) in DRAWN {
        inked.push(ink(&font, text, at, |x, y, cover| {
            let pixel = picture.get_pixel_mut(x, y);
            pixel.0[0] = pixel.0[0].min(255 - cover);
        }));
    }

    let path = cache().join("drawn-words.png");
    picture.save(&path).expect("could not write the picture");
    (path.to_string_lossy().into_owned(), inked)
}

/// The picture of the button, drawn once: its path, and where the ink of the
/// button's lettering is.
fn button() -> &'static (String, Area) {
    static DRAWING: OnceLock<(String, Area)> = OnceLock::new();
    DRAWING.get_or_init(draw_button)
}

/// Lay `colour` over a pixel as far as `cover` says.
fn lay(picture: &mut RgbImage, x: u32, y: u32, colour: [u8; 3], cover: u8) {
    let pixel = picture.get_pixel_mut(x, y);
    for (under, over) in pixel.0.iter_mut().zip(colour) {
        let mixed = *under as i32 + (over as i32 - *under as i32) * cover as i32 / 255;
        *under = mixed as u8;
    }
}

/// Draw the button and what is beside it, and write it under `.cache/tests`.
fn draw_button() -> (String, Area) {
    let (width, height) = PICTURE;
    let mut picture = RgbImage::from_pixel(width, height, Rgb(WINDOW));

    // A pill is every pixel within half its height of the line between the
    // middles of its two round ends.
    let (left, top, wide, high) = BUTTON;
    let radius = high as f32 / 2.0;
    for y in top..top + high {
        for x in left..left + wide {
            let across = x as f32 + 0.5;
            let on_the_line = across.clamp(left as f32 + radius, (left + wide) as f32 - radius);
            let (dx, dy) = (across - on_the_line, y as f32 + 0.5 - (top as f32 + radius));
            if dx * dx + dy * dy <= radius * radius {
                picture.put_pixel(x as u32, y as u32, Rgb(ACCENT));
            }
        }
    }

    let regular = face(&Properties::new());
    for (text, at) in BESIDE_THE_BUTTON {
        ink(&regular, text, at, |x, y, cover| lay(&mut picture, x, y, DARK, cover));
    }
    let bold = face(Properties::new().weight(Weight::BOLD));
    let (text, at) = ON_THE_BUTTON;
    let lettering = ink(&bold, text, at, |x, y, cover| lay(&mut picture, x, y, LETTERING, cover));

    let path = cache().join("drawn-button.png");
    picture.save(&path).expect("could not write the picture");
    (path.to_string_lossy().into_owned(), lettering)
}

/// The picture of the strip of tabs, drawn once: its path, and where the ink
/// of the `+` is.
fn tabs() -> &'static (String, Area) {
    static DRAWING: OnceLock<(String, Area)> = OnceLock::new();
    DRAWING.get_or_init(draw_tabs)
}

/// Draw the strip of tabs and write it under `.cache/tests`.
fn draw_tabs() -> (String, Area) {
    let (width, height) = PICTURE;
    let mut picture = RgbImage::from_pixel(width, height, Rgb(STRIP));
    for (left, top, wide, high) in [TAB, ADD] {
        for y in top..top + high {
            for x in left..left + wide {
                let on_the_edge =
                    x == left || y == top || x == left + wide - 1 || y == top + high - 1;
                let colour = if on_the_edge { EDGE } else { BOX };
                picture.put_pixel(x as u32, y as u32, Rgb(colour));
            }
        }
    }

    let regular = face(&Properties::new());
    let (text, at) = ON_THE_TAB;
    ink(&regular, text, at, |x, y, cover| lay(&mut picture, x, y, ON_THE_STRIP, cover));
    let (text, at) = ON_ADD;
    let plus = ink(&regular, text, at, |x, y, cover| lay(&mut picture, x, y, ON_THE_STRIP, cover));

    let path = cache().join("drawn-tabs.png");
    picture.save(&path).expect("could not write the picture");
    (path.to_string_lossy().into_owned(), plus)
}

/// The picture of the kept picture, drawn once: its path, and where the ink
/// of each line of `KEPT` is.
fn note() -> &'static (String, Vec<Area>) {
    static DRAWING: OnceLock<(String, Vec<Area>)> = OnceLock::new();
    DRAWING.get_or_init(draw_note)
}

/// Draw `KEPT` and write it under `.cache/tests`.
fn draw_note() -> (String, Vec<Area>) {
    let (width, height) = PICTURE;
    let mut picture = RgbImage::from_pixel(width, height, Rgb(NOTE));
    let regular = face(&Properties::new());
    let inked = KEPT
        .iter()
        .map(|(text, at)| {
            ink_sized(&regular, NOTE_SIZE, text, *at, |x, y, cover| {
                lay(&mut picture, x, y, ON_THE_NOTE, cover)
            })
        })
        .collect();

    let path = cache().join("drawn-note.png");
    picture.save(&path).expect("could not write the picture");
    (path.to_string_lossy().into_owned(), inked)
}

fn find_text(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_find-text"))
        .args(arguments)
        .output()
        .expect("find-text did not start")
}

fn errors(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// What the program printed: its first line, then the box each later line
/// starts with and whatever follows the box.
fn printed(output: &Output) -> (String, Vec<(Area, String)>) {
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let size = lines.next().unwrap_or_default().to_string();
    let rows = lines
        .map(|line| {
            let mut parts = line.splitn(5, ' ');
            let mut number = || {
                parts
                    .next()
                    .and_then(|part| part.parse::<i32>().ok())
                    .unwrap_or_else(|| panic!("{line:?} does not start with a box"))
            };
            let area = (number(), number(), number(), number());
            (area, parts.next().unwrap_or_default().to_string())
        })
        .collect();
    (size, rows)
}

/// Whether every edge of `area` is within `SLACK` of the same edge of `ink`.
fn near(area: Area, ink: Area) -> bool {
    let edges = [
        area.0 - ink.0,
        area.1 - ink.1,
        (area.0 + area.2) - (ink.0 + ink.2),
        (area.1 + area.3) - (ink.1 + ink.3),
    ];
    edges.iter().all(|edge| edge.abs() <= SLACK)
}

#[test]
fn words_drawn_into_a_picture_are_read_where_they_were_drawn() {
    let (path, inked) = picture();
    for (drawn, wanted) in [(0, "Save preset"), (2, "Open the notes")] {
        let output = find_text(&[path, wanted]);
        assert_eq!(output.status.code(), Some(0), "{}", errors(&output));

        let (size, rows) = printed(&output);
        assert_eq!(size, "480 200");
        assert!(!rows.is_empty(), "no box was printed for {wanted:?}");
        for (area, _) in rows {
            assert!(
                near(area, inked[drawn]),
                "{wanted:?} was drawn at {:?} and read at {area:?}",
                inked[drawn]
            );
        }
    }
}

#[test]
fn words_that_were_not_drawn_are_not_found() {
    let (path, _) = picture();
    let drawn = find_text(&[path, "Save"]);
    assert_eq!(drawn.status.code(), Some(0), "{}", errors(&drawn));

    let output = find_text(&[path, "Cancel"]);
    assert_eq!(output.status.code(), Some(1), "{}", errors(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
}

#[test]
fn an_exact_match_is_printed_before_one_that_holds_the_text() {
    let (path, inked) = picture();
    let (holding, alone) = (inked[0], inked[1]);
    let output = find_text(&[path, "Save"]);
    assert_eq!(output.status.code(), Some(0), "{}", errors(&output));

    let (_, rows) = printed(&output);
    let exact = rows.iter().take_while(|(area, _)| near(*area, alone)).count();
    assert!(exact > 0, "the first box is not the lone word: {rows:?}");
    assert!(rows.len() > exact, "the word inside a line was not found: {rows:?}");
    for (area, _) in &rows[exact..] {
        let at_the_line_start =
            (area.0 - holding.0).abs() <= SLACK && (area.1 - holding.1).abs() <= SLACK;
        assert!(at_the_line_start, "{area:?} is not the start of {holding:?}");
    }
}

#[test]
fn with_no_text_every_line_read_is_printed() {
    let (path, inked) = picture();
    let output = find_text(&[path]);
    assert_eq!(output.status.code(), Some(0), "{}", errors(&output));

    let (size, rows) = printed(&output);
    assert_eq!(size, "480 200");
    for ((text, _), ink) in DRAWN.iter().zip(inked) {
        assert!(
            rows.iter().any(|(area, read)| read == text && near(*area, *ink)),
            "{text:?} at {ink:?} is not among {rows:?}"
        );
    }
}

#[test]
fn white_lettering_on_a_coloured_button_is_read() {
    let (path, lettering) = button();
    let output = find_text(&[path, ON_THE_BUTTON.0]);
    assert_eq!(output.status.code(), Some(0), "{}", errors(&output));

    let (_, rows) = printed(&output);
    assert!(!rows.is_empty(), "no box was printed for the button");
    for (area, _) in rows {
        assert!(
            near(area, *lettering),
            "the button's lettering is at {lettering:?} and was read at {area:?}"
        );
    }

    // What is beside the button is still read, as what it says.
    let output = find_text(&[path]);
    assert_eq!(output.status.code(), Some(0), "{}", errors(&output));
    let (_, rows) = printed(&output);
    for (text, _) in BESIDE_THE_BUTTON {
        assert!(
            rows.iter().any(|(_, read)| read == text),
            "{text:?} is not among {rows:?}"
        );
    }
}

#[test]
fn a_plus_alone_on_a_button_is_read() {
    let (path, plus) = tabs();
    let output = find_text(&[path]);
    assert_eq!(output.status.code(), Some(0), "{}", errors(&output));

    let (_, rows) = printed(&output);
    assert!(
        rows.iter().any(|(area, read)| read == ON_ADD.0 && near(*area, *plus)),
        "a line that is {:?} alone at {plus:?} is not among {rows:?}",
        ON_ADD.0
    );
    assert!(
        rows.iter().any(|(_, read)| read == ON_THE_TAB.0),
        "{:?} is not among {rows:?}",
        ON_THE_TAB.0
    );
}

#[test]
fn digits_among_letters_and_marks_are_read_as_digits() {
    let (path, inked) = note();
    let output = find_text(&[path, "base64"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}; the picture reads: {:?}",
        errors(&output),
        printed(&find_text(&[path])).1
    );

    let (_, rows) = printed(&output);
    assert!(!rows.is_empty(), "no box was printed for base64");
    for (area, _) in rows {
        let on_its_line = (area.1 - inked[1].1).abs() <= SLACK
            && area.0 >= inked[1].0 - SLACK
            && area.0 + area.2 <= inked[1].0 + inked[1].2 + SLACK;
        assert!(on_its_line, "base64 is on the line at {:?} and was read at {area:?}", inked[1]);
    }
}

#[test]
fn a_file_that_is_not_there_is_an_error() {
    let path = cache().join("not-there.png");
    assert!(!path.exists(), "{} exists", path.display());

    let output = find_text(&[&path.to_string_lossy(), "Save"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(errors(&output).starts_with("could not read"), "{}", errors(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
}

#[test]
fn a_file_that_is_not_a_picture_is_an_error() {
    let path = cache().join("not-a-picture.png");
    std::fs::write(&path, "words, and not a picture").expect("could not write the file");

    let output = find_text(&[&path.to_string_lossy(), "Save"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(errors(&output).contains("is not a picture"), "{}", errors(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
}

#[test]
fn without_tesseract_the_error_says_so() {
    let (path, _) = picture();
    let nothing = cache().join("no-programs");
    std::fs::create_dir_all(&nothing).expect("could not make the empty directory");
    let held = std::fs::read_dir(&nothing).expect("could not list the directory").count();
    assert_eq!(held, 0, "{} is not empty", nothing.display());

    let output = Command::new(env!("CARGO_BIN_EXE_find-text"))
        .args([path.as_str(), "Save"])
        .env("PATH", &nothing)
        .output()
        .expect("find-text did not start");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        errors(&output).contains("tesseract program, which is not on PATH"),
        "{}",
        errors(&output)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
}
