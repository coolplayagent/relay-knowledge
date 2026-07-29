use super::*;

#[test]
fn freshness_merge_adds_returned_context_paths_to_agent_instructions() {
    let mut freshness =
        CodeRepositoryFreshnessDiagnostics::code_query(CodeRepositoryFreshnessInput {
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
