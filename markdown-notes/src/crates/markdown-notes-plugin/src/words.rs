//! A diagram's words as hinted outlines.
//!
//! resvg draws `<text>` from unhinted glyph outlines, so at the sizes a
//! diagram is lettered in every stem falls between pixels and comes out as
//! two columns of grey. Here each `<text>` is shaped with HarfBuzz, its glyphs
//! are taken from the same font hinted to the pixel grid, and the SVG gets a
//! `<path>` of them in place of the text, which resvg fills as it is.

use resvg::usvg::fontdb;
use skrifa::outline::{DrawSettings, HintingInstance, HintingOptions, OutlinePen};
use skrifa::prelude::{LocationRef, Size};
use skrifa::{GlyphId, MetadataProvider};

/// Every `<text>` of `svg` replaced by a path of its words, hinted for a
/// picture drawn at `scale` pixels per unit. A text no font on the machine can
/// draw is left as it was.
pub fn as_hinted_paths(svg: &str, fonts: &fontdb::Database, scale: f32) -> String {
    let rules = class_rules(svg);
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find("<text ") {
        let Some(end) = rest[at..].find("</text>") else {
            break;
        };
        let element = &rest[at..at + end + "</text>".len()];
        out.push_str(&rest[..at]);
        match path_for(element, &rules, fonts, scale) {
            Some(path) => out.push_str(&path),
            None => out.push_str(element),
        }
        rest = &rest[at + end + "</text>".len()..];
    }
    out.push_str(rest);
    out
}

/// The declarations of every rule in the SVG's stylesheets that selects a
/// class, by the class: `#merman .titleText{text-anchor:middle;...}` is
/// `("titleText", "text-anchor:middle;...")`. In the order written, so a later
/// rule for the same class comes later.
fn class_rules(svg: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = svg;
    while let Some(at) = rest.find("<style") {
        let Some(open) = rest[at..].find('>') else {
            break;
        };
        let sheet = &rest[at + open + 1..];
        let end = sheet.find("</style>").unwrap_or(sheet.len());
        for rule in sheet[..end].split('}') {
            let Some((selectors, declarations)) = rule.split_once('{') else {
                continue;
            };
            for selector in selectors.split(',') {
                if let Some(class) = selector.split_whitespace().last().and_then(|s| s.strip_prefix('.')) {
                    if class.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                        found.push((class.to_string(), declarations.trim().to_string()));
                    }
                }
            }
        }
        rest = &sheet[end..];
    }
    found
}

/// What a `<text>` element says about its words.
struct Words {
    text: String,
    x: f32,
    /// The baseline, or the middle of the line when `central`.
    y: f32,
    size: f32,
    families: String,
    bold: bool,
    /// Anchored at their middle rather than their start.
    centred: bool,
    /// `y` is the middle of the line, the way a label is placed, rather than
    /// the baseline.
    central: bool,
    fill: String,
}

