//! Mermaid code, drawn as a picture.
//!
//! `merman` turns the code into SVG and `resvg` turns the SVG into pixels.
//! Both are slow next to a frame, so a picture is drawn once and kept for as
//! long as something on screen still asks for it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use resvg::usvg::fontdb;
use resvg::{tiny_skia, usvg};

/// Widest and tallest a picture is ever rasterised, in pixels. A diagram that
/// lays out larger than this is scaled down to fit rather than refused.
const MAX_SIDE: f32 = 4096.0;

/// What a picture is drawn to match.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Look {
    pub dark: bool,
    /// The page the picture sits on, which becomes its own background.
    pub background: [u8; 3],
    /// Pixels per point, in thousandths, so a picture is sharp on the screen
    /// it is shown on and redrawn when that changes.
    pub scale_milli: u32,
}

impl Look {
    pub fn new(dark: bool, background: [u8; 3], pixels_per_point: f32) -> Look {
        Look {
            dark,
            background,
            scale_milli: (pixels_per_point.max(0.1) * 1000.0).round() as u32,
        }
    }

    fn scale(&self) -> f32 {
        self.scale_milli as f32 / 1000.0
    }
}

/// A rasterised diagram: premultiplied RGBA, and how large it is in points.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub points: [f32; 2],
}

/// The family every diagram is lettered in.
pub const FONT_FAMILY: &str = "Arial";

/// The fonts the text in a diagram is drawn with: the machine's Arial faces
/// and nothing else, standing in for every generic family too.
pub fn system_fonts() -> Arc<fontdb::Database> {
    let mut fonts = fontdb::Database::new();
    fonts.load_system_fonts();
    let others: Vec<fontdb::ID> = fonts
        .faces()
        .filter(|face| !face.families.iter().any(|(name, _)| name == FONT_FAMILY))
        .map(|face| face.id)
        .collect();
    for id in others {
        fonts.remove_face(id);
    }
    fonts.set_serif_family(FONT_FAMILY);
    fonts.set_sans_serif_family(FONT_FAMILY);
    fonts.set_monospace_family(FONT_FAMILY);
    fonts.set_cursive_family(FONT_FAMILY);
    fonts.set_fantasy_family(FONT_FAMILY);
    Arc::new(fonts)
}

/// Space between two states side by side. Mermaid's default is 50, measured
/// between the states as merman sizes them, which is before
/// [`crate::node_widths`] widens the narrow ones into it.
const STATE_NODE_SPACING: f64 = 50.0 + crate::node_widths::MIN_WIDTH - NARROWEST_STATE;

/// About how wide merman draws a state with a name one character long.
const NARROWEST_STATE: f64 = 30.0;

/// What merman is asked to leave between one row and the next. It adds the
/// height of a label to that, 21, which comes to what mermaid.live leaves
/// between the bottom of one row and the top of the next: 93 where a state is
/// drawn 105 wide, so 106 at a state's real width of 120.
const STATE_RANK_SPACING: f64 = 85.0;

/// What stands in for a block mermaid could not draw.
pub const ERROR_TEXT: &str = "error in mermaid code";

/// Draw `code`, or nothing when it is not a diagram mermaid understands.
///
/// A panic inside the renderer counts as not understanding it. That is only
/// something this can see in a build that unwinds: the release profile aborts
/// on panic, and there a panic ends the process before it gets back here.
pub fn draw(code: &str, look: Look, fonts: &Arc<fontdb::Database>) -> Option<Picture> {
    if code.trim().is_empty() {
        return None;
    }

    let svg = std::panic::catch_unwind(|| svg(code, look)).ok()??;
    // The words last, hinted for the scale the picture is drawn at: resvg
    // would draw them unhinted.
    let lettered = crate::words::as_hinted_paths(&svg, fonts, look.scale());
    rasterise(&lettered, look, fonts)
}

