use super::*;

#[test]
fn scope_metadata_filters_preserve_first_seen_order_without_duplicates() {
    let merged = merged_filters(
        &["src/**".to_owned(), "tests/**".to_owned()],
        &["tests/**".to_owned(), "docs/**".to_owned()],
    );

    assert_eq!(merged, ["src/**", "tests/**", "docs/**"]);
}

#[test]
fn freshness_merge_adds_returned_context_paths_to_agent_instructions() {
    let mut freshness =
        CodeRepositoryFreshnessDiagnostics::code_query(CodeRepositoryFreshnessInput {
            content_integrity: Default::default(),
            query_degraded: true,
            graph_version: 1,
            freshness_policy: FreshnessPolicy::AllowStale,
            source_scope: Some("scope".to_owned()),
            requested_ref: "HEAD".to_owned(),
            requested_resolved_ref: "new".to_owned(),
            served_ref: "old".to_owned(),
            scope_stale: true,
            stale_reason: Some("active index".to_owned()),
            degraded_reason: None,
            pending: CodeRepositoryPendingIndexWork::default(),
            cursor: None,
            direct_source_read_paths: vec!["src/lib.rs".to_owned()],
        });

    freshness.merge_direct_source_read_paths(["src/main.rs".to_owned(), "src/lib.rs".to_owned()]);

    assert_eq!(
        freshness.direct_source_read_paths,
        vec!["src/lib.rs".to_owned(), "src/main.rs".to_owned()]
    );
    assert!(freshness.agent_instructions.iter().any(|instruction| {
        instruction.contains("src/lib.rs") && instruction.contains("src/main.rs")
    }));
}

#[test]
fn content_integrity_does_not_override_version_or_query_failures() {
    let make = |query_degraded, scope_stale, active| {
        CodeRepositoryFreshnessDiagnostics::code_query(CodeRepositoryFreshnessInput {
            content_integrity: crate::domain::CodeContentIntegrity::measured("scope".into(), 25),
            query_degraded,
            graph_version: 1,
            freshness_policy: FreshnessPolicy::WaitUntilFresh,
            source_scope: Some("scope".into()),
            requested_ref: "HEAD".into(),
            requested_resolved_ref: "commit".into(),
            served_ref: "commit".into(),
            scope_stale,
            stale_reason: None,
            degraded_reason: Some("partial diagnostics".into()),
            pending: CodeRepositoryPendingIndexWork {
                active_matches_request: active,
                ..Default::default()
            },
            cursor: None,
            direct_source_read_paths: Vec::new(),
        })
    };
    assert_eq!(
        make(false, false, false).state,
        CodeRepositoryFreshnessState::Fresh
    );
    assert_eq!(
        make(true, false, false).state,
        CodeRepositoryFreshnessState::Degraded
    );
    assert_eq!(
        make(false, true, false).state,
        CodeRepositoryFreshnessState::Stale
    );
    assert_eq!(
        make(false, true, true).state,
        CodeRepositoryFreshnessState::Pending
    );
    let mut response = make(false, false, false);
    response.merge_direct_source_read_paths(["healthy.py".into()]);
    assert!(
        response
            .agent_instructions
            .iter()
            .any(|s| s.contains("content is partial"))
    );
    let mut json = serde_json::to_value(response).unwrap();
    json.as_object_mut().unwrap().remove("content_integrity");
    let legacy: CodeRepositoryFreshnessDiagnostics = serde_json::from_value(json).unwrap();
    assert_eq!(
        legacy.content_integrity.state,
        crate::domain::CodeContentIntegrityState::Unknown
    );
}
