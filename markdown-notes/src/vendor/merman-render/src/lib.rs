#![forbid(unsafe_code)]
// Cutting twenty-eight diagram types out left upstream helpers with nothing
// calling them. The linker drops them, and deleting them one by one through a
// crate this size would only make the next merge from upstream harder.
#![allow(dead_code, unused_imports)]

//! Headless layout + rendering for Mermaid diagrams.
//!
//! This crate consumes `merman-core`'s semantic models and produces:
//! - a layout JSON (geometry + routes)
//! - Mermaid-like SVG output with DOM parity checks against upstream baselines

// This fork draws flowcharts and gantt charts. The modules for the other
// twenty-eight diagram types are gone, along with the parts of the crate that
// only they used.
mod chart_palette;
mod config;
mod entities;
pub mod error;
pub mod flowchart;
pub mod gantt;
mod generated;
mod json;
pub mod math;
mod mermaid_style;
pub mod model;
pub mod svg;
pub mod text;
mod theme;
mod trig_tables;

use crate::math::MathRenderer;
use crate::model::{LayoutDiagram, LayoutMeta, LayoutedDiagram};
use crate::text::{DeterministicTextMeasurer, TextMeasurer};
use merman_core::{ParsedDiagram, ParsedDiagramRender, RenderSemanticModel};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported diagram type for layout: {diagram_type}")]
    UnsupportedDiagram { diagram_type: String },
    #[error("invalid semantic model: {message}")]
    InvalidModel { message: String },
    #[error("SVG postprocessor `{pass}` failed: {message}")]
    SvgPostprocess { pass: String, message: String },
    #[error("semantic model JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn svg_postprocess(pass: impl Into<String>, message: impl Into<String>) -> Self {
        Self::SvgPostprocess {
            pass: pass.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone)]
pub struct LayoutOptions {
    pub text_measurer: Arc<dyn TextMeasurer + Send + Sync>,
    /// Optional math renderer for `$$...$$` style labels.
    pub math_renderer: Option<Arc<dyn MathRenderer + Send + Sync>>,
    pub viewport_width: f64,
    pub viewport_height: f64,
    /// Enable experimental layout engines (e.g. Cytoscape COSE/FCoSE ports) for diagrams that
    /// currently use placeholder layouts in merman.
    pub use_manatee_layout: bool,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            text_measurer: Arc::new(DeterministicTextMeasurer::default()),
            math_renderer: None,
            viewport_width: 800.0,
            viewport_height: 600.0,
            use_manatee_layout: false,
        }
    }
}

impl LayoutOptions {
    /// Returns layout defaults suitable for headless SVG rendering in UI integrations.
    ///
    /// The caller is expected to supply a measurer through `with_text_measurer`
    /// that measures with the fonts the SVG will be drawn in. Upstream's table
    /// of browser metrics is not carried in this fork, so what is left here is
    /// the rough fallback.
    pub fn headless_svg_defaults() -> Self {
        Self {
            text_measurer: Arc::new(crate::text::DeterministicTextMeasurer::default()),
            // Mermaid parity fixtures for diagrams like mindmap/architecture rely on the COSE
            // layout port (manatee). Make the headless defaults "just work" for UI integrations.
            use_manatee_layout: true,
            ..Default::default()
        }
    }

    pub fn with_text_measurer(mut self, measurer: Arc<dyn TextMeasurer + Send + Sync>) -> Self {
        self.text_measurer = measurer;
        self
    }

    pub fn with_math_renderer(mut self, renderer: Arc<dyn MathRenderer + Send + Sync>) -> Self {
        self.math_renderer = Some(renderer);
        self
    }
}

pub fn layout_parsed(parsed: &ParsedDiagram, options: &LayoutOptions) -> Result<LayoutedDiagram> {
    let meta = LayoutMeta::from_parse_metadata(&parsed.meta);
    let layout = layout_parsed_layout_only(parsed, options)?;

    Ok(LayoutedDiagram {
        meta,
        semantic: crate::json::clone_value_nonrecursive(&parsed.model),
        layout,
    })
}

pub fn layout_parsed_layout_only(
    parsed: &ParsedDiagram,
    options: &LayoutOptions,
) -> Result<LayoutDiagram> {
    let diagram_type = parsed.meta.diagram_type.as_str();
    let title = parsed.meta.title.as_deref();
    layout_json_by_type(
        diagram_type,
        &parsed.model,
        &parsed.meta.effective_config,
        title,
        options,
    )
}

pub fn layout_parsed_render_layout_only(
    parsed: &ParsedDiagramRender,
    options: &LayoutOptions,
) -> Result<LayoutDiagram> {
    let diagram_type = parsed.meta.diagram_type.as_str();
    let effective_config = parsed.meta.effective_config.as_value();
    let title = parsed.meta.title.as_deref();

    if !parsed.model.supports_diagram_type(diagram_type) {
        return Err(Error::InvalidModel {
            message: format!(
                "unexpected render model variant {} for diagram type: {diagram_type}",
                parsed.model.kind()
            ),
        });
    }

    match &parsed.model {
        RenderSemanticModel::Flowchart(model) => Ok(LayoutDiagram::FlowchartV2(Box::new(
            flowchart::layout_flowchart_v2_typed(
                model,
                &parsed.meta.effective_config,
                options.text_measurer.as_ref(),
                options.math_renderer.as_deref(),
            )?,
        ))),
        RenderSemanticModel::Gantt(model) => Ok(LayoutDiagram::GanttDiagram(Box::new(
            gantt::layout_gantt_diagram_typed(
                model,
                effective_config,
                options.text_measurer.as_ref(),
            )?,
        ))),
        RenderSemanticModel::Json(semantic) => layout_json_by_type(
            diagram_type,
            semantic,
            &parsed.meta.effective_config,
            title,
            options,
        ),
    }
}

fn layout_json_by_type(
    diagram_type: &str,
    semantic: &Value,
    effective_config: &merman_core::MermaidConfig,
    _title: Option<&str>,
    options: &LayoutOptions,
) -> Result<LayoutDiagram> {
    let effective_config_value = effective_config.as_value();

    match diagram_type {
        "error" => Ok(LayoutDiagram::ErrorDiagram(Box::new(
            error::layout_error_diagram(
                semantic,
                effective_config_value,
                options.text_measurer.as_ref(),
            )?,
        ))),
        "flowchart-v2" => Ok(LayoutDiagram::FlowchartV2(Box::new(
            flowchart::layout_flowchart_v2(
                semantic,
                effective_config,
                options.text_measurer.as_ref(),
                options.math_renderer.as_deref(),
            )?,
        ))),
        "gantt" => Ok(LayoutDiagram::GanttDiagram(Box::new(
            gantt::layout_gantt_diagram(
                semantic,
                effective_config_value,
                options.text_measurer.as_ref(),
            )?,
        ))),
        other => Err(Error::UnsupportedDiagram {
            diagram_type: other.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use merman_core::{Engine, ParseOptions};

    #[test]
    fn render_model_dispatch_accepts_diagram_type_aliases() {
        let parsed = Engine::new()
            .parse_diagram_for_render_model_with_type_sync(
                "flowchart-elk",
                "flowchart-elk TD\nA-->B;",
                ParseOptions::strict(),
            )
            .unwrap()
            .unwrap();

        let layout = layout_parsed_render_layout_only(&parsed, &LayoutOptions::default()).unwrap();
        assert!(matches!(layout, LayoutDiagram::FlowchartV2(_)));
    }

    // Upstream also checked that a sequence diagram's model is refused when
    // the metadata claims it is a flowchart. There is no sequence diagram in
    // this fork to build that mismatch from.
}
