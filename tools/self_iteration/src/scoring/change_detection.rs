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

fn metric_budget_failures(metrics: &[MetricObservation]) -> Vec<Value> {
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

fn bug_fix_priority_improved(improvements: &[Value]) -> bool {
    improvements.iter().any(|item| {
        matches!(
            item.get("kind").and_then(Value::as_str),
            Some("case" | "gate")
        )
    })
}

fn objective_scores(cases: &[CaseObservation], objective: &str, aliases: &[&str]) -> Vec<f64> {
    cases
        .iter()
        .filter(|case| case.objective == objective || aliases.contains(&case.objective.as_str()))
        .map(CaseObservation::score)
        .collect()
}

fn previous_number(run: &Value, name: &str) -> f64 {
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

fn previous_metrics(run: &Value) -> BTreeMap<String, f64> {
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