/// `code` as SVG, in the light or the dark theme `look` asks for, with the
/// edges squared off where [`crate::elbows`] can place them and the labels on
/// them solid.
///
/// Flowcharts get straight edges by default. A block's own frontmatter is
/// read after this and wins.
pub fn svg(code: &str, look: Look) -> Option<String> {
    let dark = look.dark;
    // The theme and look Mermaid 12 gives a state diagram when it is told
    // nothing, from `StateDiagramConfig` in its `config.schema.yaml`. merman
    // follows Mermaid 11, whose own default is the older lilac one.
    let config = merman::config::MermaidConfig::from_value(serde_json::json!({
        "theme": if dark { "redux-dark-color" } else { "redux-color" },
        "fontFamily": FONT_FAMILY,
        "look": "neo",
        "flowchart": { "curve": "linear", "rankSpacing": STATE_RANK_SPACING },
        "state": { "nodeSpacing": STATE_NODE_SPACING, "rankSpacing": STATE_RANK_SPACING },
    }));
    let svg = merman::render::HeadlessRenderer::new()
        .with_site_config(config)
        // Measure with the fonts resvg will draw the SVG in, so a label never
        // overflows the box laid out for it.
        .with_text_measurer(crate::measure::shared())
        .render_svg_sync(code)
        .ok()??;

    // The edges first. What makes the SVG safe for resvg writes each label
    // out again as plain text wherever the label is by then, so a label moved
    // afterwards would leave its words behind.
    let widened = crate::node_widths::widen_the_states(&svg);
    let squared = if runs_down_the_page(code) {
        crate::elbows::square_the_edges(&widened)
    } else {
        widened
    };
    let safe = merman::render::SvgPipeline::resvg_safe()
        .process_to_string(&squared)
        .ok()?;
    let solid = with_solid_labels(&without_its_own_page(&safe), look.background);
    Some(on_the_pixel_grid(&with_padded_labels(&solid)))
}

/// Put the picture on the pixel grid before it is rasterised.
///
/// merman lays nodes, edges and words out at fractions of a pixel, and resvg
/// draws them anti-aliased there: a two pixel border or edge becomes three
/// rows, two of them grey. Every placement is rounded to a whole pixel, and a
/// node's box is made an even number of pixels wide and tall, so its two pixel
/// border, centred on the box's edge, covers two pixels exactly. Diagonal runs
/// stay anti-aliased, which is how a diagonal is drawn.
/// The grid is the SVG's, so the picture is crisp at a whole number of pixels
/// per point.
fn on_the_pixel_grid(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find('<') {
        let Some(len) = rest[at..].find('>') else {
            break;
        };
        out.push_str(&rest[..at]);
        out.push_str(&snapped(&rest[at..at + len + 1]));
        rest = &rest[at + len + 1..];
    }
    out.push_str(rest);
    out
}

/// One tag of the SVG with its placement rounded to whole pixels.
fn snapped(tag: &str) -> String {
    let name = tag[1..]
        .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .next()
        .unwrap_or("");
    let round = |n: f64| n.round();
    let placed = with_translate(tag, round);
    match name {
        "rect" if tag.contains("label-container") => centred_box(&placed),
        "rect" | "text" | "circle" | "line" => {
            ["x", "y", "width", "height", "cx", "cy", "x1", "y1", "x2", "y2"]
                .iter()
                .fold(placed, |tag, name| with_number(&tag, name, round))
        }
        "path" if tag.contains("data-edge=\"true\"") => with_path_rounded(&placed),
        _ => placed,
    }
}

/// A node's box, which sits centred on the node's origin with a two pixel
/// border: an even number of pixels wide and tall, and still centred, so its
/// edges fall on whole pixels and the border fills two.
fn centred_box(tag: &str) -> String {
    let spaced = format!(" {tag}");
    let number = |name: &str| attribute(&spaced, name)?.parse::<f64>().ok();
    let (Some(x), Some(y), Some(w), Some(h)) = (number("x"), number("y"), number("width"), number("height")) else {
        return tag.to_string();
    };
    if (x + w / 2.0).abs() > 0.01 || (y + h / 2.0).abs() > 0.01 {
        return ["x", "y", "width", "height"]
            .iter()
            .fold(tag.to_string(), |tag, name| with_number(&tag, name, f64::round));
    }
    let even = |n: f64| 2.0 * (n / 2.0).round();
    let (w, h) = (even(w), even(h));
    let tag = with_number(tag, "width", |_| w);
    let tag = with_number(&tag, "height", |_| h);
    let tag = with_number(&tag, "x", |_| -w / 2.0);
    with_number(&tag, "y", |_| -h / 2.0)
}

/// The value of `name="..."` in `tag`, which starts with a space.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    let len = tag[at..].find('"')?;
    Some(&tag[at..at + len])
}

