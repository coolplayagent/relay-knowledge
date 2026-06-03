use std::{sync::Arc, time::Duration};

use super::*;
use crate::{
    application::RuntimeConfiguration,
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeIndexSnapshot, CodeIndexTaskState,
        CodeParseStatus, CodeRepositoryRegistration, RepositoryCodeFileRecord,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskSeed, CodeRepositorySetMemberSeed,
        CodeRepositorySetSeed, CodeRepositoryStore, SqliteGraphStore,
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
    let worker = tokio::spawn(super::service_cli::run_code_repository_set_refresh_loop(
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

#[tokio::test]
async fn service_code_index_worker_pool_uses_configured_parallelism() {
    let service = RelayKnowledgeService::with_store(
        runtime().await,
        Arc::new(SqliteGraphStore::open_in_memory().expect("store should open")),
    );
    let (shutdown, shutdown_receiver) = tokio::sync::watch::channel(false);

    let workers = super::service_cli::run_code_index_worker_pool(
        service,
        3,
        Duration::from_millis(10),
        shutdown_receiver,
    );

    assert_eq!(workers.len(), 3);
    let _ = shutdown.send(true);
    for worker in workers {
        worker.await.expect("worker should stop");
    }
}

#[tokio::test]
async fn service_startup_recovers_orphaned_code_index_worker_leases() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo-a", "app", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    let live = store
        .queue_code_index_task(code_index_seed("fp-live", "scope-live"))
        .await
        .expect("live task should queue");
    let orphaned = store
        .queue_code_index_task(code_index_seed("fp-orphaned", "scope-orphaned"))
        .await
        .expect("orphaned task should queue");
    let now_ms = current_time_millis();
    for (task_id, lease_owner) in [
        (
            live.task_id.clone(),
            format!("code-index-worker-{}", std::process::id()),
        ),
        (
            orphaned.task_id.clone(),
            "code-index-worker-999999".to_owned(),
        ),
    ] {
        store
            .claim_code_index_task(CodeIndexTaskClaimRequest {
                task_id: Some(task_id),
                lease_owner,
                lease_duration_ms: 60_000,
                max_attempts: 3,
                now_ms,
            })
            .await
            .expect("task claim should read")
            .expect("task should claim");
    }

    let service = RelayKnowledgeService::with_store(runtime().await, store.clone());
    let recovered = service
        .recover_orphaned_code_index_tasks_on_startup()
        .await
        .expect("startup recovery should run");
    let live_after = store
        .code_index_task(live.task_id)
        .await
        .expect("live task should load")
        .expect("live task should exist");
    let orphaned_after = store
        .code_index_task(orphaned.task_id)
        .await
        .expect("orphaned task should load")
        .expect("orphaned task should exist");

    assert_eq!(recovered, 1);
    let live_owner = format!("code-index-worker-{}", std::process::id());
    assert_eq!(live_after.state, CodeIndexTaskState::Running);
    assert_eq!(live_after.lease_owner.as_deref(), Some(live_owner.as_str()));
    assert_eq!(orphaned_after.state, CodeIndexTaskState::Retrying);
    assert!(orphaned_after.lease_owner.is_none());
    assert_eq!(
        orphaned_after.last_error_kind.as_deref(),
        Some("lease_orphaned")
    );
}

async fn runtime() -> RuntimeConfiguration {
    let environment = test_environment();
    RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose")
}

#[cfg(windows)]
fn test_environment() -> EnvironmentConfig {
    EnvironmentConfig::from_pairs(
        PlatformKind::Windows,
        [
            ("USERPROFILE", "C:\\Users\\alice"),
            ("APPDATA", "C:\\Users\\alice\\AppData\\Roaming"),
            ("LOCALAPPDATA", "C:\\Users\\alice\\AppData\\Local"),
            ("TEMP", "C:\\Users\\alice\\AppData\\Local\\Temp"),
            ("RELAY_KNOWLEDGE_HOME", "C:\\relay"),
        ],
    )
    .expect("environment should parse")
}

#[cfg(not(windows))]
fn test_environment() -> EnvironmentConfig {
    EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse")
}

fn code_index_seed(fingerprint: &str, source_scope: &str) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: "repo-a".to_owned(),
        alias: "app".to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: format!("commit-{source_scope}"),
        tree_hash: format!("tree-{source_scope}"),
        source_scope: source_scope.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        mode: CodeIndexMode::Full,
        input_fingerprint: fingerprint.to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms: 1,
    }
}

fn current_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
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
