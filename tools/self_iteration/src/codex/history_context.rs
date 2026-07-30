use serde_json::Value;

use crate::history::{HistoryPaths, adopted, is_evaluate_run, load_runs};

pub(super) fn run_brief(run: &Value) -> String {
    format!(
        "run_id={} score={} base_score={} ceiling_bonus={} competitive={} semantic_vector={} research_judge={} performance={} reasons={}",
        value(run, "run_id"),
        value(run, "score"),
        value(run, "base_score"),
        value(run, "capability_ceiling_bonus"),
        value(run, "competitive_capability"),
        value(run, "semantic_vector"),
        value(run, "research_judge"),
        value(run, "performance"),
        run.get("reject_reasons")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default()
    )
}

pub(super) fn capability_snapshot(
    latest: Option<&Value>,
    best: Option<&Value>,
    profile_best: Option<&Value>,
) -> String {
    [
        ("latest", latest),
        ("category_best", best),
        ("profile_best", profile_best),
    ]
    .into_iter()
    .map(|(label, run)| {
        let Some(run) = run else {
            return format!("- {label}: none");
        };
        format!(
            "- {label}: score={} competitive={} semantic_vector={} research_judge={} performance={} stability={} ceiling_bonus={}",
            value(run, "score"),
            value(run, "competitive_capability"),
            value(run, "semantic_vector"),
            value(run, "research_judge"),
            value(run, "performance"),
            value(run, "stability"),
            value(run, "capability_ceiling_bonus"),
        )
    })
    .collect::<Vec<_>>()
    .join("\n")
}

pub(super) fn competitive_feature_targets(cases_config: &Value, limit: usize) -> String {
    suite_strings(cases_config, "competitive_feature_targets", limit)
}

pub(super) fn implementation_guardrails(cases_config: &Value, limit: usize) -> String {
    suite_strings(cases_config, "implementation_guardrails", limit)
}

fn suite_strings(cases_config: &Value, field: &str, limit: usize) -> String {
    let items = cases_config
        .get("research_judge_suite")
        .and_then(|suite| suite.get(field))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .take(limit)
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if items.is_empty() {
        "No research judge targets configured.".to_owned()
    } else {
        items.join("\n")
    }
}

pub(super) fn recent_rejections(paths: &HistoryPaths) -> String {
    let Ok(runs) = load_runs(paths) else {
        return "No rejected v2 historical run with reasons yet.".to_owned();
    };
    let lines = runs
        .iter()
        .rev()
        .filter(|run| !adopted(run) && !is_evaluate_run(run))
        .take(3)
        .map(|run| {
            format!(
                "- run_id={} score={} reasons={}",
                value(run, "run_id"),
                value(run, "score"),
                run.get("reject_reasons")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        "No rejected v2 historical run with reasons yet.".to_owned()
    } else {
        lines.join("\n")
    }
}

fn value(run: &serde_json::Value, name: &str) -> String {
    let value = run.get(name).unwrap_or(&serde_json::Value::Null);
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
#[path = "history_context_tests.rs"]
mod history_context_tests;
