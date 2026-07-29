use crate::scoring::{
    CaseObservation, EvaluationObservation, GateObservation, MetricObservation, ScoreBaselines,
    score_evaluation,
};

#[test]
fn profile_best_accepted_rejects_first_category_run_below_global_bar() {
    let profile_best = serde_json::json!({
        "run_id": "run-semantic-best",
        "score": 0.950057
    });
    let observation = mixed_capability_observation();

    let score = score_evaluation(
        &observation,
        ScoreBaselines {
            workload_previous: None,
            profile_best_accepted: Some(&profile_best),
        },
    );

    assert!((score.base_score - 0.86613).abs() < 0.00001);
    assert!(!score.accepted);
    assert!(
        score
            .reject_reasons
            .iter()
            .any(|reason| { reason.contains("did not beat profile best accepted score 0.950057") })
    );
}

fn mixed_capability_observation() -> EvaluationObservation {
    EvaluationObservation {
        gates: Vec::new(),
        cases: vec![
            case("foundation", "foundational_capability", 0.947917),
            case("competitive", "competitive_capability", 0.621212),
            case("semantic", "semantic_vector", 1.0),
        ],
        metrics: vec![MetricObservation {
            name: "query_p95_ms".to_owned(),
            value: 1000.0,
            budget: Some(782.9),
            lower_is_better: true,
            key: true,
        }],
        generated_diff: true,
    }
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
fn first_category_run_without_profile_best_remains_accepted() {
    let score = score_evaluation(&mixed_capability_observation(), ScoreBaselines::default());

    assert!(score.accepted);
}

#[test]
fn first_category_run_can_beat_profile_best() {
    let profile_best = serde_json::json!({
        "run_id": "run-old-best",
        "score": 0.8
    });

    let score = score_evaluation(
        &mixed_capability_observation(),
        ScoreBaselines {
            workload_previous: None,
            profile_best_accepted: Some(&profile_best),
        },
    );

    assert!(score.accepted);
}

#[test]
fn dynamic_ceiling_rewards_high_baseline_competitive_and_research_progress() {
    let previous = serde_json::json!({
        "score": 0.9,
        "foundational_capability": 0.95,
        "competitive_capability": 0.90,
        "semantic_vector": 0.90,
        "research_judge": 0.80,
        "performance": 0.90,
        "stability": 1.0,
        "gates": [],
        "cases": [],
        "metrics": []
    });
    let observation = EvaluationObservation {
        gates: Vec::new(),
        cases: vec![
            case("foundation", "foundational_capability", 0.95),
            case("competitive", "competitive_capability", 0.95),
            case("semantic", "semantic_vector", 0.92),
            case("research", "research_judge", 0.88),
        ],
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

    assert!(score.capability_ceiling_bonus > 0.0);
    assert!(score.score > score.base_score);
    assert_eq!(score.scoring_policy, "dynamic_capability_ceiling_v1");
    assert!(score.accepted);
}

#[test]
fn dynamic_ceiling_does_not_create_research_bonus_without_current_judge() {
    let previous = serde_json::json!({
        "score": 0.7,
        "research_judge": 0.8,
        "gates": [],
        "cases": [],
        "metrics": []
    });
    let observation = EvaluationObservation {
        gates: Vec::new(),
        cases: vec![
            case("foundation", "foundational_capability", 0.9),
            case("competitive", "competitive_capability", 0.9),
            case("semantic", "semantic_vector", 0.9),
        ],
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

    assert_eq!(score.research_judge, None);
    assert_eq!(score.capability_ceiling_bonus, 0.0);
}

#[test]
fn dynamic_ceiling_ignores_unmeasured_performance_progress() {
    let previous = serde_json::json!({
        "score": 0.7,
        "foundational_capability": 0.9,
        "competitive_capability": 0.9,
        "semantic_vector": 0.9,
        "performance": 0.5,
        "stability": 1.0,
        "gates": [],
        "cases": [],
        "metrics": []
    });
    let observation = EvaluationObservation {
        gates: Vec::new(),
        cases: vec![
            case("foundation", "foundational_capability", 0.9),
            case("competitive", "competitive_capability", 0.9),
            case("semantic", "semantic_vector", 0.9),
        ],
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

    assert_eq!(score.performance, 1.0);
    assert_eq!(score.capability_ceiling_bonus, 0.0);
}

#[test]
fn dynamic_ceiling_bonus_does_not_override_failed_gates() {
    let previous = serde_json::json!({
        "score": 0.9,
        "foundational_capability": 0.9,
        "competitive_capability": 0.8,
        "semantic_vector": 0.8,
        "performance": 0.8,
        "stability": 1.0,
        "gates": [],
        "cases": [],
        "metrics": []
    });
    let observation = EvaluationObservation {
        gates: vec![GateObservation {
            name: "cargo_test".to_owned(),
            passed: false,
            duration_ms: 1,
            message: "failed".to_owned(),
        }],
        cases: vec![
            case("foundation", "foundational_capability", 0.9),
            case("competitive", "competitive_capability", 0.95),
            case("semantic", "semantic_vector", 0.9),
        ],
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

    assert!(score.capability_ceiling_bonus > 0.0);
    assert!(!score.accepted);
    assert!(
        score
            .reject_reasons
            .iter()
            .any(|reason| reason.contains("quality gates failed"))
    );
}
