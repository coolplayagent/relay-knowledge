use super::{agent_query_command, agent_workflow_case_in_profile, fallback_ratio};

#[test]
fn profile_selection_and_fallback_ratio_enforce_workflow_budgets() {
    let full_case = serde_json::json!({"profile": "full"});
    let exhaustive_case = serde_json::json!({"profile": "exhaustive"});

    assert!(agent_workflow_case_in_profile("full", &full_case));
    assert!(!agent_workflow_case_in_profile("fast", &full_case));
    assert!(!agent_workflow_case_in_profile("full", &exhaustive_case));
    assert_eq!(fallback_ratio(0, 0), 0.0);
    assert_eq!(fallback_ratio(2, 4), 0.5);
}

#[test]
fn query_command_keeps_filters_and_freshness_explicit() {
    let step = serde_json::json!({
        "query": "Service",
        "kind": "definition",
        "path_filters": ["src"],
        "language_filters": ["rust"]
    });

    let command = agent_query_command(
        std::path::Path::new("relay-knowledge"),
        "repo",
        "HEAD",
        &step,
    );

    assert!(command.windows(2).any(|pair| pair == ["--path", "src"]));
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--language", "rust"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--freshness", "wait-until-fresh"])
    );
}
