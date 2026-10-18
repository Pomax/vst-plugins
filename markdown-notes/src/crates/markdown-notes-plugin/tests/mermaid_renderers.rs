//! One state diagram, drawn by each mermaid renderer, and the pictures kept
//! beside the UI tests' screenshots so the two can be looked at side by side.

use markdown_notes_plugin::diagram::{draw, rasterise, system_fonts, Look, Picture};

/// Thirty transitions, every one of them labelled, several of them crossing
/// more than one rank.
const STATES: &str = "stateDiagram
  1 --> 2 : 2,2 (0.5)
  1 --> 10 : 2,3,4 (0)
  1 --> 12 : 2,2,2 (2)
  2 --> 3 : 2,3,4 (0)
  10 --> 3 : 2,2 (0.5)
  10 --> 11 : 2,3,4 (0)
  12 --> 13 : 3,3 (0.5)
  12 --> 17 : 3,4,5 (0)
  12 --> 19 : 3,3,3 (2)
  3 --> 4 : 3,3 (0.5)
  3 --> 9 : 3,4,5 (0)
  13 --> 14 : 3,4,5 (0)
  11 --> 5 : 2,3,4 (0)
  17 --> 14 : 3,3 (0.5)
  17 --> 18 : 3,4,5 (0)
  19 --> 20 : 4,4 (0.5)
  19 --> 23 : 4,5,6 (0)
  19 --> 5 : 4,4,4 (2)
  4 --> 5 : 4,4 (0.5)
  4 --> 8 : 4,5,6 (0)
  9 --> 6 : 3,4,5 (0)
  14 --> 15 : 4,4 (0.5)
  14 --> 16 : 4,5,6 (0)
  20 --> 21 : 4,5,6 (0)
  5 --> 6 : 5,5 (0.5)
  5 --> 7 : 5,5,5 (2)
  18 --> 7 : 3,4,5 (0)
  23 --> 21 : 4,4 (0.5)
  15 --> 7 : 5,5 (0.5)
  21 --> 22 : 5,5 (0.5)
  22 --> [*]
";

fn look() -> Look {
    Look::new(false, [255, 255, 255], 1.0)
}

