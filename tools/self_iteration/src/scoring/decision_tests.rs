use serde_json::Value;

use crate::scoring::{
    CaseObservation, EvaluationObservation, GateObservation, MetricObservation, ScoreBaselines,
    score_evaluation,
};

#[test]
fn failed_gate_rejects() {
    let observation = EvaluationObservation {
        gates: vec![GateObservation {
            name: "cargo_test".to_owned(),
            passed: false,
            duration_ms: 1,
            message: "failed".to_owned(),
        }],
        cases: Vec::new(),
        metrics: Vec::new(),
        generated_diff: true,
    };

    let score = score_evaluation(&observation, ScoreBaselines::default());

    assert!(!score.accepted);
    assert!(score.reject_reasons[0].contains("quality gates failed"));
}

#[test]
fn key_metric_over_budget_rejects_even_with_bug_fix_priority() {
    let previous = serde_json::json!({
        "score": 0.9,
        "foundational_capability": 0.0,
        "competitive_capability": 0.0,
        "semantic_vector": 0.0,
        "performance": 1.0,
        "stability": 1.0,
        "gates": [{"name": "cargo_test", "passed": false}],
        "cases": [],
        "metrics": []
    });
    let observation = EvaluationObservation {
        gates: vec![GateObservation {
            name: "cargo_test".to_owned(),
            passed: true,
            duration_ms: 1,
            message: "fixed".to_owned(),
        }],
        cases: Vec::new(),
        metrics: vec![MetricObservation {
            name: "query_p95_ms".to_owned(),
            value: 240.0,
            budget: Some(200.0),
            lower_is_better: true,
            key: true,
        }],
        generated_diff: true,
    };

    let score = score_evaluation(
        &observation,
        ScoreBaselines {
            workload_previous: Some(&previous),
            profile_best_accepted: None,
        },
    );

    assert!(!score.accepted);
    assert!(score.improvements.iter().any(|item| {
        item.get("kind").and_then(Value::as_str) == Some("gate")
            && item.get("name").and_then(Value::as_str) == Some("cargo_test")
    }));
    assert!(
        score
            .reject_reasons
            .iter()
            .any(|reason| { reason == "key metric budgets failed: query_p95_ms=240 budget=200" })
    );
}

#[test]
fn non_key_metric_over_budget_does_not_hard_reject() {
    let observation = EvaluationObservation {
        gates: Vec::new(),
        cases: Vec::new(),
        metrics: vec![MetricObservation {
            name: "diagnostic_p95_ms".to_owned(),
            value: 240.0,
            budget: Some(200.0),
            lower_is_better: true,
            key: false,
        }],
        generated_diff: true,
    };

    let score = score_evaluation(&observation, ScoreBaselines::default());

    assert!(score.accepted);
    assert!(score.metric_budget_failures.is_empty());
}

#[test]
fn key_metric_within_budget_can_proceed() {
    let observation = EvaluationObservation {
        gates: Vec::new(),
        cases: Vec::new(),
        metrics: vec![MetricObservation {
            name: "text_fallback_ratio".to_owned(),
            value: 0.166,
            budget: Some(0.75),
            lower_is_better: true,
            key: true,
        }],
        generated_diff: true,
    };

    let score = score_evaluation(&observation, ScoreBaselines::default());

    assert!(score.accepted);
    assert!(score.metric_budget_failures.is_empty());
}

#[test]
fn higher_is_better_key_metric_below_budget_hard_rejects() {
    let observation = EvaluationObservation {
        gates: Vec::new(),
        cases: Vec::new(),
        metrics: vec![MetricObservation {
            name: "minimum_recall".to_owned(),
            value: 0.7,
            budget: Some(0.8),
            lower_is_better: false,
            key: true,
        }],
        generated_diff: true,
    };

    let score = score_evaluation(&observation, ScoreBaselines::default());

    assert!(!score.accepted);
    assert_eq!(score.metric_budget_failures.len(), 1);
    assert!(
        score
            .reject_reasons
            .iter()
            .any(|reason| { reason == "key metric budgets failed: minimum_recall=0.7 budget=0.8" })
    );
}

fn case(case_id: &str, objective: &str, score_override: f64) -> CaseObservation {
    CaseObservation {
        case_id: case_id.to_owned(),
        repository: "repo".to_owned(),
        passed: true,
        guardrail: false,
        rank: Some(1),
        max_rank: 1,
        false_positive_count: 0,
        message: "ok".to_owned(),
        objective: objective.to_owned(),
        score_override: Some(score_override),
    }
}

