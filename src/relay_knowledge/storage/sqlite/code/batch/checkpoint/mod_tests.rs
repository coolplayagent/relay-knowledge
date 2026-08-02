//! Direct tests for checkpoint state transitions and decoding.

use rusqlite::{Connection, params};

use super::{load, mark_completed, mark_state};
use crate::domain::CodeIndexResourceBudget;

#[test]
fn checkpoint_state_transitions_update_the_persisted_record() {
    let mut connection = checkpoint_database();

    mark_state(&mut connection, "scope", "finalizing:references").expect("state should advance");
    assert_eq!(
        load(&mut connection, "scope")
            .expect("checkpoint should load")
            .state,
        "finalizing:references"
    );

    let transaction = connection.transaction().expect("transaction should open");
    mark_completed(&transaction, "scope").expect("checkpoint should complete");
    transaction.commit().expect("transaction should commit");
    assert_eq!(
        load(&mut connection, "scope")
            .expect("checkpoint should load")
            .state,
        "completed"
    );
}

fn checkpoint_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_index_checkpoints (
                repository_id TEXT NOT NULL,
                source_scope TEXT PRIMARY KEY,
                state TEXT NOT NULL,
                total_path_count INTEGER NOT NULL,
                parsed_file_count INTEGER NOT NULL,
                committed_file_count INTEGER NOT NULL,
                committed_symbol_count INTEGER NOT NULL,
                committed_reference_count INTEGER NOT NULL,
                committed_chunk_count INTEGER NOT NULL,
                batch_count INTEGER NOT NULL,
                last_path TEXT,
                resource_budget_json TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                error_message TEXT
            );
            ",
        )
        .expect("checkpoint schema should be created");
    connection
        .execute(
            "
            INSERT INTO code_repository_index_checkpoints (
                repository_id, source_scope, state, total_path_count, parsed_file_count,
                committed_file_count, committed_symbol_count, committed_reference_count,
                committed_chunk_count, batch_count, last_path, resource_budget_json,
                updated_at_ms, error_message
            )
            VALUES ('repo', 'scope', 'indexing', 2, 1, 1, 3, 4, 5, 1, 'src/lib.rs', ?1, 1, 'old')
            ",
            params![
                serde_json::to_string(&CodeIndexResourceBudget::default())
                    .expect("budget should serialize")
            ],
        )
        .expect("checkpoint should be inserted");
    connection
}
