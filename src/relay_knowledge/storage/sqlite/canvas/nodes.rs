use std::collections::BTreeMap;

use crate::{domain::GraphVersion, storage::GraphCanvasStorageNode};

pub(super) fn entity_node(
    id: &str,
    label: &str,
    graph_version: GraphVersion,
    source_scope: Option<String>,
) -> GraphCanvasStorageNode {
    GraphCanvasStorageNode {
        id: entity_node_id(id),
        kind: "entity".to_owned(),
        label: label.to_owned(),
        subtitle: source_scope.clone(),
        source_scope,
        graph_version,
        weight: 3,
        status: None,
        details: detail_map([("id", id), ("label", label)]),
    }
}

pub(super) fn detail_map<const N: usize>(pairs: [(&str, &str); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

pub(super) fn truncate_label(value: &str, max_chars: usize) -> String {
    let mut text = value.trim().replace('\n', " ");
    if text.chars().count() > max_chars {
        text = text.chars().take(max_chars.saturating_sub(1)).collect();
        text.push_str("...");
    }
    text
}

pub(super) fn entity_node_id(id: &str) -> String {
    format!("entity:{id}")
}

pub(super) fn evidence_node_id(id: &str) -> String {
    format!("evidence:{id}")
}

pub(super) fn claim_node_id(id: &str) -> String {
    format!("claim:{id}")
}

pub(super) fn event_node_id(id: &str) -> String {
    format!("event:{id}")
}

pub(super) fn scope_node_id(scope: &str) -> String {
    format!("scope:{scope}")
}

pub(super) fn code_file_node_id(scope: &str, path: &str) -> String {
    format!("code-file:{scope}:{path}")
}

pub(super) fn code_symbol_node_id(scope: &str, path: &str, symbol_id: &str) -> String {
    format!("code-symbol:{scope}:{path}:{symbol_id}")
}

#[cfg(test)]
#[path = "nodes_tests.rs"]
mod tests;
