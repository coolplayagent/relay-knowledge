use super::super::{queue_task, task_by_id as load_task_by_id};
use super::{claim_task, recover_task_leases_by_task, running_task_leases};
use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexTaskState,
        CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeIndexTaskLeaseRecovery,
        CodeIndexTaskSeed, CodeRepositoryStore, SqliteGraphStore,
    },
};

#[tokio::test]
async fn code_index_task_targeted_claim_preserves_repository_fifo_publication_order() {
    let store = registered_store().await;
    let first = queue(
        &store,
        seed_for_repo("repo", "fixture", "fp-first", "scope-first", 100),
    )
    .await;
    let second = queue(
        &store,
        seed_for_repo("repo", "fixture", "fp-second", "scope-second", 100),
    )
    .await;
    assert_eq!(second.created_at_ms, first.created_at_ms + 1);

    let skipped = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(second.task_id.clone()),
            lease_owner: "worker-later".to_owned(),
            lease_duration_ms: 1_000,
            max_attempts: 3,
            now_ms: 102,
        })
        .await
        .expect("targeted claim should inspect repository order");
    assert!(skipped.is_none());

    let running_first = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(first.task_id),
            lease_owner: "worker-first".to_owned(),
            lease_duration_ms: 1_000,
            max_attempts: 3,
            now_ms: 103,
        })
        .await
        .expect("first claim should run")
        .expect("first repository task should be claimable");
    store
        .complete_code_index_task(CodeIndexTaskCompletion {
            task_id: running_first.task_id.clone(),
            lease_owner: "worker-first".to_owned(),
            attempt_count: running_first.attempt_count,
            now_ms: 104,
        })
        .await
        .expect("first repository task should complete");

    let running_second = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(second.task_id),
            lease_owner: "worker-later".to_owned(),
            lease_duration_ms: 1_000,
            max_attempts: 3,
            now_ms: 105,
        })
        .await
        .expect("second claim should run")
        .expect("second task should become claimable after its predecessor completes");
    assert!(
        running_second.publication_generation > running_first.publication_generation,
        "publication generations must follow durable repository queue order"
    );
}

#[tokio::test]
async fn code_index_task_takeover_rejects_stale_sqlite_publication_fence() {
    let store = registered_store().await;
    let queued = queue(
        &store,
        seed_for_repo("repo", "fixture", "fp-fence", "scope-fence", 0),
    )
    .await;
    let first = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-old".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 1,
                    },
                )
            }
        })
        .await
        .expect("first claim should run")
        .expect("first attempt should claim");
    let second = store
        .run({
            let task_id = queued.task_id;
            move |connection| {
                claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-new".to_owned(),
                        lease_duration_ms: 100,
                        max_attempts: 3,
                        now_ms: 11,
                    },
                )
            }
        })
        .await
        .expect("takeover claim should run")
        .expect("expired attempt should be reclaimed");

    assert!(second.publication_generation > first.publication_generation);
    let error = store
        .run(move |connection| {
            let fence = CodeIndexPublicationFence {
                repository_id: first.repository_id,
                task_id: first.task_id,
                lease_owner: "worker-old".to_owned(),
                attempt_count: first.attempt_count,
                generation: first.publication_generation,
            };
            let guard = crate::storage::sqlite::code::lifecycle::publication_fence::prepare_guard(
                connection, fence, None,
            )?;
            let transaction = connection.transaction()?;
            transaction.execute(
                "UPDATE code_repositories SET state = 'fresh' WHERE repository_id = 'repo'",
                [],
            )?;
            guard.validate(&transaction)?;
            transaction.commit()?;
            Ok(())
        })
        .await
        .expect_err("stale attempt must be fenced before commit");
    assert!(error.to_string().contains("no longer active"));
}

#[tokio::test]
async fn selected_running_code_index_task_leases_recover_before_ttl_expiry() {
    let store = registered_store().await;
    let queued_a = queue(
        &store,
        seed_for_repo("repo", "fixture", "fp-a", "scope-a", 10),
    )
    .await;
    let queued_b = queue(
        &store,
        seed_for_repo("repo-other", "fixture-other", "fp-b", "scope-b", 10),
    )
    .await;
    for (task_id, owner) in [
        (queued_a.task_id.clone(), "worker-a"),
        (queued_b.task_id.clone(), "worker-b"),
    ] {
        store
            .run(move |connection| {
                claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: owner.to_owned(),
                        lease_duration_ms: 10_000,
                        max_attempts: 3,
                        now_ms: 20,
                    },
                )
            })
            .await
            .expect("task should claim")
            .expect("task should be running");
    }

    let leases = store
        .run_read(running_task_leases)
        .await
        .expect("running leases should list");
    assert_eq!(leases.len(), 2);
    let recovered = store
        .run({
            let task_id = queued_a.task_id.clone();
            move |connection| {
                recover_task_leases_by_task(
                    connection,
                    CodeIndexTaskLeaseRecovery {
                        task_ids: vec![task_id],
                        now_ms: 30,
                        max_attempts: 3,
                        error_kind: "lease_orphaned".to_owned(),
                        error_message: "owner exited".to_owned(),
                    },
                )
            }
        })
        .await
        .expect("selected lease should recover");
    assert_eq!(recovered, 1);

    let first = task_by_id(&store, queued_a.task_id).await;
    let second = task_by_id(&store, queued_b.task_id).await;

    assert_eq!(first.state, CodeIndexTaskState::Retrying);
    assert!(first.lease_owner.is_none());
    assert_eq!(first.lease_expires_at_ms, None);
    assert_eq!(first.next_retry_at_ms, 30);
    assert_eq!(first.last_error_kind.as_deref(), Some("lease_orphaned"));
    assert_eq!(second.state, CodeIndexTaskState::Running);
    assert_eq!(second.lease_owner.as_deref(), Some("worker-b"));
    assert_eq!(second.lease_expires_at_ms, Some(10_020));
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    for (repository_id, alias, root_path) in [
        ("repo", "fixture", "/tmp/repo"),
        ("repo-other", "fixture-other", "/tmp/repo-other"),
    ] {
        store
            .upsert_code_repository(
                CodeRepositoryRegistration::new(
                    repository_id,
                    alias,
                    root_path,
                    vec!["src".to_owned()],
                    vec!["rust".to_owned()],
                )
                .expect("registration should validate"),
            )
            .await
            .expect("repository should persist");
    }
    store
}

async fn queue(
    store: &SqliteGraphStore,
    seed: CodeIndexTaskSeed,
) -> crate::domain::CodeIndexTaskRecord {
    store
        .run(move |connection| queue_task(connection, seed))
        .await
        .expect("task should queue")
}

async fn task_by_id(
    store: &SqliteGraphStore,
    task_id: String,
) -> crate::domain::CodeIndexTaskRecord {
    store
        .run(move |connection| load_task_by_id(connection, &task_id))
        .await
        .expect("task should load")
        .expect("task should exist")
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
