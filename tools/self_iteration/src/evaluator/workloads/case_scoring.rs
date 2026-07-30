use serde_json::Value;

use crate::{
    cases::{number_or, string_or},
    command::CommandResult,
    scoring::CaseObservation,
};

pub(super) fn payload_constraint_failures(
    case: &Value,
    payload: &Value,
    results_len: usize,
) -> Vec<String> {
    let mut failures = Vec::new();
    if let Some(max_results) = case.get("max_results").and_then(Value::as_u64) {
        if results_len > max_results as usize {
            failures.push(format!("results={results_len} max_results={max_results}"));
        }
    }
    if let Some(expected) = case.get("truncated").and_then(Value::as_bool) {
        let actual = payload
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if actual != expected {
            failures.push(format!("truncated={actual} expected={expected}"));
        }
    }
    if case.get("degraded_reason").is_some() {
        let actual = payload.get("degraded_reason").and_then(Value::as_str);
        match case.get("degraded_reason").expect("checked above") {
            Value::Null if actual.is_some() => {
                failures.push(format!("degraded_reason={}", actual.unwrap_or_default()));
            }
            Value::Bool(false) if actual.is_some() => {
                failures.push(format!("degraded_reason={}", actual.unwrap_or_default()));
            }
            Value::String(expected) if actual != Some(expected.as_str()) => {
                failures.push(format!(
                    "degraded_reason={} expected={expected}",
                    actual.unwrap_or("missing")
                ));
            }
            _ => {}
        }
    }
    if let Some(expected) = case
        .get("degraded_reason_contains")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        let actual = payload
            .get("degraded_reason")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !actual.contains(expected) {
            failures.push(format!("degraded_reason={actual} missing={expected}"));
        }
    }
    failures
}

pub(super) fn parse_json_case_output(
    case: &Value,
    repository: &str,
    objective: &str,
    result: &CommandResult,
) -> Result<Value, Box<CaseObservation>> {
    crate::evaluator::runtime::reporting::parse_json_output_value(&result.stdout).ok_or_else(|| {
        Box::new(CaseObservation {
            case_id: string_or(case, "id", "case").to_owned(),
            repository: repository.to_owned(),
            passed: false,
            guardrail: is_guardrail_case(case),
            rank: None,
            max_rank: number_or(case, "max_rank", 1) as usize,
            false_positive_count: 0,
            message: "invalid JSON output from --format json command".to_owned(),
            objective: objective.to_owned(),
            score_override: Some(0.0),
        })
    })
}

pub(super) fn failed_case(
    case: &Value,
    repository: &str,
    objective: &str,
    result: &CommandResult,
) -> CaseObservation {
    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: repository.to_owned(),
        passed: false,
        guardrail: is_guardrail_case(case),
        rank: None,
        max_rank: number_or(case, "max_rank", 1) as usize,
        false_positive_count: 0,
        message: result.gate_message(),
        objective: objective.to_owned(),
        score_override: None,
    }
}

pub(super) fn is_guardrail_case(case: &Value) -> bool {
    case.get("guardrail")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "case_scoring_tests.rs"]
mod tests;
