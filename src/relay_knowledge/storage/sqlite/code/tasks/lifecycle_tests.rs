use rusqlite::params;

use super::test_support::{
    complete_task_at_request_time, fail_task_at_request_time, persist_published_task_target,
};
use crate::{
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeIndexTaskState, CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeIndexTaskFailure,
        CodeIndexTaskLeaseRenewal, CodeIndexTaskSeed, CodeRepositoryStore, SqliteGraphStore,
    },
};

mod tasks {
    pub(super) use super::super::{active_task, queue_task, renew_task_lease_at, task_by_id};

    pub(super) fn claim_task(
        connection: &mut rusqlite::Connection,
        request: crate::storage::CodeIndexTaskClaimRequest,
    ) -> Result<Option<crate::domain::CodeIndexTaskRecord>, crate::storage::StorageError> {
        let execution_now_ms = request.now_ms;
        super::super::claim_task_at(connection, request, execution_now_ms)
    }

    pub(super) fn recover_expired_task_leases(
        connection: &mut rusqlite::Connection,
        now_ms: u64,
        max_attempts: u32,
    ) -> Result<(), crate::storage::StorageError> {
        super::super::recover_expired_task_leases_at(connection, now_ms, max_attempts, now_ms)
    }
}

#[tokio::test]
async fn code_index_task_queue_claim_and_complete_round_trip() {
    let store = registered_store().await;
    let queued = store
        .run(|connection| tasks::queue_task(connection, seed("fp-a", "scope-a", 100)))
        .await
        .expect("task should queue");
    let duplicate = store
        .run(|connection| tasks::queue_task(connection, seed("fp-a", "scope-a", 101)))
        .await
        .expect("matching active task should be reused");
    let distinct = store
        .run(|connection| tasks::queue_task(connection, seed("fp-b", "scope-b", 101)))
        .await
        .expect("distinct fingerprint should queue");

    assert_eq!(queued.task_id, duplicate.task_id);
    assert_ne!(queued.task_id, distinct.task_id);
    assert_eq!(queued.state, CodeIndexTaskState::Queued);
    assert_eq!(queued.path_filters, ["src"]);
    assert_eq!(queued.language_filters, ["rust"]);
    assert_eq!(queued.mode, CodeIndexMode::Full);

    let running = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-a".to_owned(),
                        lease_duration_ms: 50,
                        max_attempts: 3,
                        now_ms: 110,
                    },
                )
            }
        })
        .await
        .expect("claim should query")
        .expect("task should be claimable");
    assert_eq!(running.state, CodeIndexTaskState::Running);
    assert_eq!(running.lease_owner.as_deref(), Some("worker-a"));
    assert_eq!(running.lease_expires_at_ms, Some(160));
    assert_eq!(running.attempt_count, 1);
    let running_duplicate = store
        .run(|connection| tasks::queue_task(connection, seed("fp-a", "scope-a", 120)))
        .await
        .expect("matching running task should be reused");
    assert_eq!(running_duplicate.task_id, running.task_id);
    assert_eq!(running_duplicate.state, CodeIndexTaskState::Running);
    assert_eq!(running_duplicate.attempt_count, 1);

    let invalid_complete = store
        .run({
            let task_id = running.task_id.clone();
            let publication_generation = running.publication_generation;
            move |connection| {
                complete_task_at_request_time(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id,
                        lease_owner: "other-worker".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        now_ms: 120,
                    },
                )
            }
        })
        .await
        .expect_err("wrong lease owner should be rejected");
    assert!(invalid_complete.to_string().contains("lease"));

    store
        .run({
            let running = running.clone();
            move |connection| persist_published_task_target(connection, &running)
        })
        .await
        .expect("task target should be durably published before completion");

    let completed = store
        .run({
            let task_id = running.task_id.clone();
            let publication_generation = running.publication_generation;
            move |connection| {
                complete_task_at_request_time(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        now_ms: 130,
                    },
                )
            }
        })
        .await
        .expect("complete should persist");
    assert_eq!(completed.state, CodeIndexTaskState::Succeeded);
    assert!(completed.lease_owner.is_none());
    let next_claim = store
        .run(|connection| {
            tasks::claim_task(
                connection,
                CodeIndexTaskClaimRequest {
                    task_id: None,
                    lease_owner: "worker-next".to_owned(),
                    lease_duration_ms: 50,
                    max_attempts: 3,
                    now_ms: 140,
                },
            )
        })
        .await
        .expect("next queued task should query")
        .expect("distinct fingerprint should remain queued");
    assert_eq!(next_claim.task_id, distinct.task_id);

    let requeried = store
        .run(|connection| tasks::queue_task(connection, seed("fp-a", "scope-a", 200)))
        .await
        .expect("terminal duplicate should be reset");
    assert_eq!(requeried.task_id, queued.task_id);
    assert_eq!(requeried.state, CodeIndexTaskState::Queued);
    assert_eq!(requeried.attempt_count, 0);
}