/// What a `<text>` says about its words, from its own attributes and, when
/// the words sit in one `<tspan>`, from that tspan's, which win. Several
/// tspans are several lines, which this does not read. `rules` are the
/// stylesheet's class rules, which the text's classes pick from; the text's
/// own style outranks them.
fn words_of(element: &str, rules: &[(String, String)]) -> Option<Words> {
    let tag_end = element.find('>')?;
    let outer = format!(" {}", &element[..tag_end]);
    let inner = &element[tag_end + 1..element.len() - "</text>".len()];
    let (text, span) = match inner.matches("<tspan").count() {
        0 if !inner.contains('<') => (inner, None),
        1 => {
            let at = inner.find("<tspan")?;
            let len = inner[at..].find('>')?;
            let close = inner.find("</tspan>")?;
            if !inner[..at].trim().is_empty() || !inner[close + "</tspan>".len()..].trim().is_empty() {
                return None;
            }
            (&inner[at + len + 1..close], Some(format!(" {}", &inner[at + "<tspan".len()..at + len])))
        }
        _ => return None,
    };
    let text = unescaped(text);
    // The tspan's attributes first, then the text's.
    let attr = |name: &str| {
        span.as_deref()
            .and_then(|span| attribute(span, name))
            .or_else(|| attribute(&outer, name))
    };
    let number = |name: &str| attr(name)?.parse::<f32>().ok();
    // The class rules first and the text's own style after them, so the
    // last declaration of a property is the one that counts. Unescaped
    // before it is split on semicolons: `&quot;` has one.
    let mut style = String::new();
    for class in attr("class").unwrap_or("").split_whitespace() {
        for (_, declarations) in rules.iter().filter(|(name, _)| name == class) {
            style.push_str(declarations);
            style.push(';');
        }
    }
    style.push_str(attr("style").unwrap_or(""));
    let style = unescaped(&style);
    let property = |name: &str| {
        style
            .split(';')
            .filter_map(|rule| rule.split_once(':'))
            .filter(|(key, _)| key.trim() == name)
            .map(|(_, value)| value.trim())
            .last()
    };
    let size = property("font-size")
        .and_then(|s| s.trim_end_matches("px").parse::<f32>().ok())
        .or_else(|| number("font-size"))?;
    let families = property("font-family")
        .map(str::to_string)
        .or_else(|| attr("font-family").map(unescaped))
        .unwrap_or_else(|| "sans-serif".to_string());
    let weight = property("font-weight").or_else(|| attr("font-weight"));
    let bold = weight.is_some_and(|w| w == "bold" || w.parse::<u16>().is_ok_and(|w| w >= 600));
    let anchor = property("text-anchor").or_else(|| attr("text-anchor"));
    let baseline = property("dominant-baseline")
        .or_else(|| attr("dominant-baseline"))
        .or_else(|| property("alignment-baseline"))
        .or_else(|| attr("alignment-baseline"));
    // A `dy` moves the words down from `y`, in pixels or in ems.
    let dy = attr("dy")
        .map(|dy| match dy.strip_suffix("em") {
            Some(ems) => ems.parse::<f32>().unwrap_or(0.0) * size,
            None => dy.trim_end_matches("px").parse::<f32>().unwrap_or(0.0),
        })
        .unwrap_or(0.0);
    Some(Words {
        text,
        x: number("x").unwrap_or(0.0),
        y: number("y").unwrap_or(0.0) + dy,
        size,
        families,
        bold,
        centred: anchor == Some("middle"),
        central: baseline == Some("central") || baseline == Some("middle"),
        fill: property("fill").or_else(|| attr("fill")).unwrap_or("#000").to_string(),
    })
}

/// The `<path>` for one `<text>`, or nothing when no font can draw it.
fn path_for(
    element: &str,
    rules: &[(String, String)],
    fonts: &fontdb::Database,
    scale: f32,
) -> Option<String> {
    let words = words_of(element, rules)?;
    if words.text.trim().is_empty() {
        return None;
    }
    let id = crate::measure::face_for(fonts, &words.families, words.bold)?;
    fonts.with_face_data(id, |data, index| outline(&words, data, index, scale))?
}

