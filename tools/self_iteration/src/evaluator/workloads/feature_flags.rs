//! Feature-flag CLI execution and projection into shared evidence scoring.
use std::path::Path;

use serde_json::Value;

use crate::{
    cases::{number_or, string_field, string_or, string_vec},
    command::CommandResult,
    scoring::CaseObservation,
};

pub(super) fn query_command(
    binary: &Path,
    alias: &str,
    reference: &str,
    case: &Value,
) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo".into(),
        "feature-flags".into(),
        alias.into(),
        "--ref".into(),
        string_or(case, "ref", reference).into(),
        "--freshness".into(),
        "wait-until-fresh".into(),
        "--limit".into(),
        number_or(case, "limit", 20).to_string(),
        "--format".into(),
        "json".into(),
    ];
    for (field, option) in [
        ("query", "--query"),
        ("domain", "--domain"),
        ("source", "--source"),
    ] {
        if let Some(value) = string_field(case, field) {
            command.extend([option.into(), value.into()]);
        }
    }
    for (field, option) in [
        ("path_filters", "--path"),
        ("language_filters", "--language"),
    ] {
        for value in string_vec(case, field) {
            command.extend([option.into(), value]);
        }
    }
    if let Some(value) = case.get("hot_reload").and_then(Value::as_bool) {
        command.extend(["--hot-reload".into(), value.to_string()]);
    }
    if case.get("consistency").and_then(Value::as_bool) == Some(true) {
        command.push("--consistency".into());
    }
    command
}

pub(super) fn score(repo: &str, case: &Value, result: &CommandResult) -> CaseObservation {
    let Ok(mut payload) = serde_json::from_str::<Value>(&result.stdout) else {
        return super::repository_scoring::score_query_case(repo, case, result);
    };
    let flags = payload.get("flags").and_then(Value::as_array);
    if flags.is_none_or(|flags| {
        flags.iter().any(|flag| {
            flag.get("source_key").and_then(Value::as_str).is_none()
                || flag
                    .get("usages")
                    .and_then(Value::as_array)
                    .is_none_or(|usages| {
                        usages.iter().any(|usage| {
                            usage.get("path").and_then(Value::as_str).is_none()
                                || usage.get("edge_kind").and_then(Value::as_str).is_none()
                        })
                    })
        })
    }) {
        let mut observation = super::repository_scoring::score_query_case(repo, case, result);
        observation.passed = false;
        observation.score_override = Some(0.0);
        observation.message =
            "feature-flags response must contain valid keys and source usages".into();
        return observation;
    }
    let flags = flags.expect("validated flags array");
    let hits = flags
        .iter()
        .flat_map(|flag| {
            flag.get("usages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|usage| {
                    let mut hit = usage.clone();
                    hit["name"] = flag["source_key"].clone();
                    hit["kind"] = usage["edge_kind"].clone();
                    hit
                })
        })
        .collect::<Vec<_>>();
    payload["results"] = Value::Array(hits);
    let projected = CommandResult {
        stdout: payload.to_string(),
        ..result.clone()
    };
    super::repository_scoring::score_query_case(repo, case, &projected)
}

#[cfg(test)]
#[path = "feature_flags_tests.rs"]
mod tests;
