//! Inline name filters preserve externally unresolved callees without changing direction.
use super::*;

#[tokio::test]
async fn exact_call_name_filters_preserve_external_targets_and_caller_direction() {
    let repo = FixtureRepo::create("unresolved-call-name");
    repo.write("src/Client.java", "package demo;\nclass Client {\n void run() { External.send(); local(); }\n void local() {}\n void other() { run(); }\n}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "External callee name filters"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-external-name"),
        )
        .await
        .unwrap();
    let definitions = query(&service, "run", CodeQueryKind::Definition).await;
    let canonical = definitions
        .results
        .iter()
        .filter_map(|h| h.canonical_symbol_id.as_deref())
        .find(|id| id.ends_with(".run"))
        .unwrap();
    let plain = query(&service, canonical, CodeQueryKind::Callees).await;
    let external = plain
        .results
        .iter()
        .find(|h| h.edge_target_hint.as_deref() == Some("send"))
        .unwrap();
    assert_eq!(
        external.edge_resolution_state.as_deref(),
        Some("unresolved")
    );
    let filtered = query(
        &service,
        &format!("{canonical} name:SEND"),
        CodeQueryKind::Callees,
    )
    .await;
    assert_eq!(filtered.results.len(), 1);
    assert_eq!(
        filtered.results[0].edge_resolution_state,
        external.edge_resolution_state
    );
    assert_eq!(
        filtered.results[0].edge_target_hint,
        external.edge_target_hint
    );
    assert!(filtered.results[0].canonical_symbol_id.is_none());
    assert!(filtered.results[0].degraded_reason.is_none());
    assert!(
        query(
            &service,
            &format!("{canonical} name:absent"),
            CodeQueryKind::Callees
        )
        .await
        .results
        .is_empty()
    );
    let callers = query(
        &service,
        &format!("{canonical} name:other"),
        CodeQueryKind::Callers,
    )
    .await;
    assert_eq!(callers.results.len(), 1);
    assert!(
        callers.results[0]
            .canonical_symbol_id
            .as_deref()
            .unwrap()
            .ends_with(".other")
    );
    assert!(
        query(
            &service,
            &format!("{canonical} name:run"),
            CodeQueryKind::Callers
        )
        .await
        .results
        .is_empty()
    );
}
