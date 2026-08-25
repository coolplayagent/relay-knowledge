use serde_json::Value;

use super::{
    EvaluationObservation, RATIO_EPSILON, ScoreBaselines, ScoreComponents,
    capability::pareto_improved,
    change_detection::{bug_fix_priority_improved, previous_number},
};

const SCORE_EPSILON: f64 = 0.0005;

pub(super) fn reject_reasons(
    observation: &EvaluationObservation,
    current: ScoreComponents,
    baselines: ScoreBaselines<'_>,
    improvements: &[Value],
) -> Vec<String> {
    let mut reasons = Vec::new();
    if !observation.generated_diff {
        reasons.push("codex produced no candidate diff".to_owned());
    }
    let failed_gates = observation
        .gates
        .iter()
        .filter(|gate| !gate.passed)
        .map(|gate| gate.name.clone())
        .collect::<Vec<_>>();
    if !failed_gates.is_empty() {
        reasons.push(format!("quality gates failed: {}", failed_gates.join(", ")));
    }
    let failed_key_metrics = observation
        .metrics
        .iter()
        .filter(|metric| metric.key_budget_failed())
        .map(|metric| {
            format!(
                "{}={} budget={}",
                metric.name,
                metric.value,
                metric.budget.unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    if !failed_key_metrics.is_empty() {
        reasons.push(format!(
            "key metric budgets failed: {}",
            failed_key_metrics.join(", ")
        ));
    }
    let bug_fix_priority = bug_fix_priority_improved(improvements);
    let Some(previous) = baselines.workload_previous else {
        if let Some(reason) = profile_best_score_reject_reason(
            current,
            baselines.profile_best_accepted,
            bug_fix_priority,
        ) {
            reasons.push(reason);
        }
        return reasons;
    };
    for (name, value) in [
        ("foundational_capability", current.foundational_capability),
        ("competitive_capability", current.competitive_capability),
        ("semantic_vector", current.semantic_vector),
        ("stability", current.stability),
    ] {
        if value + RATIO_EPSILON < previous_number(previous, name) {
            reasons.push(format!("{name} regressed"));
        }
    }
    if let Some(value) = current.research_judge {
        if value + RATIO_EPSILON < previous_number(previous, "research_judge") {
            reasons.push("research_judge regressed".to_owned());
        }
    }
    if reasons.iter().any(|reason| reason.contains("regressed")) {
        return reasons;
    }
    if let Some(reason) =
        profile_best_score_reject_reason(current, baselines.profile_best_accepted, bug_fix_priority)
    {
        reasons.push(reason);
    }
    if bug_fix_priority
        || current.score > previous_number(previous, "score") + SCORE_EPSILON
        || pareto_improved(current, previous)
    {
        return reasons;
    }
    let metric_improvement_count = improvements
        .iter()
        .filter(|item| item.get("kind").and_then(Value::as_str) == Some("metric"))
        .count();
    if metric_improvement_count > 0 {
        reasons.push(format!(
            "local metric improvements ({metric_improvement_count}) did not beat latest baseline score delta {:+.6}",
            current.score - previous_number(previous, "score")
        ));
    }
    reasons.push("candidate did not improve score or tracked objectives beyond epsilon".to_owned());
    reasons
}

fn profile_best_score_reject_reason(
    current: ScoreComponents,
    profile_best_accepted: Option<&Value>,
    bug_fix_priority: bool,
) -> Option<String> {
    if bug_fix_priority {
        return None;
    }
    let previous = profile_best_accepted?;
    let profile_best_score = previous_number(previous, "score");
    if current.score > profile_best_score + SCORE_EPSILON {
        return None;
    }
    Some(format!(
        "candidate score {:.6} did not beat profile best accepted score {:.6} beyond epsilon",
        current.score, profile_best_score
    ))
}

#[cfg(test)]
#[path = "decision_tests.rs"]
mod decision_tests;