/// `tag` with the two numbers of its `translate(x, y)` put through `f`.
fn with_translate(tag: &str, f: impl Fn(f64) -> f64) -> String {
    const OPENING: &str = "translate(";
    let Some(at) = tag.find(OPENING).map(|at| at + OPENING.len()) else {
        return tag.to_string();
    };
    let Some(len) = tag[at..].find(')') else {
        return tag.to_string();
    };
    let numbers: Option<Vec<f64>> = tag[at..at + len]
        .split(',')
        .map(|n| n.trim().parse::<f64>().ok())
        .collect();
    match numbers.as_deref() {
        Some([x, y]) => format!("{}{},{}{}", &tag[..at], f(*x), f(*y), &tag[at + len..]),
        _ => tag.to_string(),
    }
}

/// `tag` with every number in its `d` rounded to a whole pixel. The arcs'
/// radii and flags are whole already and stay so.
fn with_path_rounded(tag: &str) -> String {
    let spaced = format!(" {tag}");
    let Some(d) = attribute(&spaced, "d") else {
        return tag.to_string();
    };
    let mut rounded = String::with_capacity(d.len());
    let mut number = String::new();
    let flush = |number: &mut String, rounded: &mut String| {
        if let Ok(n) = number.parse::<f64>() {
            rounded.push_str(&n.round().to_string());
        } else {
            rounded.push_str(number);
        }
        number.clear();
    };
    for c in d.chars() {
        if c.is_ascii_digit() || c == '.' || (c == '-' && number.is_empty()) {
            number.push(c);
        } else {
            flush(&mut number, &mut rounded);
            rounded.push(c);
        }
    }
    flush(&mut number, &mut rounded);
    tag.replacen(&format!("d=\"{d}\""), &format!("d=\"{rounded}\""), 1)
}

/// `tag` with the number in its `name` attribute put through `f`.
fn with_number(tag: &str, name: &str, f: impl Fn(f64) -> f64) -> String {
    let spaced = format!(" {tag}");
    let key = format!(" {name}=\"");
    let Some(at) = spaced.find(&key).map(|at| at + key.len()) else {
        return tag.to_string();
    };
    let Some(len) = spaced[at..].find('"') else {
        return tag.to_string();
    };
    match spaced[at..at + len].parse::<f64>() {
        Ok(n) => format!("{}{}{}", &spaced[1..at], f(n), &spaced[at + len..]),
        Err(_) => tag.to_string(),
    }
}

/// Room between a label's words and the edge of the box behind them: left
/// and right, above, and below.
const LABEL_PADDING: (f64, f64, f64) = (5.0, 2.0, 4.0);

/// How far the corners of the box behind a label are rounded.
const LABEL_CORNER: f64 = 4.0;

/// Grow the box behind every edge label by [`LABEL_PADDING`] on each side,
/// and round its corners by [`LABEL_CORNER`].
///
/// merman draws the box exactly round the words, with square corners.
fn with_padded_labels(svg: &str) -> String {
    const BOX: &str = "labelBkg\"><rect ";
    let (across, above, below) = LABEL_PADDING;
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find(BOX) {
        let start = at + BOX.len();
        let Some(len) = rest[start..].find("/>") else {
            break;
        };
        let padded = [("x", -across), ("y", -above), ("width", 2.0 * across), ("height", above + below)]
            .into_iter()
            .fold(rest[start..start + len].to_string(), |tag, (name, by)| moved(&tag, name, by));
        out.push_str(&rest[..start]);
        out.push_str(&padded);
        out.push_str(&format!(" rx=\"{LABEL_CORNER}\" ry=\"{LABEL_CORNER}\""));
        rest = &rest[start + len..];
    }
    out.push_str(rest);
    out
}

/// `tag` with the number in its `name` attribute moved by `by`.
fn moved(tag: &str, name: &str, by: f64) -> String {
    with_number(tag, name, |n| n + by)
}

/// Whether the diagram's rows run down the page, which is the only way
/// [`crate::elbows`] can route an edge: out of the bottom of one node and into
/// the top of the next. A flowchart declared `LR` or `RL` runs across the page,
/// and so does a state diagram with a `direction LR` or `direction RL` line;
/// their edges keep the paths merman drew.
fn runs_down_the_page(code: &str) -> bool {
    let mut lines = code
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
        .peekable();
    // A frontmatter block is settings, not the diagram.
    if lines.peek() == Some(&"---") {
        lines.next();
        for line in lines.by_ref() {
            if line == "---" {
                break;
            }
        }
    }
    let across = |direction: Option<&str>| matches!(direction, Some("LR" | "RL"));
    let Some(header) = lines.next() else {
        return true;
    };
    let mut words = header.split_whitespace();
    if matches!(words.next(), Some("flowchart" | "graph")) {
        return !across(words.next());
    }
    !lines.any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some("direction") && across(words.next())
    })
}

