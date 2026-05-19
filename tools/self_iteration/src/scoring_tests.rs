mod tests {
    use super::*;

    #[test]
    fn ranked_assessment_scores_expected_sequence() {
        let case = serde_json::json!({
            "max_rank": 2,
            "expected_sequence": [{"path": "a"}, {"path": "b"}]
        });
        let hits = vec![
            serde_json::json!({"path": "a"}),
            serde_json::json!({"path": "b"}),
        ];

        let assessment = assess_ranked_hits(&case, &hits, &[], &[]);

        assert!(assessment.failures.is_empty());
        assert_eq!(assessment.score, 1.0);
    }

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

        let score = score_evaluation(&observation, None);

        assert!(!score.accepted);
        assert!(score.reject_reasons[0].contains("quality gates failed"));
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

        let score = score_evaluation(&observation, Some(&previous));

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

        let score = score_evaluation(&observation, Some(&previous));

        assert!(!score.accepted);
        assert!(!score
            .improvements
            .iter()
            .any(|item| item.get("kind").and_then(Value::as_str) == Some("case")));
    }
}
