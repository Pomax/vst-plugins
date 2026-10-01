//! Reading a picture on Linux, with the `tesseract` program.

use std::io::{Cursor, ErrorKind, Write};
use std::process::{Command, Stdio};

use image::imageops::{self, FilterType};
use image::{GrayImage, ImageError, ImageFormat, Luma, RgbImage};

use crate::{enlargements, Line, Reading};

/// Everything readable in the picture at `path`.
///
/// The picture is read once at each of its enlargements and everything found
/// at any of them is kept, so the same words can be in the answer more than
/// once.
pub fn read(path: &str) -> Result<Reading, String> {
    let picture = image::open(path)
        .map_err(|e| match e {
            ImageError::IoError(e) => format!("could not read {path}: {e}"),
            e => format!("{path} is not a picture: {e}"),
        })?
        .to_rgb8();
    let size = picture.dimensions();

    let mut lines = Vec::new();
    for scale in enlargements(size) {
        lines.extend(at_scale(&picture, scale)?);
    }
    Ok(Reading { lines, size })
}

/// Read the picture once, enlarged `scale` times over.
fn at_scale(picture: &RgbImage, scale: u32) -> Result<Vec<Line>, String> {
    // Cubic, the filter the Windows reader enlarges with: interface text is
    // drawn a stroke wide, and a smoothing filter spreads those strokes into
    // the background.
    let (width, height) = picture.dimensions();
    let enlarged = imageops::resize(
        picture,
        width * scale,
        height * scale,
        FilterType::CatmullRom,
    );

    // The greys are pulled apart with the weights and the doubling of the
    // Windows reader's `deepen`, so that mid grey lettering on light grey
    // comes out dark on white.
    let grey = GrayImage::from_fn(enlarged.width(), enlarged.height(), |x, y| {
        let [red, green, blue] = enlarged.get_pixel(x, y).0;
        let grey = (blue as u32 * 29 + green as u32 * 150 + red as u32 * 77) / 256;
        Luma([((grey as i32 - 128) * 2 + 128).clamp(0, 255) as u8])
    });
    let mut png = Vec::new();
    grey.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|e| format!("could not encode the enlarged picture: {e}"))?;

    // Page mode 11 is sparse text: words wherever they are, in no particular
    // layout, which is what a window is. `-l` and `--psm` have to come before
    // `tsv`, which names the config file that makes the output a table.
    let mut child = Command::new("tesseract")
        .args(["stdin", "stdout", "--psm", "11", "-l", "eng", "tsv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => {
                "find-text reads pictures with the tesseract program, which is not on PATH"
                    .to_string()
            }
            _ => format!("could not start tesseract: {e}"),
        })?;

    // The picture goes in from a thread of its own, so that the output is
    // being collected while the input is still being written.
    let mut stdin = child.stdin.take().ok_or("tesseract has no input to write to")?;
    let writer = std::thread::spawn(move || stdin.write_all(&png));
    let output = child
        .wait_with_output()
        .map_err(|e| format!("tesseract did not finish: {e}"))?;
    let written = writer
        .join()
        .map_err(|_| "handing the picture to tesseract panicked")?;
    if !output.status.success() {
        return Err(format!(
            "tesseract failed, {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    written.map_err(|e| format!("could not hand the picture to tesseract: {e}"))?;

    Ok(parse_tsv(&String::from_utf8_lossy(&output.stdout), scale))
}

/// The words in `tesseract`'s table, gathered into the lines it put them on.
///
/// A row is `level page_num block_num par_num line_num word_num left top
/// width height conf text`, separated by tabs. A word is a row of level 5.
/// The rows of the levels above it are the page, block, paragraph and line it
/// sits in, and carry a confidence of -1. Words that share a block, a
/// paragraph and a line number are one line, in the order they come.
///
/// The picture was read `scale` times enlarged, so every box is divided by
/// that to put it back in the picture's own pixels.
fn parse_tsv(text: &str, scale: u32) -> Vec<Line> {
    let scale = scale as i32;
    let mut lines: Vec<((u32, u32, u32), Line)> = Vec::new();
    for row in text.lines() {
        let fields: Vec<&str> = row.splitn(12, '\t').collect();
        let [level, _, block, paragraph, line, _, left, top, width, height, confidence, word] =
            fields.as_slice()
        else {
            continue;
        };
        if *level != "5" {
            continue;
        }
        let word = word.trim();
        if word.is_empty() || confidence.parse::<f32>().map_or(true, |c| c < 0.0) {
            continue;
        }
        let (Ok(block), Ok(paragraph), Ok(line)) = (
            block.parse::<u32>(),
            paragraph.parse::<u32>(),
            line.parse::<u32>(),
        ) else {
            continue;
        };
        let (Ok(left), Ok(top), Ok(width), Ok(height)) = (
            left.parse::<i32>(),
            top.parse::<i32>(),
            width.parse::<i32>(),
            height.parse::<i32>(),
        ) else {
            continue;
        };
        let box_of = (left / scale, top / scale, width / scale, height / scale);

        let key = (block, paragraph, line);
        match lines.last_mut() {
            Some((last, line)) if *last == key => {
                let (a, b) = (line.box_of, box_of);
                let left = a.0.min(b.0);
                let top = a.1.min(b.1);
                let right = (a.0 + a.2).max(b.0 + b.2);
                let bottom = (a.1 + a.3).max(b.1 + b.3);
                line.box_of = (left, top, right - left, bottom - top);
                line.text.push(' ');
                line.text.push_str(word);
                line.words.push((word.to_string(), box_of));
            }
            _ => lines.push((
                key,
                Line {
                    text: word.to_string(),
                    box_of,
                    words: vec![(word.to_string(), box_of)],
                },
            )),
        }
    }
    lines.into_iter().map(|(_, line)| line).collect()
}

#[cfg(test)]
mod tests {
    use super::parse_tsv;

    #[test]
    fn a_word_row_becomes_a_word_with_its_box_scaled_back() {
        let lines = parse_tsv("5\t1\t1\t1\t1\t1\t160\t240\t128\t48\t96.5\tSave\n", 4);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Save");
        assert_eq!(lines[0].box_of, (40, 60, 32, 12));
        assert_eq!(lines[0].words, vec![("Save".to_string(), (40, 60, 32, 12))]);
    }

    #[test]
    fn words_on_one_line_are_one_line_in_reading_order() {
        let table = "\
5\t1\t1\t1\t1\t1\t20\t40\t60\t24\t95.1\tSave
5\t1\t1\t1\t1\t2\t90\t44\t100\t28\t93.4\tpreset
5\t1\t1\t1\t2\t1\t20\t100\t70\t24\t91.0\tOpen
5\t1\t2\t1\t1\t1\t300\t100\t80\t24\t90.2\tnotes
";
        let lines = parse_tsv(table, 2);
        let read: Vec<(&str, (i32, i32, i32, i32))> = lines
            .iter()
            .map(|line| (line.text.as_str(), line.box_of))
            .collect();
        assert_eq!(
            read,
            vec![
                ("Save preset", (10, 20, 85, 16)),
                ("Open", (10, 50, 35, 12)),
                ("notes", (150, 50, 40, 12)),
            ]
        );
        assert_eq!(
            lines[0].words,
            vec![
                ("Save".to_string(), (10, 20, 30, 12)),
                ("preset".to_string(), (45, 22, 50, 14)),
            ]
        );
    }

    #[test]
    fn rows_that_are_not_words_are_dropped() {
        let table = "\
level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext
1\t1\t0\t0\t0\t0\t0\t0\t480\t200\t-1\t
2\t1\t1\t0\t0\t0\t20\t40\t170\t32\t-1\t
3\t1\t1\t1\t0\t0\t20\t40\t170\t32\t-1\t
4\t1\t1\t1\t1\t0\t20\t40\t170\t32\t-1\t
4\t1\t1\t1\t1\t0\t20\t40\t170\t32\t90.0\tline
5\t1\t1\t1\t1\t1\t20\t40\t60\t24\t95.0\t \n\
5\t1\t1\t1\t1\t2\t90\t44\t100\t28\t-1\tghost
5\t1\t1\t1\t1\t3\t200\t40\t60\t24\t96.0\tSave
";
        let lines = parse_tsv(table, 1);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Save");
        assert_eq!(lines[0].words, vec![("Save".to_string(), (200, 40, 60, 24))]);
    }
}
