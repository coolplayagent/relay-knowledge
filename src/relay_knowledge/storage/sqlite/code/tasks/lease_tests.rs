use super::super::test_support::persist_published_task_target;
use super::super::{
    complete_task_at, fail_task_at, queue_task, reset_tasks_at, task_by_id as load_task_by_id,
};

fn complete_task(
    connection: &mut rusqlite::Connection,
    request: CodeIndexTaskCompletion,
) -> Result<crate::domain::CodeIndexTaskRecord, crate::storage::StorageError> {
    let execution_now_ms = request.now_ms;
    complete_task_at(connection, request, execution_now_ms)
}

fn fail_task(
    connection: &mut rusqlite::Connection,
    request: CodeIndexTaskFailure,
) -> Result<crate::domain::CodeIndexTaskRecord, crate::storage::StorageError> {
    let execution_now_ms = request.now_ms;
    fail_task_at(connection, request, execution_now_ms)
}

fn reset_tasks(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    now_ms: u64,
) -> Result<Vec<crate::domain::CodeIndexTaskRecord>, crate::storage::StorageError> {
    reset_tasks_at(connection, repository_id, now_ms, now_ms)
}
use super::{claim_task_at, recover_task_leases_by_task, running_task_leases};
use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexTaskState,
        CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeIndexTaskFailure,
        CodeIndexTaskLeaseRecovery, CodeIndexTaskLeaseRenewal, CodeIndexTaskSeed,
        CodeRepositoryStore, SqliteGraphStore,
    },
};

fn claim_task(
    connection: &mut rusqlite::Connection,
    request: CodeIndexTaskClaimRequest,
) -> Result<Option<crate::domain::CodeIndexTaskRecord>, crate::storage::StorageError> {
    let execution_now_ms = request.now_ms;
    claim_task_at(connection, request, execution_now_ms)
}

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
        .run({
            let running_first = running_first.clone();
            move |connection| persist_published_task_target(connection, &running_first)
        })
        .await
        .expect("first target should be durably published before completion");
    store
        .complete_code_index_task(CodeIndexTaskCompletion {
            task_id: running_first.task_id.clone(),
            lease_owner: "worker-first".to_owned(),
            attempt_count: running_first.attempt_count,
            publication_generation: running_first.publication_generation,
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
async fn code_index_task_authoritative_execution_time_rejects_stale_observed_renewal() {
    let store = registered_store().await;
    let queued = queue(
        &store,
        seed_for_repo("repo", "fixture", "fp-late-resume", "scope-late-resume", 0),
    )
    .await;
    let running = store
        .run({
            let task_id = queued.task_id;
            move |connection| {
                claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-a".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 1,
                    },
                )
            }
        })
        .await
        .expect("claim should run")
        .expect("task should claim");

    let wrong_generation = store
        .run({
            let task_id = running.task_id.clone();
            let publication_generation = running.publication_generation.saturating_add(1);
            move |connection| {
                super::renew_task_lease_at(
                    connection,
                    CodeIndexTaskLeaseRenewal {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count: running.attempt_count,
                        publication_generation,
                        lease_duration_ms: 50,
                        now_ms: 5,
                    },
                    11,
                )
            }
        })
        .await
        .expect_err("a non-authoritative publication generation must not renew the lease");
    assert!(wrong_generation.to_string().contains("active lease"));

    let expired = store
        .run({
            let task_id = running.task_id.clone();
            let attempt_count = running.attempt_count;
            let publication_generation = running.publication_generation;
            move |connection| {
                super::renew_task_lease_at(
                    connection,
                    CodeIndexTaskLeaseRenewal {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count,
                        publication_generation,
                        lease_duration_ms: 50,
                        now_ms: 5,
                    },
                    11,
                )
            }
        })
        .await
        .expect_err("expiry must revoke renewal even while the attempt fence is unchanged");
    assert!(expired.to_string().contains("active lease"));
    let unchanged = task_by_id(&store, running.task_id).await;
    assert_eq!(unchanged.state, CodeIndexTaskState::Running);
    assert_eq!(unchanged.lease_owner.as_deref(), Some("worker-a"));
    assert_eq!(unchanged.lease_expires_at_ms, Some(11));
}

