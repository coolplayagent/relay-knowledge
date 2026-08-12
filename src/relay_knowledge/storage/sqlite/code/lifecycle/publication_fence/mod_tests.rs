use rusqlite::Connection;

use super::{PublicationFenceGuard, prepare_guard};
use crate::{domain::CodeIndexPublicationFence, storage::StorageError};

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
    connection
        .execute_batch(
            "CREATE TABLE code_repository_index_tasks (
                 task_id TEXT PRIMARY KEY, repository_id TEXT NOT NULL,
                 source_scope TEXT NOT NULL, publication_generation INTEGER NOT NULL,
                 state TEXT NOT NULL, lease_owner TEXT, attempt_count INTEGER NOT NULL,
                 lease_expires_at_ms INTEGER
             );
             CREATE TABLE code_repository_publication_fences (
                 repository_id TEXT PRIMARY KEY, generation INTEGER NOT NULL,
                 task_id TEXT NOT NULL, attempt_count INTEGER NOT NULL,
                 lease_owner TEXT NOT NULL, updated_at_ms INTEGER NOT NULL
             );
             INSERT INTO code_repository_index_tasks VALUES
                 ('task', 'repo', 'scope', 1, 'running', 'worker-old', 1, 9223372036854775807);
             INSERT INTO code_repository_publication_fences VALUES
                 ('repo', 1, 'task', 1, 'worker-old', 0);",
        )
        .expect("publication authority fixture should initialize");
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