#[tokio::test]
async fn code_index_task_claim_skips_live_repository_writer_and_claims_other_repositories() {
    let store = registered_store().await;
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo-other",
                "fixture-other",
                "/tmp/repo-other",
                vec!["src".to_owned()],
                vec!["rust".to_owned()],
            )
            .expect("second registration should validate"),
        )
        .await
        .expect("second repository should persist");
    let first = store
        .run(|connection| tasks::queue_task(connection, seed("fp-a", "scope-a", 100)))
        .await
        .expect("first task should queue");
    let second = store
        .run(|connection| tasks::queue_task(connection, seed("fp-b", "scope-b", 101)))
        .await
        .expect("second task should queue");
    let other_repository = store
        .run(|connection| {
            tasks::queue_task(
                connection,
                seed_for_repo("repo-other", "fixture-other", "fp-c", "scope-c", 102),
            )
        })
        .await
        .expect("other repository task should queue");

    let first_running = store
        .run({
            let task_id = first.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-a".to_owned(),
                        lease_duration_ms: 100,
                        max_attempts: 3,
                        now_ms: 110,
                    },
                )
            }
        })
        .await
        .expect("first claim should query")
        .expect("first task should claim");
    let same_repository_claim = store
        .run({
            let task_id = second.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-blocked".to_owned(),
                        lease_duration_ms: 100,
                        max_attempts: 3,
                        now_ms: 111,
                    },
                )
            }
        })
        .await
        .expect("same repository claim should query");
    let other_repository_running = store
        .run(|connection| {
            tasks::claim_task(
                connection,
                CodeIndexTaskClaimRequest {
                    task_id: None,
                    lease_owner: "worker-b".to_owned(),
                    lease_duration_ms: 100,
                    max_attempts: 3,
                    now_ms: 111,
                },
            )
        })
        .await
        .expect("other repository claim should query")
        .expect("other repository should claim while first is running");

    assert_eq!(first_running.task_id, first.task_id);
    assert!(same_repository_claim.is_none());
    assert_eq!(other_repository_running.task_id, other_repository.task_id);
    assert_eq!(first_running.lease_owner.as_deref(), Some("worker-a"));
    assert_eq!(
        other_repository_running.lease_owner.as_deref(),
        Some("worker-b")
    );
    assert_ne!(
        first_running.repository_id,
        other_repository_running.repository_id
    );
}

#[tokio::test]
async fn code_index_sqlite_lock_cases_task_transitions_return_updated_rows() {
    let store = registered_store().await;
    let queued = store
        .run(|connection| {
            tasks::queue_task(
                connection,
                seed("fp-transition-complete", "scope-complete", 10),
            )
        })
        .await
        .expect("task should queue");
    let running = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-a".to_owned(),
                        lease_duration_ms: 100,
                        max_attempts: 3,
                        now_ms: 20,
                    },
                )
            }
        })
        .await
        .expect("claim should query")
        .expect("task should claim");

    let renewed = store
        .run({
            let task_id = running.task_id.clone();
            let publication_generation = running.publication_generation;
            move |connection| {
                tasks::renew_task_lease_at(
                    connection,
                    CodeIndexTaskLeaseRenewal {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        lease_duration_ms: 200,
                        now_ms: 30,
                    },
                    30,
                )
            }
        })
        .await
        .expect("renewed task should be returned");
    assert_eq!(renewed.state, CodeIndexTaskState::Running);
    assert_eq!(renewed.lease_owner.as_deref(), Some("worker-a"));
    assert_eq!(renewed.lease_expires_at_ms, Some(230));

    store
        .run({
            let renewed = renewed.clone();
            move |connection| persist_published_task_target(connection, &renewed)
        })
        .await
        .expect("task target should be durably published before completion");

    let completed = store
        .run({
            let task_id = running.task_id.clone();
            let publication_generation = renewed.publication_generation;
            move |connection| {
                complete_task_at_request_time(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        now_ms: 40,
                    },
                )
            }
        })
        .await
        .expect("completed task should be returned");
    assert_eq!(completed.state, CodeIndexTaskState::Succeeded);
    assert!(completed.lease_owner.is_none());
    assert!(completed.lease_expires_at_ms.is_none());
    assert_eq!(completed.updated_at_ms, 40);

    let queued_failure = store
        .run(|connection| {
            tasks::queue_task(connection, seed("fp-transition-fail", "scope-fail", 50))
        })
        .await
        .expect("failure task should queue");
    let running_failure = store
        .run({
            let task_id = queued_failure.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-b".to_owned(),
                        lease_duration_ms: 100,
                        max_attempts: 3,
                        now_ms: 60,
                    },
                )
            }
        })
        .await
        .expect("failure task claim should query")
        .expect("failure task should claim");

    let failed = store
        .run({
            let task_id = running_failure.task_id.clone();
            let publication_generation = running_failure.publication_generation;
            move |connection| {
                fail_task_at_request_time(
                    connection,
                    CodeIndexTaskFailure {
                        task_id,
                        lease_owner: "worker-b".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        error_kind: "code_index".to_owned(),
                        error_message: "retryable failure".to_owned(),
                        retry_backoff_ms: 25,
                        max_attempts: 3,
                        now_ms: 70,
                    },
                )
            }
        })
        .await
        .expect("failed task should be returned");
    assert_eq!(failed.state, CodeIndexTaskState::Retrying);
    assert!(failed.lease_owner.is_none());
    assert!(failed.lease_expires_at_ms.is_none());
    assert_eq!(failed.next_retry_at_ms, 95);
    assert_eq!(
        failed.last_error_message.as_deref(),
        Some("retryable failure")
    );
}

