use std::collections::HashSet;

use super::{
    ExploreBudget, apply_agent_code_budget, apply_repository_graph_budget, container_outline,
    explore_budget, serialize_repository_graph_output, serialized_len,
};
use serde_json::json;

#[test]
fn explore_budget_scales_by_indexed_file_count() {
    assert_eq!(explore_budget(0).max_files, 5);
    assert_eq!(explore_budget(500).calls, 2);
    assert_eq!(explore_budget(5_000).max_output_chars, 45_000);
    assert_eq!(explore_budget(15_000).max_files, 25);
}

#[test]
fn repository_graph_budget_keeps_focus_and_bounds_serialized_output() {
    let oversized = "界".repeat(900);
    let mut structured = json!({
        "schema_version": 1,
        "metadata": {"request_id": "request", "trace_id": "trace"},
        "scope": {"path_filters": ["knowledge"], "language_filters": ["markdown"]},
        "request": {
            "focus_path": "knowledge/focus.md",
            "repository": {
                "repository": "fixture",
                "path_filters": ["knowledge"],
                "language_filters": ["markdown"]
            }
        },
        "nodes": [
            {"id": "focus", "kind": "okf_concept", "label": "Focus", "path": "knowledge/focus.md", "details": {"description": oversized}},
            {"id": "neighbor-a", "kind": "okf_concept", "label": oversized, "details": {}},
            {"id": "neighbor-b", "kind": "external_source", "label": oversized, "details": {}}
        ],
        "edges": [
            {"id": "a", "source": "focus", "target": "neighbor-a", "details": {}},
            {"id": "b", "source": "focus", "target": "neighbor-b", "details": {}}
        ],
        "truncated": false
    });

    assert!(apply_repository_graph_budget(&mut structured, 768));
    assert!(serialized_len(&structured) <= 768);
    assert_eq!(structured["nodes"][0]["id"], "focus");
    assert_eq!(structured["truncated"], true);
    let node_ids = structured["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .filter_map(|node| node["id"].as_str())
        .collect::<HashSet<_>>();
    for edge in structured["edges"].as_array().expect("edges") {
        assert!(node_ids.contains(edge["source"].as_str().expect("edge source")));
        assert!(node_ids.contains(edge["target"].as_str().expect("edge target")));
    }
}

#[tokio::test]
async fn repository_graph_worker_caps_the_complete_structured_content() {
    let response = json!({
        "schema_version": 1,
        "metadata": {"request_id": "r".repeat(2_000), "trace_id": "t".repeat(2_000)},
        "scope": {"path_filters": ["knowledge"], "language_filters": ["markdown"]},
        "request": {"focus_path": "knowledge/focus.md"},
        "nodes": [{
            "id": "focus",
            "kind": "okf_concept",
            "label": "Focus",
            "path": "knowledge/focus.md",
            "details": {"description": "evidence ".repeat(1_000)}
        }],
        "edges": [],
        "truncated": false
    });

    let structured = serialize_repository_graph_output(response, 1_024)
        .await
        .expect("bounded structuredContent should fit after compaction");

    assert!(serialized_len(&structured) <= 1_024);
    assert_eq!(structured["nodes"][0]["id"], "focus");
    assert_eq!(structured["truncated"], true);
}

#[test]
fn repository_graph_budget_rejects_when_the_minimal_envelope_cannot_fit() {
    let mut structured = json!({
        "schema_version": 1,
        "nodes": [{"id": "focus", "kind": "okf_concept", "label": "Focus"}],
        "edges": [],
        "truncated": false
    });

    assert!(!apply_repository_graph_budget(&mut structured, 32));
    assert_eq!(structured["nodes"][0]["id"], "focus");
}

#[test]
fn container_outline_keeps_signatures_and_line_numbers() {
    let outline = container_outline(
        "class Cache {\n public:\n  virtual Handle* Insert(const Slice& key) = 0;\n  virtual Handle* Lookup(const Slice& key) = 0;\n};",
        20,
    );

    assert!(outline.contains("20: class Cache {"));
    assert!(outline.contains("22: virtual Handle* Insert"));
    assert!(outline.contains("23: virtual Handle* Lookup"));
    assert!(!outline.contains("21: public:"));
}

#[test]
fn budget_sets_audit_truncation_and_enforces_final_size() {
    let long_excerpt = "body ".repeat(500);
    let mut structured = json!({
        "status": {"members": (0..20).map(|index| json!({
            "alias": format!("member-{index}"),
            "indexed_file_count": 100,
            "diagnostics": "x".repeat(120),
        })).collect::<Vec<_>>()},
        "results": (0..8).map(|index| json!({
            "path": format!("src/generated/{index}/very_long_file_name.rs"),
            "line_range": {"start": 1, "end": 80},
            "excerpt": long_excerpt,
        })).collect::<Vec<_>>()
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 1_400,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert_eq!(structured["agent_output"]["truncated"], true);
    assert!(serialized_len(&structured) <= budget.max_output_chars);
}

#[test]
fn budget_preserves_service_truncation_signal() {
    let mut structured = json!({
        "truncated": true,
        "results": [{"path": "src/lib.rs", "excerpt": "short"}]
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 15_000,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert_eq!(structured["agent_output"]["truncated"], true);
}

#[test]
fn budget_compacts_status_before_dropping_results() {
    let mut structured = json!({
        "status": {"members": (0..30).map(|index| json!({
            "alias": format!("member-{index}"),
            "indexed_file_count": 100,
            "diagnostics": "x".repeat(140),
        })).collect::<Vec<_>>()},
        "results": [{"path": "src/lib.rs", "excerpt": "short"}]
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 1_200,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert_eq!(structured["results"].as_array().expect("results").len(), 1);
    assert!(serialized_len(&structured) <= budget.max_output_chars);
}

#[test]
fn budget_compacts_repository_set_metadata_before_dropping_results() {
    let mut structured = json!({
        "status": {
            "repository_set": {
                "set_id": "set-1",
                "alias": "workspace",
                "description": "description ".repeat(900),
                "default_ref_policy_json": "{\"rules\":".to_owned() + &"x".repeat(5_000) + "}",
                "created_at_ms": 1,
                "updated_at_ms": 2
            },
            "members": []
        },
        "results": [
            {"path": "src/lib.rs", "excerpt": "target one"},
            {"path": "src/main.rs", "excerpt": "target two"}
        ]
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 800,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert!(serialized_len(&structured) <= budget.max_output_chars);
    assert_eq!(structured["results"].as_array().expect("results").len(), 2);
    assert_eq!(structured["status"]["repository_set"]["alias"], "workspace");
    assert!(
        structured["status"]["repository_set"]
            .get("description")
            .is_none()
    );
    assert!(
        structured["status"]["repository_set"]
            .get("default_ref_policy_json")
            .is_none()
    );
    assert_eq!(
        structured["status"]["repository_set"]["description_omitted_by_agent_budget_chars"],
        10_800
    );
    assert!(
            structured["status"]["repository_set"]
                ["default_ref_policy_json_omitted_by_agent_budget_chars"]
                .as_u64()
                .expect("policy omitted chars")
                > 5_000
        );
}

#[test]
fn budget_compacts_request_and_scope_echoes_before_dropping_results() {
    let mut structured = json!({
        "request": {
            "query": "target",
            "repository": {
                "repository": "workspace",
                "path_filters": ["src/".repeat(1_100)],
                "language_filters": ["rust"]
            },
            "freshness_policy": "wait_until_fresh",
            "limit": 10
        },
        "scope": {
            "alias": "workspace",
            "path_filters": ["src/".repeat(1_100)],
            "language_filters": ["rust"]
        },
        "results": [
            {"path": "src/lib.rs", "excerpt": "target one"},
            {"path": "src/main.rs", "excerpt": "target two"}
        ]
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 900,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert!(serialized_len(&structured) <= budget.max_output_chars);
    assert_eq!(structured["results"].as_array().expect("results").len(), 2);
    assert!(
        structured["request"]["repository"]
            .get("path_filters")
            .is_none()
    );
    assert!(structured["scope"].get("path_filters").is_none());
}

#[test]
fn budget_compacts_freshness_echoes_before_dropping_results() {
    let mut structured = json!({
        "freshness": {
            "state": "stale",
            "direct_source_read_required": true,
            "direct_source_read_paths": [
                "src/".repeat(1_100),
                "tests/".repeat(1_000)
            ],
            "agent_instructions": [
                "refresh ".repeat(900),
                "scan ".repeat(800)
            ]
        },
        "results": [
            {"path": "src/lib.rs", "excerpt": "target one"},
            {"path": "src/main.rs", "excerpt": "target two"}
        ]
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 700,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert!(serialized_len(&structured) <= budget.max_output_chars);
    assert_eq!(structured["results"].as_array().expect("results").len(), 2);
    assert!(
        structured["freshness"]
            .get("direct_source_read_paths")
            .is_none()
    );
    assert!(structured["freshness"].get("agent_instructions").is_none());
    assert_eq!(
        structured["freshness"]["direct_source_read_paths_omitted_by_agent_budget"],
        2
    );
    assert_eq!(
        structured["freshness"]["agent_instructions_omitted_by_agent_budget"],
        2
    );
}

#[test]
fn budget_compacts_metadata_before_accepting_oversized_payloads() {
    let mut structured = json!({
        "metadata": {
            "request_id": "mcp|string:".to_owned() + &"r".repeat(5_000),
            "trace_id": "trace-mcp|string:".to_owned() + &"t".repeat(5_000),
            "graph_version": 1,
            "stale": false
        },
        "results": []
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 800,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert!(serialized_len(&structured) <= budget.max_output_chars);
    assert!(
        structured["metadata"]["request_id"]
            .as_str()
            .expect("request id")
            .contains("[truncated by MCP adaptive output budget]")
    );
    assert!(
        structured["metadata"]["trace_id"]
            .as_str()
            .expect("trace id")
            .contains("[truncated by MCP adaptive output budget]")
    );
    assert!(
        structured["metadata"]["request_id_omitted_by_agent_budget_chars"]
            .as_u64()
            .expect("request id omitted chars")
            > 5_000
    );
}

#[test]
fn budget_compacts_result_member_filters_before_dropping_results() {
    let mut structured = json!({
        "results": [
            {
                "member": {
                    "repository_alias": "core",
                    "source_scope": "scope-core",
                    "path_filters": ["src/".repeat(1_100), "tests/".repeat(1_000)],
                    "language_filters": ["rust", "typescript"]
                },
                "hit": {"path": "src/lib.rs", "excerpt": "target one"}
            },
            {
                "member": {
                    "repository_alias": "web",
                    "source_scope": "scope-web",
                    "path_filters": ["web/".repeat(1_100)],
                    "language_filters": ["typescript"]
                },
                "hit": {"path": "web/app.ts", "excerpt": "target two"}
            }
        ]
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 1_200,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert!(serialized_len(&structured) <= budget.max_output_chars);
    assert_eq!(structured["results"].as_array().expect("results").len(), 2);
    assert_eq!(
        structured["results"][0]["member"]["repository_alias"],
        "core"
    );
    assert!(
        structured["results"][0]["member"]
            .get("path_filters")
            .is_none()
    );
    assert_eq!(
        structured["results"][0]["member"]["path_filters_omitted_by_agent_budget"],
        2
    );
    assert_eq!(
        structured["results"][1]["member"]["path_filters_omitted_by_agent_budget"],
        1
    );
}

#[test]
fn budget_compacts_echoed_request_fields() {
    let mut structured = json!({
        "request": {
            "query": "q".repeat(2_000),
            "set_alias": "workspace",
            "freshness_policy": "wait_until_fresh",
            "limit": 20,
            "path_filters": ["p".repeat(4_096), "nested/".repeat(600)],
            "language_filters": ["rust"]
        },
        "results": []
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 650,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert!(serialized_len(&structured) <= budget.max_output_chars);
    assert_eq!(structured["request"]["set_alias"], "workspace");
    assert_eq!(
        structured["request"]["freshness_policy"],
        "wait_until_fresh"
    );
    assert_eq!(structured["request"]["limit"], 20);
    assert_eq!(structured["request"]["fields_omitted_by_agent_budget"], 3);
}

#[test]
fn budget_compacts_scope_filters_before_exceeding_output_budget() {
    let mut structured = json!({
        "scope": {
            "alias": "workspace",
            "path_filters": ["src/".repeat(1_100), "tests/".repeat(1_000)],
            "language_filters": ["rust", "cpp"]
        },
        "results": []
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 650,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert!(serialized_len(&structured) <= budget.max_output_chars);
    assert!(structured["scope"].get("path_filters").is_none());
    assert_eq!(
        structured["scope"]["path_filters_omitted_by_agent_budget"],
        2
    );
    assert_eq!(
        structured["scope"]["language_filters_omitted_by_agent_budget"],
        2
    );
}

#[test]
fn budget_preserves_repository_request_scope_when_slimming_request() {
    let mut structured = json!({
        "request": {
            "repository": {
                "host": "github.com",
                "owner": "coolplayagent",
                "repository": "relay-knowledge",
                "path": "ignored/".repeat(300)
            },
            "query": "q".repeat(2_000),
            "path_filters": ["src/".repeat(1_000)]
        },
        "results": []
    });
    let budget = ExploreBudget {
        calls: 1,
        max_output_chars: 650,
        max_files: 5,
    };

    apply_agent_code_budget(&mut structured, budget, false);

    assert_eq!(structured["truncated"], true);
    assert!(serialized_len(&structured) <= budget.max_output_chars);
    assert_eq!(
        structured["request"]["repository"]["repository"],
        "relay-knowledge"
    );
    assert_eq!(structured["request"]["fields_omitted_by_agent_budget"], 2);
}
