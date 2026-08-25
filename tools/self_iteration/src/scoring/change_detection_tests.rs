use serde_json::json;

use super::*;

#[test]
fn rank_changes_distinguish_improvements_from_regressions() {
    assert_eq!(optional_rank_better(Some(1), Some(3)), Some(true));
    assert_eq!(optional_rank_better(Some(4), Some(2)), Some(false));
    assert_eq!(optional_rank_better(Some(2), Some(2)), None);
    assert_eq!(optional_rank_better(None, Some(2)), Some(false));
}

#[test]
fn previous_metrics_keep_only_typed_name_value_pairs() {
    let run = json!({
        "metrics": [
            {"name": "latency", "value": 12.0},
            {"name": "missing-value"},
            {"value": 4.0},
        ],
    });

    assert_eq!(previous_metrics(&run).get("latency"), Some(&12.0));
    assert_eq!(previous_metrics(&run).len(), 1);
}

#[test]
fn subunit_metric_changes_use_relative_thresholds_and_handle_zero_or_equal() {
    let previous = |value| {
        json!({
            "metrics": [{"name": "text_fallback_ratio", "value": value}]
        })
    };
    let observation = |value| EvaluationObservation {
        gates: Vec::new(),
        cases: Vec::new(),
        metrics: vec![MetricObservation {
            name: "text_fallback_ratio".to_owned(),
            value,
            budget: Some(0.75),
            lower_is_better: true,
            key: true,
        }],
        generated_diff: true,
    };
    let components = ScoreComponents {
        score: 0.0,
        foundational_capability: 0.0,
        competitive_capability: 0.0,
        semantic_vector: 0.0,
        research_judge: None,
        performance: 0.0,
        stability: 0.0,
    };

    assert!(
        changes(&observation(0.4), components, Some(&previous(0.5)), true)
            .iter()
            .any(|change| change["kind"].as_str() == Some("metric"))
    );
    assert!(changes(&observation(0.5), components, Some(&previous(0.5)), true).is_empty());
    assert!(
        changes(&observation(0.1), components, Some(&previous(0.0)), false)
            .iter()
            .any(|change| change["kind"].as_str() == Some("metric"))
    );
    assert!(changes(&observation(0.0), components, Some(&previous(0.0)), false).is_empty());
    assert!(changes(&observation(1e-12), components, Some(&previous(0.0)), false).is_empty());
}

#[test]
fn higher_is_better_subunit_changes_use_the_same_noise_floor() {
    let previous = |value| {
        json!({
            "metrics": [{"name": "minimum_recall", "value": value}]
        })
    };
    let observation = |value| EvaluationObservation {
        gates: Vec::new(),
        cases: Vec::new(),
        metrics: vec![MetricObservation {
            name: "minimum_recall".to_owned(),
            value,
            budget: Some(0.8),
            lower_is_better: false,
            key: true,
        }],
        generated_diff: true,
    };
    let components = ScoreComponents {
        score: 0.0,
        foundational_capability: 0.0,
        competitive_capability: 0.0,
        semantic_vector: 0.0,
        research_judge: None,
        performance: 0.0,
        stability: 0.0,
    };

    assert!(
        changes(&observation(0.7), components, Some(&previous(0.5)), true)
            .iter()
            .any(|change| change["kind"].as_str() == Some("metric"))
    );
    assert!(
        changes(&observation(0.3), components, Some(&previous(0.5)), false)
            .iter()
            .any(|change| change["kind"].as_str() == Some("metric"))
    );
    assert!(changes(&observation(1e-12), components, Some(&previous(0.0)), true).is_empty());
}

#[test]
fn metric_changes_remain_symmetric_when_values_cross_one() {
    let previous = |value| {
        json!({
            "metrics": [{"name": "text_fallback_ratio", "value": value}]
        })
    };
    let observation = |value| EvaluationObservation {
        gates: Vec::new(),
        cases: Vec::new(),
        metrics: vec![MetricObservation {
            name: "text_fallback_ratio".to_owned(),
            value,
            budget: Some(0.75),
            lower_is_better: true,
            key: true,
        }],
        generated_diff: true,
    };
    let components = ScoreComponents {
        score: 0.0,
        foundational_capability: 0.0,
        competitive_capability: 0.0,
        semantic_vector: 0.0,
        research_judge: None,
        performance: 0.0,
        stability: 0.0,
    };

    assert!(
        changes(&observation(1.1), components, Some(&previous(0.5)), false)
            .iter()
            .any(|change| change["kind"].as_str() == Some("metric")),
        "0.5 -> 1.1 must remain a visible lower-is-better regression"
    );
    assert!(
        changes(&observation(0.5), components, Some(&previous(1.1)), true)
            .iter()
            .any(|change| change["kind"].as_str() == Some("metric")),
        "1.1 -> 0.5 must remain a visible lower-is-better improvement"
    );
    assert_eq!(
        metric_change_threshold(0.5, 1.1, Some(0.75)),
        metric_change_threshold(1.1, 0.5, Some(0.75)),
        "the threshold must not depend on comparison direction"
    );
}

#[test]
fn latency_metric_threshold_combines_relative_scale_with_a_bounded_budget_floor() {
    let near_budget = metric_change_threshold(1_000.0, 1_040.0, Some(1_000.0));
    let reversed = metric_change_threshold(1_040.0, 1_000.0, Some(1_000.0));
    let low_observations = metric_change_threshold(100.0, 110.0, Some(1_000.0));

    assert!((near_budget - 31.2).abs() < 1e-12);
    assert_eq!(near_budget, reversed);
    assert_eq!(low_observations, 25.0);
}
