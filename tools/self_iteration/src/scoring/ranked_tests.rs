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

#[test]
fn equivalent_relationship_candidates_can_be_checked_by_edge_contract() {
    let hit = serde_json::json!({
        "path": "src/any-importer.java",
        "retrieval_layers": ["import_graph"],
        "edge_kind": "import",
        "edge_resolution_state": "resolved",
        "edge_target_hint": "src/vendor/SharedType.java",
        "excerpt": "import vendor.SharedType;"
    });
    let pattern = serde_json::json!({
        "edge_kind": "import",
        "edge_resolution_state": "resolved",
        "edge_target_hint": "SharedType.java",
        "retrieval_layer": "import_graph",
        "excerpt_contains": "vendor.SharedType"
    });

    let wrong_kind = serde_json::json!({
        "path": "src/any-importer.java",
        "retrieval_layers": ["import_graph"],
        "edge_kind": "reference",
        "edge_resolution_state": "resolved",
        "edge_target_hint": "src/vendor/SharedType.java",
        "excerpt": "import vendor.SharedType;"
    });
    let misleading_suffix = serde_json::json!({
        "path": "src/any-importer.java",
        "retrieval_layers": ["import_graph"],
        "edge_kind": "import",
        "edge_resolution_state": "resolved",
        "edge_target_hint": "src/vendor/FakeSharedType.java",
        "excerpt": "import vendor.SharedType;"
    });

    assert!(hit_matches_any(&hit, std::slice::from_ref(&pattern)));
    assert!(!hit_matches_any(
        &wrong_kind,
        std::slice::from_ref(&pattern),
    ));
    assert!(!hit_matches_any(&misleading_suffix, &[pattern]));
}
