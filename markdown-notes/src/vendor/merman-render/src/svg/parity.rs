use super::icon_registry::IconRegistry;
use super::pipeline::{ScopedCssPostprocessor, SvgPipeline, SvgPostprocessMetadata};
use crate::model::{
    Bounds, ErrorDiagramLayout, FlowchartV2Layout, LayoutCluster, LayoutNode,
};
use crate::text::{TextMeasurer, TextStyle, WrapMode};
use crate::{Error, Result};
use base64::Engine as _;
use indexmap::IndexMap;
use std::fmt::Write as _;

mod css;
mod curve;
mod emitted_bounds;
mod error;
mod flowchart;
mod gantt;
mod layout_debug;
mod path_bounds;
mod root_svg;
mod roughjs_common;
mod style;
pub(crate) mod theme;
mod timing;
mod util;
use crate::math::MathRenderer;
use css::{gantt_css, info_css_with_config};
use path_bounds::svg_path_bounds_from_d;
pub use emitted_bounds::{
    SvgEmittedBoundsContributor, SvgEmittedBoundsDebug, debug_svg_emitted_bounds,
};
use emitted_bounds::{svg_emitted_bounds_from_svg, svg_emitted_bounds_from_svg_inner};
use style::{is_rect_style_key, is_text_style_key, parse_style_decl};
use theme::PresentationTheme;
use util::{
    SvgTheme, apply_root_viewport_override, config_bool, config_diagram_look, config_f64,
    config_f64_css_px, config_string, css_rgba_fade, decode_mermaid_entities_for_render_text,
    escape_attr, escape_attr_display, escape_attr_into, escape_xml, escape_xml_display,
    escape_xml_into, fmt, fmt_debug_3dp, fmt_display, fmt_into, fmt_max_width_px, fmt_path,
    fmt_path_into, fmt_points, fmt_string, json_stringify_points, json_stringify_points_into,
    normalize_css_font_family, push_points_attr, scoped_svg_id, scoped_svg_url, theme_color,
};

const MERMAID_SEQUENCE_BASE_DEFS_11_12_2: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/sequence_base_defs_11_12_2.svgfrag"
));

#[derive(Debug, Clone)]
pub struct SvgRenderOptions {
    /// Adds extra space around the computed viewBox.
    pub viewbox_padding: f64,
    /// Optional diagram id used for Mermaid-like marker ids.
    pub diagram_id: Option<String>,
    /// Optional override for the root SVG `aria-roledescription` attribute.
    ///
    /// This is primarily used to reproduce Mermaid's per-header accessibility metadata quirks
    /// (e.g. `classDiagram-v2` differs from `classDiagram` at Mermaid 11.12.2).
    pub aria_roledescription: Option<String>,
    /// When true, include edge polylines.
    pub include_edges: bool,
    /// When true, include node bounding boxes and ids.
    pub include_nodes: bool,
    /// When true, include cluster bounding boxes and titles.
    pub include_clusters: bool,
    /// When true, draw markers that visualize Mermaid cluster positioning metadata.
    pub include_cluster_debug_markers: bool,
    /// When true, label edge routes with edge ids.
    pub include_edge_id_labels: bool,
    /// Optional override for "current time" used by diagrams that render time-dependent markers
    /// (e.g. Gantt `today` line). This exists to make parity/golden comparisons reproducible.
    pub now_ms_override: Option<i64>,
    /// Optional math renderer for `$$...$$` style labels.
    pub math_renderer: Option<std::sync::Arc<dyn MathRenderer + Send + Sync>>,
    /// Optional Iconify-compatible registry used by icon-capable renderers.
    pub icon_registry: Option<std::sync::Arc<IconRegistry>>,
    /// When false, renderers that support root viewport override lookup emit computed bounds.
    pub apply_root_overrides: bool,
}

impl Default for SvgRenderOptions {
    fn default() -> Self {
        Self {
            viewbox_padding: 8.0,
            diagram_id: None,
            aria_roledescription: None,
            include_edges: true,
            include_nodes: true,
            include_clusters: true,
            include_cluster_debug_markers: false,
            include_edge_id_labels: false,
            now_ms_override: None,
            math_renderer: None,
            icon_registry: None,
            apply_root_overrides: true,
        }
    }
}

