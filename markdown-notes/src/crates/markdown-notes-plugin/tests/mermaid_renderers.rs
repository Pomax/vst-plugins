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

#[test]
fn the_editor_draws_the_state_diagram() {
    let svg = markdown_notes_plugin::diagram::svg(STATES, look()).expect("no SVG was made");
    let kept = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cache/uitests/states-editor.svg");
    let _ = std::fs::write(kept, &svg);

    let picture = draw(STATES, look(), &system_fonts()).expect("nothing was drawn");
    println!(
        "the editor: {}x{} at {}",
        picture.width,
        picture.height,
        saved(&picture, "states-editor")
    );
    assert!(inked(&picture) > 1000);
}

/// On a dark page the lines and the words have to be light, or they are not
/// there to be read.
#[test]
fn the_editor_draws_the_state_diagram_on_a_dark_page() {
    let page = [27u8, 27, 27];
    let picture = draw(STATES, Look::new(true, page, 1.0), &system_fonts())
        .expect("nothing was drawn");
    println!(
        "the editor, dark: {}x{} at {}",
        picture.width,
        picture.height,
        saved(&picture, "states-editor-dark")
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
#[test]
fn merman_draws_the_state_diagram_with_each_edge_shape() {
    let fonts = system_fonts();
    for curve in ["linear", "step", "stepBefore", "stepAfter", "rounded"] {
        let config = merman::config::MermaidConfig::from_value(serde_json::json!({
            "flowchart": { "curve": curve },
            "state": { "curve": curve },
        }));
        let svg = merman::render::HeadlessRenderer::new()
            .with_site_config(config)
            .render_svg_resvg_safe_sync(STATES)
            .unwrap_or_else(|e| panic!("merman could not render with {curve}: {e}"))
            .expect("merman did not see a diagram");
        let picture = rasterise(&svg, look(), &fonts).expect("the SVG did not rasterise");
        println!(
            "merman, {curve}: {}x{} at {}",
            picture.width,
            picture.height,
            saved(&picture, &format!("states-merman-{curve}"))
        );
        assert!(inked(&picture) > 1000, "{curve} drew nothing");
    }
}

/// What Mermaid 12 uses for a state diagram when it is given no configuration,
/// which is what mermaid.live gives it: taken from `StateDiagramConfig` in
/// mermaid's `config.schema.yaml`. merman 0.7.0 follows Mermaid 11, so this
/// says how much of it merman knows.
#[test]
fn merman_draws_the_state_diagram_with_the_settings_mermaid_12_defaults_to() {
    let config = merman::config::MermaidConfig::from_value(serde_json::json!({
        "theme": "redux-color",
        "look": "neo",
        "layout": "elk",
        "state": {
            "theme": "redux-color",
            "look": "neo",
            "layout": "elk",
            "minNodeWidth": 120,
            "wrappingWidth": 120,
        },
    }));
    let svg = merman::render::HeadlessRenderer::new()
        .with_site_config(config)
        .render_svg_resvg_safe_sync(STATES)
        .unwrap_or_else(|e| panic!("merman could not render: {e}"))
        .expect("merman did not see a diagram");
    let picture = rasterise(&svg, look(), &system_fonts()).expect("the SVG did not rasterise");
    println!(
        "merman, mermaid 12 defaults: {}x{} at {}; themes it knows: {:?}",
        picture.width,
        picture.height,
        saved(&picture, "states-merman-v12-defaults"),
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

#[test]
fn merman_draws_the_state_diagram() {
    let svg = merman::render::HeadlessRenderer::new()
        .render_svg_resvg_safe_sync(STATES)
        .expect("merman could not render the diagram")
        .expect("merman did not see a diagram");
    let kept = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cache/uitests/states-merman.svg");
    let _ = std::fs::write(kept, &svg);
    let picture = rasterise(&svg, look(), &system_fonts()).expect("the SVG did not rasterise");
    println!(
        "merman: {}x{} at {}",
        picture.width,
        picture.height,
        saved(&picture, "states-merman")
    );
    assert!(inked(&picture) > 1000);
}
