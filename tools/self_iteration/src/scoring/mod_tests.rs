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
