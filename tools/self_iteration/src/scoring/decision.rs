fn reject_reasons(
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

fn changes(
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
