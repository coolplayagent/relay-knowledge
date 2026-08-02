//! Owns durable code-index task lease acquisition, renewal, and recovery.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    domain::CodeIndexTaskRecord,
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskLeaseRecord, CodeIndexTaskLeaseRecovery,
        CodeIndexTaskLeaseRenewal, StorageError,
    },
};

use super::record_mapping::{task_from_row, task_select_sql, task_update_returning_sql};

pub(in crate::storage::sqlite::code) fn claim_task(
    connection: &mut Connection,
    request: CodeIndexTaskClaimRequest,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        claim_task_once(connection, &request)
    })
}

fn claim_task_once(
    connection: &mut Connection,
    request: &CodeIndexTaskClaimRequest,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    let lease_owner = validate_claim_request(
        &request.lease_owner,
        request.lease_duration_ms,
        request.max_attempts,
    )?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    recover_expired_leases(&transaction, request.now_ms, request.max_attempts)?;
    let task_id = if let Some(task_id) = request.task_id.as_deref() {
        transaction
            .query_row(
                "
                SELECT candidate.task_id
                FROM code_repository_index_tasks candidate
                WHERE candidate.task_id = ?1
                  AND candidate.next_retry_at_ms <= ?2
                  AND candidate.attempt_count < ?3
                  AND candidate.state IN ('queued', 'retrying')
                  AND NOT EXISTS (
                      SELECT 1
                      FROM code_repository_index_tasks live
                      WHERE live.repository_id = candidate.repository_id
                        AND live.state = 'running'
                        AND live.lease_expires_at_ms > ?2
                  )
                ",
                params![task_id, request.now_ms, request.max_attempts],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    } else {
        transaction
            .query_row(
                "
                SELECT candidate.task_id
                FROM code_repository_index_tasks candidate
                WHERE candidate.next_retry_at_ms <= ?1
                  AND candidate.attempt_count < ?2
                  AND candidate.state IN ('queued', 'retrying')
                  AND NOT EXISTS (
                      SELECT 1
                      FROM code_repository_index_tasks live
                      WHERE live.repository_id = candidate.repository_id
                        AND live.state = 'running'
                        AND live.lease_expires_at_ms > ?1
                  )
                ORDER BY candidate.created_at_ms ASC, candidate.task_id ASC
                LIMIT 1
                ",
                params![request.now_ms, request.max_attempts],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    };

    let Some(task_id) = task_id else {
        transaction.commit()?;
        return Ok(None);
    };
    let changed = transaction.execute(
        "
        UPDATE code_repository_index_tasks
        SET state = 'running',
            lease_owner = ?2,
            lease_expires_at_ms = ?3,
            attempt_count = attempt_count + 1,
            updated_at_ms = ?4
        WHERE task_id = ?1
          AND next_retry_at_ms <= ?4
          AND attempt_count < ?5
          AND (
            state IN ('queued', 'retrying')
            OR (state = 'running' AND lease_expires_at_ms <= ?4)
          )
          AND NOT EXISTS (
              SELECT 1
              FROM code_repository_index_tasks live
              WHERE live.repository_id = code_repository_index_tasks.repository_id
                AND live.task_id <> code_repository_index_tasks.task_id
                AND live.state = 'running'
                AND live.lease_expires_at_ms > ?4
          )
        ",
        params![
            &task_id,
            lease_owner,
            request.now_ms.saturating_add(request.lease_duration_ms),
            request.now_ms,
            request.max_attempts,
        ],
    )?;
    if changed == 0 {
        transaction.commit()?;
        return Ok(None);
    }
    let sql = task_select_sql("WHERE task_id = ?1");
    let task = transaction.query_row(&sql, params![&task_id], task_from_row)?;
    transaction.commit()?;

    Ok(Some(task))
}

pub(in crate::storage::sqlite::code) fn renew_task_lease(
    connection: &mut Connection,
    request: CodeIndexTaskLeaseRenewal,
) -> Result<CodeIndexTaskRecord, StorageError> {
    let lease_owner = validate_lease_owner(&request.lease_owner)?;
    if request.lease_duration_ms == 0 {
        return Err(StorageError::InvalidInput(
            "code index task lease duration must be greater than zero".to_owned(),
        ));
    }
    let sql = task_update_returning_sql(
        "
        UPDATE code_repository_index_tasks
        SET lease_expires_at_ms = ?4,
            updated_at_ms = ?5
        WHERE task_id = ?1
          AND state = 'running'
          AND lease_owner = ?2
          AND attempt_count = ?3
          AND lease_expires_at_ms > ?5
        ",
    );
    let renewed = super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        connection
            .query_row(
                &sql,
                params![
                    &request.task_id,
                    lease_owner,
                    request.attempt_count,
                    request.now_ms.saturating_add(request.lease_duration_ms),
                    request.now_ms,
                ],
                task_from_row,
            )
            .optional()
            .map_err(StorageError::from)
    })?;

    renewed.ok_or_else(|| inactive_lease_error(&request.task_id))
}

