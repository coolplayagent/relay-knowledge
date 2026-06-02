use rusqlite::{Connection, params};

use crate::storage::StorageError;

pub(super) fn mark_checkpoint_state(
    connection: &mut Connection,
    source_scope: &str,
    state: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE code_repository_index_checkpoints
        SET state = ?2, updated_at_ms = ?3
        WHERE source_scope = ?1
        ",
        params![source_scope, state, now_millis()],
    )?;

    Ok(())
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