/// The words shaped and drawn in device pixels, then written in SVG units.
fn outline(words: &Words, data: &[u8], index: u32, scale: f32) -> Option<String> {
    let ppem = words.size * scale;
    let shaping_font = harfrust::FontRef::from_index(data, index).ok()?;
    let shaping = harfrust::ShaperData::new(&shaping_font);
    let shaper = shaping.shaper(&shaping_font).build();
    let mut buffer = harfrust::UnicodeBuffer::new();
    buffer.push_str(&words.text);
    buffer.guess_segment_properties();
    let shaped = shaper.shape(buffer, harfrust::ShapeOptions::new());
    let per_unit = ppem / shaper.units_per_em() as f32;

    let font = skrifa::FontRef::from_index(data, index).ok()?;
    let outlines = font.outline_glyphs();
    let hinting = HintingInstance::new(
        &outlines,
        Size::new(ppem),
        LocationRef::default(),
        HintingOptions::default(),
    )
    .ok()?;
    let metrics = font.metrics(Size::new(ppem), LocationRef::default());

    // Where the words start, in device pixels, on the grid: hinting fits each
    // glyph to the grid at its own origin, so the origins have to be on it.
    let width: f32 = shaped
        .glyph_positions()
        .iter()
        .map(|position| position.x_advance as f32 * per_unit)
        .sum();
    let start = words.x * scale - if words.centred { width / 2.0 } else { 0.0 };
    // The baseline: `y` itself, or for words centred on their line the
    // baseline that puts the line's middle at `y`.
    let middle = if words.central { (metrics.ascent + metrics.descent) / 2.0 } else { 0.0 };
    let baseline = (words.y * scale + middle).round();

    let mut d = String::new();
    let mut pen_x = start.round();
    for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
        let origin = (
            (pen_x + position.x_offset as f32 * per_unit).round(),
            baseline - (position.y_offset as f32 * per_unit).round(),
        );
        if let Some(glyph) = outlines.get(GlyphId::new(info.glyph_id)) {
            let mut pen = Pen { d: &mut d, origin, scale };
            let _ = glyph.draw(DrawSettings::hinted(&hinting, false), &mut pen);
        }
        pen_x += (position.x_advance as f32 * per_unit).round();
    }
    if d.is_empty() {
        return None;
    }
    // In a style as well as attributes: the stylesheet merman writes gives
    // every path inside a node the node's border and fill, and a rule
    // outranks an attribute.
    let fill = &words.fill;
    Some(format!(
        "<path d=\"{d}\" fill=\"{fill}\" stroke=\"none\" style=\"fill:{fill};stroke:none\"/>"
    ))
}

/// Writes a glyph's outline as SVG path commands, from the glyph's own
/// coordinates, which go up, to the picture's, which go down and are in
/// units of `1 / scale` pixels.
struct Pen<'a> {
    d: &'a mut String,
    origin: (f32, f32),
    scale: f32,
}

impl Pen<'_> {
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        ((self.origin.0 + x) / self.scale, (self.origin.1 - y) / self.scale)
    }

    fn write(&mut self, command: char, points: &[(f32, f32)]) {
        self.d.push(command);
        for (i, (x, y)) in points.iter().enumerate() {
            if i > 0 {
                self.d.push(' ');
            }
            let (x, y) = self.point(*x, *y);
            self.d.push_str(&format!("{x:.2},{y:.2}"));
        }
    }
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.write('M', &[(x, y)]);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.write('L', &[(x, y)]);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.write('Q', &[(cx0, cy0), (x, y)]);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.write('C', &[(cx0, cy0), (cx1, cy1), (x, y)]);
    }

    fn close(&mut self) {
        self.d.push('Z');
    }
}

/// The value of `name="..."` in `tag`, which starts with a space.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    let len = tag[at..].find('"')?;
    Some(&tag[at..at + len])
}

