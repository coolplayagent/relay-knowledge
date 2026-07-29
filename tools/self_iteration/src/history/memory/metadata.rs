use std::{collections::BTreeSet, fs, path::PathBuf};

use serde_json::Value;

use crate::{candidate_git::changed_paths_from_diff, history};

pub(super) fn markdown_list(title: &str, values: &[String]) -> String {
    let body = if values.is_empty() {
        "- none recorded".to_owned()
    } else {
        values
            .iter()
            .map(|value| format!("- {value}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!("## {title}\n\n{body}")
}

pub(super) fn changed_paths(record: &Value) -> Vec<String> {
    if let Some(paths) = record
        .get("optimization_plan")
        .and_then(|plan| plan.get("changed_paths"))
        .and_then(Value::as_array)
    {
        return paths
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect();
    }
    record
        .get("patch")
        .and_then(|patch| patch.get("path"))
        .and_then(Value::as_str)
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|diff| changed_paths_from_diff(&diff))
        .unwrap_or_default()
}

pub(super) fn related_paths(record: &Value) -> Vec<String> {
    [
        record
            .get("patch")
            .and_then(|patch| patch.get("path"))
            .and_then(Value::as_str),
        record.get("report").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .map(ToOwned::to_owned)
    .collect()
}

pub(super) fn score_impact(record: &Value) -> Value {
    serde_json::json!({
        "accepted": history::adopted(record),
        "score_accepted": record.get("score_accepted").cloned().unwrap_or(Value::Null),
        "score": record.get("score").cloned().unwrap_or(Value::Null),
        "foundational_capability": record.get("foundational_capability").cloned().unwrap_or(Value::Null),
        "competitive_capability": record.get("competitive_capability").cloned().unwrap_or(Value::Null),
        "semantic_vector": record.get("semantic_vector").cloned().unwrap_or(Value::Null),
        "research_judge": record.get("research_judge").cloned().unwrap_or(Value::Null),
        "performance": record.get("performance").cloned().unwrap_or(Value::Null),
        "stability": record.get("stability").cloned().unwrap_or(Value::Null),
        "improvement_count": value_array(record, "improvements").len(),
        "degradation_count": value_array(record, "degradations").len(),
    })
}

pub(super) fn run_tags(record: &Value, kind: &str) -> Vec<String> {
    let mut tags = BTreeSet::from([
        safe_tag(kind),
        if history::adopted(record) {
            "accepted".to_owned()
        } else {
            "rejected".to_owned()
        },
    ]);
    for path in changed_paths(record).into_iter().take(8) {
        tags.insert(safe_tag(&path));
    }
    for gate in failed_gate_names(record).into_iter().take(4) {
        tags.insert(safe_tag(&gate));
    }
    tags.into_iter().collect()
}

pub(super) fn failed_gate_names(record: &Value) -> Vec<String> {
    value_array(record, "gates")
        .iter()
        .filter(|gate| !gate.get("passed").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|gate| gate.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn key_metric_lines(record: &Value) -> Vec<String> {
    value_array(record, "metrics")
        .iter()
        .take(8)
        .filter_map(|metric| {
            Some(format!(
                "{}={}",
                metric.get("name")?.as_str()?,
                metric.get("value")?
            ))
        })
        .collect()
}

pub(super) fn case_signal_lines(record: &Value) -> Vec<String> {
    value_array(record, "cases")
        .iter()
        .filter(|case| !case.get("passed").and_then(Value::as_bool).unwrap_or(false))
        .take(8)
        .map(|case| {
            format!(
                "{} failed: {}",
                string_field(case, "case_id"),
                string_field(case, "message")
            )
        })
        .collect()
}

pub(super) fn patch_changed_paths(path: &PathBuf, run: Option<&Value>) -> Vec<String> {
    if let Some(run) = run {
        let paths = changed_paths(run);
        if !paths.is_empty() {
            return paths;
        }
    }
    fs::read_to_string(path)
        .map(|diff| changed_paths_from_diff(&diff))
        .unwrap_or_default()
}

pub(super) fn value_array<'a>(record: &'a Value, name: &str) -> &'a [Value] {
    record
        .get(name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub(super) fn string_array(record: &Value, name: &str) -> Vec<String> {
    value_array(record, name)
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn field_string(record: &Value, name: &str) -> String {
    record.get(name).map(Value::to_string).unwrap_or_default()
}

pub(super) fn string_field(record: &Value, name: &str) -> String {
    record
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

pub(super) fn safe_id(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(160)
        .collect::<String>();
    if slug.is_empty() {
        "memory".to_owned()
    } else {
        slug
    }
}

fn safe_tag(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '/' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(80)
        .collect()
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod metadata_tests;
