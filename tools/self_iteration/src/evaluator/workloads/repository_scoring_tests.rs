use super::{score_framework_case, score_query_case, score_software_case};
use crate::command::CommandResult;

#[test]
fn repository_case_enforces_payload_constraints() {
    let case = serde_json::json!({
        "id": "repo_constraints",
        "degraded_reason_contains": "budget",
        "expected": [{"path": "src/component.tsx", "retrieval_layer": "text_fallback"}]
    });
    let result = CommandResult {
        name: "repo_query".to_owned(),
        command: vec!["relay-knowledge".to_owned()],
        exit_code: 0,
        duration_ms: 1,
        stdout: serde_json::json!({
            "results": [{
                "path": "src/component.tsx",
                "retrieval_layers": ["lexical", "text_fallback"],
                "excerpt": "import React from \"react\";"
            }],
            "degraded_reason": "ripgrep materialized-byte budget exhausted"
        })
        .to_string(),
        stderr: String::new(),
    };

    let observation = score_query_case("typescript_syntax_fixture", &case, &result);

    assert!(observation.passed);
    assert!(observation.message.contains("rank=Some(1)"));
}

#[test]
fn repository_case_expect_empty_preserves_payload_constraint_failures() {
    let case = serde_json::json!({
        "id": "repo_empty_constraints",
        "expect_empty": true,
        "degraded_reason_contains": "budget"
    });
    let result = CommandResult {
        name: "repo_query".to_owned(),
        command: vec!["relay-knowledge".to_owned()],
        exit_code: 0,
        duration_ms: 1,
        stdout: serde_json::json!({
            "results": [],
            "degraded_reason": "stale"
        })
        .to_string(),
        stderr: String::new(),
    };

    let observation = score_query_case("typescript_syntax_fixture", &case, &result);

    assert!(!observation.passed);
    assert!(observation.message.contains("missing=budget"));
}

#[test]
fn framework_case_scores_nodes_and_edges_as_ranked_hits() {
    let case = serde_json::json!({
        "id": "framework_graph",
        "expected": [
            {"kind": "component", "name": "VersionSelect"},
            {"kind": "renders", "target_hint": "Copy"}
        ],
        "max_rank": 2
    });
    let result = CommandResult {
        name: "repo_framework".to_owned(),
        command: vec!["relay-knowledge".to_owned()],
        exit_code: 0,
        duration_ms: 1,
        stdout: serde_json::json!({
            "graph": {
                "nodes": [{"kind": "component", "name": "VersionSelect"}],
                "edges": [{"kind": "renders", "target_hint": "Copy"}]
            },
            "degraded_reason": null
        })
        .to_string(),
        stderr: String::new(),
    };

    let observation = score_framework_case("vue", &case, &result);

    assert!(observation.passed, "{}", observation.message);
    assert_eq!(observation.rank, Some(1));
}

#[test]
fn software_statement_case_scores_ontology_fields_and_status_contract() {
    let case = serde_json::json!({
        "id": "software_statements",
        "kind": "statements",
        "guardrail": true,
        "max_rank": 1,
        "status_ontology_version": "1.0.0",
        "status_projection_schema_version": 6,
        "status_completeness_basis_points_min": 10000,
        "expected": [{
            "software_slice": "statement",
            "predicate": "depends_on",
            "assertion_mode": "declared",
            "fact_state": "active",
            "extractor_version": "1.0.0"
        }]
    });
    let result = CommandResult {
        name: "repo_software".to_owned(),
        command: vec!["relay-knowledge".to_owned()],
        exit_code: 0,
        duration_ms: 1,
        stdout: serde_json::json!({
            "status": {
                "ontology_version": "1.0.0",
                "projection_schema_version": 6,
                "completeness_basis_points": 10000
            },
            "entities": [],
            "statements": [{
                "predicate": "depends_on",
                "assertion_mode": "declared",
                "fact_state": "active",
                "extractor_version": "1.0.0"
            }],
            "diagnostics": []
        })
        .to_string(),
        stderr: String::new(),
    };

    let observation = score_software_case("software_global_fixture", &case, &result);

    assert!(observation.passed, "{}", observation.message);
    assert_eq!(observation.rank, Some(1));
}