/// Write the picture out over a white page, and say where it went.
fn saved(picture: &Picture, name: &str) -> String {
    let mut pixels = picture.rgba.clone();
    for p in pixels.chunks_exact_mut(4) {
        let clear = 255 - p[3];
        for channel in &mut p[..3] {
            *channel = channel.saturating_add(clear);
        }
        p[3] = 255;
    }
    let image =
        image::RgbaImage::from_raw(picture.width as u32, picture.height as u32, pixels)
            .expect("the picture is not the size it says it is");

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

fn inked(picture: &Picture) -> usize {
    picture
        .rgba
        .chunks_exact(4)
        .filter(|p| p[3] != 0 && p[..3] != [255, 255, 255])
        .count()
}

/// On a dark page the lines and the words have to be light, or they are not
/// there to be read.
#[test]
fn the_editor_draws_the_flowchart_on_a_dark_page() {
    let page = [27u8, 27, 27];
    let picture = draw(&as_a_flowchart(), Look::new(true, page, 1.0), &system_fonts())
        .expect("nothing was drawn");
    println!(
        "the editor, dark: {}x{} at {}",
        picture.width,
        picture.height,
        saved(&picture, "flowchart-editor-dark")
    );
    let light = picture
        .rgba
        .chunks_exact(4)
        .filter(|p| p[..3].iter().all(|c| *c > 170))
        .count();
    assert!(light > 1000, "only {light} light pixels on the dark page");
}

/// mermaid-rs-renderer draws right-angled edges by itself. What it lacks at
/// its defaults is room, so this is the same diagram with room given to it.
#[test]
fn mermaid_rs_renderer_draws_the_state_diagram_with_room() {
    let fonts = system_fonts();
    for (node, rank) in [(80.0, 100.0), (120.0, 120.0), (160.0, 140.0)] {
        let options = mermaid_rs_renderer::RenderOptions::default()
            .with_node_spacing(node)
            .with_rank_spacing(rank);
        let svg = mermaid_rs_renderer::render_with_options(STATES, options)
            .expect("mermaid-rs-renderer failed");
        let picture = rasterise(&svg, look(), &fonts).expect("the SVG did not rasterise");
        println!(
            "mermaid-rs-renderer, {node}/{rank}: {}x{} at {}",
            picture.width,
            picture.height,
            saved(&picture, &format!("states-mermaid-rs-renderer-{node}-{rank}"))
        );
        assert!(inked(&picture) > 1000);
    }
}

/// The same, asked for the way a diagram asks for it: in its own frontmatter,
/// and as a `stateDiagram-v2`, and as a flowchart.
#[test]
fn mermaid_rs_renderer_draws_the_diagram_with_room_asked_for_in_the_text() {
    const ROOM: &str = "---\nconfig:\n  flowchart:\n    nodeSpacing: 120\n    rankSpacing: 120\n  \
                        state:\n    nodeSpacing: 120\n    rankSpacing: 120\n---\n";
    let fonts = system_fonts();
    let texts = [
        ("state", format!("{ROOM}{STATES}")),
        ("state-v2", format!("{ROOM}{}", STATES.replacen("stateDiagram", "stateDiagram-v2", 1))),
        ("flowchart", format!("{ROOM}{}", as_a_flowchart())),
    ];
    for (name, text) in texts {
        let options = mermaid_rs_renderer::RenderOptions::default()
            .with_node_spacing(120.0)
            .with_rank_spacing(120.0);
        let svg = mermaid_rs_renderer::render_with_options(&text, options)
            .unwrap_or_else(|e| panic!("mermaid-rs-renderer failed on {name}: {e}"));
        let picture = rasterise(&svg, look(), &fonts).expect("the SVG did not rasterise");
        println!(
            "mermaid-rs-renderer, {name}: {}x{} at {}",
            picture.width,
            picture.height,
            saved(&picture, &format!("room-mermaid-rs-renderer-{name}"))
        );
        assert!(inked(&picture) > 1000);
    }
}

#[test]
fn mermaid_rs_renderer_draws_the_state_diagram() {
    let svg = mermaid_rs_renderer::render(STATES).expect("mermaid-rs-renderer failed");
    let picture = rasterise(&svg, look(), &system_fonts()).expect("the SVG did not rasterise");
    println!(
        "mermaid-rs-renderer: {}x{} at {}",
        picture.width,
        picture.height,
        saved(&picture, "states-mermaid-rs-renderer")
    );
    assert!(inked(&picture) > 1000);
}

/// The edge shapes mermaid's `curve` setting names, each drawn by merman and
/// kept, so the one that reads best can be picked by looking.
/// What Mermaid 12 uses when it is given no configuration, which is what
/// mermaid.live gives it. merman 0.7.0 follows Mermaid 11, so this says how
/// much of it merman knows.
#[test]
fn merman_draws_the_flowchart_with_the_settings_mermaid_12_defaults_to() {
    let config = merman::config::MermaidConfig::from_value(serde_json::json!({
        "theme": "redux-color",
        "look": "neo",
        "layout": "elk",
        "flowchart": {
            "theme": "redux-color",
            "look": "neo",
            "layout": "elk",
            "wrappingWidth": 120,
        },
    }));
    let svg = merman::render::HeadlessRenderer::new()
        .with_site_config(config)
        .render_svg_resvg_safe_sync(&as_a_flowchart())
        .unwrap_or_else(|e| panic!("merman could not render: {e}"))
        .expect("merman did not see a diagram");
    let picture = rasterise(&svg, look(), &system_fonts()).expect("the SVG did not rasterise");
    println!(
        "merman, mermaid 12 defaults: {}x{} at {}; themes it knows: {:?}",
        picture.width,
        picture.height,
        saved(&picture, "flowchart-merman-v12-defaults"),
        merman::supported_themes(),
    );
    assert!(inked(&picture) > 1000);
}

/// The same graph written as a flowchart, which is the diagram type whose
/// edge shape merman reads from `flowchart.curve`.
fn as_a_flowchart() -> String {
    let mut chart = String::from("flowchart TD\n");
    for line in STATES.lines().skip(1) {
        let Some((ends, label)) = line.split_once(" : ") else {
            continue;
        };
        let Some((from, to)) = ends.trim().split_once(" --> ") else {
            continue;
        };
        chart.push_str(&format!("  {from} -->|\"{label}\"| {to}\n"));
    }
    chart
}

#[test]
fn the_editor_draws_the_flowchart() {
    let svg = markdown_notes_plugin::diagram::svg(&as_a_flowchart(), look()).expect("no SVG");
    let kept = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cache/uitests/flowchart-editor.svg");
    let _ = std::fs::write(kept, &svg);

    let picture = draw(&as_a_flowchart(), look(), &system_fonts()).expect("nothing was drawn");
    println!(
        "the editor, flowchart: {}x{} at {}",
        picture.width,
        picture.height,
        saved(&picture, "flowchart-editor")
    );
    assert!(inked(&picture) > 1000);
}

#[test]
fn merman_draws_the_flowchart_with_each_edge_shape() {
    let fonts = system_fonts();
    let chart = as_a_flowchart();
    for curve in ["basis", "linear", "step", "stepBefore", "stepAfter"] {
        let config = merman::config::MermaidConfig::from_value(serde_json::json!({
            "flowchart": { "curve": curve },
        }));
        let svg = merman::render::HeadlessRenderer::new()
            .with_site_config(config)
            .render_svg_resvg_safe_sync(&chart)
            .unwrap_or_else(|e| panic!("merman could not render with {curve}: {e}"))
            .expect("merman did not see a diagram");
        let picture = rasterise(&svg, look(), &fonts).expect("the SVG did not rasterise");
        println!(
            "merman flowchart, {curve}: {}x{} at {}",
            picture.width,
            picture.height,
            saved(&picture, &format!("flowchart-merman-{curve}"))
        );
        assert!(inked(&picture) > 1000, "{curve} drew nothing");
    }
}

/// The chart from issue 18: a left-to-right flowchart with a labelled edge
/// that drops to a lower row.
const ACROSS: &str = "flowchart LR\n  A[EQ: profile 7]\n  B[Sat: Saturn2]\n  C[Dist: Decapitator]\n  D[MTurboComp]\n  E[Bassline]\n  E2[Out]\n  A --> B\n  A --> |sidechain| E\n  E --> E2\n  B --> C\n  B --> D\n  C --> |50%| D\n";

/// The value of `name="..."` in `tag`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let len = tag[at..].find('"')?;
    Some(&tag[at..at + len])
}

