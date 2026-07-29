use std::collections::BTreeMap;

use serde_json::Value;

use super::{
    CaseObservation, EvaluationObservation, MetricObservation, PreviousCase, RATIO_EPSILON,
    ScoreComponents, score_math::clamp,
};

const CASE_SCORE_EPSILON: f64 = 0.005;
const METRIC_RELATIVE_EPSILON: f64 = 0.03;
const METRIC_ABSOLUTE_EPSILON: f64 = 25.0;

pub(super) fn changes(
    observation: &EvaluationObservation,
    current: ScoreComponents,
    previous_run: Option<&Value>,
    improved: bool,
) -> Vec<Value> {
    let Some(previous) = previous_run else {
        return Vec::new();
    };
    let mut changes = Vec::new();
    for (name, value) in [
        ("score", current.score),
        ("foundational_capability", current.foundational_capability),
        ("competitive_capability", current.competitive_capability),
        ("semantic_vector", current.semantic_vector),
        ("performance", current.performance),
        ("stability", current.stability),
    ] {
        push_score_change(
            &mut changes,
            name,
            value,
            previous_number(previous, name),
            improved,
        );
    }
    if let Some(value) = current.research_judge {
        push_score_change(
            &mut changes,
            "research_judge",
            value,
            previous_number(previous, "research_judge"),
            improved,
        );
    }
    for gate in &observation.gates {
        let Some(previous_passed) = previous_gate_passed(previous, &gate.name) else {
            continue;
        };
        if gate.passed != previous_passed
            && ((improved && gate.passed) || (!improved && !gate.passed))
        {
            changes.push(serde_json::json!({
                "kind": "gate",
                "name": gate.name,
                "previous": previous_passed,
                "current": gate.passed
            }));
        }
    }
    for case in &observation.cases {
        let Some(previous_case) = previous_case(previous, &case.case_id) else {
            continue;
        };
        if case.passed != previous_case.passed
            && ((improved && case.passed) || (!improved && !case.passed))
        {
            changes.push(serde_json::json!({
                "kind": "case",
                "name": case.case_id,
                "previous": previous_case.passed,
                "current": case.passed
            }));
            continue;
        }
        push_case_quality_changes(&mut changes, case, previous_case, improved);
    }
    let previous_metrics = previous_metrics(previous);
    for metric in &observation.metrics {
        if let Some(previous_value) = previous_metrics.get(&metric.name).copied() {
            let threshold =
                (previous_value.abs() * METRIC_RELATIVE_EPSILON).max(METRIC_ABSOLUTE_EPSILON);
            let delta = metric.value - previous_value;
            let better = if metric.lower_is_better {
                delta < -threshold
            } else {
                delta > threshold
            };
            let worse = if metric.lower_is_better {
                delta > threshold
            } else {
                delta < -threshold
            };
            if (improved && better) || (!improved && worse) {
                changes.push(serde_json::json!({
                    "kind": "metric",
                    "name": metric.name,
                    "previous": previous_value,
                    "current": metric.value
                }));
            }
        }
    }
    changes
}

fn push_score_change(
    changes: &mut Vec<Value>,
    name: &str,
    current: f64,
    previous: f64,
    improved: bool,
) {
    let delta = current - previous;
    if (improved && delta > RATIO_EPSILON) || (!improved && delta < -RATIO_EPSILON) {
        changes.push(serde_json::json!({
            "kind": "score_component",
            "name": name,
            "previous": previous,
            "current": current,
        }));
    }
}

pub(super) fn metric_budget_failures(metrics: &[MetricObservation]) -> Vec<Value> {
    metrics
        .iter()
        .filter(|metric| metric.key && metric.budget.is_some() && metric.score() < 1.0)
        .map(|metric| {
            serde_json::json!({
                "name": metric.name,
                "value": metric.value,
                "budget": metric.budget,
            })
        })
        .collect()
}

pub(super) fn bug_fix_priority_improved(improvements: &[Value]) -> bool {
    improvements.iter().any(|item| {
        matches!(
            item.get("kind").and_then(Value::as_str),
            Some("case" | "gate")
        )
    })
}