#[tokio::test]
async fn code_index_task_renewal_samples_clock_after_sqlite_writer_lock() {
    let database_path = std::env::temp_dir().join(format!(
        "relay-knowledge-renew-lock-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock should follow Unix epoch")
            .as_nanos()
    ));
    let store = SqliteGraphStore::open(&database_path).expect("file store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    let now_ms = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock should follow Unix epoch")
            .as_millis(),
    )
    .expect("epoch milliseconds should fit u64");
    let queued = queue(
        &store,
        seed_for_repo(
            "repo",
            "fixture",
            "fp-lock-clock",
            "scope-lock-clock",
            now_ms,
        ),
    )
    .await;
    let running = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-a".to_owned(),
                        lease_duration_ms: 60_000,
                        max_attempts: 3,
                        now_ms,
                    },
                )
            }
        })
        .await
        .expect("claim should run")
        .expect("task should claim");
    let blocker = rusqlite::Connection::open(&database_path).expect("blocker should open");
    blocker
        .busy_timeout(std::time::Duration::from_secs(2))
        .expect("blocker timeout should configure");
    blocker
        .execute_batch("BEGIN IMMEDIATE")
        .expect("blocker should own the writer lock");
    let (clock_sender, clock_receiver) = std::sync::mpsc::channel();
    let renewal_path = database_path.clone();
    let renewal = CodeIndexTaskLeaseRenewal {
        task_id: running.task_id,
        lease_owner: "worker-a".to_owned(),
        attempt_count: running.attempt_count,
        publication_generation: running.publication_generation,
        lease_duration_ms: 60_000,
        now_ms,
    };
    let renewal_thread = std::thread::spawn(move || {
        let mut connection =
            rusqlite::Connection::open(renewal_path).expect("renewal connection should open");
        connection
            .busy_timeout(std::time::Duration::from_secs(2))
            .expect("renewal timeout should configure");
        super::renew_task_lease_with_clock(&mut connection, renewal, || {
            clock_sender.send(()).expect("clock signal should send");
            Ok(now_ms.saturating_add(1))
        })
    });
    assert!(
        clock_receiver
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err(),
        "clock must not run while BEGIN IMMEDIATE is waiting for the writer"
    );
    blocker
        .execute_batch("COMMIT")
        .expect("writer lock should release");
    clock_receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("clock should run after the writer lock is acquired");
    renewal_thread
        .join()
        .expect("renewal thread should join")
        .expect("renewal should succeed");
    drop(blocker);
    drop(store);
    let _ = std::fs::remove_file(&database_path);
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
}

#[tokio::test]
async fn code_index_task_takeover_rejects_renewal_from_the_old_attempt() {
    let store = registered_store().await;
    let queued = queue(
        &store,
        seed_for_repo(
            "repo",
            "fixture",
            "fp-takeover-resume",
            "scope-takeover-resume",
            0,
        ),
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

    let error = store
        .run({
            let task_id = first.task_id;
            move |connection| {
                super::renew_task_lease_at(
                    connection,
                    CodeIndexTaskLeaseRenewal {
                        task_id,
                        lease_owner: "worker-old".to_owned(),
                        attempt_count: first.attempt_count,
                        publication_generation: first.publication_generation,
                        lease_duration_ms: 100,
                        now_ms: 12,
                    },
                    12,
                )
            }
        })
        .await
        .expect_err("the old attempt must not renew after takeover");

    assert!(error.to_string().contains("active lease"));
    assert_eq!(second.lease_owner.as_deref(), Some("worker-new"));
    assert!(second.attempt_count > first.attempt_count);
    assert!(second.publication_generation > first.publication_generation);
}

#[tokio::test]
async fn active_renewal_prevents_delayed_orphan_recovery_and_takeover() {
    let store = registered_store().await;
    let queued = queue(
        &store,
        seed_for_repo(
            "repo",
            "fixture",
            "fp-resume-takeover",
            "scope-resume-takeover",
            0,
        ),
    )
    .await;
    let running = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-a".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 1,
                    },
                )
            }
        })
        .await
        .expect("claim should run")
        .expect("task should claim");
    let observed_before_renewal = store
        .run_read(running_task_leases)
        .await
        .expect("orphan scan should observe the original expiry")
        .into_iter()
        .find(|lease| lease.task_id == running.task_id)
        .expect("running attempt should be observed");
    let renewed = store
        .run({
            let task_id = running.task_id.clone();
            let attempt_count = running.attempt_count;
            let publication_generation = running.publication_generation;
            move |connection| {
                super::renew_task_lease_at(
                    connection,
                    CodeIndexTaskLeaseRenewal {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count,
                        publication_generation,
                        lease_duration_ms: 100,
                        now_ms: 5,
                    },
                    5,
                )
            }
        })
        .await
        .expect("the active attempt should renew before recovery");
    let recovered = store
        .run(move |connection| {
            recover_task_leases_by_task(
                connection,
                CodeIndexTaskLeaseRecovery {
                    leases: vec![observed_before_renewal],
                    now_ms: 12,
                    max_attempts: 3,
                    error_kind: "lease_orphaned".to_owned(),
                    error_message: "delayed orphan scan".to_owned(),
                },
            )
        })
        .await
        .expect("delayed recovery should compare the observed lease expiry");
    let takeover = store
        .run({
            let task_id = queued.task_id;
            move |connection| {
                claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-b".to_owned(),
                        lease_duration_ms: 100,
                        max_attempts: 3,
                        now_ms: 12,
                    },
                )
            }
        })
        .await
        .expect("takeover attempt should inspect the durable task");

    assert_eq!(recovered, 0);
    assert!(takeover.is_none());
    assert_eq!(renewed.lease_expires_at_ms, Some(105));
    let current = task_by_id(&store, renewed.task_id.clone()).await;
    assert_eq!(current.lease_owner.as_deref(), Some("worker-a"));
    assert_eq!(current.attempt_count, running.attempt_count);
    assert_eq!(
        current.publication_generation,
        running.publication_generation
    );
}