/// The two numbers of a `translate(x, y)`.
fn translated(transform: &str) -> Option<(f64, f64)> {
    let inner = transform.strip_prefix("translate(")?.strip_suffix(')')?;
    let mut parts = inner.split(',').map(|n| n.trim().parse::<f64>().ok());
    Some((parts.next()??, parts.next()??))
}

/// Every number in a path's `d`, in order.
fn numbers(d: &str) -> Vec<f64> {
    d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .filter_map(|n| n.parse().ok())
        .collect()
}

/// The box of the flowchart node called `name` in the code: left, top,
/// right, bottom. The node's text is written elsewhere in the SVG, so the
/// node is known by the id merman gives its group.
fn node_box(svg: &str, name: &str) -> (f64, f64, f64, f64) {
    let group = svg
        .split("<g class=\"node")
        .find(|group| {
            let tag = &group[..group.find('>').unwrap_or(group.len())];
            attr(tag, "id").is_some_and(|id| id.contains(&format!("-{name}-")))
        })
        .unwrap_or_else(|| panic!("no node called {name}"));
    let tag = &group[..group.find('>').expect("an unclosed node tag")];
    let (cx, cy) = attr(tag, "transform")
        .and_then(translated)
        .unwrap_or_else(|| panic!("the {name} node is not placed by a translate"));
    let rect = group.find("<rect ").map(|at| &group[at..]).expect("the node has no rectangle");
    let rect = &rect[..rect.find('>').expect("an unclosed rect")];
    let size = |name: &str| attr(rect, name).and_then(|n| n.parse::<f64>().ok()).expect(name);
    let (w, h) = (size("width"), size("height"));
    (cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0)
}