pub fn render_layouted_svg(
    diagram: &crate::model::LayoutedDiagram,
    measurer: &dyn TextMeasurer,
    options: &SvgRenderOptions,
) -> Result<String> {
    render_layout_svg_parts(
        &diagram.layout,
        &diagram.semantic,
        &diagram.meta.effective_config,
        diagram.meta.title.as_deref(),
        measurer,
        options,
    )
}

pub fn render_layout_svg_parts(
    layout: &crate::model::LayoutDiagram,
    semantic: &serde_json::Value,
    effective_config: &serde_json::Value,
    title: Option<&str>,
    measurer: &dyn TextMeasurer,
    options: &SvgRenderOptions,
) -> Result<String> {
    let svg =
        render_layout_svg_parts_raw(layout, semantic, effective_config, title, measurer, options)?;
    apply_theme_css(svg, effective_config)
}

fn render_layout_svg_parts_raw(
    layout: &crate::model::LayoutDiagram,
    semantic: &serde_json::Value,
    effective_config: &serde_json::Value,
    title: Option<&str>,
    measurer: &dyn TextMeasurer,
    options: &SvgRenderOptions,
) -> Result<String> {
    use crate::model::LayoutDiagram;

    match layout {
        LayoutDiagram::ErrorDiagram(layout) => {
            render_error_diagram_svg(layout, semantic, effective_config, options)
        }
        LayoutDiagram::FlowchartV2(layout) => {
            render_flowchart_v2_svg(layout, semantic, effective_config, title, measurer, options)
        }
        LayoutDiagram::GanttDiagram(layout) => {
            render_gantt_diagram_svg(layout, semantic, effective_config, options)
        }
    }
}

pub fn render_layout_svg_parts_with_config(
    layout: &crate::model::LayoutDiagram,
    semantic: &serde_json::Value,
    effective_config: &merman_core::MermaidConfig,
    title: Option<&str>,
    measurer: &dyn TextMeasurer,
    options: &SvgRenderOptions,
) -> Result<String> {
    let svg = render_layout_svg_parts_with_config_raw(
        layout,
        semantic,
        effective_config,
        title,
        measurer,
        options,
    )?;
    apply_theme_css(svg, effective_config.as_value())
}

fn render_layout_svg_parts_with_config_raw(
    layout: &crate::model::LayoutDiagram,
    semantic: &serde_json::Value,
    effective_config: &merman_core::MermaidConfig,
    title: Option<&str>,
    measurer: &dyn TextMeasurer,
    options: &SvgRenderOptions,
) -> Result<String> {
    use crate::model::LayoutDiagram;

    let effective_config_value = effective_config.as_value();

    match layout {
        LayoutDiagram::ErrorDiagram(layout) => {
            render_error_diagram_svg(layout, semantic, effective_config_value, options)
        }
        LayoutDiagram::FlowchartV2(layout) => render_flowchart_v2_svg_with_config(
            layout,
            semantic,
            effective_config,
            title,
            measurer,
            options,
        ),
        LayoutDiagram::GanttDiagram(layout) => {
            render_gantt_diagram_svg(layout, semantic, effective_config_value, options)
        }
    }
}

pub fn render_layout_svg_parts_for_render_model_with_config(
    layout: &crate::model::LayoutDiagram,
    semantic: &merman_core::RenderSemanticModel,
    effective_config: &merman_core::MermaidConfig,
    title: Option<&str>,
    measurer: &dyn TextMeasurer,
    options: &SvgRenderOptions,
) -> Result<String> {
    let svg = render_layout_svg_parts_for_render_model_with_config_raw(
        layout,
        semantic,
        effective_config,
        title,
        measurer,
        options,
    )?;
    apply_theme_css(svg, effective_config.as_value())
}

fn render_layout_svg_parts_for_render_model_with_config_raw(
    layout: &crate::model::LayoutDiagram,
    semantic: &merman_core::RenderSemanticModel,
    effective_config: &merman_core::MermaidConfig,
    title: Option<&str>,
    measurer: &dyn TextMeasurer,
    options: &SvgRenderOptions,
) -> Result<String> {
    use crate::model::LayoutDiagram;
    use merman_core::RenderSemanticModel;

    match (layout, semantic) {
        (LayoutDiagram::FlowchartV2(layout), RenderSemanticModel::Flowchart(model)) => {
            render_flowchart_v2_svg_model_with_config(
                layout,
                model,
                effective_config,
                title,
                measurer,
                options,
            )
        }
        (LayoutDiagram::GanttDiagram(layout), RenderSemanticModel::Gantt(model)) => {
            gantt::render_gantt_diagram_svg_model(
                layout,
                model,
                effective_config.as_value(),
                options,
            )
        }
        (_, RenderSemanticModel::Json(semantic)) => render_layout_svg_parts_with_config_raw(
            layout,
            semantic,
            effective_config,
            title,
            measurer,
            options,
        ),
        _ => Err(Error::InvalidModel {
            message: "semantic model does not match layout diagram type".to_string(),
        }),
    }
}

