use super::super::test_support::{FixtureRepo, context, service_with_memory_store};
use super::*;
use crate::{
    api::{CodeRepositoryFreshnessState, CodeRepositoryRegisterRequest},
    domain::{
        CodeContentIntegrityState, CodeIndexMode, CodeIndexRequest, CodeQueryKind,
        CodeRepositorySelector, CodeRetrievalRequest, FrameworkGraphRequest, FreshnessPolicy,
    },
};

#[tokio::test]
async fn completed_partial_index_is_fresh_and_diagnostics_page_across_head_changes() {
    let repo = FixtureRepo::create("issue393-diagnostics");
    repo.write(
        "healthy.py",
        "def healthy_lookup(value):\n    return value + 1\n",
    );
    for i in 0..25 {
        repo.write(
            &format!("broken_{i:02}.py"),
            "def broken(value):\n    return (\n",
        );
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "fixture"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "demo".into(),
                path_filters: vec![],
                language_filters: vec![],
            },
            context("register"),
        )
        .await
        .unwrap();
    let selector = CodeRepositorySelector::new("demo", "HEAD", vec![], vec![]).unwrap();
    let index = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector.clone(),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index"),
        )
        .await
        .unwrap();
    assert_eq!(index.summary.degraded_file_count, 25);
    let before = service
        .code_repository_status(selector.clone(), context("before"))
        .await
        .unwrap();
    for policy in [
        FreshnessPolicy::AllowStale,
        FreshnessPolicy::WaitUntilFresh,
        FreshnessPolicy::WaitUntilFresh,
    ] {
        let request = CodeRetrievalRequest::new(
            "healthy_lookup",
            selector.clone(),
            CodeQueryKind::Definition,
            10,
            policy,
        )
        .unwrap();
        let response = service
            .query_code_repository(request, context("query"))
            .await
            .unwrap();
        assert_eq!(
            response.freshness.state,
            CodeRepositoryFreshnessState::Fresh
        );
        assert_eq!(
            response.freshness.content_integrity.state,
            CodeContentIntegrityState::Partial
        );
        assert_eq!(
            response.freshness.content_integrity.degraded_file_count,
            Some(25)
        );
        assert!(response.results.iter().all(|hit| hit.path == "healthy.py"));
        assert!(!response.results.is_empty());
    }
    let framework = service
        .query_code_repository_framework_graph(
            FrameworkGraphRequest::new(
                None,
                selector.clone(),
                vec![],
                vec![],
                10,
                FreshnessPolicy::AllowStale,
            )
            .unwrap(),
            context("framework"),
        )
        .await
        .unwrap();
    assert_eq!(
        framework.freshness.state,
        CodeRepositoryFreshnessState::Fresh
    );
    assert_eq!(
        framework.freshness.content_integrity,
        before.status.content_integrity
    );
    assert!(
        framework
            .freshness
            .agent_instructions
            .iter()
            .any(|instruction| { instruction.contains("Indexed content is partial") })
    );
    let after = service
        .code_repository_status(selector.clone(), context("after"))
        .await
        .unwrap();
    assert_eq!(before.checkpoint, after.checkpoint);
    let mut request = CodeDiagnosticsRequest {
        repository: selector,
        limit: 10,
        cursor: None,
    };
    let first = service
        .code_repository_diagnostics(request.clone(), context("page1"))
        .await
        .unwrap();
    assert_eq!(first.degraded_file_count, 25);
    assert_eq!(first.diagnostics.len(), 10);
    repo.write("new.py", "def new():\n    return 1\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "move HEAD"]);
    request.cursor = first.next_cursor;
    let second = service
        .code_repository_diagnostics(request.clone(), context("page2"))
        .await
        .unwrap();
    assert_eq!(first.scope.scope_id, second.scope.scope_id);
    request.cursor = second.next_cursor;
    let third = service
        .code_repository_diagnostics(request.clone(), context("page3"))
        .await
        .unwrap();
    assert_eq!(third.diagnostics.len(), 5);
    assert!(third.next_cursor.is_none());
    request.repository.path_filters = vec!["healthy.py".into()];
    assert!(
        service
            .code_repository_diagnostics(request, context("wrong-filter"))
            .await
            .is_err()
    );
    let mut invalid = CodeDiagnosticsRequest {
        repository: CodeRepositorySelector::new("demo", "HEAD", vec![], vec![]).unwrap(),
        limit: 10,
        cursor: Some("invalid".into()),
    };
    assert!(
        service
            .code_repository_diagnostics(invalid.clone(), context("invalid-cursor"))
            .await
            .is_err()
    );
    invalid.cursor = None;
    invalid.repository.path_filters = vec!["../outside".into()];
    assert!(
        service
            .code_repository_diagnostics(invalid, context("invalid-path"))
            .await
            .is_err()
    );
    let report = service
        .code_repository_report(
            CodeRepositorySelector::new("demo", "HEAD", vec![], vec![]).unwrap(),
            context("report"),
        )
        .await
        .unwrap();
    assert!(report.report.degradation_summary_truncated);
    assert_eq!(report.report.degradation_summary.len(), 20);
    for i in 0..25 {
        repo.write(
            &format!("broken_{i:02}.py"),
            "def repaired(value):\n    return value\n",
        );
    }
    repo.git(["mv", "broken_00.py", "renamed.py"]);
    repo.git(["rm", "-f", "broken_01.py"]);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "repair rename and remove diagnostics"]);
    let repaired = service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new("demo", "HEAD", vec![], vec![]).unwrap(),
                mode: CodeIndexMode::incremental(first.scope.resolved_commit_sha.clone(), "HEAD")
                    .unwrap(),
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("repair-index"),
        )
        .await
        .unwrap();
    assert_eq!(
        repaired.status.content_integrity.state,
        CodeContentIntegrityState::Complete
    );
    assert_eq!(
        repaired.status.content_integrity.degraded_file_count,
        Some(0)
    );
    let page = service
        .code_repository_diagnostics(
            CodeDiagnosticsRequest {
                repository: CodeRepositorySelector::new("demo", "HEAD", vec![], vec![]).unwrap(),
                limit: 50,
                cursor: None,
            },
            context("repaired-page"),
        )
        .await
        .unwrap();
    assert!(page.diagnostics.is_empty());
}

