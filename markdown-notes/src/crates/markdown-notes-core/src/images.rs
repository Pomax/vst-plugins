//! Pictures in a note.
//!
//! A picture is written into the text as a reference, `![alt][1]`, on a line
//! of its own, and its data as a definition, `[1]: data:image/png;base64,...`,
//! at the very end of the document. The definitions are the note's images
//! tail: they save as the bottom of the document, they are read back off it,
//! and the text stays legible without kilobytes of base64 in it.
//!
//! A definition nothing refers to any more is dropped, and the ones after it
//! move up, so the numbers always run 1, 2, 3 with no gaps.

use std::ops::Range;

use base64::Engine;

pub const PNG: &str = "image/png";
pub const JPEG: &str = "image/jpeg";

/// One picture's data, as it is defined at the end of the document.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Image {
    pub number: usize,
    /// [`PNG`] or [`JPEG`].
    pub mime: String,
    /// The bytes, base64.
    pub data: String,
}

impl Image {
    pub fn from_bytes(number: usize, mime: &str, bytes: &[u8]) -> Image {
        Image {
            number,
            mime: mime.to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    pub fn bytes(&self) -> Option<Vec<u8>> {
        base64::engine::general_purpose::STANDARD.decode(&self.data).ok()
    }

    pub fn url(&self) -> String {
        format!("data:{};base64,{}", self.mime, self.data)
    }

    /// The line that defines it.
    pub fn definition(&self) -> String {
        format!("[{}]: {}", self.number, self.url())
    }

    /// A definition line read back, or nothing for a line that is not one.
    pub fn parse(line: &str) -> Option<Image> {
        let rest = line.strip_prefix('[')?;
        let (number, rest) = rest.split_once("]: data:")?;
        let number: usize = number.parse().ok()?;
        let (mime, data) = rest.split_once(";base64,")?;
        if mime != PNG && mime != JPEG {
            return None;
        }
        Some(Image { number, mime: mime.to_string(), data: data.trim().to_string() })
    }
}

/// Which kind of picture these bytes are, by how they start, or nothing for
/// bytes that are neither a PNG nor a JPEG.
pub fn kind_of(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(PNG)
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(JPEG)
    } else {
        None
    }
}

/// How a picture is referred to in the text.
pub fn reference(alt: &str, number: usize) -> String {
    let alt: String = alt.chars().filter(|c| !matches!(c, '[' | ']' | '\n')).collect();
    format!("![{alt}][{number}]")
}

/// The reference on a line that is nothing but one: its alt text and number.
pub fn reference_on(line: &str) -> Option<(&str, usize)> {
    let line = line.trim();
    let rest = line.strip_prefix("![")?;
    let (alt, rest) = rest.split_once("][")?;
    let number = rest.strip_suffix(']')?;
    if alt.contains('[') || alt.contains(']') || number.is_empty() {
        return None;
    }
    let number: usize = number.parse().ok()?;
    Some((alt, number))
}

/// Every number referred to in `text`, in order of first appearance.
pub fn referenced(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices("![") {
        let rest = &text[at + 2..];
        let Some((alt, rest)) = rest.split_once("][") else { continue };
        if alt.contains('[') || alt.contains(']') || alt.contains('\n') {
            continue;
        }
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() || !rest[digits.len()..].starts_with(']') {
            continue;
        }
        if let Ok(number) = digits.parse::<usize>() {
            if !found.contains(&number) {
                found.push(number);
            }
        }
    }
    found
}

/// The document less its images tail, and the tail's definitions.
///
/// The tail is the run of definition lines at the very end, after a blank
/// line. A document without one comes back as it is.
pub fn split_off(text: &str) -> (String, Vec<Image>) {
    let trimmed = text.trim_end_matches('\n');
    let mut images: Vec<Image> = Vec::new();
    let mut body_end = trimmed.len();
    for line in trimmed.rsplit('\n') {
        match Image::parse(line) {
            Some(image) => {
                images.push(image);
                body_end -= line.len();
                body_end = body_end.saturating_sub(1);
            }
            None => break,
        }
    }
    if images.is_empty() {
        return (text.to_string(), images);
    }
    images.reverse();
    // The blank line that stood the tail apart is the tail's, not the body's.
    let body = trimmed[..body_end.min(trimmed.len())].trim_end_matches('\n');
    (body.to_string(), images)
}

/// The document with its images tail put back on the end.
pub fn join(body: &str, images: &[Image]) -> String {
    if images.is_empty() {
        return body.to_string();
    }
    let mut out = body.trim_end_matches('\n').to_string();
    out.push_str("\n\n");
    for image in images {
        out.push_str(&image.definition());
        out.push('\n');
    }
    out
}

/// Drop every definition nothing refers to and close the gaps, rewriting
/// the references in `texts` to match. Reports whether anything changed.
///
/// The order kept is the definitions' own: a picture keeps its place in the
/// tail whichever section refers to it.
pub fn tidy(texts: &mut [String], images: &mut Vec<Image>) -> bool {
    let mut used: Vec<usize> = Vec::new();
    for text in texts.iter() {
        for number in referenced(text) {
            if !used.contains(&number) {
                used.push(number);
            }
        }
    }
    let kept: Vec<Image> = images.iter().filter(|i| used.contains(&i.number)).cloned().collect();
    let renumbered: Vec<(usize, usize)> = kept
        .iter()
        .enumerate()
        .map(|(at, image)| (image.number, at + 1))
        .collect();
    let unchanged = kept.len() == images.len() && renumbered.iter().all(|(from, to)| from == to);
    if unchanged {
        return false;
    }
    for text in texts.iter_mut() {
        *text = renumber(text, &renumbered);
    }
    *images = kept
        .into_iter()
        .zip(renumbered.iter())
        .map(|(image, &(_, to))| Image { number: to, ..image })
        .collect();
    true
}

/// Every reference in `text`: the byte range of its number, and the number.
pub fn reference_spans(text: &str) -> Vec<(Range<usize>, usize)> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices("![") {
        let rest = &text[at + 2..];
        let Some(close) = rest.find("][") else { continue };
        let alt = &rest[..close];
        if alt.contains('[') || alt.contains(']') || alt.contains('\n') {
            continue;
        }
        let start = at + 2 + close + 2;
        let digits: String = text[start..].chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() || !text[start + digits.len()..].starts_with(']') {
            continue;
        }
        if let Ok(number) = digits.parse::<usize>() {
            found.push((start..start + digits.len(), number));
        }
    }
    found
}

