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
