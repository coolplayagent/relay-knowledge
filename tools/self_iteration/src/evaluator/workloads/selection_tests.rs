use super::{
    WorkloadSelection, fast_repository_names, limit_preserving_guardrails, repository_in_profile,
    select_repository_cases_for_profile, semantic_vector_suite_for_selection,
};
use crate::{
    cases::{array_field, string_or},
    config::{CategorySet, Config},
    evaluator::workloads::repository_set::select_repository_set_cases_for_profile,
};
use serde_json::Value;

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
        select_repository_set_cases_for_profile("full", config.categories.as_ref(), repo_set_cases)
            .len(),
        2
    );
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
    assert!(
        names
            .iter()
            .any(|name| name == "index_performance_many_files")
    );
    assert!(
        names
            .iter()
            .any(|name| name == "index_performance_c_fragment")
    );
    assert!(
        !names
            .iter()
            .any(|name| name == "index_performance_wide_mixed_files")
    );
    assert!(
        names
            .iter()
            .any(|name| name == "nonstandard_layout_fixture")
    );
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
fn fast_default_repositories_include_cross_language_fixture() {
    if std::env::var("RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS").is_ok() {
        return;
    }

    let names = fast_repository_names();

    assert!(
        names
            .iter()
            .any(|name| name == "cross_language_syntax_fixture")
    );
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
fn fast_preserves_import_graph_and_grep_guardrail_cases() {
    let cases = vec![
        serde_json::json!({"id": "regular_a", "kind": "definition"}),
        serde_json::json!({
            "id": "typescript_syntax_external_react_import_graph",
            "repository": "typescript_syntax_fixture",
            "kind": "imports",
            "query": "react",
            "guardrail": true,
            "expected": [{
                "path": "src/component.tsx",
                "retrieval_layer": "import_graph"
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
        .find(|case| string_or(case, "id", "") == "typescript_syntax_external_react_import_graph")
        .expect("fast should preserve the structured import guardrail");
    let expected = array_field(case, "expected");

    assert_eq!(string_or(case, "kind", ""), "imports");
    assert_eq!(
        string_or(&expected[0], "retrieval_layer", ""),
        "import_graph"
    );
    assert!(case.get("degraded_reason").is_some_and(Value::is_null));
    assert!(
        case.get("guardrail")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    );
    assert!(selected.iter().any(|case| {
        string_or(case, "id", "") == "grep_budget_reference_late_comment_after_scope_budget"
            && case.get("degraded_reason").and_then(Value::as_bool) == Some(false)
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
