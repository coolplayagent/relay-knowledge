use serde_json::Value;

use crate::history;

use super::metadata::{
    case_signal_lines, changed_paths, failed_gate_names, field_string, key_metric_lines,
    markdown_list, string_array, string_field, value_array,
};

pub(super) fn primary_reject_reason(record: &Value) -> Option<String> {
    string_array(record, "reject_reasons")
        .into_iter()
        .find(|reason| !reason.trim().is_empty())
}

fn protected_floor_summary(record: &Value) -> String {
    format!(
        "foundational={}, competitive={}, semantic_vector={}, stability={}",
        field_string(record, "foundational_capability"),
        field_string(record, "competitive_capability"),
        field_string(record, "semantic_vector"),
        field_string(record, "stability")
    )
}

pub(super) fn compact_paths(record: &Value, limit: usize) -> String {
    let paths = changed_paths(record);
    if paths.is_empty() {
        return "none recorded".to_owned();
    }
    let omitted = paths.len().saturating_sub(limit);
    let mut selected = paths.into_iter().take(limit).collect::<Vec<_>>();
    if omitted > 0 {
        selected.push(format!("+{omitted} more"));
    }
    selected.join(", ")
}

pub(super) fn top_change_summary(record: &Value, field: &str, limit: usize) -> String {
    let changes = compact_score_changes(value_array(record, field), limit);
    if changes.is_empty() {
        "none recorded".to_owned()
    } else {
        changes.join("; ")
    }
}

fn score_delta_summary(record: &Value) -> String {
    value_array(record, "improvements")
        .iter()
        .chain(value_array(record, "degradations"))
        .find(|change| {
            change.get("kind").and_then(Value::as_str) == Some("score_component")
                && change.get("name").and_then(Value::as_str) == Some("score")
        })
        .and_then(|change| {
            Some(format!(
                "{:+.6}",
                change.get("current")?.as_f64()? - change.get("previous")?.as_f64()?
            ))
        })
        .unwrap_or_else(|| "within epsilon or unavailable".to_owned())
}

pub(super) fn primary_kind(record: &Value) -> String {
    if history::adopted(record) {
        "accepted_optimization".to_owned()
    } else if !failed_gate_names(record).is_empty() {
        "quality_gate_failure".to_owned()
    } else {
        "rejected_attempt".to_owned()
    }
}

pub(super) fn primary_title(kind: &str, record: &Value) -> String {
    let run_id = string_field(record, "run_id");
    if kind == "accepted_optimization" {
        format!("{run_id} accepted optimization")
    } else if kind == "quality_gate_failure" {
        format!(
            "{run_id} failed gates: {}",
            failed_gate_names(record).join(", ")
        )
    } else {
        format!("{run_id} rejected attempt")
    }
}

pub(super) fn primary_summary(kind: &str, record: &Value) -> String {
    let run_id = string_field(record, "run_id");
    if kind == "accepted_optimization" {
        format!(
            "Accepted run {} scored {}. Protected floors: {}. Changed paths: {}. Key improvements: {}. Known degradations: {}.",
            run_id,
            record
                .get("score")
                .map(Value::to_string)
                .unwrap_or_default(),
            protected_floor_summary(record),
            compact_paths(record, 6),
            top_change_summary(record, "improvements", 6),
            top_change_summary(record, "degradations", 4)
        )
    } else if kind == "quality_gate_failure" {
        format!(
            "Rejected run {} failed quality gates {}. Changed paths: {}. Top improvements: {}. Top degradations: {}. Inspect detail before retrying related changes.",
            run_id,
            failed_gate_names(record).join(", "),
            compact_paths(record, 6),
            top_change_summary(record, "improvements", 4),
            top_change_summary(record, "degradations", 6)
        )
    } else {
        format!(
            "Rejected run {} scored {}. Score delta: {}. Reasons: {}. Changed paths: {}. Top improvements: {}. Top degradations: {}.",
            run_id,
            record
                .get("score")
                .map(Value::to_string)
                .unwrap_or_default(),
            score_delta_summary(record),
            string_array(record, "reject_reasons").join("; "),
            compact_paths(record, 6),
            top_change_summary(record, "improvements", 6),
            top_change_summary(record, "degradations", 6)
        )
    }
}

pub(super) fn run_detail(summary: &str, record: &Value) -> String {
    [
        summary.to_owned(),
        format!(
            "## Score\n\n- score: {}\n- foundational_capability: {}\n- competitive_capability: {}\n- accuracy: {}\n- semantic_vector: {}\n- research_judge: {}\n- performance: {}\n- stability: {}",
            field_string(record, "score"),
            field_string(record, "foundational_capability"),
            field_string(record, "competitive_capability"),
            field_string(record, "accuracy"),
            field_string(record, "semantic_vector"),
            field_string(record, "research_judge"),
            field_string(record, "performance"),
            field_string(record, "stability"),
        ),
        markdown_list("Changed Paths", &changed_paths(record)),
        markdown_list("Reject Reasons", &string_array(record, "reject_reasons")),
        markdown_list("Improvements", &compact_score_changes(value_array(record, "improvements"), 12)),
        markdown_list("Degradations", &compact_score_changes(value_array(record, "degradations"), 12)),
        markdown_list("Failed Gates", &failed_gate_names(record)),
        markdown_list("Key Metrics", &key_metric_lines(record)),
        markdown_list("Case Signals", &case_signal_lines(record)),
    ]
    .join("\n\n")
}

pub fn compact_prompt_text(value: &str, limit: usize) -> String {
    let compact = value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.len() <= limit {
        return compact;
    }
    compact
        .chars()
        .rev()
        .take(limit)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

pub fn compact_score_changes(changes: &[Value], limit: usize) -> Vec<String> {
    changes
        .iter()
        .take(limit)
        .filter_map(Value::as_object)
        .map(|change| {
            let name = change
                .get("name")
                .or_else(|| change.get("case_id"))
                .or_else(|| change.get("kind"))
                .map(Value::to_string)
                .unwrap_or_default();
            format!(
                "{}:{} {}->{} {}",
                change.get("kind").and_then(Value::as_str).unwrap_or(""),
                name.trim_matches('"'),
                change
                    .get("previous")
                    .map(Value::to_string)
                    .unwrap_or_default(),
                change
                    .get("current")
                    .map(Value::to_string)
                    .unwrap_or_default(),
                change
                    .get("reason")
                    .or_else(|| change.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
            )
            .trim()
            .to_owned()
        })
        .collect()
}

#[cfg(test)]
#[path = "summaries_tests.rs"]
mod summaries_tests;
