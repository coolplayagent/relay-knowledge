//! Reviewable bounded graph neighborhoods and Mermaid with escaped authored labels.
use crate::{api::ApiError, domain::research::AuthoredEvidenceGraph};
use std::collections::BTreeSet;

pub(super) fn graph_view(
    mut graph: AuthoredEvidenceGraph,
    focus: Option<&str>,
) -> Result<serde_json::Value, ApiError> {
    if let Some(focus) = focus {
        if !graph.nodes.iter().any(|node| node.id == focus) {
            return Err(ApiError::invalid_argument("unknown neighborhood node"));
        }
        graph
            .edges
            .retain(|edge| edge.source == focus || edge.target == focus);
        let mut ids = BTreeSet::from([focus.to_owned()]);
        for edge in &graph.edges {
            ids.insert(edge.source.clone());
            ids.insert(edge.target.clone());
        }
        graph.nodes.retain(|node| ids.contains(&node.id));
    }
    let mut mermaid = String::from("graph TD\n");
    for (index, node) in graph.nodes.iter().enumerate() {
        mermaid.push_str(&format!("  n{index}[\"{}\"]\n", escape_label(&node.label)));
    }
    for edge in &graph.edges {
        let source = graph.nodes.iter().position(|node| node.id == edge.source);
        let target = graph.nodes.iter().position(|node| node.id == edge.target);
        if let (Some(source), Some(target)) = (source, target) {
            mermaid.push_str(&format!(
                "  n{source} -->|\"{}\"| n{target}\n",
                escape_label(&edge.relation)
            ));
        }
    }
    Ok(
        serde_json::json!({"graph":graph, "mermaid":mermaid, "focus":focus, "hops":if focus.is_some() {Some(1)} else {None}}),
    )
}

fn escape_label(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '&' => "&amp;".into(),
            '"' => "&quot;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '|' => "&#124;".into(),
            '\\' => "&#92;".into(),
            '`' => "&#96;".into(),
            '\n' | '\r' => " ".into(),
            value if value.is_control() => String::new(),
            value => value.to_string(),
        })
        .collect()
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