#[tokio::test]
async fn diagnostic_continuation_keeps_retained_scope_when_a_narrower_scope_is_published() {
    use super::super::test_support::service_with_store;
    use crate::{domain::code_snapshot_scope_id, storage::SqliteGraphStore};
    use rusqlite::{Connection, params};
    use std::sync::Arc;

    let repo = FixtureRepo::create("diagnostic-retained-scope");
    for name in ["a", "b", "c"] {
        repo.write(&format!("src/{name}.py"), "def broken():\n    return (\n");
    }
    repo.write("healthy.py", "def healthy():\n    return 1\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "diagnostic fixture"]);
    // A test-only connection models publication of another scope while the old one is retained.
    let database = repo.path.join(".git/diagnostics.sqlite");
    let store = Arc::new(SqliteGraphStore::open(&database).unwrap());
    let service = service_with_store(store).await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "demo".into(),
                path_filters: vec![],
                language_filters: vec![],
            },
            context("register-retained"),
        )
        .await
        .unwrap();
    let selector = CodeRepositorySelector::new("demo", "HEAD", vec![], vec![]).unwrap();
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector,
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-retained"),
        )
        .await
        .unwrap();
    let mut request = CodeDiagnosticsRequest {
        repository: CodeRepositorySelector::new("demo", "HEAD", vec!["src".into()], vec![])
            .unwrap(),
        limit: 1,
        cursor: None,
    };
    let first = service
        .code_repository_diagnostics(request.clone(), context("original-page"))
        .await
        .unwrap();
    let mut large = request.clone();
    large
        .repository
        .path_filters
        .extend((0..4).map(|i| format!("{i}{}", "x".repeat(4095))));
    let large_first = service
        .code_repository_diagnostics(large.clone(), context("large-filter-page"))
        .await
        .unwrap();
    assert!(large_first.next_cursor.as_ref().unwrap().len() < 1024);
    large.cursor = large_first.next_cursor;
    let large_second = service
        .code_repository_diagnostics(large, context("large-filter-next"))
        .await
        .unwrap();
    assert_ne!(
        large_first.diagnostics[0].path,
        large_second.diagnostics[0].path
    );

    let narrow_scope = code_snapshot_scope_id(
        &first.scope.repository_id,
        &first.scope.tree_hash,
        &["src".into()],
        &[],
    );
    assert_ne!(narrow_scope, first.scope.scope_id);
    let connection = Connection::open(&database).unwrap();
    connection
        .execute(
            "INSERT INTO code_repository_scopes (
            source_scope, repository_id, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, indexed_file_count,
            symbol_count, reference_count, chunk_count, stale, degraded_reason, retiring)
         SELECT ?2, repository_id, resolved_commit_sha, tree_hash, '[\"src\"]', '[]',
                3, 0, 0, 3, 0, degraded_reason, 0
         FROM code_repository_scopes WHERE source_scope = ?1",
            params![first.scope.scope_id, narrow_scope],
        )
        .unwrap();
    connection.execute(
        "INSERT INTO code_repository_file_diagnostics (repository_id, source_scope, path, parse_status, message)
         SELECT repository_id, ?2, path, parse_status, message
         FROM code_repository_file_diagnostics WHERE source_scope = ?1",
        params![first.scope.scope_id, narrow_scope]
    ).unwrap();
    let newly_selected = service
        .code_repository_diagnostics(request.clone(), context("narrow-page"))
        .await
        .unwrap();
    assert_eq!(newly_selected.scope.scope_id, narrow_scope);
    request.cursor = first.next_cursor;
    let second = service
        .code_repository_diagnostics(request.clone(), context("retained-next"))
        .await
        .unwrap();
    assert_eq!(second.scope, first.scope);
    assert_eq!(second.diagnostics[0].path, "src/b.py");
    assert_eq!(second.degraded_file_count, 3);
    connection
        .execute(
            "UPDATE code_repository_scopes SET retiring = 1 WHERE source_scope = ?1",
            params![first.scope.scope_id],
        )
        .unwrap();
    assert!(
        service
            .code_repository_diagnostics(request, context("retired-next"))
            .await
            .is_err()
    );
}