pub(super) fn objective_scores(
    cases: &[CaseObservation],
    objective: &str,
    aliases: &[&str],
) -> Vec<f64> {
    cases
        .iter()
        .filter(|case| case.objective == objective || aliases.contains(&case.objective.as_str()))
        .map(CaseObservation::score)
        .collect()
}

pub(super) fn previous_number(run: &Value, name: &str) -> f64 {
    run.get(name).and_then(Value::as_f64).unwrap_or(0.0)
}

fn push_case_quality_changes(
    changes: &mut Vec<Value>,
    case: &CaseObservation,
    previous: PreviousCase,
    improved: bool,
) {
    let current_score = case.score();
    let score_delta = current_score - previous.score;
    if (improved && score_delta > CASE_SCORE_EPSILON)
        || (!improved && score_delta < -CASE_SCORE_EPSILON)
    {
        changes.push(serde_json::json!({
            "kind": "case_score",
            "name": case.case_id,
            "previous": previous.score,
            "current": current_score
        }));
    }
    let rank_better = optional_rank_better(case.rank, previous.rank);
    if (improved && rank_better == Some(true)) || (!improved && rank_better == Some(false)) {
        changes.push(serde_json::json!({
            "kind": "case_rank",
            "name": case.case_id,
            "previous": previous.rank,
            "current": case.rank
        }));
    }
    if case.false_positive_count != previous.false_positive_count
        && ((improved && case.false_positive_count < previous.false_positive_count)
            || (!improved && case.false_positive_count > previous.false_positive_count))
    {
        changes.push(serde_json::json!({
            "kind": "case_false_positive_count",
            "name": case.case_id,
            "previous": previous.false_positive_count,
            "current": case.false_positive_count
        }));
    }
}

fn optional_rank_better(current: Option<usize>, previous: Option<usize>) -> Option<bool> {
    match (current, previous) {
        (Some(current), Some(previous)) if current != previous => Some(current < previous),
        (Some(_), None) => Some(true),
        (None, Some(_)) => Some(false),
        _ => None,
    }
}

fn previous_case(run: &Value, case_id: &str) -> Option<PreviousCase> {
    let case = run
        .get("cases")
        .and_then(Value::as_array)
        .and_then(|cases| {
            cases
                .iter()
                .find(|case| case.get("case_id").and_then(Value::as_str) == Some(case_id))
        })?;
    let passed = case.get("passed").and_then(Value::as_bool)?;
    let rank = case
        .get("rank")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let false_positive_count = case
        .get("false_positive_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or_default();
    Some(PreviousCase {
        passed,
        rank,
        false_positive_count,
        score: previous_case_score(case, passed, rank, false_positive_count),
    })
}

fn previous_case_score(
    case: &Value,
    passed: bool,
    rank: Option<usize>,
    false_positive_count: usize,
) -> f64 {
    if !passed {
        return 0.0;
    }
    if let Some(score) = case.get("score_override").and_then(Value::as_f64) {
        return clamp(score);
    }
    let rank_score = match rank {
        Some(rank) if rank > 0 => 1.0 / rank as f64,
        _ => 1.0,
    };
    (rank_score - (false_positive_count as f64 * 0.1).min(0.5)).max(0.0)
}

fn previous_gate_passed(run: &Value, gate_name: &str) -> Option<bool> {
    run.get("gates")
        .and_then(Value::as_array)
        .and_then(|gates| {
            gates
                .iter()
                .find(|gate| gate.get("name").and_then(Value::as_str) == Some(gate_name))
        })
        .and_then(|gate| gate.get("passed"))
        .and_then(Value::as_bool)
}

pub(super) fn previous_metrics(run: &Value) -> BTreeMap<String, f64> {
    run.get("metrics")
        .and_then(Value::as_array)
        .map(|metrics| {
            metrics
                .iter()
                .filter_map(|metric| {
                    Some((
                        metric.get("name")?.as_str()?.to_owned(),
                        metric.get("value")?.as_f64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "change_detection_tests.rs"]
mod change_detection_tests;
