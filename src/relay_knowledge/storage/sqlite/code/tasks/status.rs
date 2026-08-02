//! Owns durable code-index task lookup and queue status projections.

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{CodeIndexTaskQueueStatus, CodeIndexTaskRecord},
    storage::StorageError,
};

use super::record_mapping::{task_from_row, task_select_sql};

pub(in crate::storage::sqlite::code) fn task_by_id(
    connection: &mut Connection,
    task_id: &str,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    let sql = task_select_sql("WHERE task_id = ?1");
    connection
        .query_row(&sql, params![task_id], task_from_row)
        .optional()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code) fn active_task(
    connection: &mut Connection,
    repository_id: &str,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    let sql = task_select_sql(
        "WHERE repository_id = ?1 AND state IN ('queued', 'running', 'retrying')
         ORDER BY created_at_ms ASC, task_id ASC LIMIT 1",
    );
    connection
        .query_row(&sql, params![repository_id], task_from_row)
        .optional()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code) fn queue_status(
    connection: &mut Connection,
) -> Result<CodeIndexTaskQueueStatus, StorageError> {
    connection
        .query_row(
            "
            SELECT
                COALESCE(SUM(CASE WHEN state = 'queued' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN state = 'running' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN state = 'retrying' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN state = 'dead_letter' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(
                    CASE
                        WHEN state = 'running' AND lease_owner IS NOT NULL THEN 1
                        ELSE 0
                    END
                ), 0),
                (
                    SELECT last_error_message
                    FROM code_repository_index_tasks
                    WHERE last_error_message IS NOT NULL
                    ORDER BY updated_at_ms DESC, task_id DESC
                    LIMIT 1
                )
            FROM code_repository_index_tasks
            ",
            [],
            |row| {
                Ok(CodeIndexTaskQueueStatus {
                    queued_task_count: row.get::<_, usize>(0)?,
                    running_task_count: row.get::<_, usize>(1)?,
                    retrying_task_count: row.get::<_, usize>(2)?,
                    dead_letter_task_count: row.get::<_, usize>(3)?,
                    running_lease_count: row.get::<_, usize>(4)?,
                    last_error: row.get::<_, Option<String>>(5)?,
                })
            },
        )
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
