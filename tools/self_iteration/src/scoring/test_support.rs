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