#[tokio::test]
async fn delayed_orphan_recovery_does_not_recover_a_new_attempt() {
    let store = registered_store().await;
    let queued = queue(
        &store,
        seed_for_repo(
            "repo",
            "fixture",
            "fp-delayed-recovery",
            "scope-delayed-recovery",
            0,
        ),
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
    let observed = store
        .run_read(running_task_leases)
        .await
        .expect("orphan scan should read the first attempt")
        .into_iter()
        .find(|lease| lease.task_id == first.task_id)
        .expect("first attempt lease should be observed");
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
        .expect("expired first attempt should be reclaimed");

    let recovered = store
        .run(move |connection| {
            recover_task_leases_by_task(
                connection,
                CodeIndexTaskLeaseRecovery {
                    leases: vec![observed],
                    now_ms: 12,
                    max_attempts: 3,
                    error_kind: "lease_orphaned".to_owned(),
                    error_message: "delayed orphan scan".to_owned(),
                },
            )
        })
        .await
        .expect("delayed recovery should compare the observed attempt token");

    assert_eq!(recovered, 0);
    let current = task_by_id(&store, second.task_id.clone()).await;
    assert_eq!(current.state, CodeIndexTaskState::Running);
    assert_eq!(current.lease_owner.as_deref(), Some("worker-new"));
    assert_eq!(current.attempt_count, second.attempt_count);
    assert_eq!(
        current.publication_generation,
        second.publication_generation
    );
}

#[tokio::test]
async fn reset_and_reclaim_reject_stale_generation_completion_and_failure() {
    let store = registered_store().await;
    let queued = queue(
        &store,
        seed_for_repo("repo", "fixture", "fp-reset-aba", "scope-reset-aba", 0),
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
                        lease_owner: "same-worker".to_owned(),
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
    store
        .run(|connection| reset_tasks(connection, "repo", 11))
        .await
        .expect("expired attempt should reset");
    let current = store
        .run({
            let task_id = queued.task_id;
            move |connection| {
                claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "same-worker".to_owned(),
                        lease_duration_ms: 100,
                        max_attempts: 3,
                        now_ms: 12,
                    },
                )
            }
        })
        .await
        .expect("reclaim should run")
        .expect("reset task should reclaim");
    assert_eq!(current.attempt_count, first.attempt_count);
    assert_eq!(current.lease_owner, first.lease_owner);
    assert!(current.publication_generation > first.publication_generation);
    store
        .run({
            let current = current.clone();
            move |connection| persist_published_task_target(connection, &current)
        })
        .await
        .expect("current target should publish");

    let stale_failure = store
        .run({
            let first = first.clone();
            move |connection| {
                fail_task(
                    connection,
                    CodeIndexTaskFailure {
                        task_id: first.task_id,
                        lease_owner: "same-worker".to_owned(),
                        attempt_count: first.attempt_count,
                        publication_generation: first.publication_generation,
                        error_kind: "stale".to_owned(),
                        error_message: "stale generation".to_owned(),
                        retry_backoff_ms: 10,
                        max_attempts: 3,
                        now_ms: 13,
                    },
                )
            }
        })
        .await
        .expect_err("stale generation must not fail the current attempt");
    assert!(stale_failure.to_string().contains("active lease"));
    let stale_completion = store
        .run({
            let first = first.clone();
            move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id: first.task_id,
                        lease_owner: "same-worker".to_owned(),
                        attempt_count: first.attempt_count,
                        publication_generation: first.publication_generation,
                        now_ms: 13,
                    },
                )
            }
        })
        .await
        .expect_err("stale generation must not complete the current attempt");
    assert!(stale_completion.to_string().contains("active lease"));
    let unchanged = task_by_id(&store, current.task_id.clone()).await;
    assert_eq!(unchanged.state, CodeIndexTaskState::Running);
    assert_eq!(
        unchanged.publication_generation,
        current.publication_generation
    );
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
    let observed_a = leases
        .iter()
        .find(|lease| lease.task_id == queued_a.task_id)
        .expect("first task lease should be observed")
        .clone();
    let recovered = store
        .run({
            move |connection| {
                recover_task_leases_by_task(
                    connection,
                    CodeIndexTaskLeaseRecovery {
                        leases: vec![observed_a],
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