/// The points of the edge with `id`, from its `d`.
fn edge_points(svg: &str, id: &str) -> Vec<(f64, f64)> {
    let tag = svg
        .split("<path ")
        .map(|tag| format!(" {}", &tag[..tag.find('>').unwrap_or(tag.len())]))
        .find(|tag| attr(tag, "data-id") == Some(id))
        .unwrap_or_else(|| panic!("no edge {id}"));
    let d = attr(&tag, "d").expect("the edge has no d");
    numbers(d).chunks_exact(2).map(|pair| (pair[0], pair[1])).collect()
}

/// The markup of the label of the edge with `id`, from its group's attributes
/// on.
fn label_group<'a>(svg: &'a str, id: &str) -> &'a str {
    svg.split("<g class=\"edgeLabel\" ")
        .skip(1)
        .find(|group| {
            group
                .find("<g ")
                .map(|g| &group[g..])
                .and_then(|g| attr(&g[..g.find('>').unwrap_or(g.len())], "data-id"))
                == Some(id)
        })
        .unwrap_or_else(|| panic!("no label for {id}"))
}

/// Where the label of the edge with `id` is centred.
fn label_centre(svg: &str, id: &str) -> (f64, f64) {
    let group = label_group(svg, id);
    let tag = &group[..group.find('>').expect("an unclosed label tag")];
    attr(&format!(" {tag}"), "transform")
        .and_then(translated)
        .expect("the label is not placed by a translate")
}

/// Half the width and height of the words of the label of the edge with `id`:
/// the inner group is moved up and left by that much to centre them.
fn label_half(svg: &str, id: &str) -> (f64, f64) {
    let group = label_group(svg, id);
    let inner = group.find("<g ").map(|g| &group[g..]).expect("no inner group");
    let tag = &inner[..inner.find('>').expect("an unclosed inner tag")];
    let (x, y) = attr(tag, "transform")
        .and_then(translated)
        .expect("the words are not placed by a translate");
    (x.abs(), y.abs())
}

/// The `<rect` tag drawn behind the label of the edge with `id`, with a
/// space in front so every attribute can be found alike.
///
/// The box and the words are written apart from the label's group, in a
/// group of their own with the words at the label's centre, which is how the
/// box is known to be the label's.
fn label_rect(svg: &str, id: &str) -> String {
    let (cx, cy) = label_centre(svg, id);
    svg.split("labelBkg\"><rect ")
        .skip(1)
        .find_map(|rest| {
            let rect = &rest[..rest.find("/>")?];
            let text = rest.find("<text ").map(|at| &rest[at..])?;
            let text = &text[..text.find('>')?];
            let number = |tag: &str, name: &str| attr(tag, name)?.parse::<f64>().ok();
            let here = (number(text, "x")? - cx).abs() < 0.01 && (number(text, "y")? - cy).abs() < 0.01;
            here.then(|| format!(" {rect}"))
        })
        .unwrap_or_else(|| panic!("no box is drawn behind the label of {id}"))
}

