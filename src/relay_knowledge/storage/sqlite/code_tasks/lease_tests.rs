use super::*;
use crate::{
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeIndexTaskState, CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskLeaseRecovery, CodeIndexTaskSeed,
        CodeRepositoryStore, SqliteGraphStore,
    },
};

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
                code_tasks::claim_task(
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
        .run_read(code_tasks::running_task_leases)
        .await
        .expect("running leases should list");
    assert_eq!(leases.len(), 2);
    let recovered = store
        .run({
            let task_id = queued_a.task_id.clone();
            move |connection| {
                code_tasks::recover_task_leases_by_task(
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
        .run(move |connection| code_tasks::queue_task(connection, seed))
        .await
        .expect("task should queue")
}

async fn task_by_id(
    store: &SqliteGraphStore,
    task_id: String,
) -> crate::domain::CodeIndexTaskRecord {
    store
        .run(move |connection| code_tasks::task_by_id(connection, &task_id))
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
