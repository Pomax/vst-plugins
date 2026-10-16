//! Where a step's words and places are in a picture of a window.
//!
//! A step names a place by what is drawn there, never by a number. TEXT is
//! words the window shows, with qualifiers after a `|`: `smallest` or
//! `largest` for words the window shows at two sizes, `below LABEL` for the
//! places under the topmost LABEL. WHERE is a point: TEXT itself, `before
//! TEXT`, `after TEXT`, `below TEXT`, `beside TEXT left` or `right`, or
//! `between TEXT and TEXT`. The text finder says where words are, and
//! `drawn` says what is drawn around them.
//!
//! Everything here is in the picture's own pixels, from its top left. The
//! driver that photographed the window turns them back into its screen.

use std::path::{Path, PathBuf};
use std::process::Command;

use image::RgbaImage;

use crate::drawn::{self, Place, Side};

/// Words a step names, and the qualifiers after them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    pub text: String,
    pub qualifiers: Vec<String>,
}

impl std::fmt::Display for Named {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.text)?;
        for qualifier in &self.qualifiers {
            write!(f, "|{qualifier}")?;
        }
        Ok(())
    }
}

/// `TEXT|QUALIFIER|QUALIFIER` taken apart.
pub fn split_named(value: &str) -> Named {
    let mut pieces = value.split('|').map(str::trim);
    Named {
        text: pieces.next().unwrap_or_default().to_string(),
        qualifiers: pieces.map(str::to_string).collect(),
    }
}

/// `TEXT|QUALIFIERS|to WHERE|QUALIFIERS` taken apart: the words before the
/// `to`, and the place after it.
pub fn split_to(value: &str, step: &str) -> Result<(Named, Named), String> {
    let pieces: Vec<&str> = value.split('|').map(str::trim).collect();
    let at = pieces
        .iter()
        .position(|piece| piece.starts_with("to "))
        .filter(|at| *at >= 1)
        .ok_or_else(|| format!("cannot read {step}: {value}"))?;
    let named = split_named(&pieces[..at].join("|"));
    let mut target = vec![pieces[at][3..].trim()];
    target.extend_from_slice(&pieces[at + 1..]);
    Ok((named, split_named(&target.join("|"))))
}

/// `WHERE|SHAPE` or `SHAPE` taken apart: the place, when there is one, and
/// the cursor's shape.
pub fn split_cursor(value: &str) -> (Option<Named>, String) {
    let pieces: Vec<&str> = value.split('|').map(str::trim).collect();
    let (shape, place) = pieces.split_last().unwrap_or((&"", &[]));
    if place.is_empty() {
        (None, shape.to_string())
    } else {
        (Some(split_named(&place.join("|"))), shape.to_string())
    }
}

/// `text` lowered, with its spaces taken out, which is how the finder
/// compares text.
fn squeezed(text: &str) -> String {
    text.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect()
}

/// A line the finder read: where, and what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub place: Place,
    pub text: String,
}

/// The text finder, `binaries/find-text`.
pub struct Finder {
    tool: PathBuf,
}

/// What the finder printed: the picture's width from its first line, then a
/// box a line, each with whatever follows the box on its line.
fn parsed(output: &str) -> Option<(u32, Vec<Line>)> {
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
        found.push(Line { place: Place { x, y, width, height }, text });
    }
    Some((width, found))
}

impl Finder {
    pub fn new(tool: PathBuf) -> Finder {
        Finder { tool }
    }

