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
    fn hit_pattern_can_require_retrieval_layer_and_absent_edge_confidence() {
        let hit = serde_json::json!({
            "path": "src/driver_ops.c",
            "retrieval_layers": ["lexical", "text_fallback"],
            "excerpt": "RK_TRACE_NOTE documents fallback-only macro text"
        });
        let pattern = serde_json::json!({
            "path": "src/driver_ops.c",
            "retrieval_layer": "text_fallback",
            "edge_confidence_absent": true,
            "excerpt_contains": "RK_TRACE_NOTE"
        });

        assert!(hit_matches_any(&hit, &[pattern]));
    }