/// Make the box behind every edge label opaque.
///
/// Mermaid fills it with a half transparent grey, which a browser draws over
/// a page and nothing else. Here the edge runs through the label, so the line
/// shows through the words. The same grey, mixed with the page as it would
/// have been, and solid.
fn with_solid_labels(svg: &str, page: [u8; 3]) -> String {
    const BOX: &str = "labelBkg\"><rect ";
    const FILL: &str = "fill=\"rgba(";
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;

    while let Some(at) = rest.find(BOX) {
        let tag_end = at + rest[at..].find("/>").unwrap_or(rest.len() - at);
        let fill = rest[at..tag_end].find(FILL).map(|f| at + f + FILL.len());
        let mixed = fill.and_then(|from| {
            let len = rest[from..tag_end].find(')')?;
            let parts: Vec<f32> = rest[from..from + len]
                .split(',')
                .map(|n| n.trim().parse::<f32>())
                .collect::<Result<_, _>>()
                .ok()?;
            let [r, g, b, a] = parts[..] else {
                return None;
            };
            let over = |top: f32, under: u8| (top * a + f32::from(under) * (1.0 - a)).round() as u8;
            let solid = format!("rgb({}, {}, {})", over(r, page[0]), over(g, page[1]), over(b, page[2]));
            // In a style as well as the attribute: the stylesheet merman
            // writes has a rule making every label's box half transparent,
            // and a rule outranks an attribute.
            Some((
                from - "fill=\"rgba(".len(),
                from + len + 2,
                format!("fill=\"{solid}\" style=\"fill:{solid};opacity:1\""),
            ))
        });
        match mixed {
            Some((from, to, solid)) => {
                out.push_str(&rest[..from]);
                out.push_str(&solid);
                rest = &rest[to..];
            }
            None => {
                out.push_str(&rest[..tag_end]);
                rest = &rest[tag_end..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Drop the background merman gives the picture, which is white whatever the
/// theme, so that it sits on the page it is drawn onto.
fn without_its_own_page(svg: &str) -> String {
    const PAGE: &str = "background-color:";
    let root = svg.find('>').unwrap_or(svg.len());
    let Some(at) = svg[..root].find(PAGE) else {
        return svg.to_string();
    };
    let len = svg[at..root]
        .find([';', '"'])
        .map_or(root - at, |end| end + usize::from(svg[at..].as_bytes()[end] == b';'));
    format!("{}{}", &svg[..at], &svg[at + len..])
}

/// The picture shown in place of a block that could not be drawn: a box on
/// the page, saying [`ERROR_TEXT`].
pub fn draw_error(look: Look, fonts: &Arc<fontdb::Database>) -> Option<Picture> {
    let [r, g, b] = look.background;
    let (ink, edge) = if look.dark { ("#ff8a80", "#b04a44") } else { ("#b3261e", "#d9827c") };
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"240\" height=\"44\" viewBox=\"0 0 240 44\">\
         <rect width=\"240\" height=\"44\" fill=\"#{r:02x}{g:02x}{b:02x}\"/>\
         <rect x=\"1\" y=\"1\" width=\"238\" height=\"42\" rx=\"4\" fill=\"none\" stroke=\"{edge}\"/>\
         <text x=\"120\" y=\"27\" font-family=\"sans-serif\" font-size=\"15\" text-anchor=\"middle\" \
         fill=\"{ink}\">{ERROR_TEXT}</text>\
         </svg>"
    );
    rasterise(&svg, look, fonts)
}

/// Turn SVG into pixels, at the scale `look` asks for.
pub fn rasterise(svg: &str, look: Look, fonts: &Arc<fontdb::Database>) -> Option<Picture> {
    let mut reading = usvg::Options::default();
    reading.fontdb = Arc::clone(fonts);
    let tree = usvg::Tree::from_str(svg, &reading).ok()?;

    let size = tree.size();
    let (wide, tall) = (size.width(), size.height());
    if !(wide > 0.0 && tall > 0.0) {
        return None;
    }
    let scale = look.scale().min(MAX_SIDE / wide).min(MAX_SIDE / tall);
    let width = (wide * scale).ceil().max(1.0) as u32;
    let height = (tall * scale).ceil().max(1.0) as u32;

    let mut pixels = tiny_skia::Pixmap::new(width, height)?;
    let [r, g, b] = look.background;
    pixels.fill(tiny_skia::Color::from_rgba8(r, g, b, 255));
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixels.as_mut(),
    );

    Some(Picture {
        width: width as usize,
        height: height as usize,
        rgba: pixels.take(),
        points: [wide, tall],
    })
}

/// A picture as egui holds it.
#[derive(Clone)]
pub struct Shown {
    pub texture: egui::TextureHandle,
    /// Its natural size, in points.
    pub size: egui::Vec2,
    /// Whether this is the error picture rather than the diagram.
    pub failed: bool,
}

/// Every picture currently on screen, keyed by the code it was drawn from.
///
/// Code that fails to draw is remembered with the error picture it got, so a
/// block mermaid cannot read is tried once rather than once a frame.
#[derive(Default)]
pub struct Gallery {
    fonts: Option<Arc<fontdb::Database>>,
    pictures: HashMap<(String, Look), Option<Shown>>,
    asked_for: HashSet<(String, Look)>,
}

impl Gallery {
    /// The picture for `code`, drawn now if this is the first time it is asked
    /// for.
    pub fn picture(&mut self, ctx: &egui::Context, code: &str, look: Look) -> Option<Shown> {
        let key = (code.to_string(), look);
        self.asked_for.insert(key.clone());
        if let Some(known) = self.pictures.get(&key) {
            return known.clone();
        }

        let fonts = self.fonts.get_or_insert_with(system_fonts);
        let (picture, failed) = match draw(code, look, fonts) {
            Some(picture) => (Some(picture), false),
            None => (draw_error(look, fonts), true),
        };
        let shown = picture.map(|picture| {
            let image = egui::ColorImage::from_rgba_premultiplied(
                [picture.width, picture.height],
                &picture.rgba,
            );
            // Sized by its texels, not by the SVG: drawn at this size, every
            // texel lands on one pixel. The SVG's own size is a fraction of a
            // pixel smaller, and drawing it at that resamples the whole picture.
            Shown {
                texture: ctx.load_texture("mermaid", image, egui::TextureOptions::LINEAR),
                size: egui::vec2(picture.width as f32, picture.height as f32) / look.scale(),
                failed,
            }
        });
        self.pictures.insert(key, shown.clone());
        shown
    }

    /// Forget whatever nothing asked for since the last call.
    pub fn end_frame(&mut self) {
        let asked_for = std::mem::take(&mut self.asked_for);
        self.pictures.retain(|key, _| asked_for.contains(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOWCHART: &str = "flowchart LR\n    A[Start] --> B[End]";

    fn look() -> Look {
        Look::new(false, [255, 255, 255], 1.0)
    }

    /// A node's box is laid out from what the text measures, so a longer
    /// label has to produce a wider box.
    ///
    /// The measurement comes from shaping the label in the font the SVG will
    /// be drawn in. A measurer that returned a constant, or zero, would still
    /// draw a picture, and every other check here would still pass, while
    /// every label overflowed its box.
    #[test]
    fn a_longer_label_is_given_a_wider_box() {
        let width_of = |label: &str| -> f64 {
            let svg = svg(&format!("flowchart LR\n    A[{label}]"), look())
                .expect("no SVG was made");
            let at = svg.find("<rect").expect("the node has no box");
            let tag = &svg[at..];
            let tag = &tag[..tag.find('>').expect("an unclosed rect")];
            crate::elbows::attribute(tag, "width")
                .and_then(|w| w.trim_end_matches("px").parse().ok())
                .unwrap_or_else(|| panic!("no width in {tag}"))
        };

        let short = width_of("Hi");
        let long = width_of("A considerably longer label than that one");
        assert!(
            long > short * 2.0,
            "a label twenty times longer got a box {long} wide against {short}"
        );
    }

    #[test]
    fn a_flowchart_becomes_pixels() {
        let picture = draw(FLOWCHART, look(), &system_fonts()).expect("nothing was drawn");
        assert!(picture.width > 0 && picture.height > 0);
        assert_eq!(picture.rgba.len(), picture.width * picture.height * 4);
        let page = [255u8, 255, 255, 255];
        assert!(
            picture.rgba.chunks_exact(4).any(|p| p != page),
            "the picture is a blank page"
        );
    }

    #[test]
    fn the_picture_sits_on_the_page_it_was_given() {
        let look = Look::new(true, [10, 20, 30], 1.0);
        let picture = draw(FLOWCHART, look, &system_fonts()).expect("nothing was drawn");
        assert_eq!(&picture.rgba[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn a_sharper_screen_gets_more_pixels_and_the_same_size() {
        let fonts = system_fonts();
        let one = draw(FLOWCHART, Look::new(false, [255; 3], 1.0), &fonts).unwrap();
        let two = draw(FLOWCHART, Look::new(false, [255; 3], 2.0), &fonts).unwrap();
        assert_eq!(one.points, two.points);
        assert!(two.width > one.width && two.height > one.height);
    }

    #[test]
    fn the_box_behind_a_label_is_made_solid() {
        let svg = "<g class=\"edgeLabel label labelBkg\">\
                   <rect x=\"1\" fill=\"rgba(232, 232, 232, 0.5)\"/><text>a</text></g>\
                   <rect fill=\"rgba(1, 2, 3, 0.5)\"/>";
        let solid = with_solid_labels(svg, [0, 0, 0]);
        assert!(
            solid.contains(
                "<rect x=\"1\" fill=\"rgb(116, 116, 116)\" \
                 style=\"fill:rgb(116, 116, 116);opacity:1\"/>"
            ),
            "{solid}"
        );
        assert!(
            solid.contains("<rect fill=\"rgba(1, 2, 3, 0.5)\"/>"),
            "a box that is not a label's was changed: {solid}"
        );
    }

    /// The edge runs down through the middle of its label. In the row of
    /// pixels just inside the top of the label's box, above the words, the
    /// middle one has to be the box and not the line.
    #[test]
    fn an_edge_does_not_show_through_its_label() {
        let svg = svg("flowchart TD\n  a -->|through| b", look()).expect("no SVG");
        let number = |tag: &str, name: &str| -> f32 {
            crate::elbows::attribute(tag, name)
                .and_then(|n| n.split_whitespace().next()?.parse().ok())
                .unwrap_or_else(|| panic!("no {name} in {tag}"))
        };
        let root = &svg[..svg.find('>').unwrap()];
        let left_of_picture = number(root, "viewBox");
        let top_of_picture: f32 = crate::elbows::attribute(root, "viewBox")
            .and_then(|view| view.split_whitespace().nth(1)?.parse().ok())
            .expect("the picture has no viewBox");
        let at = svg.find("labelBkg\"><rect ").expect("the label has no box");
        let tag = &svg[at + "labelBkg\">".len()..];
        let tag = &tag[..tag.find("/>").unwrap()];
        let (x, y, wide) = (number(tag, "x"), number(tag, "y"), number(tag, "width"));

        let picture = rasterise(&svg, look(), &system_fonts()).expect("no picture");
        let column = (x + wide / 2.0 - left_of_picture).round() as usize;
        let row = (y - top_of_picture).round() as usize + 2;
        let pixel = &picture.rgba[(row * picture.width + column) * 4..][..3];
        assert!(
            pixel.iter().all(|c| *c > 200),
            "the pixel at ({column}, {row}), inside the label's box and above its words, \
             is {pixel:?}: the edge shows through"
        );
    }

    #[test]
    fn no_code_is_no_picture() {
        assert!(draw("", look(), &system_fonts()).is_none());
        assert!(draw("  \n ", look(), &system_fonts()).is_none());
    }

    #[test]
    fn words_that_are_not_a_diagram_are_no_picture() {
        assert!(draw("this is not a diagram", look(), &system_fonts()).is_none());
    }

    #[test]
    fn the_error_picture_has_something_written_on_it() {
        let picture = draw_error(look(), &system_fonts()).expect("no error picture");
        let page = [255u8, 255, 255, 255];
        let inked = picture.rgba.chunks_exact(4).filter(|p| *p != page).count();
        // The box's outline alone is about 560 pixels; the words add to that.
        assert!(inked > 800, "only {inked} pixels of the error picture are drawn on");
    }
}