#[tokio::test]
async fn code_index_task_retry_dead_letter_and_invalid_rows_are_explicit() {
    let store = registered_store().await;
    let queued = store
        .run(|connection| tasks::queue_task(connection, seed("fp-retry", "scope-retry", 10)))
        .await
        .expect("task should queue");

    let first_claim = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-a".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 20,
                    },
                )
            }
        })
        .await
        .expect("claim should query")
        .expect("task should claim");
    assert_eq!(first_claim.attempt_count, 1);

    let blocked_claim = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-b".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 25,
                    },
                )
            }
        })
        .await
        .expect("claim should query");
    assert!(blocked_claim.is_none());

    let reclaimed = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-b".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 31,
                    },
                )
            }
        })
        .await
        .expect("expired lease should query")
        .expect("expired lease should reclaim");
    assert_eq!(reclaimed.attempt_count, 2);
    assert_eq!(reclaimed.lease_owner.as_deref(), Some("worker-b"));
    assert_eq!(reclaimed.last_error_kind.as_deref(), Some("lease_expired"));
    assert_eq!(
        reclaimed.last_error_message.as_deref(),
        Some("code index task lease expired")
    );

    let stale_complete = store
        .run({
            let task_id = queued.task_id.clone();
            let publication_generation = first_claim.publication_generation;
            move |connection| {
                complete_task_at_request_time(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        now_ms: 32,
                    },
                )
            }
        })
        .await
        .expect_err("stale worker should not complete reclaimed task");
    let stale_failure = store
        .run({
            let task_id = queued.task_id.clone();
            let publication_generation = first_claim.publication_generation;
            move |connection| {
                fail_task_at_request_time(
                    connection,
                    CodeIndexTaskFailure {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        error_kind: "late_worker".to_owned(),
                        error_message: "late failure".to_owned(),
                        retry_backoff_ms: 30,
                        max_attempts: 3,
                        now_ms: 32,
                    },
                )
            }
        })
        .await
        .expect_err("stale worker should not fail reclaimed task");
    assert!(stale_complete.to_string().contains("active lease"));
    assert!(stale_failure.to_string().contains("active lease"));

    let retrying = store
        .run({
            let task_id = queued.task_id.clone();
            let publication_generation = reclaimed.publication_generation;
            move |connection| {
                fail_task_at_request_time(
                    connection,
                    CodeIndexTaskFailure {
                        task_id,
                        lease_owner: "worker-b".to_owned(),
                        attempt_count: 2,
                        publication_generation,
                        error_kind: "code_index".to_owned(),
                        error_message: "parse failed".to_owned(),
                        retry_backoff_ms: 30,
                        max_attempts: 3,
                        now_ms: 40,
                    },
                )
            }
        })
        .await
        .expect("failure should persist");
    assert_eq!(retrying.state, CodeIndexTaskState::Retrying);
    assert_eq!(retrying.next_retry_at_ms, 70);
    assert_eq!(retrying.last_error_message.as_deref(), Some("parse failed"));

    let too_early = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-c".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 69,
                    },
                )
            }
        })
        .await
        .expect("retry claim should query");
    assert!(too_early.is_none());

    let final_claim = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-c".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 70,
                    },
                )
            }
        })
        .await
        .expect("retry should claim")
        .expect("retry should be claimable");
    assert_eq!(final_claim.attempt_count, 3);

    let dead = store
        .run({
            let task_id = queued.task_id.clone();
            let publication_generation = final_claim.publication_generation;
            move |connection| {
                fail_task_at_request_time(
                    connection,
                    CodeIndexTaskFailure {
                        task_id,
                        lease_owner: "worker-c".to_owned(),
                        attempt_count: 3,
                        publication_generation,
                        error_kind: "code_index".to_owned(),
                        error_message: "still failing".to_owned(),
                        retry_backoff_ms: 30,
                        max_attempts: 3,
                        now_ms: 79,
                    },
                )
            }
        })
        .await
        .expect("dead letter should persist");
    assert_eq!(dead.state, CodeIndexTaskState::DeadLetter);

    let no_claim = store
        .run(|connection| {
            tasks::claim_task(
                connection,
                CodeIndexTaskClaimRequest {
                    task_id: None,
                    lease_owner: "worker-d".to_owned(),
                    lease_duration_ms: 10,
                    max_attempts: 3,
                    now_ms: 1000,
                },
            )
        })
        .await
        .expect("dead task should not claim");
    assert!(no_claim.is_none());

    let invalid_state_error = store
        .run({
            let task_id = queued.task_id;
            move |connection| {
                connection.execute(
                    "UPDATE code_repository_index_tasks SET state = 'mystery' WHERE task_id = ?1",
                    params![&task_id],
                )?;
                tasks::task_by_id(connection, &task_id)
            }
        })
        .await
        .expect_err("unknown task state should fail decoding");
    assert!(
        invalid_state_error
            .to_string()
            .contains("unknown code index task state")
    );

    let malformed_task = store
        .run(|connection| {
            connection.execute(
                "UPDATE code_repository_index_tasks SET state = 'queued', mode_json = 'not-json'",
                [],
            )?;
            tasks::active_task(connection, "repo")
        })
        .await
        .expect_err("malformed JSON should fail decoding");
    assert!(!malformed_task.to_string().is_empty());
}