    fn run(&self, picture: &Path, text: Option<&str>, sized: Option<&str>) -> Result<Vec<Line>, String> {
        let mut finder = Command::new(&self.tool);
        finder.arg(picture);
        if let Some(text) = text {
            finder.arg(text);
            if let Some(sized) = sized {
                finder.arg(sized);
            }
        }
        let out = finder
            .output()
            .map_err(|e| format!("running the text finder: {e}"))?;
        if out.status.code() == Some(1) {
            return Ok(Vec::new());
        }
        if !out.status.success() {
            return Err(format!(
                "the text finder failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let said = String::from_utf8_lossy(&out.stdout);
        parsed(&said)
            .map(|(_, lines)| lines)
            .ok_or_else(|| format!("the text finder said something unreadable: {}", said.trim()))
    }

    /// Every line that can be read in a picture.
    pub fn lines(&self, picture: &Path) -> Result<Vec<Line>, String> {
        self.run(picture, None, None)
    }

    /// What a picture reads as, for saying so when a step fails.
    pub fn reads(&self, picture: &Path) -> String {
        match self.lines(picture) {
            Ok(lines) => lines.into_iter().map(|line| line.text).collect::<Vec<_>>().join(" | "),
            Err(e) => e,
        }
    }

    /// Every place `text` is in a picture, best match first. With `sized`,
    /// `smallest` or `largest`, the one place with that lettering.
    pub fn all(&self, picture: &Path, text: &str, sized: Option<&str>) -> Result<Vec<Place>, String> {
        Ok(self
            .run(picture, Some(text), sized)?
            .into_iter()
            .map(|line| line.place)
            .collect())
    }

    /// Every place the named words are, kept to the qualifiers: `smallest`
    /// or `largest` is the one place with that lettering, and `below LABEL`
    /// keeps the places under the topmost LABEL.
    pub fn qualified(&self, picture: &Path, named: &Named) -> Result<Vec<Place>, String> {
        let sized = named
            .qualifiers
            .iter()
            .rev()
            .find(|q| *q == "smallest" || *q == "largest")
            .map(String::as_str);
        let mut found = self.all(picture, &named.text, sized)?;
        for qualifier in &named.qualifiers {
            if let Some(label) = qualifier.strip_prefix("below ") {
                let Some(anchor) = self.topmost(picture, label.trim())? else {
                    return Ok(Vec::new());
                };
                found.retain(|place| place.y >= anchor.y + anchor.height);
            }
        }
        Ok(found)
    }

    /// The first place the named words are, or nothing.
    pub fn first(&self, picture: &Path, named: &Named) -> Result<Option<Place>, String> {
        Ok(self.qualified(picture, named)?.into_iter().next())
    }

    fn topmost(&self, picture: &Path, text: &str) -> Result<Option<Place>, String> {
        Ok(self.all(picture, text, None)?.into_iter().min_by_key(|place| place.y))
    }

    /// The first line read in a picture that holds the named words, boxed
    /// across the whole line, kept to the qualifiers the way `qualified`
    /// keeps places.
    pub fn line_of(&self, picture: &Path, named: &Named) -> Result<Option<Place>, String> {
        let wanted = squeezed(&named.text);
        let mut lines: Vec<Line> = self
            .lines(picture)?
            .into_iter()
            .filter(|line| squeezed(&line.text).contains(&wanted))
            .collect();
        for qualifier in &named.qualifiers {
            if let Some(label) = qualifier.strip_prefix("below ") {
                let Some(anchor) = self.topmost(picture, label.trim())? else {
                    return Ok(None);
                };
                lines.retain(|line| line.place.y >= anchor.y + anchor.height);
            }
            if qualifier == "smallest" {
                lines.sort_by_key(|line| line.place.height);
            }
            if qualifier == "largest" {
                lines.sort_by_key(|line| -line.place.height);
            }
        }
        Ok(lines.into_iter().next().map(|line| line.place))
    }

    /// The line a box of words was read on: the one whose box holds the
    /// words' middle.
    pub fn line_holding(&self, picture: &Path, words: Place) -> Result<Option<Place>, String> {
        let (x, y) = words.middle();
        Ok(self
            .lines(picture)?
            .into_iter()
            .map(|line| line.place)
            .find(|line| {
                line.x <= x && x <= line.x + line.width && line.y <= y && y <= line.y + line.height
            }))
    }

    /// Where a WHERE phrase points in a picture, or nothing when what it
    /// names is not in the picture.
    pub fn point_in(&self, picture: &Path, named: &Named) -> Result<Option<(i32, i32)>, String> {
        let phrase = named.text.as_str();
        let with = |text: &str| Named { text: text.trim().to_string(), qualifiers: named.qualifiers.clone() };

        if let Some(rest) = phrase.strip_prefix("between ") {
            let Some((one, other)) = rest.split_once(" and ") else {
                return Err(format!("cannot read between: {phrase}"));
            };
            let (Some(a), Some(b)) = (self.first(picture, &with(one))?, self.first(picture, &with(other))?)
            else {
                return Ok(None);
            };
            return Ok(Some(((a.x + a.width + b.x) / 2, (a.middle().1 + b.middle().1) / 2)));
        }

        if let Some(rest) = phrase.strip_prefix("beside ") {
            let (text, side) = match rest.rsplit_once(' ') {
                Some((text, "left")) => (text, Side::Left),
                Some((text, "right")) => (text, Side::Right),
                _ => return Err(format!("cannot read beside: {phrase}")),
            };
            let Some(words) = self.first(picture, &with(text))? else {
                return Ok(None);
            };
            let line = self.line_holding(picture, words)?;
            let pixels = open(picture)?;
            let label = drawn::label_of(&pixels, words, line, side);
            return Ok(drawn::beside(&pixels, label, side).map(|x| (x, words.middle().1)));
        }

        for how in ["before ", "after ", "below "] {
            let Some(text) = phrase.strip_prefix(how) else {
                continue;
            };
            let Some(line) = self.line_of(picture, &with(text))? else {
                return Ok(None);
            };
            if how == "before " {
                return Ok(Some((line.x - 2, line.middle().1)));
            }
            let pixels = open(picture)?;
            return Ok(Some(if how == "after " {
                (drawn::after(&pixels, line), line.middle().1)
            } else {
                (line.middle().0, drawn::below(&pixels, line))
            }));
        }

        Ok(self.first(picture, named)?.map(|words| words.middle()))
    }

    /// Where the drop mark is in a picture of a window `width` wide:
    /// `before A` looks left of the words A on their row, `after A` right of
    /// them. Nothing when there is none, or A is not in the picture.
    pub fn mark_in(&self, picture: &Path, named: &Named, width: i32) -> Result<Option<(i32, i32)>, String> {
        let with = |text: &str| Named { text: text.trim().to_string(), qualifiers: named.qualifiers.clone() };
        let (words, span) = if let Some(text) = named.text.strip_prefix("before ") {
            let Some(words) = self.first(picture, &with(text))? else {
                return Ok(None);
            };
            (words, (0, words.x))
        } else if let Some(text) = named.text.strip_prefix("after ") {
            let Some(words) = self.first(picture, &with(text))? else {
                return Ok(None);
            };
            (words, (words.x + words.width, width))
        } else {
            return Err(format!("cannot read mark: {}", named.text));
        };
        Ok(drawn::mark_in(&open(picture)?, words, span.0, span.1))
    }
}

fn open(picture: &Path) -> Result<RgbaImage, String> {
    Ok(image::open(picture)
        .map_err(|e| format!("could not open {}: {e}", picture.display()))?
        .into_rgba8())
}

/// How wide a picture is, in its pixels.
pub fn width_of(picture: &Path) -> Result<u32, String> {
    Ok(open(picture)?.width())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_split_from_their_qualifiers() {
        assert_eq!(
            split_named("Save|below Open|smallest"),
            Named {
                text: "Save".to_string(),
                qualifiers: vec!["below Open".to_string(), "smallest".to_string()],
            }
        );
        assert_eq!(split_named("links"), Named { text: "links".to_string(), qualifiers: vec![] });
    }

    #[test]
    fn a_drag_is_split_at_its_to() {
        let (from, to) = split_to("gamma|to after delta", "dragwords").expect("unreadable");
        assert_eq!(from, split_named("gamma"));
        assert_eq!(to, split_named("after delta"));

        let (from, to) = split_to("Mix notes|smallest|to below Drum bus|largest", "dragtext").expect("unreadable");
        assert_eq!(from, split_named("Mix notes|smallest"));
        assert_eq!(to, split_named("below Drum bus|largest"));

        assert!(split_to("to somewhere", "dragtext").is_err());
        assert!(split_to("gamma|delta", "dragtext").is_err());
    }

    #[test]
    fn a_cursor_step_names_a_shape_and_maybe_a_place() {
        assert_eq!(split_cursor("grabbing"), (None, "grabbing".to_string()));
        assert_eq!(
            split_cursor("Auto|below Markdown source|hand"),
            (Some(split_named("Auto|below Markdown source")), "hand".to_string())
        );
    }

    #[test]
    fn the_finders_output_is_read_as_lines() {
        let (width, found) = parsed("900 683\n501 68 31 12\n560 68 52 12\n").expect("unreadable");
        assert_eq!(width, 900);
        assert_eq!(
            found,
            vec![
                Line { place: Place { x: 501, y: 68, width: 31, height: 12 }, text: String::new() },
                Line { place: Place { x: 560, y: 68, width: 52, height: 12 }, text: String::new() },
            ]
        );
        let (_, lines) = parsed("900 683\n44 41 78 12 Save preset\n").expect("unreadable");
        assert_eq!(
            lines,
            vec![Line { place: Place { x: 44, y: 41, width: 78, height: 12 }, text: "Save preset".to_string() }]
        );
        assert_eq!(parsed("900 683\n"), Some((900, Vec::new())));
        assert_eq!(parsed(""), None);
    }
}
