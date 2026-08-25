use rusqlite::Connection;

use super::{
    PendingWorktreeTarget, PublicationFenceGuard, WorktreeScopeIdentity, prepare_guard,
    system_time_millis,
};
use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget,
        CodeMonorepoWorkspaceFormat, CodeWorkspaceDetectionConfig,
        code_snapshot_scope_id_with_workspace_detection,
    },
    storage::StorageError,
};

#[test]
fn rejects_incomplete_publication_authority_before_sqlite_work() {
    let connection = Connection::open_in_memory().expect("database should open");
    let error = prepare_guard(
        &connection,
        CodeIndexPublicationFence {
            repository_id: "repo".to_owned(),
            task_id: "task".to_owned(),
            lease_owner: String::new(),
            attempt_count: 1,
            generation: 1,
        },
        None,
    )
    .expect_err("empty lease owner must be rejected");

    assert!(matches!(error, StorageError::InvalidInput(message) if message.contains("incomplete")));
}

#[test]
fn live_attempt_validates_and_takeover_fences_the_old_guard() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    initialize_authority_fixture(&connection, i64::MAX as u64);
    let guard = guard(&connection, "worker-old", 1, 1);
    let transaction = connection.transaction().expect("transaction should begin");
    guard
        .validate_target_scope(&transaction, "scope")
        .expect("target scope should match");
    guard
        .validate(&transaction)
        .expect("live attempt should validate");
    transaction.commit().expect("live validation should commit");

    connection
        .execute_batch(
            "UPDATE code_repository_index_tasks
             SET lease_owner = 'worker-new', attempt_count = 2, publication_generation = 2;
             UPDATE code_repository_publication_fences
             SET lease_owner = 'worker-new', attempt_count = 2, generation = 2;",
        )
        .expect("takeover should persist");
    let transaction = connection.transaction().expect("transaction should begin");
    let error = guard
        .validate(&transaction)
        .expect_err("old publication authority must be fenced");
    assert!(
        matches!(error, StorageError::InvalidInput(message) if message.contains("no longer active"))
    );
}

#[test]
fn takeover_fences_resource_budget_preflight_as_an_inactive_attempt() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_authority_fixture(&connection, i64::MAX as u64);
    let guard = guard(&connection, "worker-old", 1, 1);
    assert_eq!(
        guard
            .resource_budget(&connection)
            .expect("live budget should load"),
        CodeIndexResourceBudget::default()
    );

    connection
        .execute_batch(
            "UPDATE code_repository_index_tasks
             SET lease_owner = 'worker-new', attempt_count = 2, publication_generation = 2;
             UPDATE code_repository_publication_fences
             SET lease_owner = 'worker-new', attempt_count = 2, generation = 2;",
        )
        .expect("takeover should persist");

    let error = guard
        .resource_budget(&connection)
        .expect_err("old attempt budget preflight must be fenced");
    assert!(
        matches!(error, StorageError::InvalidInput(message) if message.contains("no longer active"))
    );
}

#[test]
fn code_index_task_publication_fence_rejects_execution_at_expiry_and_rolls_back() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    initialize_authority_fixture(&connection, 11);
    connection
        .execute_batch(
            "CREATE TABLE publication_fact (value TEXT NOT NULL);
             INSERT INTO publication_fact VALUES ('before');",
        )
        .expect("publication fact fixture should initialize");
    let guard = guard(&connection, "worker-old", 1, 1);
    let transaction = connection.transaction().expect("transaction should begin");
    transaction
        .execute("UPDATE publication_fact SET value = 'after'", [])
        .expect("tentative publication should write");

    let error = guard
        .validate_with_clock(&transaction, || Ok(11))
        .expect_err("the lease must be inactive at its exclusive expiry boundary");
    assert!(
        matches!(error, StorageError::InvalidInput(message) if message.contains("no longer active"))
    );
    transaction
        .rollback()
        .expect("failed publication should roll back");

    let value = connection
        .query_row("SELECT value FROM publication_fact", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("publication fact should load");
    assert_eq!(value, "before");
}

#[test]
fn code_index_task_publication_fence_samples_clock_after_sqlite_writer_lock() {
    let database_path = std::env::temp_dir().join(format!(
        "relay-knowledge-fence-lock-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock should follow Unix epoch")
            .as_nanos()
    ));
    let initializer = Connection::open(&database_path).expect("database should open");
    initialize_authority_fixture(&initializer, i64::MAX as u64);
    drop(initializer);

    let blocker = Connection::open(&database_path).expect("blocker should open");
    blocker
        .busy_timeout(std::time::Duration::from_secs(2))
        .expect("blocker timeout should configure");
    blocker
        .execute_batch("BEGIN IMMEDIATE")
        .expect("blocker should own the writer lock");
    let (clock_sender, clock_receiver) = std::sync::mpsc::channel();
    let validation_path = database_path.clone();
    let validation_thread = std::thread::spawn(move || {
        let mut connection = Connection::open(validation_path).expect("validator should open");
        connection
            .busy_timeout(std::time::Duration::from_secs(2))
            .expect("validator timeout should configure");
        let guard = guard(&connection, "worker-old", 1, 1);
        let transaction = connection.transaction().expect("transaction should begin");
        guard.validate_with_clock(&transaction, || {
            clock_sender.send(()).expect("clock signal should send");
            Ok(1)
        })?;
        transaction.commit().map_err(StorageError::from)
    });
    assert!(
        clock_receiver
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err(),
        "clock must not run while the authority writer lock is unavailable"
    );
    blocker
        .execute_batch("COMMIT")
        .expect("writer lock should release");
    clock_receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("clock should run after the authority writer lock is acquired");
    validation_thread
        .join()
        .expect("validation thread should join")
        .expect("publication fence should validate");
    drop(blocker);
    let _ = std::fs::remove_file(&database_path);
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-journal"));
}

