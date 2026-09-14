//! Bounded terminal task audit compaction and pending detection.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::storage::StorageError;

use super::{RETAIN_FAILED_TASK_AUDIT_ROWS, RETAIN_SUCCEEDED_TASK_AUDIT_ROWS, retention_gc};

pub(in crate::storage::sqlite::code::tasks) fn prune_finished_task_history(
    transaction: &Transaction<'_>,
    repository_id: &str,
    protected_task_id: Option<&str>,
) -> Result<bool, StorageError> {
    let deleted_succeeded = transaction.execute(
        "
        DELETE FROM code_repository_index_tasks
        WHERE task_id <> ?4 AND task_id IN (
            SELECT candidate.task_id
            FROM (
                SELECT task_id, source_scope, publication_generation,
                       updated_at_ms, created_at_ms
                FROM code_repository_index_tasks
                     INDEXED BY code_repository_index_tasks_publication_retention
                WHERE repository_id = ?1 AND state = 'succeeded'
                ORDER BY publication_generation DESC, updated_at_ms DESC,
                         created_at_ms DESC, task_id DESC
                LIMIT ?3 OFFSET ?2
            ) candidate
            WHERE NOT EXISTS (
                      SELECT 1
                      FROM code_repository_scopes scope
                      WHERE scope.repository_id = ?1
                        AND scope.source_scope = candidate.source_scope
                  )
               OR EXISTS (
                      SELECT 1
                      FROM code_repository_index_tasks newer
                      WHERE newer.repository_id = ?1
                        AND newer.state = 'succeeded'
                        AND newer.source_scope = candidate.source_scope
                        AND (
                            newer.publication_generation > candidate.publication_generation
                            OR (
                                newer.publication_generation = candidate.publication_generation
                                AND newer.updated_at_ms > candidate.updated_at_ms
                            )
                            OR (
                                newer.publication_generation = candidate.publication_generation
                                AND newer.updated_at_ms = candidate.updated_at_ms
                                AND newer.created_at_ms > candidate.created_at_ms
                            )
                            OR (
                                newer.publication_generation = candidate.publication_generation
                                AND newer.updated_at_ms = candidate.updated_at_ms
                                AND newer.created_at_ms = candidate.created_at_ms
                                AND newer.task_id > candidate.task_id
                            )
                        )
                  )
            ORDER BY candidate.task_id
            LIMIT ?3
        )
        ",
        params![
            repository_id,
            RETAIN_SUCCEEDED_TASK_AUDIT_ROWS,
            retention_gc::GC_ROW_BATCH_SIZE,
            protected_task_id.unwrap_or_default(),
        ],
    )?;
    let deleted_failed = transaction.execute(
        "
        DELETE FROM code_repository_index_tasks
        WHERE task_id <> ?4 AND task_id IN (
            SELECT task_id
            FROM code_repository_index_tasks
            WHERE repository_id = ?1
              AND state IN ('failed', 'dead_letter', 'cancelled')
            ORDER BY updated_at_ms DESC, created_at_ms DESC, task_id DESC
            LIMIT ?3
            OFFSET ?2
        )
        ",
        params![
            repository_id,
            RETAIN_FAILED_TASK_AUDIT_ROWS,
            retention_gc::GC_ROW_BATCH_SIZE,
            protected_task_id.unwrap_or_default(),
        ],
    )?;
    Ok(deleted_succeeded > 0 || deleted_failed > 0)
}

pub(super) fn finished_task_history_pending(
    connection: &Connection,
    repository_id: &str,
) -> Result<bool, StorageError> {
    let succeeded = connection
        .query_row(
            "SELECT 1
             FROM (
                 SELECT task_id, source_scope, publication_generation,
                        updated_at_ms, created_at_ms
                 FROM code_repository_index_tasks
                      INDEXED BY code_repository_index_tasks_publication_retention
                 WHERE repository_id = ?1 AND state = 'succeeded'
                 ORDER BY publication_generation DESC, updated_at_ms DESC,
                          created_at_ms DESC, task_id DESC
                 LIMIT ?3 OFFSET ?2
             ) candidate
             WHERE NOT EXISTS (
                       SELECT 1
                       FROM code_repository_scopes scope
                       WHERE scope.repository_id = ?1
                         AND scope.source_scope = candidate.source_scope
                   )
                OR EXISTS (
                       SELECT 1
                       FROM code_repository_index_tasks newer
                       WHERE newer.repository_id = ?1
                         AND newer.state = 'succeeded'
                         AND newer.source_scope = candidate.source_scope
                         AND (
                             newer.publication_generation > candidate.publication_generation
                             OR (
                                 newer.publication_generation = candidate.publication_generation
                                 AND newer.updated_at_ms > candidate.updated_at_ms
                             )
                             OR (
                                 newer.publication_generation = candidate.publication_generation
                                 AND newer.updated_at_ms = candidate.updated_at_ms
                                 AND newer.created_at_ms > candidate.created_at_ms
                             )
                             OR (
                                 newer.publication_generation = candidate.publication_generation
                                 AND newer.updated_at_ms = candidate.updated_at_ms
                                 AND newer.created_at_ms = candidate.created_at_ms
                                 AND newer.task_id > candidate.task_id
                             )
                         )
                   )
             LIMIT 1",
            params![
                repository_id,
                RETAIN_SUCCEEDED_TASK_AUDIT_ROWS,
                retention_gc::GC_ROW_BATCH_SIZE
            ],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if succeeded {
        return Ok(true);
    }
    connection
        .query_row(
            "SELECT 1
             FROM code_repository_index_tasks
             WHERE repository_id = ?1
               AND state IN ('failed', 'dead_letter', 'cancelled')
             ORDER BY updated_at_ms DESC, created_at_ms DESC, task_id DESC
             LIMIT 1 OFFSET ?2",
            params![repository_id, RETAIN_FAILED_TASK_AUDIT_ROWS],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(StorageError::from)
}
