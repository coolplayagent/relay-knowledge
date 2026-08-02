//! Owns durable code-index task reset transactions.

use rusqlite::{Connection, TransactionBehavior, params};

use crate::{domain::CodeIndexTaskRecord, storage::StorageError};

use super::record_mapping::{task_from_row, task_update_returning_sql};

pub(in crate::storage::sqlite::code) fn reset_tasks(
    connection: &mut Connection,
    repository_id: &str,
    now_ms: u64,
) -> Result<Vec<CodeIndexTaskRecord>, StorageError> {
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        reset_tasks_once(connection, repository_id, now_ms)
    })
}

fn reset_tasks_once(
    connection: &mut Connection,
    repository_id: &str,
    now_ms: u64,
) -> Result<Vec<CodeIndexTaskRecord>, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let sql = task_update_returning_sql(
        "
        UPDATE code_repository_index_tasks
        SET state = 'queued',
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            attempt_count = 0,
            next_retry_at_ms = ?2,
            last_error_kind = NULL,
            last_error_message = NULL,
            updated_at_ms = ?2
        WHERE repository_id = ?1
          AND NOT EXISTS (
              SELECT 1
              FROM code_repository_index_tasks live
              WHERE live.repository_id = ?1
                AND live.state = 'running'
                AND live.lease_expires_at_ms > ?2
          )
          AND (
              state IN ('queued', 'retrying')
              OR (
                  state = 'running'
                  AND (
                      lease_expires_at_ms IS NULL
                      OR lease_expires_at_ms <= ?2
                  )
              )
          )
        ",
    );
    let tasks = {
        let mut statement = transaction.prepare(&sql)?;
        let rows = statement.query_map(params![repository_id, now_ms], task_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    transaction.commit()?;

    Ok(tasks)
}

#[cfg(test)]
#[path = "reset_tests.rs"]
mod tests;
