    #[test]
    fn registration_case_scores_expected_failure_message() {
        let case = serde_json::json!({
            "id": "reject_register_language",
            "expect_failure": true,
            "stderr_contains": "registration language filters are not supported",
            "guardrail": true
        });
        let result = CommandResult {
            name: "register".to_owned(),
            command: vec!["relay-knowledge".to_owned()],
            exit_code: 1,
            duration_ms: 1,
            stdout: String::new(),
            stderr: "registration language filters are not supported; use query-time --language"
                .to_owned(),
        };

        let observation = score_registration_case("fixture", &case, &result);

        assert!(observation.passed);
        assert!(observation.guardrail);
        assert_eq!(observation.score_override, Some(1.0));
    }

    #[test]
    fn cli_contract_case_scores_idle_index_worker_json() {
        let case = serde_json::json!({
            "id": "repo_index_worker_idle_json_reports_no_claim",
            "guardrail": true,
            "json_expect": {
                "claimed": false,
                "task": null
            }
        });
        let result = CommandResult {
            name: "index_worker".to_owned(),
            command: vec!["relay-knowledge".to_owned()],
            exit_code: 0,
            duration_ms: 1,
            stdout: "{\"claimed\":false,\"task\":null}\n".to_owned(),
            stderr: String::new(),
        };

        let observation = score_cli_contract_case(&case, &result);

        assert!(observation.passed, "{}", observation.message);
        assert!(observation.guardrail);
        assert_eq!(observation.repository, "cli_contract");
    }

    #[test]
    fn cli_contract_case_scores_idle_index_worker_stream() {
        let case = serde_json::json!({
            "id": "repo_index_worker_idle_streaming_json_reports_events",
            "guardrail": true,
            "json_lines_expect": [
                {"event": "started", "operation": "code.repo.index_worker"},
                {
                    "event": "item",
                    "operation": "code.repo.index_worker",
                    "payload": {
                        "claimed": false,
                        "task": null
                    }
                },
                {"event": "completed", "operation": "code.repo.index_worker"}
            ]
        });
        let result = CommandResult {
            name: "index_worker_stream".to_owned(),
            command: vec!["relay-knowledge".to_owned()],
            exit_code: 0,
            duration_ms: 1,
            stdout: concat!(
                "{\"event\":\"started\",\"operation\":\"code.repo.index_worker\"}\n",
                "{\"event\":\"item\",\"operation\":\"code.repo.index_worker\",\"payload\":{\"claimed\":false,\"task\":null}}\n",
                "{\"event\":\"completed\",\"operation\":\"code.repo.index_worker\"}\n"
            )
            .to_owned(),
            stderr: String::new(),
        };

        let observation = score_cli_contract_case(&case, &result);

        assert!(observation.passed, "{}", observation.message);
        assert!(observation.guardrail);
    }

    #[test]
    fn cli_contract_cases_are_preserved_for_fast() {
        let cases = vec![
            serde_json::json!({"id": "regular"}),
            serde_json::json!({
                "id": "repo_index_worker_idle_json_reports_no_claim",
                "guardrail": true
            }),
        ];

        let selected = select_cli_contract_cases_for_profile("fast", None, cases);

        assert!(selected.iter().any(|case| {
            string_or(case, "id", "") == "repo_index_worker_idle_json_reports_no_claim"
                && case.get("guardrail").and_then(Value::as_bool) == Some(true)
        }));
    }

    #[test]
    fn fast_preserves_grep_and_external_import_guardrail_cases() {
        let cases = vec![
            serde_json::json!({"id": "regular_a", "kind": "definition"}),
            serde_json::json!({
                "id": "typescript_syntax_external_react_import_grep_fallback",
                "repository": "typescript_syntax_fixture",
                "kind": "imports",
                "query": "react",
                "guardrail": true,
                "expected": [{
                    "path": "src/component.tsx",
                    "retrieval_layer": "text_fallback"
                }],
                "degraded_reason": null
            }),
            serde_json::json!({
                "id": "grep_budget_reference_late_comment_after_scope_budget",
                "repository": "grep_budget_fixture",
                "kind": "references",
                "query": "RK_LATE_BUDGET_NOTE",
                "guardrail": true,
                "expected": [{
                    "path": "zzz/late_target.c",
                    "retrieval_layer": "text_fallback"
                }],
                "degraded_reason": false
            }),
            serde_json::json!({
                "id": "c_syntax_definition_nginx_external_macro_handler",
                "repository": "c_syntax_fixture",
                "kind": "definition",
                "query": "ngx_http_demo_access",
                "guardrail": true,
                "expected": [{
                    "path": "src/nginx_external_module.c",
                    "retrieval_layer": "definition"
                }],
                "degraded_reason": null
            }),
            serde_json::json!({
                "id": "nonstandard_layout_external_deps_definition_without_path_filter",
                "repository": "nonstandard_layout_fixture",
                "kind": "definition",
                "query": "ExternalTypeScriptSessionClient",
                "guardrail": true,
                "expected": [{
                    "path": "external_deps/ts_sdk/sessionClient.ts"
                }]
            }),
        ];

        let selected = select_repository_cases_for_profile("fast", None, cases);
        let case = selected
            .iter()
            .find(|case| {
                string_or(case, "id", "")
                    == "typescript_syntax_external_react_import_grep_fallback"
            })
            .expect("fast should preserve the import grep fallback guardrail");
        let expected = array_field(case, "expected");

        assert_eq!(string_or(case, "kind", ""), "imports");
        assert_eq!(string_or(&expected[0], "retrieval_layer", ""), "text_fallback");
        assert!(case.get("degraded_reason").is_some_and(Value::is_null));
        assert!(case.get("guardrail").and_then(Value::as_bool).unwrap_or(false));
        assert!(selected.iter().any(|case| {
            string_or(case, "id", "") == "grep_budget_reference_late_comment_after_scope_budget"
                && case
                    .get("degraded_reason")
                    .and_then(Value::as_bool)
                    == Some(false)
        }));
        assert!(selected.iter().any(|case| {
            string_or(case, "id", "") == "c_syntax_definition_nginx_external_macro_handler"
                && case.get("degraded_reason").is_some_and(Value::is_null)
        }));
        assert!(selected.iter().any(|case| {
            string_or(case, "id", "")
                == "nonstandard_layout_external_deps_definition_without_path_filter"
                && array_field(case, "path_filters").is_empty()
        }));
    }