/// The text with the XML escapes the SVG writes undone.
fn unescaped(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A label as merman's resvg-safe pipeline writes it.
    const LABEL: &str = "<text x=\"218\" y=\"165\" dominant-baseline=\"central\" alignment-baseline=\"central\" fill=\"#28253D\" class=\"merman-foreignobject-fallback-text root edgeLabels edgeLabel labelBkg\" style=\"text-anchor:middle;font-size:14px;font-family:&quot;Recursive Variable&quot;,arial,sans-serif\">sidechain</text>";

    /// A gantt chart's title, styled by a class rule alone.
    const TITLED: &str = "<svg><style>#merman .titleText{text-anchor:middle;font-size:18px;fill:#505050;font-family:\"ibm plex sans\",arial,sans-serif;}</style><text x=\"592\" y=\"25\" class=\"titleText\">A Gantt Diagram</text></svg>";

    #[test]
    fn a_title_takes_its_size_and_family_from_its_class_rule() {
        let rules = class_rules(TITLED);
        assert_eq!(rules.len(), 1, "{rules:?}");
        assert_eq!(rules[0].0, "titleText");
        let title = &TITLED[TITLED.find("<text ").unwrap()..TITLED.find("</text>").unwrap() + "</text>".len()];
        let words = words_of(title, &rules).expect("the title was not read");
        assert_eq!(words.text, "A Gantt Diagram");
        assert_eq!((words.x, words.y, words.size), (592.0, 25.0, 18.0));
        assert_eq!(words.families, "\"ibm plex sans\",arial,sans-serif");
        assert_eq!(words.fill, "#505050");
        assert!(words.centred);
    }

    #[test]
    fn a_label_is_read() {
        let words = words_of(LABEL, &[]).expect("the label was not read");
        assert_eq!(words.text, "sidechain");
        assert_eq!((words.x, words.y, words.size), (218.0, 165.0, 14.0));
        assert_eq!(words.families, "\"Recursive Variable\",arial,sans-serif");
        assert!(words.centred);
        assert!(!words.bold);
        assert_eq!(words.fill, "#28253D");
    }

    /// A gantt chart's section label, whose words are in a `<tspan>` that
    /// carries their x.
    const SECTION: &str = "<text x=\"5\" y=\"214\" class=\"sectionTitle sectionTitle0\" font-size=\"11\" fill=\"#333\"><tspan alignment-baseline=\"central\" x=\"10\">Precomp</tspan></text>";

    #[test]
    fn the_words_in_a_tspan_are_read_with_its_x() {
        let words = words_of(SECTION, &[]).expect("the section label was not read");
        assert_eq!(words.text, "Precomp");
        assert_eq!((words.x, words.y, words.size), (10.0, 214.0, 11.0));
        assert!(!words.centred);
        assert!(words.central, "the tspan's alignment-baseline was not read");
    }

    /// A gantt chart's axis date: no x of its own, placed by the group it is
    /// in, and moved down one em onto its baseline.
    #[test]
    fn a_date_label_without_an_x_is_read_on_its_baseline() {
        let date = "<text fill=\"#000\" y=\"3\" dy=\"1em\" stroke=\"none\" font-size=\"10\" style=\"text-anchor:middle\">01/01</text>";
        let words = words_of(date, &[]).expect("the date was not read");
        assert_eq!(words.text, "01/01");
        assert_eq!((words.x, words.y, words.size), (0.0, 13.0, 10.0));
        assert!(words.centred);
        assert!(!words.central);
    }

    #[test]
    fn a_label_is_centred_on_its_line() {
        let words = words_of(LABEL, &[]).unwrap();
        assert!(words.central, "the label's dominant-baseline was not read");
    }

    /// Several `<tspan>`s are several lines, which this does not letter:
    /// the text is left to resvg.
    #[test]
    fn a_text_of_several_tspans_is_left_alone() {
        let lines = "<text x=\"5\" y=\"20\" font-size=\"11\"><tspan x=\"5\" dy=\"0\">one</tspan><tspan x=\"5\" dy=\"12\">two</tspan></text>";
        assert!(words_of(lines, &[]).is_none());
        assert_eq!(as_hinted_paths(lines, &crate::diagram::system_fonts(), 1.0), lines);
    }

    /// A machine with any font at all answers, whether or not it has one of
    /// the families asked for.
    #[test]
    fn a_face_is_found_for_the_labels_families() {
        let fonts = crate::diagram::system_fonts();
        if fonts.is_empty() {
            return;
        }
        let words = words_of(LABEL, &[]).unwrap();
        assert!(
            crate::measure::face_for(&fonts, &words.families, false).is_some(),
            "no face on this machine answers for {}, though it has {} faces",
            words.families,
            fonts.len()
        );
    }

    #[test]
    fn a_label_becomes_a_path() {
        let fonts = crate::diagram::system_fonts();
        if fonts.is_empty() {
            return;
        }
        let words = words_of(LABEL, &[]).unwrap();
        let id = crate::measure::face_for(&fonts, &words.families, false).expect("no face");
        let drawn = fonts
            .with_face_data(id, |data, index| outline(&words, data, index, 1.0))
            .expect("the face has no data");
        let path = drawn.expect("the words were not outlined");
        assert!(path.starts_with("<path d=\"M"), "{path}");
        assert!(path.ends_with("style=\"fill:#28253D;stroke:none\"/>"), "{path}");
    }
}