/// `text` with every reference to an old number rewritten to its new one.
fn renumber(text: &str, changes: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("![") {
        out.push_str(&rest[..at + 2]);
        rest = &rest[at + 2..];
        let Some(close) = rest.find("][") else { continue };
        let alt = &rest[..close];
        if alt.contains('[') || alt.contains(']') || alt.contains('\n') {
            continue;
        }
        let after = &rest[close + 2..];
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() || !after[digits.len()..].starts_with(']') {
            continue;
        }
        let number: usize = match digits.parse() {
            Ok(number) => number,
            Err(_) => continue,
        };
        let new = changes.iter().find(|(from, _)| *from == number).map(|(_, to)| *to);
        out.push_str(alt);
        out.push_str("][");
        match new {
            Some(to) => out.push_str(&to.to_string()),
            None => out.push_str(&digits),
        }
        rest = &after[digits.len()..];
    }
    out.push_str(rest);
    out
}

/// Where a reference on a line of its own goes when inserted at `at` in
/// `text`: the text to insert, so that the reference stands on its own line
/// with a blank line before and after it. There is a blank line after it even
/// at the end of the document, so what is typed next starts a paragraph of
/// its own rather than running on from the reference.
pub fn block_to_insert(text: &str, at: usize, reference: &str) -> String {
    let before = &text[..at];
    let after = &text[at..];
    let lead = if before.is_empty() || before.ends_with("\n\n") {
        ""
    } else if before.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let trail = if after.starts_with("\n\n") {
        ""
    } else if after.starts_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{lead}{reference}{trail}")
}

