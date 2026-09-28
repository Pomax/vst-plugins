//! Diagram family facts for the pinned Mermaid baseline.
//!
//! This module owns release-facing Mermaid family facts and projects them into detector,
//! parser, render-model, and metadata surfaces.

use crate::baseline::BaselineRegistryProfile;
use crate::detect::DetectorFn;
use crate::diagram::{DiagramSemanticParser, RenderSemanticModel, RenderSemanticParser};
use crate::{MermaidConfig, ParseMetadata, Result};
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub(crate) struct DetectorFact {
    pub(crate) id: &'static str,
    pub(crate) detector: DetectorFn,
}

#[derive(Clone, Copy)]
pub(crate) struct FastDetectKeywordFact {
    keyword: &'static str,
    id: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) struct SemanticParserFact {
    pub(crate) id: &'static str,
    pub(crate) parser: DiagramSemanticParser,
}

#[derive(Clone, Copy)]
pub(crate) struct RenderParserFact {
    pub(crate) id: &'static str,
    pub(crate) metadata_id: Option<&'static str>,
    pub(crate) model_kind: &'static str,
    pub(crate) parser: RenderSemanticParser,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SupportedDiagramFact {
    pub(crate) metadata_id: &'static str,
    pub(crate) render_parser_ids: Vec<&'static str>,
}

pub(crate) fn detector_facts(profile: BaselineRegistryProfile) -> &'static [DetectorFact] {
    match profile {
        BaselineRegistryProfile::Tiny => DETECTOR_FACTS_TINY,
        BaselineRegistryProfile::Full => DETECTOR_FACTS_FULL,
    }
}

pub(crate) fn fast_detect_by_leading_keyword(
    text: &str,
    profile: BaselineRegistryProfile,
) -> Option<&'static str> {
    fn has_boundary(rest: &str) -> bool {
        rest.is_empty()
            || rest
                .chars()
                .next()
                .is_some_and(|c| c.is_whitespace() || c == ';')
    }

    let trimmed = text.trim_start();
    let keywords = match profile {
        BaselineRegistryProfile::Tiny => FAST_DETECT_KEYWORDS_TINY,
        BaselineRegistryProfile::Full => FAST_DETECT_KEYWORDS_FULL,
    };

    keywords.iter().find_map(|fact| {
        trimmed
            .strip_prefix(fact.keyword)
            .and_then(|rest| has_boundary(rest).then_some(fact.id))
    })
}

pub(crate) fn semantic_parser_facts() -> &'static [SemanticParserFact] {
    SEMANTIC_PARSER_FACTS
}

pub(crate) fn render_parser_facts() -> &'static [RenderParserFact] {
    RENDER_PARSER_FACTS
}

pub(crate) fn supported_diagram_facts() -> &'static [SupportedDiagramFact] {
    static FACTS: OnceLock<Vec<SupportedDiagramFact>> = OnceLock::new();
    FACTS
        .get_or_init(|| {
            SUPPORTED_DIAGRAM_METADATA_IDS
                .iter()
                .map(|metadata_id| SupportedDiagramFact {
                    metadata_id,
                    render_parser_ids: RENDER_PARSER_FACTS
                        .iter()
                        .filter_map(|fact| {
                            (fact.metadata_id == Some(*metadata_id)).then_some(fact.id)
                        })
                        .collect(),
                })
                .collect()
        })
        .as_slice()
}

pub(crate) fn supported_diagram_metadata_ids() -> &'static [&'static str] {
    static IDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    IDS.get_or_init(|| {
        supported_diagram_facts()
            .iter()
            .inspect(|fact| debug_assert!(!fact.render_parser_ids.is_empty()))
            .map(|fact| fact.metadata_id)
            .collect()
    })
    .as_slice()
}

pub(crate) fn render_model_kind_supports_diagram_type(
    model_kind: &'static str,
    diagram_type: &str,
) -> bool {
    RENDER_PARSER_FACTS
        .iter()
        .any(|fact| fact.model_kind == model_kind && fact.id == diagram_type)
}

pub(crate) fn permits_json_render_fallback(diagram_type: &str) -> bool {
    diagram_type == "error"
        || !SEMANTIC_PARSER_FACTS
            .iter()
            .any(|fact| fact.id == diagram_type)
}

