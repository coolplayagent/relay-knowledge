use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::ApiMetadata;

pub const GRAPH_CANVAS_DEFAULT_LIMIT: usize = 250;
pub const GRAPH_CANVAS_MAX_LIMIT: usize = 1000;

/// Graph canvas view selected by the Web workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphCanvasKind {
    Knowledge,
    Code,
    Mixed,
}

impl GraphCanvasKind {
    /// Parses the stable Web query representation.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "knowledge" => Ok(Self::Knowledge),
            "code" => Ok(Self::Code),
            "mixed" => Ok(Self::Mixed),
            _ => Err(format!("unsupported graph canvas kind '{value}'")),
        }
    }

    /// Returns the stable Web query representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge",
            Self::Code => "code",
            Self::Mixed => "mixed",
        }
    }
}

/// Bounded graph canvas request for same-origin Web exploration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCanvasRequest {
    pub kind: GraphCanvasKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub limit: usize,
}

/// Node rendered in the Web graph canvas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCanvasNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub graph_version: u64,
    pub weight: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

/// Edge rendered in the Web graph canvas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCanvasEdge {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub target: String,
    pub label: String,
    pub graph_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_basis_points: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_count: Option<usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

/// Bounded graph canvas summary for truncation and legend hints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCanvasSummary {
    pub kind: GraphCanvasKind,
    pub node_count: usize,
    pub edge_count: usize,
    pub truncated: bool,
    pub available_kinds: Vec<String>,
}

/// Same-origin graph canvas response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCanvasResponse {
    pub metadata: ApiMetadata,
    pub nodes: Vec<GraphCanvasNode>,
    pub edges: Vec<GraphCanvasEdge>,
    pub summary: GraphCanvasSummary,
}

#[cfg(test)]
#[path = "graph_canvas_tests.rs"]
mod tests;
