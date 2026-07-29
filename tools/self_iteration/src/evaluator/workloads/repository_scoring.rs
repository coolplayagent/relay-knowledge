use serde_json::Value;

use crate::{
    cases::{number_or, string_field, string_or},
    command::CommandResult,
    scoring::{
        CaseObservation, RankedAssessment, array_field as score_array_field, assess_ranked_hits,
    },
};

use super::super::parse_json_case_output;
use super::{
    case_scoring::{failed_case, payload_constraint_failures},
    selection::is_guardrail_case,
};

pub(super) fn score_query_case(
    repo_name: &str,
    case: &Value,
    result: &CommandResult,
) -> CaseObservation {
    let objective = repository_case_objective(case);
    if !result.passed() {
        return failed_case(case, repo_name, &objective, result);
    }
    let payload = match parse_json_case_output(case, repo_name, &objective, result) {
        Ok(payload) => payload,
        Err(observation) => return *observation,
    };
    let hits = score_array_field(&payload, "results");
    let expected = score_array_field(case, "expected");
    let forbidden = score_array_field(case, "forbidden");
    let payload_failures = payload_constraint_failures(case, &payload, hits.len());
    let mut assessment = assess_ranked_hits(case, hits, expected, forbidden);
    assessment.failures.extend(payload_failures.clone());
    if !payload_failures.is_empty() {
        assessment.details = format!(
            "{} payload_failures={}",
            assessment.details,
            payload_failures.join("; ")
        );
    }
    let mut rank = assessment.rank;
    let mut passed = assessment.failures.is_empty();
    if case
        .get("expect_empty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let mut failures = if hits.is_empty() {
            Vec::new()
        } else {
            vec![format!("expected_empty_results={}", hits.len())]
        };
        failures.extend(payload_failures);
        passed = failures.is_empty();
        rank = passed.then_some(0);
        assessment = RankedAssessment {
            rank,
            false_positive_count: 0,
            score: if passed { 1.0 } else { 0.0 },
            details: if failures.is_empty() {
                "expect_empty".to_owned()
            } else {
                format!("expect_empty failures={}", failures.join("; "))
            },
            failures,
        };
    }
    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: repo_name.to_owned(),
        passed,
        guardrail: is_guardrail_case(case),
        rank,
        max_rank: number_or(case, "max_rank", 1) as usize,
        false_positive_count: assessment.false_positive_count,
        message: format!(
            "results={} rank={rank:?} {}",
            hits.len(),
            assessment.details
        ),
        objective,
        score_override: Some(assessment.score),
    }
}

pub(super) fn score_software_case(
    repo_name: &str,
    case: &Value,
    result: &CommandResult,
) -> CaseObservation {
    let objective = repository_case_objective(case);
    if !result.passed() {
        return failed_case(case, repo_name, &objective, result);
    }
    let payload = match parse_json_case_output(case, repo_name, &objective, result) {
        Ok(payload) => payload,
        Err(observation) => return *observation,
    };
    let hits = software_hits_for_kind(&payload, string_or(case, "kind", "all"));
    let expected = score_array_field(case, "expected");
    let forbidden = score_array_field(case, "forbidden");
    let payload_failures = payload_constraint_failures(case, &payload, hits.len());
    let mut assessment = assess_ranked_hits(case, &hits, expected, forbidden);
    assessment.failures.extend(payload_failures.clone());
    if !payload_failures.is_empty() {
        assessment.details = format!(
            "{} payload_failures={}",
            assessment.details,
            payload_failures.join("; ")
        );
    }
    let mut rank = assessment.rank;
    let mut passed = assessment.failures.is_empty();
    if case
        .get("expect_empty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let mut failures = if hits.is_empty() {
            Vec::new()
        } else {
            vec![format!("expected_empty_results={}", hits.len())]
        };
        failures.extend(payload_failures);
        passed = failures.is_empty();
        rank = passed.then_some(0);
        assessment = RankedAssessment {
            rank,
            false_positive_count: 0,
            score: if passed { 1.0 } else { 0.0 },
            details: if failures.is_empty() {
                "expect_empty".to_owned()
            } else {
                format!("expect_empty failures={}", failures.join("; "))
            },
            failures,
        };
    }
    CaseObservation {
        case_id: string_or(case, "id", "software_case").to_owned(),
        repository: repo_name.to_owned(),
        passed,
        guardrail: is_guardrail_case(case),
        rank,
        max_rank: number_or(case, "max_rank", 1) as usize,
        false_positive_count: assessment.false_positive_count,
        message: format!(
            "software_kind={} results={} rank={rank:?} {}",
            string_or(case, "kind", "all"),
            hits.len(),
            assessment.details
        ),
        objective,
        score_override: Some(assessment.score),
    }
}

fn software_hits_for_kind(payload: &Value, kind: &str) -> Vec<Value> {
    let mut hits = Vec::new();
    match kind {
        "dependencies" => {
            append_software_hits(payload, "components", "component", &mut hits);
            append_software_hits(payload, "dependency_usages", "dependency_usage", &mut hits);
        }
        "sdks" => append_software_hits(payload, "sdk_usages", "sdk_usage", &mut hits),
        "files" => append_software_hits(payload, "files", "file", &mut hits),
        "topics" => append_software_hits(payload, "topics", "topic", &mut hits),
        "relationships" => {
            append_software_hits(payload, "relationships", "relationship", &mut hits);
        }
        "build" => append_software_hits(payload, "build_targets", "build_target", &mut hits),
        "iac" => append_software_hits(payload, "iac_resources", "iac_resource", &mut hits),
        "design" => append_software_hits(payload, "design_elements", "design_element", &mut hits),
        _ => {
            append_software_hits(payload, "components", "component", &mut hits);
            append_software_hits(payload, "dependency_usages", "dependency_usage", &mut hits);
            append_software_hits(payload, "sdk_usages", "sdk_usage", &mut hits);
            append_software_hits(payload, "files", "file", &mut hits);
            append_software_hits(payload, "topics", "topic", &mut hits);
            append_software_hits(payload, "relationships", "relationship", &mut hits);
            append_software_hits(payload, "build_targets", "build_target", &mut hits);
            append_software_hits(payload, "iac_resources", "iac_resource", &mut hits);
            append_software_hits(payload, "design_elements", "design_element", &mut hits);
        }
    }
    hits
}

fn append_software_hits(payload: &Value, field: &str, slice: &str, hits: &mut Vec<Value>) {
    for item in score_array_field(payload, field) {
        let mut hit = item.clone();
        if let Some(object) = hit.as_object_mut() {
            object.insert("software_slice".to_owned(), Value::String(slice.to_owned()));
        }
        hits.push(hit);
    }
}

pub(super) fn repository_case_objective(case: &Value) -> String {
    if let Some(objective) = string_field(case, "objective").filter(|value| !value.is_empty()) {
        return objective.to_owned();
    }
    let kind = string_or(case, "kind", "").to_ascii_lowercase();
    let case_id = string_or(case, "id", "").to_ascii_lowercase();
    let competitive_kinds = ["hybrid", "callers", "callees"];
    let markers = [
        "hybrid",
        "fuzzy",
        "full_scope",
        "fanout",
        "callers",
        "callees",
    ];
    if competitive_kinds.contains(&kind.as_str())
        || markers.iter().any(|marker| case_id.contains(marker))
    {
        "competitive_capability".to_owned()
    } else {
        "foundational_capability".to_owned()
    }
}

#[cfg(test)]
#[path = "repository_scoring_tests.rs"]
mod tests;