pub(crate) fn apply_known_type_detector_side_effects(
    diagram_type: &str,
    effective_config: &mut MermaidConfig,
) {
    if diagram_type == "flowchart-elk" {
        effective_config.set_value("layout", Value::String("elk".to_string()));
        return;
    }

    if matches!(diagram_type, "flowchart-v2" | "flowchart")
        && effective_config.get_str("flowchart.defaultRenderer") == Some("elk")
    {
        effective_config.set_value("layout", Value::String("elk".to_string()));
    }
}

const DETECTOR_FACTS_FULL: &[DetectorFact] = &[
    DetectorFact {
        id: "error",
        detector: crate::detect::detector_error,
    },
    DetectorFact {
        id: "---",
        detector: crate::detect::detector_frontmatter_unparsed,
    },
    DetectorFact {
        id: "gantt",
        detector: crate::detect::detector_gantt,
    },
    DetectorFact {
        id: "flowchart-v2",
        detector: crate::detect::detector_flowchart_v2,
    },
    DetectorFact {
        id: "flowchart",
        detector: crate::detect::detector_flowchart_dagre_d3_graph,
    },
];

const DETECTOR_FACTS_TINY: &[DetectorFact] = &[
    DetectorFact {
        id: "error",
        detector: crate::detect::detector_error,
    },
    DetectorFact {
        id: "---",
        detector: crate::detect::detector_frontmatter_unparsed,
    },
    DetectorFact {
        id: "gantt",
        detector: crate::detect::detector_gantt,
    },
    DetectorFact {
        id: "flowchart-v2",
        detector: crate::detect::detector_flowchart_v2,
    },
    DetectorFact {
        id: "flowchart",
        detector: crate::detect::detector_flowchart_dagre_d3_graph,
    },
];

const FAST_DETECT_KEYWORDS_FULL: &[FastDetectKeywordFact] = &[
    FastDetectKeywordFact {
        keyword: "gantt",
        id: "gantt",
    },
];

const FAST_DETECT_KEYWORDS_TINY: &[FastDetectKeywordFact] = &[
    FastDetectKeywordFact {
        keyword: "gantt",
        id: "gantt",
    },
];

const SEMANTIC_PARSER_FACTS: &[SemanticParserFact] = &[
    SemanticParserFact {
        id: "error",
        parser: crate::diagrams::error_diagram::parse_error,
    },
    SemanticParserFact {
        id: "flowchart-v2",
        parser: crate::diagrams::flowchart::parse_flowchart,
    },
    SemanticParserFact {
        id: "flowchart",
        parser: crate::diagrams::flowchart::parse_flowchart,
    },
    SemanticParserFact {
        id: "flowchart-elk",
        parser: crate::diagrams::flowchart::parse_flowchart,
    },
    SemanticParserFact {
        id: "gantt",
        parser: crate::diagrams::gantt::parse_gantt,
    },
];

macro_rules! render_parser {
    ($fn_name:ident, $parser:path, $variant:path) => {
        fn $fn_name(code: &str, meta: &ParseMetadata) -> Result<RenderSemanticModel> {
            $parser(code, meta).map($variant)
        }
    };
}

render_parser!(
    render_flowchart,
    crate::diagrams::flowchart::parse_flowchart_model_for_render,
    RenderSemanticModel::Flowchart
);
render_parser!(
    render_gantt,
    crate::diagrams::gantt::parse_gantt_model_for_render,
    RenderSemanticModel::Gantt
);

const RENDER_PARSER_FACTS: &[RenderParserFact] = &[
    RenderParserFact {
        id: "flowchart-v2",
        metadata_id: Some("flowchart"),
        model_kind: "flowchart",
        parser: render_flowchart,
    },
    RenderParserFact {
        id: "flowchart",
        metadata_id: Some("flowchart"),
        model_kind: "flowchart",
        parser: render_flowchart,
    },
    RenderParserFact {
        id: "flowchart-elk",
        metadata_id: Some("flowchart"),
        model_kind: "flowchart",
        parser: render_flowchart,
    },
    RenderParserFact {
        id: "gantt",
        metadata_id: Some("gantt"),
        model_kind: "gantt",
        parser: render_gantt,
    },
];

const SUPPORTED_DIAGRAM_METADATA_IDS: &[&str] = &[
    "architecture",
    "block",
    "c4",
    "class",
    "er",
    "flowchart",
    "gantt",
    "gitgraph",
    "info",
    "journey",
    "kanban",
    "mindmap",
    "packet",
    "pie",
    "quadrantchart",
    "radar",
    "requirement",
    "sankey",
    "sequence",
    "state",
    "timeline",
    "treemap",
    "venn",
    "xychart",
    "zenuml",
];