#[test]
fn missing_diff_rejects_without_zeroing_gate_stability() {
    let observation = EvaluationObservation {
        gates: vec![GateObservation {
            name: "cargo_test".to_owned(),
            passed: true,
            duration_ms: 1,
            message: "ok".to_owned(),
        }],
        cases: vec![
            case("foundation", "foundational_capability", 1.0),
            case("competitive", "competitive_capability", 1.0),
            case("semantic", "semantic_vector", 1.0),
        ],
        metrics: vec![MetricObservation {
            name: "query_p95_ms".to_owned(),
            value: 95.0,
            budget: Some(100.0),
            lower_is_better: true,
            key: true,
        }],
        generated_diff: false,
    };

    let score = score_evaluation(&observation, ScoreBaselines::default());

    assert_eq!(score.stability, 1.0);
    assert!(score.score > 0.95);
    assert!(!score.accepted);
    assert!(
        score
            .reject_reasons
            .iter()
            .any(|reason| reason.contains("no candidate diff"))
    );
}

#[test]
fn fixed_gate_gets_bug_fix_priority() {
    let previous = serde_json::json!({
        "score": 0.9,
        "foundational_capability": 0.0,
        "competitive_capability": 0.0,
        "semantic_vector": 0.0,
        "performance": 1.0,
        "stability": 1.0,
        "gates": [{"name": "cargo_test", "passed": false}],
        "cases": [],
        "metrics": []
    });
    let observation = EvaluationObservation {
        gates: vec![GateObservation {
            name: "cargo_test".to_owned(),
            passed: true,
            duration_ms: 1,
            message: "ok".to_owned(),
        }],
        cases: Vec::new(),
        metrics: Vec::new(),
        generated_diff: true,
    };

    let score = score_evaluation(
        &observation,
        ScoreBaselines {
            workload_previous: Some(&previous),
            profile_best_accepted: None,
        },
    );

    assert!(score.accepted);
    assert!(score.improvements.iter().any(|item| {
        item.get("kind").and_then(Value::as_str) == Some("gate")
            && item.get("name").and_then(Value::as_str) == Some("cargo_test")
    }));
}

#[test]
fn newly_added_passing_case_is_not_bug_fix_priority() {
    let previous = serde_json::json!({
        "score": 0.9,
        "foundational_capability": 1.0,
        "competitive_capability": 1.0,
        "semantic_vector": 1.0,
        "performance": 1.0,
        "stability": 1.0,
        "gates": [{"name": "cargo_test", "passed": true}],
        "cases": [],
        "metrics": []
    });
    let observation = EvaluationObservation {
        gates: vec![GateObservation {
            name: "cargo_test".to_owned(),
            passed: true,
            duration_ms: 1,
            message: "ok".to_owned(),
        }],
        cases: vec![CaseObservation {
            case_id: "new_case".to_owned(),
            repository: "repo".to_owned(),
            passed: true,
            guardrail: false,
            rank: Some(1),
            max_rank: 1,
            false_positive_count: 0,
            message: "ok".to_owned(),
            objective: "competitive_capability".to_owned(),
            score_override: Some(1.0),
        }],
        metrics: Vec::new(),
        generated_diff: true,
    };

    let score = score_evaluation(
        &observation,
        ScoreBaselines {
            workload_previous: Some(&previous),
            profile_best_accepted: None,
        },
    );

    assert!(!score.accepted);
    assert!(
        !score
            .improvements
            .iter()
            .any(|item| item.get("kind").and_then(Value::as_str) == Some("case"))
    );
}

#[test]
fn passing_case_rank_and_score_changes_are_recorded() {
    let previous = serde_json::json!({
        "score": 0.5,
        "foundational_capability": 0.5,
        "competitive_capability": 0.5,
        "semantic_vector": 0.0,
        "performance": 1.0,
        "stability": 1.0,
        "gates": [{"name": "cargo_test", "passed": true}],
        "cases": [{
            "case_id": "ranked_case",
            "passed": true,
            "rank": 4,
            "false_positive_count": 1,
            "score_override": 0.25
        }],
        "metrics": []
    });
    let observation = EvaluationObservation {
        gates: vec![GateObservation {
            name: "cargo_test".to_owned(),
            passed: true,
            duration_ms: 1,
            message: "ok".to_owned(),
        }],
        cases: vec![CaseObservation {
            case_id: "ranked_case".to_owned(),
            repository: "repo".to_owned(),
            passed: true,
            guardrail: false,
            rank: Some(1),
            max_rank: 5,
            false_positive_count: 0,
            message: "better".to_owned(),
            objective: "competitive_capability".to_owned(),
            score_override: Some(1.0),
        }],
        metrics: Vec::new(),
        generated_diff: true,
    };

    let score = score_evaluation(
        &observation,
        ScoreBaselines {
            workload_previous: Some(&previous),
            profile_best_accepted: None,
        },
    );

    assert!(
        score
            .improvements
            .iter()
            .any(|item| item.get("kind").and_then(Value::as_str) == Some("case_rank"))
    );
    assert!(
        score
            .improvements
            .iter()
            .any(|item| item.get("kind").and_then(Value::as_str) == Some("case_score"))
    );
    assert!(score.improvements.iter().any(|item| {
        item.get("kind").and_then(Value::as_str) == Some("case_false_positive_count")
    }));
}