/// The box drawn behind the label of the edge with `id`: x, y, width, height.
fn label_box(svg: &str, id: &str) -> (f64, f64, f64, f64) {
    let rect = label_rect(svg, id);
    let number = |name: &str| attr(&rect, name).and_then(|n| n.parse::<f64>().ok()).expect(name);
    (number("x"), number("y"), number("width"), number("height"))
}

/// The red channel of the pixel at `column`, `row` of `picture`.
fn red_at(picture: &Picture, column: usize, row: usize) -> u8 {
    picture.rgba[(row * picture.width + column) * 4]
}

/// A pixel that is not the white page the picture is drawn on.
fn marked(r: u8) -> bool {
    r < 250
}

/// Every line in the picture lands on whole pixels: a two pixel edge is two
/// rows of ink with nothing half drawn above or below, and a two pixel box
/// border is two columns of one ink with clean page beside them. Lines drawn
/// between pixels are smeared over an extra row or column of grey by the
/// anti-aliasing.
#[test]
fn the_pictures_lines_land_on_whole_pixels() {
    let svg = markdown_notes_plugin::diagram::svg(ACROSS, look()).expect("no SVG");
    let picture = draw(ACROSS, look(), &system_fonts()).expect("nothing was drawn");

    // The sidechain edge's last run, into Bassline, is horizontal: a column
    // through its middle crosses it once.
    let points = edge_points(&svg, "L_A_E_0");
    let (a, b) = (points[points.len() - 2], points[points.len() - 1]);
    assert_eq!(a.1, b.1, "the last run of the edge is not horizontal: {points:?}");
    let column = ((a.0 + b.0) / 2.0).round() as usize;
    let row = b.1.round() as usize;
    let rows: Vec<u8> = (row - 4..=row + 4).map(|y| red_at(&picture, column, y)).collect();
    let ink: Vec<u8> = rows.iter().copied().filter(|r| marked(*r)).collect();
    assert!(
        ink.len() == 2 && ink[0] == ink[1],
        "column {column} around row {row} reads {rows:?}: expected two rows of one ink and no grey beside them"
    );

    // Bassline's left border, crossed by a row a little below the node's top,
    // clear of the edge arriving at its middle: page up to the border, then
    // the border in two columns of one ink, centred on the box's edge.
    let (left, top, _, _) = node_box(&svg, "E");
    let row = (top + 6.0).round() as usize;
    let edge = left.round() as usize;
    let columns: Vec<u8> = (edge - 3..edge + 1).map(|x| red_at(&picture, x, row)).collect();
    assert!(
        columns[..2].iter().all(|r| !marked(*r)) && marked(columns[2]) && columns[2] == columns[3],
        "columns {}..={edge} at row {row} read {columns:?}: expected clean page and then the border in two columns",
        edge - 3
    );
}

/// The words in the picture are hinted: every `<text>` is replaced by a path
/// of glyphs fitted to the pixel grid, and the letters come out with fewer
/// half-covered grey pixels than resvg's unhinted lettering leaves.
#[test]
fn the_pictures_words_are_hinted() {
    let fonts = system_fonts();
    let svg = markdown_notes_plugin::diagram::svg(ACROSS, look()).expect("no SVG");
    let texts = svg.matches("<text ").count();
    assert!(texts > 0, "the SVG has no words");

    let lettered = markdown_notes_plugin::words::as_hinted_paths(&svg, &fonts, 1.0);
    assert!(
        !lettered.contains("<text "),
        "{} of the {texts} texts were left as text",
        lettered.matches("<text ").count()
    );

    let (left, top, right, bottom) = node_box(&svg, "E");
    // Inside the box, clear of its border.
    let (left, top, right, bottom) = (left as usize + 3, top as usize + 3, right as usize - 3, bottom as usize - 3);
    let greys = |picture: &Picture| {
        (left..right)
            .flat_map(|x| (top..bottom).map(move |y| (x, y)))
            .filter(|(x, y)| (70..=230).contains(&red_at(picture, *x, *y)))
            .count()
    };
    // All the ink in the box, a full pixel counting 255: hinting moves it
    // onto whole pixels and does not add to it. Letters drawn with a border,
    // or filled in, put far more on the page.
    let ink = |picture: &Picture| -> usize {
        (left..right)
            .flat_map(|x| (top..bottom).map(move |y| (x, y)))
            .map(|(x, y)| 255 - usize::from(red_at(picture, x, y)))
            .sum()
    };
    let before = rasterise(&svg, look(), &fonts).expect("the SVG did not rasterise");
    let after = rasterise(&lettered, look(), &fonts).expect("the lettered SVG did not rasterise");
    let (unhinted, hinted) = (greys(&before), greys(&after));
    println!("grey pixels in Bassline: {unhinted} unhinted, {hinted} hinted");
    assert!(
        hinted < unhinted,
        "the hinted words leave {hinted} grey pixels in Bassline, the unhinted {unhinted}"
    );
    let (was, is) = (ink(&before), ink(&after));
    assert!(
        is <= was * 12 / 10 && is >= was * 8 / 10,
        "the hinted words put {is} of ink in Bassline where the unhinted put {was}: the letters are drawn heavier or lighter, not sharper"
    );
}