pub(in crate::storage::sqlite::code) fn recover_expired_task_leases(
    connection: &mut Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<(), StorageError> {
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        recover_expired_task_leases_once(connection, now_ms, max_attempts)
    })
}

fn recover_expired_task_leases_once(
    connection: &mut Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<(), StorageError> {
    if max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "code index task max attempts must be greater than zero".to_owned(),
        ));
    }
    recover_expired_leases(connection, now_ms, max_attempts)
}

pub(in crate::storage::sqlite::code) fn running_task_leases(
    connection: &mut Connection,
) -> Result<Vec<CodeIndexTaskLeaseRecord>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT task_id, lease_owner, lease_expires_at_ms, attempt_count
        FROM code_repository_index_tasks
        WHERE state = 'running'
          AND lease_owner IS NOT NULL
        ORDER BY created_at_ms ASC, task_id ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(CodeIndexTaskLeaseRecord {
            task_id: row.get(0)?,
            lease_owner: row.get(1)?,
            lease_expires_at_ms: row.get(2)?,
            attempt_count: row.get(3)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code) fn recover_task_leases_by_task(
    connection: &mut Connection,
    request: CodeIndexTaskLeaseRecovery,
) -> Result<usize, StorageError> {
    if request.max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "code index task max attempts must be greater than zero".to_owned(),
        ));
    }
    if request.task_ids.is_empty() {
        return Ok(0);
    }

    let transaction = connection.transaction()?;
    let mut recovered = 0usize;
    for task_id in unique_task_ids(&request.task_ids) {
        recovered = recovered.saturating_add(transaction.execute(
            "
            UPDATE code_repository_index_tasks
            SET state = CASE
                    WHEN attempt_count >= ?2 THEN 'dead_letter'
                    ELSE 'retrying'
                END,
                lease_owner = NULL,
                lease_expires_at_ms = NULL,
                next_retry_at_ms = ?3,
                last_error_kind = ?4,
                last_error_message = ?5,
                updated_at_ms = ?3
            WHERE task_id = ?1
              AND state = 'running'
            ",
            params![
                task_id,
                request.max_attempts,
                request.now_ms,
                &request.error_kind,
                &request.error_message,
            ],
        )?);
    }
    transaction.commit()?;

    Ok(recovered)
}

fn validate_claim_request(
    lease_owner: &str,
    lease_duration_ms: u64,
    max_attempts: u32,
) -> Result<&str, StorageError> {
    let lease_owner = validate_lease_owner(lease_owner)?;
    if lease_duration_ms == 0 {
        return Err(StorageError::InvalidInput(
            "code index task lease duration must be greater than zero".to_owned(),
        ));
    }
    if max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "code index task max attempts must be greater than zero".to_owned(),
        ));
    }

    Ok(lease_owner)
}

pub(super) fn validate_lease_owner(lease_owner: &str) -> Result<&str, StorageError> {
    let lease_owner = lease_owner.trim();
    if lease_owner.is_empty() {
        return Err(StorageError::InvalidInput(
            "code index task lease owner must not be empty".to_owned(),
        ));
    }

    Ok(lease_owner)
}

fn unique_task_ids(task_ids: &[String]) -> BTreeSet<&str> {
    task_ids
        .iter()
        .map(String::as_str)
        .filter(|task_id| !task_id.trim().is_empty())
        .collect()
}

fn recover_expired_leases(
    connection: &Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE code_repository_index_tasks
        SET state = CASE
                WHEN attempt_count >= ?2 THEN 'dead_letter'
                ELSE 'retrying'
            END,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            next_retry_at_ms = ?1,
            last_error_kind = 'lease_expired',
            last_error_message = 'code index task lease expired',
            updated_at_ms = ?1
        WHERE state = 'running'
          AND lease_expires_at_ms IS NOT NULL
          AND lease_expires_at_ms <= ?1
        ",
        params![now_ms, max_attempts],
    )?;

    Ok(())
}

pub(super) fn inactive_lease_error(task_id: &str) -> StorageError {
    StorageError::InvalidInput(format!(
        "code index task '{task_id}' is not held by an active lease"
    ))
}

#[cfg(test)]
#[path = "lease_tests.rs"]
mod tests;
