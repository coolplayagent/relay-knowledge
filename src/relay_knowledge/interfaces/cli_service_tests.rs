use std::{sync::Arc, time::Duration};

use super::*;
use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration, RepositoryCodeFileRecord,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{
        CodeRepositorySetMemberSeed, CodeRepositorySetSeed, CodeRepositoryStore, SqliteGraphStore,
    },
};

#[tokio::test]
async fn service_repo_set_refresh_loop_drains_queued_overlay_tasks() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo-a", "app", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(snapshot("repo-a", "scope-a"))
        .await
        .expect("snapshot should persist");
    store
        .create_code_repository_set(CodeRepositorySetSeed {
            alias: "workspace".to_owned(),
            description: None,
            default_ref_policy_json: "{\"default_ref\":\"HEAD\"}".to_owned(),
            now_ms: 10,
        })
        .await
        .expect("set should persist");
    store
        .add_code_repository_set_member(CodeRepositorySetMemberSeed {
            set_alias: "workspace".to_owned(),
            repository_id: "repo-a".to_owned(),
            repository_alias: "app".to_owned(),
            ref_selector: "commit-scope-a".to_owned(),
            resolved_commit_sha: "commit-scope-a".to_owned(),
            source_scope: "scope-a".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            priority: 0,
        })
        .await
        .expect("member should persist");

    let service = RelayKnowledgeService::with_store(runtime().await, store);
    let queued = service
        .start_code_repository_set_refresh("workspace".to_owned(), context("queue-refresh"))
        .await
        .expect("refresh should queue");
    assert!(queued.task.is_some());

    let (shutdown, shutdown_receiver) = tokio::sync::watch::channel(false);
    let worker = tokio::spawn(run_code_repository_set_refresh_loop(
        service.clone(),
        Duration::from_millis(10),
        shutdown_receiver,
    ));
    let mut status = queued.status;
    for _ in 0..50 {
        if !status.overlay.stale {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        status = service
            .code_repository_set_status("workspace".to_owned(), context("poll-refresh"))
            .await
            .expect("status should load")
            .status;
    }
    let _ = shutdown.send(true);
    worker.await.expect("worker should stop");

    assert_eq!(status.overlay.state, "fresh");
    assert!(!status.overlay.stale);
    assert!(
        service
            .run_code_repository_set_refresh_task_once(None, context("drained-refresh"))
            .await
            .expect("queue should be readable")
            .is_none()
    );
}

async fn runtime() -> RuntimeConfiguration {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse");
    RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose")
}

fn snapshot(repository_id: &str, source_scope: &str) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: repository_id.to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: format!("commit-{source_scope}"),
        tree_hash: format!("tree-{source_scope}"),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![RepositoryCodeFileRecord {
            repository_id: repository_id.to_owned(),
            source_scope: source_scope.to_owned(),
            file_id: format!("file-{source_scope}"),
            path: "src/lib.rs".to_owned(),
            language_id: "rust".to_owned(),
            blob_hash: format!("blob-{source_scope}"),
            byte_len: 1,
            line_count: 1,
            parse_status: CodeParseStatus::Parsed,
            degraded_reason: None,
        }],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn context(operation: &str) -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Cli,
        format!("req-{operation}"),
        format!("trace-{operation}"),
    )
}