#[tokio::test]
async fn code_index_task_lease_validation_recovery_and_renewal_are_explicit() {
    let store = registered_store().await;
    let queued = store
        .run(|connection| tasks::queue_task(connection, seed("fp-renew", "scope-renew", 10)))
        .await
        .expect("task should queue");
    for request in [
        CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id.clone()),
            lease_owner: "  ".to_owned(),
            lease_duration_ms: 10,
            max_attempts: 3,
            now_ms: 20,
        },
        CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id.clone()),
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 0,
            max_attempts: 3,
            now_ms: 20,
        },
        CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id.clone()),
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 10,
            max_attempts: 0,
            now_ms: 20,
        },
    ] {
        store
            .run(move |connection| tasks::claim_task(connection, request))
            .await
            .expect_err("invalid claim should fail");
    }

    let running = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-a".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 2,
                        now_ms: 20,
                    },
                )
            }
        })
        .await
        .expect("claim should load")
        .expect("task should claim");

    let publication_generation = running.publication_generation;
    let renewed = store
        .run({
            let task_id = running.task_id.clone();
            move |connection| {
                tasks::renew_task_lease_at(
                    connection,
                    CodeIndexTaskLeaseRenewal {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        lease_duration_ms: 50,
                        now_ms: 25,
                    },
                    25,
                )
            }
        })
        .await
        .expect("active lease should renew");
    assert_eq!(renewed.lease_expires_at_ms, Some(75));
    let stale_renew = store
        .run({
            let task_id = running.task_id.clone();
            move |connection| {
                tasks::renew_task_lease_at(
                    connection,
                    CodeIndexTaskLeaseRenewal {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        lease_duration_ms: 10,
                        now_ms: 75,
                    },
                    75,
                )
            }
        })
        .await
        .expect_err(
            "expired lease must not renew even while its attempt fence remains authoritative",
        );
    assert!(stale_renew.to_string().contains("active lease"));

    store
        .run(|connection| tasks::recover_expired_task_leases(connection, 76, 2))
        .await
        .expect("expired lease should recover");
    let recovered = store
        .run({
            let task_id = running.task_id.clone();
            move |connection| {
                tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-b".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 2,
                        now_ms: 76,
                    },
                )
            }
        })
        .await
        .expect("recovered task should load")
        .expect("recovered task should claim");
    assert_eq!(recovered.attempt_count, 2);
    assert_eq!(recovered.last_error_kind.as_deref(), Some("lease_expired"));

    store
        .run(|connection| tasks::recover_expired_task_leases(connection, 87, 2))
        .await
        .expect("expired terminal lease should recover");
    let dead = store
        .run({
            let task_id = recovered.task_id.clone();
            move |connection| tasks::task_by_id(connection, &task_id)
        })
        .await
        .expect("dead task should load")
        .expect("dead task should exist");
    assert_eq!(dead.state, CodeIndexTaskState::DeadLetter);
    assert!(dead.lease_owner.is_none());
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
