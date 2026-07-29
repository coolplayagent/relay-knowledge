use super::*;
use crate::{
    domain::{CodeIndexMode, CodeIndexResourceBudget, CodeRepositoryRegistration},
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskFailure, CodeIndexTaskSeed, CodeRepositoryStore,
        SqliteGraphStore,
    },
};

#[tokio::test]
async fn code_index_task_queue_status_reports_master_worker_backlog() {
    let store = registered_store().await;
    for (repository_id, alias) in [
        ("repo-other", "fixture-other"),
        ("repo-third", "fixture-third"),
    ] {
        store
            .upsert_code_repository(
                CodeRepositoryRegistration::new(
                    repository_id,
                    alias,
                    format!("/tmp/{repository_id}"),
                    vec!["src".to_owned()],
                    vec!["rust".to_owned()],
                )
                .expect("registration should validate"),
            )
            .await
            .expect("repository should persist");
    }
    let retrying = store
        .run(|connection| code_tasks::queue_task(connection, seed("fp-retry", "scope-retry", 10)))
        .await
        .expect("retrying task should queue");
    let _queued = store
        .run(|connection| code_tasks::queue_task(connection, seed("fp-queued", "scope-queued", 11)))
        .await
        .expect("queued task should persist");
    let dead = store
        .run(|connection| {
            code_tasks::queue_task(
                connection,
                seed_for_repo("repo-other", "fixture-other", "fp-dead", "scope-dead", 12),
            )
        })
        .await
        .expect("dead-letter task should queue");
    let running = store
        .run(|connection| {
            code_tasks::queue_task(
                connection,
                seed_for_repo(
                    "repo-third",
                    "fixture-third",
                    "fp-running",
                    "scope-running",
                    13,
                ),
            )
        })
        .await
        .expect("running task should queue");

    let retrying = claim_task(&store, retrying.task_id, "worker-retry", 20).await;
    fail_task(&store, retrying, "fixture_retry", "retry later", 3, 21).await;
    let dead = claim_task(&store, dead.task_id, "worker-dead", 22).await;
    fail_task(&store, dead, "fixture_dead", "dead letter reason", 1, 30).await;
    let _running = claim_task(&store, running.task_id, "worker-running", 31).await;

    let status = store
        .run_read(code_tasks::queue_status)
        .await
        .expect("queue status should load");

    assert_eq!(status.queued_task_count, 1);
    assert_eq!(status.running_task_count, 1);
    assert_eq!(status.retrying_task_count, 1);
    assert_eq!(status.dead_letter_task_count, 1);
    assert_eq!(status.running_lease_count, 1);
    assert_eq!(status.last_error.as_deref(), Some("dead letter reason"));
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo",
                "fixture",
                "/tmp/repo",
                vec!["src".to_owned()],
                vec!["rust".to_owned()],
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
}

async fn claim_task(
    store: &SqliteGraphStore,
    task_id: String,
    lease_owner: &str,
    now_ms: u64,
) -> crate::domain::CodeIndexTaskRecord {
    let lease_owner = lease_owner.to_owned();
    store
        .run(move |connection| {
            code_tasks::claim_task(
                connection,
                CodeIndexTaskClaimRequest {
                    task_id: Some(task_id),
                    lease_owner,
                    lease_duration_ms: 100,
                    max_attempts: 3,
                    now_ms,
                },
            )
        })
        .await
        .expect("claim should query")
        .expect("task should claim")
}

async fn fail_task(
    store: &SqliteGraphStore,
    task: crate::domain::CodeIndexTaskRecord,
    error_kind: &str,
    error_message: &str,
    max_attempts: u32,
    now_ms: u64,
) {
    let error_kind = error_kind.to_owned();
    let error_message = error_message.to_owned();
    store
        .run(move |connection| {
            code_tasks::fail_task(
                connection,
                CodeIndexTaskFailure {
                    task_id: task.task_id,
                    lease_owner: task.lease_owner.expect("task should have lease owner"),
                    attempt_count: task.attempt_count,
                    error_kind,
                    error_message,
                    retry_backoff_ms: 10,
                    max_attempts,
                    now_ms,
                },
            )
        })
        .await
        .expect("task failure should persist");
}

fn seed(fingerprint: &str, scope: &str, now_ms: u64) -> CodeIndexTaskSeed {
    seed_for_repo("repo", "fixture", fingerprint, scope, now_ms)
}

fn seed_for_repo(
    repository_id: &str,
    alias: &str,
    fingerprint: &str,
    scope: &str,
    now_ms: u64,
) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: repository_id.to_owned(),
        alias: alias.to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: format!("commit-{scope}"),
        tree_hash: format!("tree-{scope}"),
        source_scope: scope.to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        mode: CodeIndexMode::Full,
        input_fingerprint: fingerprint.to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms,
    }
}
