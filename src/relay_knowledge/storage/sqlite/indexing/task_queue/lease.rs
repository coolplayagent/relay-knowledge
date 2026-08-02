use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{GraphVersion, IndexKind},
    storage::{IndexRefreshClaimRequest, IndexRefreshTask, StorageError},
};

use super::record::read_task;

pub(crate) fn claim_index_refresh_task(
    connection: &mut Connection,
    request: IndexRefreshClaimRequest,
) -> Result<Option<IndexRefreshTask>, StorageError> {
    let lease_owner = request.lease_owner.trim();
    if lease_owner.is_empty() {
        return Err(StorageError::InvalidInput(
            "index refresh lease owner must not be empty".to_owned(),
        ));
    }
    if request.lease_duration_ms == 0 {
        return Err(StorageError::InvalidInput(
            "index refresh lease duration must be greater than zero".to_owned(),
        ));
    }
    if request.max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "index refresh max attempts must be greater than zero".to_owned(),
        ));
    }

    recover_expired_leases(connection, request.now_ms, request.max_attempts)?;
    loop {
        let candidate = connection
            .query_row(
                "
                SELECT task_id, target_graph_version
                FROM index_refresh_tasks
                WHERE state = 'queued'
                   OR (state = 'retrying' AND next_retry_at_ms <= ?1)
                ORDER BY created_at_ms ASC, target_graph_version ASC, task_id ASC
                LIMIT 1
                ",
                params![request.now_ms],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
            )
            .optional()?;

        let Some((task_id, expected_target)) = candidate else {
            return Ok(None);
        };
        let updated = connection.execute(
            "
            UPDATE index_refresh_tasks
            SET state = 'running',
                lease_owner = ?2,
                lease_expires_at_ms = ?3,
                attempt_count = attempt_count + 1,
                updated_at_ms = ?4
            WHERE task_id = ?1
              AND (
                  state = 'queued'
                  OR (state = 'retrying' AND next_retry_at_ms <= ?4)
              )
              AND target_graph_version = ?5
            ",
            params![
                task_id,
                lease_owner,
                request.now_ms.saturating_add(request.lease_duration_ms),
                request.now_ms,
                expected_target
            ],
        )?;

        if updated == 1 {
            return read_task(connection, &task_id)?.map(Some).ok_or_else(|| {
                StorageError::InvalidInput("claimed index refresh task is missing".to_owned())
            });
        }
    }
}

fn recover_expired_leases(
    connection: &Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<(), StorageError> {
    let dead_letter_kinds = expired_dead_letter_kinds(connection, now_ms, max_attempts)?;
    connection.execute(
        "
        UPDATE index_cursors
        SET state = 'failed',
            last_error = 'index refresh task lease expired'
        WHERE EXISTS (
            SELECT 1
            FROM index_refresh_tasks task
            WHERE task.kind = index_cursors.kind
              AND task.source_scope = index_cursors.source_scope
              AND task.modality = index_cursors.modality
              AND task.state = 'running'
              AND task.lease_expires_at_ms IS NOT NULL
              AND task.lease_expires_at_ms <= ?1
              AND task.attempt_count >= ?2
        )
        ",
        params![now_ms, max_attempts],
    )?;
    connection.execute(
        "
        UPDATE index_refresh_tasks
        SET state = CASE
                WHEN attempt_count >= ?2 THEN 'dead_letter'
                ELSE 'retrying'
            END,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            next_retry_at_ms = ?1,
            last_error_kind = 'lease_expired',
            last_error_message = 'index refresh task lease expired',
            updated_at_ms = ?1
        WHERE state = 'running'
          AND lease_expires_at_ms IS NOT NULL
          AND lease_expires_at_ms <= ?1
        ",
        params![now_ms, max_attempts],
    )?;
    for kind in dead_letter_kinds {
        super::super::recompute_aggregate_status(connection, kind, GraphVersion::ZERO)?;
    }

    Ok(())
}

fn expired_dead_letter_kinds(
    connection: &Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<Vec<IndexKind>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT DISTINCT kind
        FROM index_refresh_tasks
        WHERE state = 'running'
          AND lease_expires_at_ms IS NOT NULL
          AND lease_expires_at_ms <= ?1
          AND attempt_count >= ?2
        ",
    )?;
    let rows = statement.query_map(params![now_ms, max_attempts], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|kind| super::super::parse_index_kind(&kind))
        .collect()
}

#[cfg(test)]
#[path = "lease_tests.rs"]
mod lease_tests;