/// The byte range of the reference on line `line` of `text`, when that line
/// is nothing but one.
pub fn reference_range(text: &str, line: usize) -> Option<Range<usize>> {
    let mut start = 0;
    for (index, row) in text.split('\n').enumerate() {
        if index == line {
            return reference_on(row).map(|_| start..start + row.len());
        }
        start += row.len() + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Image {
        Image::from_bytes(1, PNG, b"\x89PNG\r\n\x1a\nabc")
    }

    #[test]
    fn bytes_are_told_apart_by_how_they_start() {
        assert_eq!(kind_of(b"\x89PNG\r\n\x1a\n...."), Some(PNG));
        assert_eq!(kind_of(b"\xff\xd8\xff\xe0...."), Some(JPEG));
        assert_eq!(kind_of(b"GIF89a"), None);
        assert_eq!(kind_of(b""), None);
    }

    #[test]
    fn a_definition_round_trips() {
        let image = png();
        let line = image.definition();
        assert!(line.starts_with("[1]: data:image/png;base64,"));
        assert_eq!(Image::parse(&line), Some(image.clone()));
        assert_eq!(image.bytes().as_deref(), Some(&b"\x89PNG\r\n\x1a\nabc"[..]));
    }

    #[test]
    fn only_png_and_jpeg_definitions_are_read() {
        assert!(Image::parse("[1]: data:image/gif;base64,AAAA").is_none());
        assert!(Image::parse("[1]: https://example.com/a.png").is_none());
        assert!(Image::parse("[x]: data:image/png;base64,AAAA").is_none());
    }

    #[test]
    fn a_reference_is_read_off_a_line_of_its_own() {
        assert_eq!(reference_on("![a cat][2]"), Some(("a cat", 2)));
        assert_eq!(reference_on("  ![a cat][2]  "), Some(("a cat", 2)));
        assert_eq!(reference_on("see ![a cat][2]"), None);
        assert_eq!(reference_on("![a cat](cat.png)"), None);
        assert_eq!(reference_on("![a [cat]][2]"), None);
    }

    #[test]
    fn brackets_are_kept_out_of_alt_text() {
        assert_eq!(reference("a [cat]", 3), "![a cat][3]");
    }

    #[test]
    fn the_tail_is_split_off_the_document_and_put_back() {
        let body = "# Notes\n\n![a][1]\n\nmore";
        let images = vec![png(), Image::from_bytes(2, JPEG, b"\xff\xd8\xffx")];
        let joined = join(body, &images);
        assert!(joined.starts_with("# Notes\n\n![a][1]\n\nmore\n\n[1]: data:image/png;base64,"));
        assert!(joined.ends_with("\n"));

        let (back, found) = split_off(&joined);
        assert_eq!(back, body);
        assert_eq!(found, images);
    }

    #[test]
    fn a_document_without_a_tail_is_left_alone() {
        let text = "# Notes\n\nplain\n\n";
        let (body, images) = split_off(text);
        assert_eq!(body, text);
        assert!(images.is_empty());
        assert_eq!(join(text, &[]), text);
    }

    #[test]
    fn references_are_found_in_order_of_first_appearance() {
        assert_eq!(referenced("![b][2] and ![a][1] and ![b][2]"), vec![2, 1]);
        assert_eq!(referenced("![x](x.png) ![y][x] ![z][3"), Vec::<usize>::new());
    }

    #[test]
    fn the_numbers_of_references_are_found_with_their_ranges() {
        let text = "see ![a][12] and ![b](x.png) and ![c][3]";
        assert_eq!(reference_spans(text), vec![(9..11, 12), (38..39, 3)]);
        assert_eq!(&text[9..11], "12");
        assert_eq!(&text[38..39], "3");
    }

    #[test]
    fn a_definition_nothing_refers_to_is_dropped_and_the_rest_close_up() {
        let mut texts = vec!["![a][1]\n\n![c][3]".to_string(), "![d][4]".to_string()];
        let mut images = vec![
            Image::from_bytes(1, PNG, b"1"),
            Image::from_bytes(2, PNG, b"2"),
            Image::from_bytes(3, PNG, b"3"),
            Image::from_bytes(4, PNG, b"4"),
        ];

        assert!(tidy(&mut texts, &mut images));

        assert_eq!(texts, vec!["![a][1]\n\n![c][2]".to_string(), "![d][3]".to_string()]);
        let numbers: Vec<usize> = images.iter().map(|i| i.number).collect();
        assert_eq!(numbers, vec![1, 2, 3]);
        assert_eq!(images[1].bytes().unwrap(), b"3");
        assert_eq!(images[2].bytes().unwrap(), b"4");
    }

    #[test]
    fn a_tidy_document_is_left_as_it_is() {
        let mut texts = vec!["![a][1] ![b][2]".to_string()];
        let mut images = vec![Image::from_bytes(1, PNG, b"1"), Image::from_bytes(2, PNG, b"2")];
        assert!(!tidy(&mut texts, &mut images));
        assert_eq!(texts[0], "![a][1] ![b][2]");
    }

    #[test]
    fn a_reference_goes_on_a_line_of_its_own_with_blank_lines_around_it() {
        assert_eq!(block_to_insert("", 0, "![a][1]"), "![a][1]\n\n");
        assert_eq!(block_to_insert("text", 4, "![a][1]"), "\n\n![a][1]\n\n");
        assert_eq!(block_to_insert("text\n", 5, "![a][1]"), "\n![a][1]\n\n");
        assert_eq!(block_to_insert("text\n\n", 6, "![a][1]"), "![a][1]\n\n");
        assert_eq!(block_to_insert("text", 2, "![a][1]"), "\n\n![a][1]\n\n");
        assert_eq!(block_to_insert("text\n\nmore", 5, "![a][1]"), "\n![a][1]\n");
    }

    #[test]
    fn the_range_of_a_reference_line_is_found_by_line_number() {
        let text = "# Notes\n\n![a][1]\n\nmore";
        assert_eq!(reference_range(text, 2), Some(9..16));
        assert_eq!(reference_range(text, 0), None);
        assert_eq!(reference_range(text, 9), None);
    }
}
