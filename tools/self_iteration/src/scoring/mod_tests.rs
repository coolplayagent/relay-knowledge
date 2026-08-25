use super::*;

#[test]
fn case_observation_clamps_overrides_and_penalizes_false_positives() {
    let mut observation = CaseObservation {
        case_id: "ranked".to_owned(),
        repository: "repo".to_owned(),
        passed: true,
        guardrail: false,
        rank: Some(2),
        max_rank: 5,
        false_positive_count: 1,
        message: "ok".to_owned(),
        objective: "competitive_capability".to_owned(),
        score_override: None,
    };

    assert!((observation.score() - 0.4).abs() < f64::EPSILON);
    observation.score_override = Some(1.5);
    assert_eq!(observation.score(), 1.0);
    observation.passed = false;
    assert_eq!(observation.score(), 0.0);
}

#[test]
fn metric_observation_uses_budget_direction() {
    let lower = MetricObservation {
        name: "latency".to_owned(),
        value: 200.0,
        budget: Some(100.0),
        lower_is_better: true,
        key: true,
    };
    let higher = MetricObservation {
        name: "throughput".to_owned(),
        value: 50.0,
        budget: Some(100.0),
        lower_is_better: false,
        key: true,
    };

    assert_eq!(lower.score(), 0.5);
    assert_eq!(higher.score(), 0.5);
}

#[test]
fn lower_is_better_metric_scoring_handles_zero_and_fractional_values() {
    let score = |value, budget| {
        MetricObservation {
            name: "text_fallback_ratio".to_owned(),
            value,
            budget: Some(budget),
            lower_is_better: true,
            key: true,
        }
        .score()
    };

    assert_eq!(score(0.0, 0.5), 1.0);
    assert_eq!(score(0.166, 0.75), 1.0);
    assert!((score(0.8, 0.5) - 0.625).abs() < f64::EPSILON);
}

#[test]
fn higher_is_better_metric_scoring_handles_zero_equal_and_above_budget() {
    let score = |value| {
        MetricObservation {
            name: "minimum_recall".to_owned(),
            value,
            budget: Some(0.8),
            lower_is_better: false,
            key: true,
        }
        .score()
    };

    assert_eq!(score(0.0), 0.0);
    assert_eq!(score(0.8), 1.0);
    assert_eq!(score(0.9), 1.0);
}
