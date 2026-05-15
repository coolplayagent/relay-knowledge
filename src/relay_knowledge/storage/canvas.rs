use std::collections::BTreeMap;

use crate::domain::GraphVersion;

/// Storage-level graph canvas selection without depending on Web API types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphCanvasSelection {
    Knowledge,
    Code,
    Mixed,
}

impl GraphCanvasSelection {
    pub const fn includes_knowledge(self) -> bool {
        matches!(self, Self::Knowledge | Self::Mixed)
    }

    pub const fn includes_code(self) -> bool {
        matches!(self, Self::Code | Self::Mixed)
    }
}

/// Bounded graph canvas request against an explicit graph snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCanvasStorageRequest {
    pub selection: GraphCanvasSelection,
    pub source_scope: Option<String>,
    pub query: Option<String>,
    pub graph_version: GraphVersion,
    pub limit: usize,
}

/// Storage node projected into the Web graph canvas contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCanvasStorageNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub subtitle: Option<String>,
    pub source_scope: Option<String>,
    pub graph_version: GraphVersion,
    pub weight: u32,
    pub status: Option<String>,
    pub details: BTreeMap<String, String>,
}

/// Storage edge projected into the Web graph canvas contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCanvasStorageEdge {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub target: String,
    pub label: String,
    pub graph_version: GraphVersion,
    pub confidence_basis_points: Option<u16>,
    pub evidence_count: Option<usize>,
    pub details: BTreeMap<String, String>,
}

/// Bounded storage snapshot for one graph canvas view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCanvasStorageSnapshot {
    pub nodes: Vec<GraphCanvasStorageNode>,
    pub edges: Vec<GraphCanvasStorageEdge>,
    pub available_kinds: Vec<String>,
    pub truncated: bool,
}
