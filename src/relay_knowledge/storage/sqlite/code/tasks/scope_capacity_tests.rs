//! Admission distinguishes fenced clone residue from resumable direct writers.

use super::*;

const CLONE: &str = "staging:incremental_clone:v1:2:13:66:369829:cdc15ec72c3a3095";

fn connection() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE code_repository_index_checkpoints (
            repository_id TEXT, source_scope TEXT, state TEXT, updated_at_ms INTEGER);
         CREATE TABLE code_repository_index_tasks (
            repository_id TEXT, source_scope TEXT, state TEXT);",
        )
        .unwrap();
    connection
}

fn checkpoint(connection: &Connection, scope: &str, state: &str) {
    connection
        .execute(
            "INSERT INTO code_repository_index_checkpoints VALUES ('repo', ?1, ?2, 1)",
            params![scope, state],
        )
        .unwrap();
}

#[test]
fn code_index_task_admission_keeps_clones_distinct_after_terminal_audit_pruning() {
    let mut connection = connection();
    checkpoint(&connection, "clone", CLONE);
    for owner in [Some("dead_letter"), Some("cancelled"), None] {
        connection
            .execute("DELETE FROM code_repository_index_tasks", [])
            .unwrap();
        if let Some(state) = owner {
            connection
                .execute(
                    "INSERT INTO code_repository_index_tasks VALUES ('repo', 'clone', ?1)",
                    [state],
                )
                .unwrap();
        }
        let transaction = connection.transaction().unwrap();
        reject_unowned_checkpoint_conflict(&transaction, "repo", "next").unwrap();
    }
}

#[test]
fn code_index_task_admission_checks_direct_writers_after_clone_residue() {
    for state in [
        "indexing",
        "finalizing:build_query_indexes",
        "staging:incremental_clone:broken",
    ] {
        let mut connection = connection();
        checkpoint(&connection, "a-clone", CLONE);
        checkpoint(&connection, "b-direct", state);
        // A historical terminal row is not proof that a current ordinary
        // checkpoint still belongs to that durable writer.
        connection.execute(
            "INSERT INTO code_repository_index_tasks VALUES ('repo', 'b-direct', 'dead_letter')", [],
        ).unwrap();
        let transaction = connection.transaction().unwrap();
        let error = reject_unowned_checkpoint_conflict(&transaction, "repo", "next").unwrap_err();
        assert!(
            matches!(error, StorageError::InvalidInput(message) if message.contains("b-direct"))
        );
        reject_unowned_checkpoint_conflict(&transaction, "repo", "b-direct").unwrap();
    }
}

#[test]
fn code_index_task_admission_preserves_unfinished_owner_and_repository_isolation() {
    let mut connection = connection();
    checkpoint(&connection, "direct", "indexing");
    connection
        .execute(
            "INSERT INTO code_repository_index_tasks VALUES ('other', 'direct', 'running')",
            [],
        )
        .unwrap();
    {
        let transaction = connection.transaction().unwrap();
        assert!(reject_unowned_checkpoint_conflict(&transaction, "repo", "next").is_err());
    }
    for state in ["queued", "running", "retrying"] {
        connection
            .execute("DELETE FROM code_repository_index_tasks", [])
            .unwrap();
        connection
            .execute(
                "INSERT INTO code_repository_index_tasks VALUES ('repo', 'direct', ?1)",
                [state],
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        reject_unowned_checkpoint_conflict(&transaction, "repo", "next").unwrap();
    }
}

#[test]
fn code_index_task_admission_bounds_the_orphan_checkpoint_scan() {
    let mut connection = connection();
    for i in 0..MAX_SCOPE_SLOTS_PER_REPOSITORY {
        checkpoint(&connection, &format!("clone-{i}"), CLONE);
    }
    {
        let transaction = connection.transaction().unwrap();
        reject_unowned_checkpoint_conflict(&transaction, "repo", "next").unwrap();
    }
    checkpoint(&connection, "overflow", CLONE);
    let transaction = connection.transaction().unwrap();
    assert!(matches!(
        reject_unowned_checkpoint_conflict(&transaction, "repo", "next"),
        Err(StorageError::CapacityExceeded(_))
    ));
}
