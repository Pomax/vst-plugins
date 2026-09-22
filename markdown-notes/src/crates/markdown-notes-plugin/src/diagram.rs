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

/// The fonts the text in a diagram is drawn with: whatever the machine has.
pub fn system_fonts() -> Arc<fontdb::Database> {
    let mut fonts = fontdb::Database::new();
    fonts.load_system_fonts();
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
    rasterise(&svg, look, fonts)
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
        "look": "neo",
        "flowchart": { "curve": "linear", "rankSpacing": STATE_RANK_SPACING },
        "state": { "nodeSpacing": STATE_NODE_SPACING, "rankSpacing": STATE_RANK_SPACING },
    }));
    let svg = merman::render::HeadlessRenderer::new()
        .with_site_config(config)
        .render_svg_sync(code)
        .ok()??;

    // The edges first. What makes the SVG safe for resvg writes each label
    // out again as plain text wherever the label is by then, so a label moved
    // afterwards would leave its words behind.
    let widened = crate::node_widths::widen_the_states(&svg);
    let squared = crate::elbows::square_the_edges(&widened);
    let safe = merman::render::SvgPipeline::resvg_safe()
        .process_to_string(&squared)
        .ok()?;
    Some(with_solid_labels(&without_its_own_page(&safe), look.background))
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
            Shown {
                texture: ctx.load_texture("mermaid", image, egui::TextureOptions::LINEAR),
                size: egui::vec2(picture.points[0], picture.points[1]),
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
        let svg = svg("stateDiagram\n  a --> b : through", look()).expect("no SVG");
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
