use std::collections::{BTreeMap, BTreeSet};

use crate::{
    domain::GraphVersion,
    storage::{
        GraphCanvasStorageEdge, GraphCanvasStorageNode, GraphCanvasStorageSnapshot, StorageError,
    },
};

use super::nodes::{detail_map, scope_node_id};

pub(super) fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, StorageError> {
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) struct CanvasFilter {
    pub(super) source_scope: Option<String>,
    pub(super) query: Option<String>,
    pub(super) graph_version: GraphVersion,
    pub(super) limit: usize,
}

impl CanvasFilter {
    pub(super) fn new(
        source_scope: Option<String>,
        query: Option<String>,
        graph_version: GraphVersion,
        limit: usize,
    ) -> Self {
        Self {
            source_scope: normalized_filter(source_scope),
            query: normalized_filter(query),
            graph_version,
            limit,
        }
    }

    pub(super) fn sql_limit(&self) -> i64 {
        i64::try_from(self.limit.saturating_add(1)).unwrap_or(i64::MAX)
    }
}

fn normalized_filter(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_owned())
        .filter(|trimmed| !trimmed.is_empty())
}

pub(super) struct CanvasBuilder {
    nodes: BTreeMap<String, GraphCanvasStorageNode>,
    edges: BTreeMap<String, GraphCanvasStorageEdge>,
    available_kinds: BTreeSet<String>,
    limit: usize,
    truncated: bool,
}

impl CanvasBuilder {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            available_kinds: BTreeSet::new(),
            limit,
            truncated: false,
        }
    }

    pub(super) fn observe_query_len(&mut self, len: usize) {
        if len > self.limit {
            self.truncated = true;
        }
    }

    pub(super) fn insert_scope_node(&mut self, scope: &str, graph_version: GraphVersion) {
        self.insert_node(GraphCanvasStorageNode {
            id: scope_node_id(scope),
            kind: "source_scope".to_owned(),
            label: scope.to_owned(),
            subtitle: Some("source scope".to_owned()),
            source_scope: Some(scope.to_owned()),
            graph_version,
            weight: 3,
            status: None,
            details: detail_map([("source_scope", scope)]),
        });
    }

    pub(super) fn insert_node(&mut self, node: GraphCanvasStorageNode) {
        self.available_kinds.insert(node.kind.clone());
        if self.nodes.contains_key(&node.id) {
            return;
        }
        if self.total_items() >= self.limit {
            self.truncated = true;
            return;
        }
        self.nodes.insert(node.id.clone(), node);
    }

    pub(super) fn insert_edge(&mut self, edge: GraphCanvasStorageEdge) {
        self.available_kinds.insert(edge.kind.clone());
        if self.edges.contains_key(&edge.id) {
            return;
        }
        if !self.nodes.contains_key(&edge.source) || !self.nodes.contains_key(&edge.target) {
            return;
        }
        if self.total_items() >= self.limit {
            self.truncated = true;
            return;
        }
        self.edges.insert(edge.id.clone(), edge);
    }

    fn total_items(&self) -> usize {
        self.nodes.len() + self.edges.len()
    }

    pub(super) fn into_snapshot(self) -> GraphCanvasStorageSnapshot {
        GraphCanvasStorageSnapshot {
            nodes: self.nodes.into_values().collect(),
            edges: self.edges.into_values().collect(),
            available_kinds: self.available_kinds.into_iter().collect(),
            truncated: self.truncated,
        }
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
