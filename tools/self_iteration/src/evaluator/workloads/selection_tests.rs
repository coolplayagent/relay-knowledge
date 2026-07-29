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
        assert!(!selection.runs_agent_workflows("fast"));
        assert!(!selection.runs_file_fixtures("fast"));
        assert!(!selection.runs_research_judge("fast"));
        assert_eq!(
            selection.skipped_suites("fast"),
            vec!["file_fixtures", "agent_workflows", "research_judge"]
        );
    }

    #[test]
    fn focused_agent_workflows_runs_agent_suite() {
        let config = Config::parse(vec![
            "evaluate".to_owned(),
            "--categories".to_owned(),
            "agent_workflows".to_owned(),
        ])
        .expect("config should parse");
        let selection = WorkloadSelection::new(&config);

        assert!(selection.runs_repository_workload("fast"));
        assert!(selection.runs_repository_sets("fast"));
        assert!(selection.runs_semantic_vector("fast"));
        assert!(selection.runs_agent_workflows("fast"));
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
        assert!(selection.runs_agent_workflows("full"));
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
        assert!(names.iter().any(|name| name == "grep_budget_fixture"));
        assert!(names
            .iter()
            .any(|name| name == "index_performance_many_files"));
        assert!(!names
            .iter()
            .any(|name| name == "index_performance_wide_mixed_files"));
        assert!(names.iter().any(|name| name == "nonstandard_layout_fixture"));
        assert!(names.iter().any(|name| name == "project_alias_fixture"));
    }

    #[test]
    fn wide_index_performance_fixture_is_full_only() {
        let repo_config = serde_json::json!({
            "generated_fixture": "index_performance_wide_mixed_files_v1"
        });

        assert!(!repository_in_profile(
            "fast",
            "index_performance_wide_mixed_files",
            &repo_config
        ));
        assert!(repository_in_profile(
            "full",
            "index_performance_wide_mixed_files",
            &repo_config
        ));
        assert!(repository_in_profile(
            "exhaustive",
            "index_performance_wide_mixed_files",
            &repo_config
        ));
    }

    #[test]
    fn register_command_can_omit_alias_for_default_project_name() {
        let binary = Path::new("relay-knowledge");
        let root = Path::new("/work/project");

        assert_eq!(
            register_command(binary, root, None),
            vec![
                "relay-knowledge",
                "repo",
                "register",
                "/work/project",
                "--format",
                "json"
            ]
        );
        assert_eq!(
            register_command(binary, root, Some("stable")),
            vec![
                "relay-knowledge",
                "repo",
                "register",
                "/work/project",
                "--alias",
                "stable",
                "--format",
                "json"
            ]
        );
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
    fn registration_guardrail_cases_are_preserved_for_fast() {
        let cases = vec![
            serde_json::json!({"id": "regular", "repository": "fixture"}),
            serde_json::json!({
                "id": "reject_register_language",
                "repository": "fixture",
                "expect_failure": true,
                "language_filters": ["cpp"],
                "guardrail": true
            }),
        ];

        let selected = select_registration_cases_for_profile("fast", None, cases);

        assert!(selected.iter().any(|case| {
            string_or(case, "id", "") == "reject_register_language"
                && case.get("guardrail").and_then(Value::as_bool) == Some(true)
        }));
    }
