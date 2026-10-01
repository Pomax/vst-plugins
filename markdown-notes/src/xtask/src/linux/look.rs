//! Finding words on the screen by looking.
//!
//! A part of the screen is photographed and `binaries/find-text` reads the
//! picture, so a click goes where a label really is and not where a layout
//! constant says it ought to be.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::portal::Portal;
use super::screen;
use super::Rect;

/// Where the text finder is, built if it is not there yet.
pub fn finder() -> Result<PathBuf, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or("no directory above this project")?
        .to_path_buf();
    let tool = root.join("binaries").join("find-text");
    if tool.exists() {
        return Ok(tool);
    }
    let status = Command::new("sh")
        .arg(root.join("tools").join("find-text").join("build.sh"))
        .status()
        .map_err(|e| format!("building the text finder: {e}"))?;
    if !status.success() {
        return Err("building the text finder failed".to_string());
    }
    tool.exists()
        .then_some(tool)
        .ok_or_else(|| "the text finder did not build".to_string())
}

/// What the text finder printed: the picture's width from its first line, and
/// after it one box a line, each with whatever follows the box on its line.
fn parsed(output: &str) -> Option<(u32, Vec<(Rect, String)>)> {
    let mut lines = output.lines();
    let width = lines.next()?.split_whitespace().next()?.parse().ok()?;
    let mut found = Vec::new();
    for line in lines {
        let mut parts = line.splitn(5, ' ');
        let mut number = || parts.next()?.parse::<i32>().ok();
        let (Some(x), Some(y), Some(width), Some(height)) = (number(), number(), number(), number())
        else {
            continue;
        };
        let text = parts.next().unwrap_or_default().to_string();
        found.push((Rect { x, y, width, height }, text));
    }
    Some((width, found))
}

fn run_finder(picture: &Path, text: Option<&str>) -> Result<(u32, Vec<(Rect, String)>), String> {
    let mut finder = Command::new(finder()?);
    finder.arg(picture);
    if let Some(text) = text {
        finder.arg(text);
    }
    let out = finder
        .output()
        .map_err(|e| format!("running the text finder: {e}"))?;
    // Not there is an answer, not a failure.
    if out.status.code() == Some(1) {
        return Ok((0, Vec::new()));
    }
    if !out.status.success() {
        return Err(format!(
            "the text finder failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let said = String::from_utf8_lossy(&out.stdout);
    parsed(&said).ok_or_else(|| format!("the text finder said something unreadable: {}", said.trim()))
}

/// Every place `text` is in a picture, best match first, in the picture's
/// pixels, with the picture's width.
pub fn find_all(picture: &Path, text: &str) -> Result<(u32, Vec<Rect>), String> {
    let (width, found) = run_finder(picture, Some(text))?;
    Ok((width, found.into_iter().map(|(area, _)| area).collect()))
}

/// Every line that can be read in a picture, with where it is, in the
/// picture's pixels, and the picture's width.
pub fn read_all(picture: &Path) -> Result<(u32, Vec<(Rect, String)>), String> {
    run_finder(picture, None)
}

/// The box whose middle is closest to a spot.
pub fn nearest(boxes: &[Rect], near: (i32, i32)) -> Option<Rect> {
    boxes.iter().copied().min_by_key(|found| {
        let dx = (found.x + found.width / 2 - near.0) as i64;
        let dy = (found.y + found.height / 2 - near.1) as i64;
        dx * dx + dy * dy
    })
}

/// A box in a picture of `area`, in `area`'s own units from its top left.
///
/// A picture is in the screen's pixels, which on a scaled monitor are not
/// the units windows are placed in. Its width over the area's says how many
/// of one make one of the other.
fn in_area(found: Rect, picture_width: u32, area: Rect) -> Rect {
    let scale = picture_width as f64 / area.width.max(1) as f64;
    if scale <= 0.0 {
        return found;
    }
    Rect {
        x: (found.x as f64 / scale) as i32,
        y: (found.y as f64 / scale) as i32,
        width: (found.width as f64 / scale) as i32,
        height: (found.height as f64 / scale) as i32,
    }
}

/// Photograph `area` into `probe` and find `text` in it: every place it is,
/// best match first, measured from the area's top left.
pub fn find_in(portal: &Portal, area: Rect, text: &str, probe: &Path) -> Result<Vec<Rect>, String> {
    screen::shot(portal, area, probe)?;
    let (width, found) = find_all(probe, text)?;
    Ok(found.into_iter().map(|found| in_area(found, width, area)).collect())
}

/// Photograph `area` into `probe` and read it: every line, with where it is
/// measured from the area's top left.
pub fn read_in(portal: &Portal, area: Rect, probe: &Path) -> Result<Vec<(Rect, String)>, String> {
    screen::shot(portal, area, probe)?;
    let (width, lines) = read_all(probe)?;
    Ok(lines
        .into_iter()
        .map(|(found, text)| (in_area(found, width, area), text))
        .collect())
}

/// The box `text` occupies on screen inside `area`, found by looking.
pub fn find_box_on_screen(
    portal: &Portal,
    area: Rect,
    text: &str,
    probe: &Path,
) -> Result<Option<Rect>, String> {
    Ok(find_in(portal, area, text, probe)?.first().map(|found| Rect {
        x: area.x + found.x,
        y: area.y + found.y,
        ..*found
    }))
}

/// Where `text` is on screen inside `area`: the middle of it.
pub fn find_on_screen(
    portal: &Portal,
    area: Rect,
    text: &str,
    probe: &Path,
) -> Result<Option<(i32, i32)>, String> {
    Ok(find_box_on_screen(portal, area, text, probe)?
        .map(|found| (found.x + found.width / 2, found.y + found.height / 2)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_text_output_is_read_as_boxes() {
        // Asked for one text: the picture's size, then a box a line.
        let (width, found) = parsed("900 683\n501 68 31 12\n560 68 52 12\n").expect("unreadable");
        assert_eq!(width, 900);
        assert_eq!(
            found,
            vec![
                (Rect { x: 501, y: 68, width: 31, height: 12 }, String::new()),
                (Rect { x: 560, y: 68, width: 52, height: 12 }, String::new()),
            ]
        );

        // Asked for everything: each box is followed by what was read there,
        // spaces and all.
        let (_, lines) = parsed("900 683\n44 41 78 12 Save preset\n").expect("unreadable");
        assert_eq!(
            lines,
            vec![(Rect { x: 44, y: 41, width: 78, height: 12 }, "Save preset".to_string())]
        );

        // A picture with nothing in it to read is its size and no boxes.
        assert_eq!(parsed("900 683\n"), Some((900, Vec::new())));
        assert_eq!(parsed(""), None);
    }

    #[test]
    fn the_box_nearest_the_expected_spot_wins() {
        // The same word twice: a tab at the top and a heading further down.
        let tab = Rect { x: 40, y: 105, width: 40, height: 14 };
        let heading = Rect { x: 26, y: 150, width: 90, height: 24 };

        assert_eq!(nearest(&[heading, tab], (60, 113)), Some(tab));
        assert_eq!(nearest(&[tab, heading], (60, 160)), Some(heading));
        assert_eq!(nearest(&[], (60, 113)), None);
    }
}
