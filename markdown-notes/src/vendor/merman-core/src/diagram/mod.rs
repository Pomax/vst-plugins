use crate::{Error, MermaidConfig, ParseMetadata, Result};
use serde_json::Value;

/// Parser used by the semantic JSON path for one Mermaid diagram family.
pub type DiagramSemanticParser = fn(code: &str, meta: &ParseMetadata) -> Result<Value>;

/// Parser used by the typed render-model path for one Mermaid diagram family.
pub type RenderSemanticParser = fn(code: &str, meta: &ParseMetadata) -> Result<RenderSemanticModel>;

/// Registry for semantic JSON parsers keyed by Mermaid diagram type id.
#[derive(Debug, Clone, Default)]
pub struct DiagramRegistry {
    parsers: std::collections::HashMap<&'static str, DiagramSemanticParser>,
}

impl DiagramRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers or replaces the parser for a Mermaid diagram type id.
    pub fn insert(&mut self, diagram_type: &'static str, parser: DiagramSemanticParser) {
        self.parsers.insert(diagram_type, parser);
    }

    /// Looks up a parser by Mermaid diagram type id.
    pub fn get(&self, diagram_type: &str) -> Option<DiagramSemanticParser> {
        self.parsers.get(diagram_type).copied()
    }

    /// Builds the semantic parser registry for the repository's pinned Mermaid baseline.
    pub fn for_pinned_mermaid_baseline() -> Self {
        let mut reg = Self::new();
        for fact in crate::family::semantic_parser_facts() {
            reg.insert(fact.id, fact.parser);
        }

        reg
    }

    #[cfg(test)]
    pub(crate) fn parser_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.parsers.keys().copied()
    }
}

/// Parsed diagram metadata plus the Mermaid-compatible semantic JSON model.
#[derive(Debug, Clone)]
pub struct ParsedDiagram {
    /// Diagram type and effective configuration extracted during preprocessing.
    pub meta: ParseMetadata,
    /// Semantic JSON model matching Mermaid's parser/database output shape where possible.
    pub model: Value,
}

/// Typed semantic model used by the headless renderer.
///
/// Most public callers should use [`ParsedDiagram`] when they need JSON output. This enum is for
/// render paths that benefit from typed data and avoiding a JSON round trip.
#[derive(Debug, Clone)]
pub enum RenderSemanticModel {
    Json(Value),
    Flowchart(crate::diagrams::flowchart::FlowchartV2Model),
    Gantt(crate::diagrams::gantt::GanttDiagramRenderModel),
}

impl RenderSemanticModel {
    /// Applies Mermaid common DB sanitization to family-owned typed fields.
    pub(crate) fn sanitize_common_db_fields(&mut self, config: &MermaidConfig) {
        match self {
            Self::Json(v) => crate::common_db::apply_common_db_sanitization(v, config),
            Self::Flowchart(v) => v.sanitize_common_db_fields(config),
            Self::Gantt(v) => v.sanitize_common_db_fields(config),
        }
    }

    /// Returns a stable family label for diagnostics and timing output.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Json(_) => "json",
            Self::Flowchart(_) => "flowchart",
            Self::Gantt(_) => "gantt",
        }
    }

    /// Returns whether this typed model can represent the given Mermaid diagram type id.
    pub fn supports_diagram_type(&self, diagram_type: &str) -> bool {
        match self {
            Self::Json(_) => true,
            other => {
                crate::family::render_model_kind_supports_diagram_type(other.kind(), diagram_type)
            }
        }
    }
}

/// Registry for typed render-model parsers keyed by Mermaid diagram type id.
#[derive(Debug, Clone, Default)]
pub struct RenderDiagramRegistry {
    parsers: std::collections::HashMap<&'static str, RenderSemanticParser>,
}

impl RenderDiagramRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers or replaces the typed render parser for a Mermaid diagram type id.
    pub fn insert(&mut self, diagram_type: &'static str, parser: RenderSemanticParser) {
        self.parsers.insert(diagram_type, parser);
    }

    /// Looks up a typed render parser by Mermaid diagram type id.
    pub fn get(&self, diagram_type: &str) -> Option<RenderSemanticParser> {
        self.parsers.get(diagram_type).copied()
    }

    #[cfg(test)]
    pub(crate) fn remove(&mut self, diagram_type: &str) -> Option<RenderSemanticParser> {
        self.parsers.remove(diagram_type)
    }

    /// Builds the typed render parser registry for the repository's pinned Mermaid baseline.
    pub fn for_pinned_mermaid_baseline() -> Self {
        let mut reg = Self::new();
        for fact in crate::family::render_parser_facts() {
            reg.insert(fact.id, fact.parser);
        }

        reg
    }

    #[cfg(test)]
    pub(crate) fn parser_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.parsers.keys().copied()
    }
}

/// Parsed diagram metadata plus a typed render model.
#[derive(Debug, Clone)]
pub struct ParsedDiagramRender {
    /// Diagram type and effective configuration extracted during preprocessing.
    pub meta: ParseMetadata,
    /// Typed model consumed by layout and SVG renderers.
    pub model: RenderSemanticModel,
}

/// Parses with a registry entry or reports an unsupported Mermaid diagram type.
pub fn parse_or_unsupported(
    registry: &DiagramRegistry,
    diagram_type: &str,
    code: &str,
    meta: &ParseMetadata,
) -> Result<Value> {
    let Some(parser) = registry.get(diagram_type) else {
        return Err(Error::UnsupportedDiagram {
            diagram_type: diagram_type.to_string(),
        });
    };
    parser(code, meta)
}