#[test]
fn code_index_task_publication_fence_propagates_authoritative_clock_failure() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    initialize_authority_fixture(&connection, i64::MAX as u64);
    connection
        .execute_batch(
            "CREATE TABLE publication_fact (value TEXT NOT NULL);
             INSERT INTO publication_fact VALUES ('before');",
        )
        .expect("publication fact fixture should initialize");
    let guard = guard(&connection, "worker-old", 1, 1);
    let transaction = connection.transaction().expect("transaction should begin");
    transaction
        .execute("UPDATE publication_fact SET value = 'after'", [])
        .expect("tentative publication should write");

    let error = guard
        .validate_with_clock(&transaction, || {
            Err(StorageError::Invariant(
                "authoritative publication clock is unavailable".to_owned(),
            ))
        })
        .expect_err("an unavailable authoritative clock must fail closed");
    assert!(
        matches!(error, StorageError::Invariant(message) if message.contains("clock is unavailable"))
    );
    transaction
        .rollback()
        .expect("failed publication should roll back");

    let value = connection
        .query_row("SELECT value FROM publication_fact", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("publication fact should load");
    assert_eq!(value, "before");
}

#[test]
fn publication_clock_rejects_time_before_unix_epoch() {
    let before_epoch = std::time::SystemTime::UNIX_EPOCH
        .checked_sub(std::time::Duration::from_millis(1))
        .expect("test clock should represent pre-epoch time");

    let error = system_time_millis(before_epoch)
        .expect_err("pre-epoch publication time must not fall back to epoch zero");

    assert!(
        matches!(error, StorageError::Invariant(message) if message.contains("before Unix epoch"))
    );
}

#[test]
fn pending_worktree_rebind_requires_the_exact_workspace_semantic() {
    let enabled_pnpm = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![CodeMonorepoWorkspaceFormat::Pnpm],
    };
    let enabled_all = CodeWorkspaceDetectionConfig::enabled_all();
    let enabled_empty = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: Vec::new(),
    };
    let pending_pnpm = pending_target(&enabled_pnpm);
    let pending_empty = pending_target(&enabled_empty);

    assert!(
        pending_pnpm
            .matches_real_scope("repo", &real_target(Some(1)))
            .expect("matching semantic should validate")
    );
    assert!(
        !pending_pnpm
            .matches_real_scope("repo", &real_target(Some(7)))
            .expect("different enabled masks should compare")
    );
    assert!(
        !pending_empty
            .matches_real_scope("repo", &real_target(None))
            .expect("enabled mask zero and disabled must remain distinct")
    );
    assert_ne!(
        pending_pnpm.source_scope,
        pending_target(&enabled_all).source_scope
    );
}

fn pending_target(config: &CodeWorkspaceDetectionConfig) -> PendingWorktreeTarget {
    let tree_hash = "worktree:pending:base";
    PendingWorktreeTarget {
        source_scope: code_snapshot_scope_id_with_workspace_detection(
            "repo",
            tree_hash,
            &[],
            &[],
            config,
        ),
        resolved_commit_sha: tree_hash.to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters_json: "[]".to_owned(),
        language_filters_json: "[]".to_owned(),
        mode_json: serde_json::to_string(&CodeIndexMode::WorktreeOverlay)
            .expect("mode should serialize"),
    }
}

fn real_target(workspace_semantic: Option<u8>) -> WorktreeScopeIdentity {
    WorktreeScopeIdentity {
        repository_id: "repo".to_owned(),
        base_commit: Some("base".to_owned()),
        resolved_commit_sha: "worktree:base:0123456789abcdef".to_owned(),
        tree_hash: "worktree:0123456789abcdef".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        workspace_semantic,
    }
}

fn guard(
    connection: &Connection,
    lease_owner: &str,
    attempt_count: u32,
    generation: u64,
) -> PublicationFenceGuard {
    prepare_guard(
        connection,
        CodeIndexPublicationFence {
            repository_id: "repo".to_owned(),
            task_id: "task".to_owned(),
            lease_owner: lease_owner.to_owned(),
            attempt_count,
            generation,
        },
        None,
    )
    .expect("complete publication guard should prepare")
}

fn initialize_authority_fixture(connection: &Connection, lease_expires_at_ms: u64) {
    connection
        .execute_batch(
            "CREATE TABLE code_repository_index_tasks (
                 task_id TEXT PRIMARY KEY, repository_id TEXT NOT NULL,
                 source_scope TEXT NOT NULL, publication_generation INTEGER NOT NULL,
                 state TEXT NOT NULL, lease_owner TEXT, attempt_count INTEGER NOT NULL,
                 lease_expires_at_ms INTEGER, resource_budget_json TEXT NOT NULL
             );
             CREATE TABLE code_repository_publication_fences (
                 repository_id TEXT PRIMARY KEY, generation INTEGER NOT NULL,
                 task_id TEXT NOT NULL, attempt_count INTEGER NOT NULL,
                 lease_owner TEXT NOT NULL, updated_at_ms INTEGER NOT NULL
             );",
        )
        .expect("publication authority schema should initialize");
    connection
        .execute(
            "INSERT INTO code_repository_index_tasks VALUES
                 ('task', 'repo', 'scope', 1, 'running', 'worker-old', 1, ?1, ?2)",
            rusqlite::params![
                lease_expires_at_ms,
                serde_json::to_string(&CodeIndexResourceBudget::default())
                    .expect("budget should serialize")
            ],
        )
        .expect("task authority should initialize");
    connection
        .execute(
            "INSERT INTO code_repository_publication_fences VALUES
                 ('repo', 1, 'task', 1, 'worker-old', 0)",
            [],
        )
        .expect("publication fence should initialize");
}