/// The explainer's timeline: a gantt chart, whose section labels merman
/// writes as a `<tspan>` inside each `<text>`.
const GANTT: &str = "gantt\n  title A Gantt Diagram\n  dateFormat YYYY-MM-DD\n  axisFormat %m/%d\n\n  section Precomp\n  Outline :done, o1, 2024-01-01, 10d\n  Instrument selection :active, ins, after o1, 8d\n\n  section Preapproval\n  Soundbite :sb, 2024-01-20, 1d\n  Teaser :ts, after sb, 2d\n\n  section Full track\n  Main :m, 2024-01-24, 7d\n  Vocals :v, after m, 4d\n  Candy :c, after v, 1d\n";

/// A gantt chart's words are lettered like a flowchart's: the words inside
/// a `<tspan>` are the words, not the markup round them, and no text is left
/// for resvg.
#[test]
fn a_gantt_charts_words_are_lettered() {
    let fonts = system_fonts();
    let svg = markdown_notes_plugin::diagram::svg(GANTT, look()).expect("no SVG");
    assert!(svg.contains("<tspan"), "the gantt chart has no tspans to letter");
    let lettered = markdown_notes_plugin::words::as_hinted_paths(&svg, &fonts, 1.0);
    assert!(
        !lettered.contains("<text ") && !lettered.contains("tspan"),
        "{} texts and {} tspans were left: {}",
        lettered.matches("<text ").count(),
        lettered.matches("<tspan").count(),
        lettered.split("<text ").nth(1).map(|t| &t[..t.len().min(200)]).unwrap_or("")
    );
    let picture = draw(GANTT, look(), &fonts).expect("nothing was drawn");
    println!("gantt: {}", saved(&picture, "gantt-editor"));
}

/// How far the corners of an edge label's box are rounded.
const LABEL_CORNER: f64 = 4.0;

/// The box behind an edge label has rounded corners.
#[test]
fn an_edge_labels_box_has_rounded_corners() {
    let svg = markdown_notes_plugin::diagram::svg(ACROSS, look()).expect("no SVG");
    let rect = label_rect(&svg, "L_A_E_0");
    let radius = |name: &str| attr(&rect, name).and_then(|n| n.parse::<f64>().ok());
    assert_eq!(
        (radius("rx"), radius("ry")),
        (Some(LABEL_CORNER), Some(LABEL_CORNER)),
        "the box's corners are not rounded by {LABEL_CORNER}: {rect}"
    );
}

/// Room between a label's words and the edge of its box: left and right,
/// above, and below.
const LABEL_PADDING: (f64, f64, f64) = (5.0, 2.0, 4.0);

