use super::super::claim_task_at;
use super::queue_task;
use crate::{
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeIndexTaskRecord, CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskSeed, RepositoryCatalogStore as _,
        SqliteGraphStore, StorageError,
    },
};

#[tokio::test]
async fn terminal_fingerprint_reset_deletes_the_previous_publication_receipt() {
    let store = registered_store().await;
    let queued = store
        .run(|connection| queue_task(connection, seed("terminal-reset", 100)))
        .await
        .expect("task should queue");
    let claimed = claim_at(
        &store,
        CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "worker-old".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 101,
        },
    )
    .await;
    insert_receipt(&store, &claimed).await;
    store
        .run({
            let task_id = claimed.task_id.clone();
            move |connection| {
                connection.execute(
                    "UPDATE code_repository_index_tasks
                     SET state = 'succeeded', lease_owner = NULL, lease_expires_at_ms = NULL
                     WHERE task_id = ?1",
                    [&task_id],
                )?;
                Ok(())
            }
        })
        .await
        .expect("terminal fixture should persist");

    let reset = store
        .run(|connection| queue_task(connection, seed("terminal-reset", 200)))
        .await
        .expect("terminal fingerprint should reset");

    assert_eq!(reset.task_id, claimed.task_id);
    assert_eq!(reset.attempt_count, 0);
    assert_eq!(reset.publication_generation, 0);
    assert_eq!(receipt_audit(&store, &reset.task_id).await, (0, None));
}

#[tokio::test]
async fn expired_lease_reclaim_preserves_the_previous_generation_receipt() {
    let store = registered_store().await;
    let queued = store
        .run(|connection| queue_task(connection, seed("lease-reclaim", 100)))
        .await
        .expect("task should queue");
    let first = claim_at(
        &store,
        CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "worker-old".to_owned(),
            lease_duration_ms: 10,
            max_attempts: 3,
            now_ms: 101,
        },
    )
    .await;
    insert_receipt(&store, &first).await;

    let reclaimed = claim_at(
        &store,
        CodeIndexTaskClaimRequest {
            task_id: Some(first.task_id.clone()),
            lease_owner: "worker-new".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 112,
        },
    )
    .await;

    assert_eq!(reclaimed.task_id, first.task_id);
    assert_eq!(reclaimed.attempt_count, 2);
    assert!(reclaimed.publication_generation > first.publication_generation);
    assert_eq!(
        receipt_audit(&store, &reclaimed.task_id).await,
        (1, Some(first.publication_generation))
    );
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

async fn claim_at(
    store: &SqliteGraphStore,
    request: CodeIndexTaskClaimRequest,
) -> CodeIndexTaskRecord {
    let execution_now_ms = request.now_ms;
    store
        .run(move |connection| claim_task_at(connection, request, execution_now_ms))
        .await
        .expect("task claim should run")
        .expect("task should be claimable")
}

fn seed(fingerprint: &str, now_ms: u64) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        source_scope: "scope".to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        mode: CodeIndexMode::Full,
        input_fingerprint: fingerprint.to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms,
    }
}

async fn insert_receipt(store: &SqliteGraphStore, task: &CodeIndexTaskRecord) {
    let task = task.clone();
    store
        .run(move |connection| {
            connection.execute(
                "INSERT INTO code_repository_publication_receipts (
                     task_id, repository_id, source_scope,
                     publication_generation, published_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, 102)",
                rusqlite::params![
                    task.task_id,
                    task.repository_id,
                    task.source_scope,
                    task.publication_generation
                ],
            )?;
            Ok(())
        })
        .await
        .expect("publication receipt fixture should persist");
}

async fn receipt_audit(store: &SqliteGraphStore, task_id: &str) -> (usize, Option<u64>) {
    let task_id = task_id.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT COUNT(*), MAX(publication_generation)
                     FROM code_repository_publication_receipts WHERE task_id = ?1",
                    [&task_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("publication receipt audit should load")
}