fn apply_theme_css(svg: String, effective_config: &serde_json::Value) -> Result<String> {
    let Some(theme_css) = effective_config
        .get("themeCSS")
        .and_then(serde_json::Value::as_str)
        .filter(|css| !css.trim().is_empty())
    else {
        return Ok(svg);
    };

    let metadata = SvgPostprocessMetadata::from_svg(&svg);
    let pipeline = SvgPipeline::parity().with_postprocessor(ScopedCssPostprocessor::new(theme_css));
    pipeline.process_to_string_with_metadata(&svg, &metadata)
}

pub fn render_flowchart_v2_debug_svg(
    layout: &FlowchartV2Layout,
    options: &SvgRenderOptions,
) -> String {
    flowchart::render_flowchart_v2_debug_svg(layout, options)
}

pub fn render_error_diagram_svg(
    layout: &ErrorDiagramLayout,
    _semantic: &serde_json::Value,
    _effective_config: &serde_json::Value,
    options: &SvgRenderOptions,
) -> Result<String> {
    error::render_error_diagram_svg(layout, _semantic, _effective_config, options)
}

pub fn render_gantt_diagram_svg(
    layout: &crate::model::GanttDiagramLayout,
    semantic: &serde_json::Value,
    _effective_config: &serde_json::Value,
    options: &SvgRenderOptions,
) -> Result<String> {
    gantt::render_gantt_diagram_svg(layout, semantic, _effective_config, options)
}


pub fn render_flowchart_v2_svg(
    layout: &FlowchartV2Layout,
    semantic: &serde_json::Value,
    effective_config: &serde_json::Value,
    diagram_title: Option<&str>,
    measurer: &dyn TextMeasurer,
    options: &SvgRenderOptions,
) -> Result<String> {
    flowchart::render_flowchart_v2_svg(
        layout,
        semantic,
        effective_config,
        diagram_title,
        measurer,
        options,
    )
}

pub fn render_flowchart_v2_svg_with_config(
    layout: &FlowchartV2Layout,
    semantic: &serde_json::Value,
    effective_config: &merman_core::MermaidConfig,
    diagram_title: Option<&str>,
    measurer: &dyn TextMeasurer,
    options: &SvgRenderOptions,
) -> Result<String> {
    flowchart::render_flowchart_v2_svg_with_config(
        layout,
        semantic,
        effective_config,
        diagram_title,
        measurer,
        options,
    )
}

pub fn render_flowchart_v2_svg_model_with_config(
    layout: &FlowchartV2Layout,
    model: &merman_core::diagrams::flowchart::FlowchartV2Model,
    effective_config: &merman_core::MermaidConfig,
    diagram_title: Option<&str>,
    measurer: &dyn TextMeasurer,
    options: &SvgRenderOptions,
) -> Result<String> {
    flowchart::render_flowchart_v2_svg_model_with_config(
        layout,
        model,
        effective_config,
        diagram_title,
        measurer,
        options,
    )
}

// Ported from D3 `curveBasis` (d3-shape v3.x), used by Mermaid ER renderer `@11.12.2`.
fn curve_basis_path_d(points: &[crate::model::LayoutPoint]) -> String {
    curve::curve_basis_path_d(points)
}
fn render_node(out: &mut String, n: &LayoutNode) {
    layout_debug::render_node(out, n)
}

fn render_state_node(out: &mut String, n: &LayoutNode) {
    layout_debug::render_state_node(out, n)
}

fn render_cluster(out: &mut String, c: &LayoutCluster, include_markers: bool) {
    layout_debug::render_cluster(out, c, include_markers)
}

fn compute_layout_bounds(
    clusters: &[LayoutCluster],
    nodes: &[LayoutNode],
    edges: &[crate::model::LayoutEdge],
) -> Option<Bounds> {
    layout_debug::compute_layout_bounds(clusters, nodes, edges)
}