/// The box behind an edge label reaches past its words on every side, so
/// they are not pressed against its edge. The box is rounded to whole pixels
/// after it is padded, so each of its sides is within half a pixel of the
/// padding.
#[test]
fn an_edge_labels_box_has_room_around_its_words() {
    let svg = markdown_notes_plugin::diagram::svg(ACROSS, look()).expect("no SVG");
    let (cx, cy) = label_centre(&svg, "L_A_E_0");
    let (hx, hy) = label_half(&svg, "L_A_E_0");
    let (x, y, w, h) = label_box(&svg, "L_A_E_0");
    let close = |a: f64, b: f64| (a - b).abs() <= 1.0;
    assert!(
        close(x, cx - hx - LABEL_PADDING.0)
            && close(y, cy - hy - LABEL_PADDING.1)
            && close(w, 2.0 * hx + 2.0 * LABEL_PADDING.0)
            && close(h, 2.0 * hy + LABEL_PADDING.1 + LABEL_PADDING.2),
        "the box is at {x},{y} and {w} by {h}, for words {} by {} centred at {cx},{cy}",
        2.0 * hx,
        2.0 * hy
    );
}

/// How far `p` is from the nearest piece of the polyline `points`.
fn distance_to(points: &[(f64, f64)], p: (f64, f64)) -> f64 {
    points
        .windows(2)
        .map(|seg| {
            let (a, b) = (seg[0], seg[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 == 0.0 {
                0.0
            } else {
                (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
            };
            let (nx, ny) = (a.0 + t * dx, a.1 + t * dy);
            ((p.0 - nx).powi(2) + (p.1 - ny).powi(2)).sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

/// In a chart that runs across the page, an edge to a lower row still
/// arrives at the side of the node it reaches, and its label sits on the
/// edge rather than on that node. The picture is kept beside the UI tests'
/// screenshots, to be looked at.
#[test]
fn an_edge_across_the_page_arrives_at_the_side_of_its_node_with_its_label_on_it() {
    let svg = markdown_notes_plugin::diagram::svg(ACROSS, look()).expect("no SVG");
    let kept = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cache/uitests/flowchart-across.svg");
    let _ = std::fs::write(kept, &svg);
    let picture = draw(ACROSS, look(), &system_fonts()).expect("nothing was drawn");
    println!("across the page: {}", saved(&picture, "flowchart-across"));

    let (left, top, right, bottom) = node_box(&svg, "E");
    let points = edge_points(&svg, "L_A_E_0");
    let end = *points.last().expect("the edge has no points");
    // The line stops short of the side by the length of the arrowhead drawn
    // on its end, which is what reaches the node.
    assert!(
        (0.0..=8.0).contains(&(left - end.0)) && end.1 > top && end.1 < bottom,
        "the edge ends at {end:?}, not at the left side of Bassline, which spans {left}..{right} by {top}..{bottom}"
    );

    let label = label_centre(&svg, "L_A_E_0");
    let off = distance_to(&points, label);
    assert!(off <= 3.0, "the label at {label:?} is {off} from its edge {points:?}");
    assert!(
        label.0 < left - 3.0 || label.0 > right + 3.0 || label.1 < top - 3.0 || label.1 > bottom + 3.0,
        "the label at {label:?} sits on Bassline, which spans {left}..{right} by {top}..{bottom}"
    );
}

#[test]
fn merman_draws_the_flowchart() {
    let svg = merman::render::HeadlessRenderer::new()
        .render_svg_resvg_safe_sync(&as_a_flowchart())
        .expect("merman could not render the diagram")
        .expect("merman did not see a diagram");
    let kept = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cache/uitests/flowchart-merman.svg");
    let _ = std::fs::write(kept, &svg);
    let picture = rasterise(&svg, look(), &system_fonts()).expect("the SVG did not rasterise");
    println!(
        "merman: {}x{} at {}",
        picture.width,
        picture.height,
        saved(&picture, "flowchart-merman")
    );
    assert!(inked(&picture) > 1000);
}
