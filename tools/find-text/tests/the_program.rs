//! The program, run on a picture these tests draw themselves: what it prints
//! and how it exits.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use font_kit::canvas::{Canvas, Format, RasterizationOptions};
use font_kit::family_name::FamilyName;
use font_kit::hinting::HintingOptions;
use font_kit::properties::Properties;
use font_kit::source::SystemSource;
use image::{GrayImage, Luma};
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

/// Draw `DRAWN` dark on white in this machine's sans-serif face and write it
/// under `.cache/tests`.
fn draw() -> (String, Vec<Area>) {
    let font = SystemSource::new()
        .select_best_match(&[FamilyName::SansSerif], &Properties::new())
        .expect("this machine has no sans-serif face")
        .load()
        .expect("the sans-serif face did not load");
    let per_unit = SIZE / font.metrics().units_per_em as f32;
    let (width, height) = PICTURE;

    let mut picture = GrayImage::from_pixel(width, height, Luma([255]));
    let mut inked = Vec::new();
    for (text, (x, baseline)) in DRAWN {
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
                    SIZE,
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
                SIZE,
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
                let pixel = picture.get_pixel_mut(px as u32, py as u32);
                pixel.0[0] = pixel.0[0].min(255 - cover);
                left = left.min(px);
                top = top.min(py);
                right = right.max(px);
                bottom = bottom.max(py);
            }
            let advance = font.advance(glyph).expect("a glyph has no advance");
            pen += advance.x() * per_unit;
        }
        inked.push((left, top, right - left + 1, bottom - top + 1));
    }

    let path = cache().join("drawn-words.png");
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
