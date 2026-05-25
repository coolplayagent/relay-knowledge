mod tests {
    use super::*;

    #[test]
    fn shell_split_keeps_quoted_argument() {
        assert_eq!(
            shell_split("tool run \"hello world\" --file {prompt_file}").expect("split"),
            vec!["tool", "run", "hello world", "--file", "{prompt_file}"]
        );
    }

    #[test]
    fn judge_defaults_to_opencode_cli_agent() {
        let settings = judge_settings(&BTreeMap::new());
        assert!(settings.enabled);
        assert_eq!(settings.backend, JudgeBackend::Cli);
        assert!(settings.command.starts_with("opencode run "));
        assert!(settings.missing.is_empty());
    }

    #[test]
    fn full_profile_quality_gates_run_in_dependency_stages() {
        let stages = quality_gate_stages("full");

        assert_eq!(stages.len(), 3);
        match &stages[0] {
            QualityGateStage::Parallel(gates) => {
                assert_eq!(
                    gates.iter().map(|gate| gate.name).collect::<Vec<_>>(),
                    vec!["cargo_fmt_check", "self_iteration_cargo_fmt_check"]
                );
            }
            QualityGateStage::Rails(_) => panic!("fmt gates should be parallel"),
        }
        match &stages[1] {
            QualityGateStage::Parallel(gates) => {
                assert_eq!(
                    gates.iter().map(|gate| gate.name).collect::<Vec<_>>(),
                    vec![
                        "cargo_build_release",
                        "self_iteration_cargo_build_release"
                    ]
                );
            }
            QualityGateStage::Rails(_) => panic!("build gates should be parallel"),
        }
        match &stages[2] {
            QualityGateStage::Rails(rails) => {
                let rail_names = rails
                    .iter()
                    .map(|rail| rail.iter().map(|gate| gate.name).collect::<Vec<_>>())
                    .collect::<Vec<_>>();
                assert_eq!(
                    rail_names,
                    vec![
                        vec!["cargo_clippy", "cargo_test"],
                        vec!["self_iteration_cargo_clippy", "self_iteration_cargo_test"]
                    ]
                );
            }
            QualityGateStage::Parallel(_) => panic!("clippy/test gates should use rails"),
        }
    }

    #[test]
    fn fast_profile_skips_full_quality_gates_and_slow_suites() {
        let stages = quality_gate_stages("fast");

        assert_eq!(stages.len(), 3);
        let gate_names = stages
            .iter()
            .flat_map(|stage| match stage {
                QualityGateStage::Parallel(gates) => gates
                    .iter()
                    .map(|gate| gate.name)
                    .collect::<Vec<_>>(),
                QualityGateStage::Rails(rails) => rails
                    .iter()
                    .flat_map(|rail| rail.iter().map(|gate| gate.name))
                    .collect::<Vec<_>>(),
            })
            .collect::<Vec<_>>();
        assert!(gate_names.contains(&"cargo_build_debug"));
        assert!(gate_names.contains(&"self_iteration_cargo_check"));
        assert!(!gate_names.contains(&"cargo_build_release"));
        assert!(!gate_names.contains(&"cargo_clippy"));
        assert!(!gate_names.contains(&"cargo_test"));
        assert!(!profile_runs_slow_suites("fast"));
        assert!(profile_runs_repository_sets("fast"));
        assert_eq!(
            WorkloadSelection { categories: None }.skipped_suites("fast"),
            vec!["file_fixtures", "research_judge"]
        );
    }

    #[test]
    fn focused_semantic_vector_keeps_bottom_line_workloads() {
        let config = Config::parse(vec![
            "evaluate".to_owned(),
            "--categories".to_owned(),
            "semantic_vector".to_owned(),
        ])
        .expect("config should parse");
        let selection = WorkloadSelection::new(&config);

        assert!(selection.runs_repository_workload("fast"));
        assert!(selection.runs_repository_sets("fast"));
        assert!(selection.runs_semantic_vector("fast"));
        assert!(!selection.runs_file_fixtures("fast"));
        assert!(!selection.runs_research_judge("fast"));
        assert_eq!(
            selection.skipped_suites("fast"),
            vec!["file_fixtures", "research_judge"]
        );
    }

    #[test]
    fn excluded_research_judge_skips_judge_suite_in_full_all_selection() {
        let config = Config::parse(vec![
            "evaluate".to_owned(),
            "--profile".to_owned(),
            "full".to_owned(),
            "--categories".to_owned(),
            "all".to_owned(),
            "--exclude-categories".to_owned(),
            "research_judge".to_owned(),
        ])
        .expect("config should parse");
        let selection = WorkloadSelection::new(&config);

        assert!(selection.runs_repository_workload("full"));
        assert!(selection.runs_repository_sets("full"));
        assert!(selection.runs_file_fixtures("full"));
        assert!(selection.runs_semantic_vector("full"));
        assert!(!selection.runs_research_judge("full"));
        assert_eq!(selection.skipped_suites("full"), vec!["research_judge"]);
    }

    #[test]
    fn focused_repository_cases_include_guardrails_and_selected_objective() {
        let categories = CategorySet::parse("competitive").expect("categories should parse");
        let cases = vec![
            serde_json::json!({
                "id": "foundation_guardrail",
                "kind": "definition",
                "guardrail": true
            }),
            serde_json::json!({
                "id": "foundation_regular",
                "kind": "definition"
            }),
            serde_json::json!({
                "id": "competitive_regular",
                "kind": "hybrid"
            }),
        ];

        let selected = select_repository_cases_for_profile("full", Some(&categories), cases);
        let ids = selected
            .iter()
            .map(|case| string_or(case, "id", "case"))
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["foundation_guardrail", "competitive_regular"]);
    }

    #[test]
    fn focused_performance_repository_cases_keep_query_workload() {
        let categories = CategorySet::parse("performance").expect("categories should parse");
        let cases = vec![
            serde_json::json!({
                "id": "foundation_guardrail",
                "kind": "definition",
                "guardrail": true
            }),
            serde_json::json!({
                "id": "foundation_regular",
                "kind": "definition"
            }),
            serde_json::json!({
                "id": "competitive_regular",
                "kind": "hybrid"
            }),
        ];

        let selected = select_repository_cases_for_profile("full", Some(&categories), cases);
        let ids = selected
            .iter()
            .map(|case| string_or(case, "id", "case"))
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "foundation_guardrail",
                "foundation_regular",
                "competitive_regular"
            ]
        );
    }

    #[test]
    fn focused_performance_runs_full_performance_suites() {
        let config = Config::parse(vec![
            "evaluate".to_owned(),
            "--categories".to_owned(),
            "performance".to_owned(),
        ])
        .expect("config should parse");
        let selection = WorkloadSelection::new(&config);
        let semantic_suite = serde_json::json!({
            "query_cases": [
                {"id": "guardrail", "guardrail": true},
                {"id": "full"}
            ]
        });
        let repo_set_cases = vec![
            serde_json::json!({"id": "guardrail", "guardrail": true}),
            serde_json::json!({"id": "regular"}),
        ];

        assert!(selection.runs_file_fixtures("fast"));
        assert_eq!(
            array_field(
                &semantic_vector_suite_for_selection(
                    &semantic_suite,
                    "fast",
                    config.categories.as_ref()
                ),
                "query_cases"
            )
            .len(),
            2
        );
        assert_eq!(
            select_repository_set_cases_for_profile(
                "full",
                config.categories.as_ref(),
                repo_set_cases
            )
            .len(),
            2
        );
    }

    #[test]
    fn selected_repository_set_members_follow_selected_cases() {
        let categories = CategorySet::parse("semantic_vector").expect("categories should parse");
        let cases_config = serde_json::json!({
            "repository_sets": {
                "guarded_workspace": {
                    "members": [
                        {"repository": "member_a"},
                        {"repository": "member_b"}
                    ]
                },
                "regular_workspace": {
                    "members": [
                        {"repository": "member_c"}
                    ]
                }
            },
            "repository_set_query_cases": [
                {
                    "id": "guardrail_case",
                    "repository_set": "guarded_workspace",
                    "guardrail": true
                },
                {
                    "id": "regular_case",
                    "repository_set": "regular_workspace"
                }
            ]
        });

        let members =
            selected_repository_set_member_names(&cases_config, "full", Some(&categories));

        assert!(members.contains("member_a"));
        assert!(members.contains("member_b"));
        assert!(!members.contains("member_c"));
    }

    #[test]
    fn fast_limits_preserve_guardrail_cases() {
        let cases = vec![
            serde_json::json!({"id": "regular_a", "kind": "definition"}),
            serde_json::json!({"id": "regular_b", "kind": "definition"}),
            serde_json::json!({"id": "guardrail_late", "kind": "hybrid", "guardrail": true}),
        ];

        let selected = limit_preserving_guardrails(cases, 1);
        let ids = selected
            .iter()
            .map(|case| string_or(case, "id", "case"))
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["guardrail_late", "regular_a"]);
    }

    #[test]
    fn fast_default_repositories_include_typescript_import_grep_fixture() {
        if std::env::var("RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS").is_ok() {
            return;
        }

        let names = fast_repository_names();

        assert!(names.iter().any(|name| name == "typescript_syntax_fixture"));
        assert!(names.iter().any(|name| name == "nonstandard_layout_fixture"));
    }

    #[test]
    fn fast_default_repositories_include_cross_language_fixture() {
        if std::env::var("RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS").is_ok() {
            return;
        }

        let names = fast_repository_names();

        assert!(names
            .iter()
            .any(|name| name == "cross_language_syntax_fixture"));
    }

    #[test]
    fn fast_preserves_typescript_import_grep_guardrail_case() {
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
                "degraded_reason_contains": "external dependency import is not indexed"
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
        assert_eq!(
            string_or(case, "degraded_reason_contains", ""),
            "external dependency import is not indexed"
        );
        assert!(case.get("guardrail").and_then(Value::as_bool).unwrap_or(false));
    }

    #[test]
    fn semantic_vector_selection_uses_guardrail_for_fast_default() {
        let suite = serde_json::json!({
            "query_cases": [
                {"id": "guardrail", "guardrail": true},
                {"id": "full"}
            ]
        });

        let selected = semantic_vector_suite_for_selection(&suite, "fast", None);
        let cases = array_field(&selected, "query_cases");

        assert_eq!(cases.len(), 1);
        assert_eq!(string_or(&cases[0], "id", ""), "guardrail");
    }

    #[test]
    fn semantic_vector_focus_runs_full_suite() {
        let categories = CategorySet::parse("semantic_vector").expect("categories should parse");
        let suite = serde_json::json!({
            "query_cases": [
                {"id": "guardrail", "guardrail": true},
                {"id": "full"}
            ]
        });

        let selected = semantic_vector_suite_for_selection(&suite, "fast", Some(&categories));

        assert_eq!(array_field(&selected, "query_cases").len(), 2);
    }

    #[test]
    fn generated_language_fixtures_write_syntax_dense_sources() {
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-self-iteration-fixture-test-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("remove stale fixture");
        }

        create_generated_repository_files(&root.join("c"), "c_syntax_v1")
            .expect("c fixture should write");
        create_generated_repository_files(&root.join("cpp"), "cpp_syntax_v1")
            .expect("cpp fixture should write");
        create_generated_repository_files(&root.join("python"), "python_syntax_v2")
            .expect("python fixture should write");
        create_generated_repository_files(&root.join("typescript"), "typescript_syntax_v2")
            .expect("typescript fixture should write");
        create_generated_repository_files(&root.join("go"), "go_syntax_v2")
            .expect("go fixture should write");
        create_generated_repository_files(&root.join("swift"), "swift_syntax_v2")
            .expect("swift fixture should write");
        create_generated_repository_files(&root.join("nonstandard"), "nonstandard_layout_v1")
            .expect("nonstandard fixture should write");

        let c_source =
            std::fs::read_to_string(root.join("c/src/driver_ops.c")).expect("c source");
        let cpp_source =
            std::fs::read_to_string(root.join("cpp/src/pipeline.cpp")).expect("cpp source");
        let python_source = std::fs::read_to_string(root.join("python/syntax_service/service.py"))
            .expect("python source");
        let python_operations_doc =
            std::fs::read_to_string(root.join("python/docs/operations.md"))
                .expect("python operations doc");
        let typescript_source =
            std::fs::read_to_string(root.join("typescript/src/provider.ts"))
                .expect("typescript source");
        let go_source =
            std::fs::read_to_string(root.join("go/processor/worker.go")).expect("go source");
        let go_pipeline =
            std::fs::read_to_string(root.join("go/processor/pipeline.go")).expect("go pipeline");
        let swift_source =
            std::fs::read_to_string(root.join("swift/Sources/App/RequestPipeline.swift"))
                .expect("swift source");
        let nonstandard_ts = std::fs::read_to_string(
            root.join("nonstandard/external_deps/ts_sdk/sessionClient.ts"),
        )
        .expect("nonstandard TypeScript source");
        let nonstandard_cpp = std::fs::read_to_string(
            root.join("nonstandard/external_deps/cpp_sdk/session_client.cpp"),
        )
        .expect("nonstandard C++ source");
        assert!(c_source.contains(".read = rk_driver_read"));
        assert!(c_source.contains("const struct rk_driver_ops rk_default_ops"));
        assert!(cpp_source.contains("auto append_event = [&cache, &pipeline]"));
        assert!(cpp_source.contains("cache_alias::Cache<std::string>"));
        assert!(python_source.contains("@traced_operation(\"dispatch\")"));
        assert!(python_source.contains("lambda value: value.strip()"));
        assert!(python_operations_doc.contains("ServiceRunner class owns"));
        assert!(python_operations_doc.contains("dispatch_event function normalizes"));
        assert!(typescript_source.contains("await import(\"./protocol\")"));
        assert!(typescript_source.contains("trimPayload(payload)"));
        assert!(go_source.contains("ctxalias \"context\""));
        assert!(go_pipeline.contains("notify := func(payload string) string"));
        assert!(swift_source.contains("let request = { (url: URL) async throws -> Data in"));
        assert!(nonstandard_ts.contains("ExternalTypeScriptSessionClient"));
        assert!(nonstandard_cpp.contains("#include <external_session_client.hpp>"));

        std::fs::remove_dir_all(&root).expect("cleanup fixture");
    }

    #[test]
    fn generated_repository_names_cannot_escape_run_home() {
        let run_home = std::env::temp_dir().join("relay-knowledge-self-iteration-safe-roots");

        assert_eq!(
            generated_repository_root(&run_home, "c_syntax_fixture")
                .expect("safe name")
                .strip_prefix(&run_home)
                .expect("root should stay under run home"),
            Path::new("generated-repositories/c_syntax_fixture")
        );
        for unsafe_name in [
            "",
            ".",
            "..",
            "../outside",
            "nested/repo",
            "nested\\repo",
            "/absolute",
            "repo.name",
        ] {
            assert!(
                generated_repository_root(&run_home, unsafe_name).is_err(),
                "{unsafe_name:?} should be rejected"
            );
        }
    }

    #[test]
    fn judge_uses_openai_compatible_http_when_configured() {
        let env = BTreeMap::from([
            (
                "RELAY_KNOWLEDGE_JUDGE_BASE_URL".to_owned(),
                "http://localhost:11434/v1".to_owned(),
            ),
            ("RELAY_KNOWLEDGE_JUDGE_API_KEY".to_owned(), "token".to_owned()),
            (
                "RELAY_KNOWLEDGE_JUDGE_MODEL".to_owned(),
                "judge-model".to_owned(),
            ),
        ]);
        let settings = judge_settings(&env);
        assert_eq!(settings.backend, JudgeBackend::Http);
        assert!(settings.missing.is_empty());
        assert_eq!(
            normalize_judge_chat_url(&settings.http_base_url),
            "http://localhost:11434/v1/chat/completions"
        );
        let (command, body) = judge_http_command(&settings, "judge prompt").expect("http command");
        assert!(!command.join(" ").contains("token"));
        assert!(body.contains("judge-model"));
        assert!(body.contains("judge prompt"));
    }

    #[test]
    fn judge_backend_http_env_selects_http_runner() {
        let env = BTreeMap::from([
            (
                "RELAY_KNOWLEDGE_JUDGE_BACKEND".to_owned(),
                "http".to_owned(),
            ),
            (
                "RELAY_KNOWLEDGE_JUDGE_BASE_URL".to_owned(),
                "http://localhost:11434".to_owned(),
            ),
            ("RELAY_KNOWLEDGE_JUDGE_API_KEY".to_owned(), "token".to_owned()),
            (
                "RELAY_KNOWLEDGE_JUDGE_MODEL".to_owned(),
                "judge-model".to_owned(),
            ),
        ]);
        let settings = judge_settings(&env);
        assert_eq!(settings.backend, JudgeBackend::Http);
        assert_eq!(settings_summary(&settings)["backend"], "http");
    }

    #[test]
    fn judge_rejects_unsupported_backend() {
        let env = BTreeMap::from([(
            "RELAY_KNOWLEDGE_JUDGE_BACKEND".to_owned(),
            "httpp".to_owned(),
        )]);

        let settings = judge_settings(&env);

        assert!(settings.configuration_error.is_some());
        assert!(!settings_summary(&settings)["configured"]
            .as_bool()
            .expect("configured should be boolean"));
    }

    #[test]
    fn explicit_cli_judge_command_wins_over_stray_http_env() {
        let env = BTreeMap::from([
            (
                "RELAY_KNOWLEDGE_JUDGE_BASE_URL".to_owned(),
                "http://localhost:11434".to_owned(),
            ),
            (
                "RELAY_KNOWLEDGE_JUDGE_COMMAND".to_owned(),
                "custom-judge --file {prompt_file}".to_owned(),
            ),
        ]);

        let settings = judge_settings(&env);

        assert_eq!(settings.backend, JudgeBackend::Cli);
        assert!(settings.missing.is_empty());
        assert_eq!(
            shell_split(&settings.command).expect("split").first(),
            Some(&"custom-judge".to_owned())
        );
    }

    #[test]
    fn file_case_enforces_payload_constraints() {
        let case = serde_json::json!({
            "id": "file_constraints",
            "max_results": 1,
            "truncated": true,
            "degraded_reason_contains": "budget",
            "expected": [{"relative_path": "a.md"}]
        });
        let result = CommandResult {
            name: "files_query".to_owned(),
            command: vec!["relay-knowledge".to_owned()],
            exit_code: 0,
            duration_ms: 1,
            stdout: serde_json::json!({
                "results": [{"relative_path": "a.md"}, {"relative_path": "b.md"}],
                "truncated": false,
                "degraded_reason": "stale"
            })
            .to_string(),
            stderr: String::new(),
        };

        let observation = score_file_case("fixture", &case, &result);

        assert!(!observation.passed);
        assert!(observation.message.contains("max_results=1"));
        assert!(observation.message.contains("truncated=false expected=true"));
        assert!(observation.message.contains("missing=budget"));
    }

    #[test]
    fn repository_case_enforces_payload_constraints() {
        let case = serde_json::json!({
            "id": "repo_constraints",
            "degraded_reason_contains": "external dependency import",
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
                "degraded_reason": "external dependency import is not indexed in the code graph"
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
    fn malformed_json_fails_file_case() {
        let case = serde_json::json!({"id": "negative", "expect_empty": true});
        let result = CommandResult {
            name: "files_query".to_owned(),
            command: vec!["relay-knowledge".to_owned()],
            exit_code: 0,
            duration_ms: 1,
            stdout: "not json".to_owned(),
            stderr: String::new(),
        };

        let observation = score_file_case("fixture", &case, &result);

        assert!(!observation.passed);
        assert!(observation.message.contains("invalid JSON"));
    }

    #[test]
    fn provider_probe_ok_false_fails_gate() {
        let mut result = CommandResult {
            name: "semantic_vector_provider_probe".to_owned(),
            command: vec!["relay-knowledge".to_owned()],
            exit_code: 0,
            duration_ms: 1,
            stdout: serde_json::json!({"ok": false, "error_code": "auth_failed"}).to_string(),
            stderr: String::new(),
        };

        assert!(!validate_provider_probe(&mut result));
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, "auth_failed");
    }

    #[test]
    fn judge_prompt_truncates_diff_on_char_boundary_and_includes_targets() {
        let suite = serde_json::json!({
            "max_diff_chars": 1,
            "max_doc_chars": 1,
            "competitive_feature_targets": ["repo-set"],
            "implementation_guardrails": ["no fixture special casing"],
            "rubric": {"retrieval": 0.5}
        });

        let prompt = build_judge_prompt(JudgePromptInput {
            workspace: std::path::Path::new("."),
            suite: &suite,
            generated_diff: true,
            candidate_diff: "汉字",
            gates: &[],
            cases: &[],
            metrics: &[],
            repo_reports: &[],
        });

        assert!(prompt.contains("汉\n...diff truncated..."));
        assert!(prompt.contains("competitive_feature_targets"));
        assert!(prompt.contains("repo-set"));
        assert!(prompt.contains("implementation_guardrails"));
    }

    #[test]
    fn percentile_selects_expected_rank() {
        assert_eq!(percentile(&[10, 20, 30, 40], 50), 20);
        assert_eq!(percentile(&[10, 20, 30, 40], 95), 30);
    }
}
