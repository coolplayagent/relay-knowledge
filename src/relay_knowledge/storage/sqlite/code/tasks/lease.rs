//! Owns durable code-index task lease acquisition, renewal, and recovery.

use std::time::{SystemTime, UNIX_EPOCH};

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
    claim_task_with_clock(connection, request, system_now_millis)
}

fn claim_task_with_clock(
    connection: &mut Connection,
    request: CodeIndexTaskClaimRequest,
    mut clock: impl FnMut() -> Result<u64, StorageError>,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    let lease_owner = validate_claim_request(
        &request.lease_owner,
        request.lease_duration_ms,
        request.max_attempts,
    )?
    .to_owned();
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        claim_task_once(connection, &request, &lease_owner, &mut clock)
    })
}

fn claim_task_once(
    connection: &mut Connection,
    request: &CodeIndexTaskClaimRequest,
    lease_owner: &str,
    clock: &mut impl FnMut() -> Result<u64, StorageError>,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let execution_now_ms = clock()?;
    validate_observed_execution_time(request.now_ms, execution_now_ms)?;
    recover_expired_leases(&transaction, execution_now_ms, request.max_attempts)?;
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
                      FROM code_repository_index_tasks predecessor
                      WHERE predecessor.repository_id = candidate.repository_id
                        AND predecessor.state IN ('queued', 'running', 'retrying')
                        AND (
                            predecessor.created_at_ms < candidate.created_at_ms
                            OR (
                                predecessor.created_at_ms = candidate.created_at_ms
                                AND predecessor.task_id < candidate.task_id
                            )
                        )
                  )
                  AND NOT EXISTS (
                      SELECT 1
                      FROM code_repository_index_tasks live
                      WHERE live.repository_id = candidate.repository_id
                        AND live.state = 'running'
                        AND live.lease_expires_at_ms > ?2
                  )
                ",
                params![task_id, execution_now_ms, request.max_attempts],
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
                      FROM code_repository_index_tasks predecessor
                      WHERE predecessor.repository_id = candidate.repository_id
                        AND predecessor.state IN ('queued', 'running', 'retrying')
                        AND (
                            predecessor.created_at_ms < candidate.created_at_ms
                            OR (
                                predecessor.created_at_ms = candidate.created_at_ms
                                AND predecessor.task_id < candidate.task_id
                            )
                        )
                  )
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
                params![execution_now_ms, request.max_attempts],
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
            execution_now_ms.saturating_add(request.lease_duration_ms),
            execution_now_ms,
            request.max_attempts,
        ],
    )?;
    if changed == 0 {
        transaction.commit()?;
        return Ok(None);
    }
    let (repository_id, attempt_count) = transaction.query_row(
        "SELECT repository_id, attempt_count FROM code_repository_index_tasks WHERE task_id = ?1",
        params![&task_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
    )?;
    let generation = transaction.query_row(
        "
        INSERT INTO code_repository_publication_fences (
            repository_id, generation, task_id, attempt_count, lease_owner, updated_at_ms
        )
        VALUES (?1, 1, ?2, ?3, ?4, ?5)
        ON CONFLICT(repository_id) DO UPDATE SET
            generation = code_repository_publication_fences.generation + 1,
            task_id = excluded.task_id,
            attempt_count = excluded.attempt_count,
            lease_owner = excluded.lease_owner,
            updated_at_ms = excluded.updated_at_ms
        RETURNING generation
        ",
        params![
            repository_id,
            &task_id,
            attempt_count,
            lease_owner,
            execution_now_ms
        ],
        |row| row.get::<_, u64>(0),
    )?;
    transaction.execute(
        "UPDATE code_repository_index_tasks SET publication_generation = ?2 WHERE task_id = ?1",
        params![&task_id, generation],
    )?;
    let sql = task_select_sql("WHERE task_id = ?1");
    let task = transaction.query_row(&sql, params![&task_id], task_from_row)?;
    transaction.commit()?;

    Ok(Some(task))
}

#[cfg(test)]
pub(super) fn claim_task_at(
    connection: &mut Connection,
    request: CodeIndexTaskClaimRequest,
    execution_now_ms: u64,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    claim_task_with_clock(connection, request, || Ok(execution_now_ms))
}

pub(in crate::storage::sqlite::code) fn renew_task_lease(
    connection: &mut Connection,
    request: CodeIndexTaskLeaseRenewal,
) -> Result<CodeIndexTaskRecord, StorageError> {
    renew_task_lease_with_clock(connection, request, system_now_millis)
}

fn renew_task_lease_with_clock(
    connection: &mut Connection,
    request: CodeIndexTaskLeaseRenewal,
    mut clock: impl FnMut() -> Result<u64, StorageError>,
) -> Result<CodeIndexTaskRecord, StorageError> {
    let lease_owner = validate_lease_owner(&request.lease_owner)?.to_owned();
    if request.lease_duration_ms == 0 {
        return Err(StorageError::InvalidInput(
            "code index task lease duration must be greater than zero".to_owned(),
        ));
    }
    let sql = task_update_returning_sql(
        "
        UPDATE code_repository_index_tasks AS task
        SET lease_expires_at_ms = MAX(task.lease_expires_at_ms, ?5),
            updated_at_ms = MAX(task.updated_at_ms, ?6)
        WHERE task.task_id = ?1
          AND task.state = 'running'
          AND task.lease_owner = ?2
          AND task.attempt_count = ?3
          AND task.publication_generation = ?4
          AND task.lease_expires_at_ms IS NOT NULL
          AND task.lease_expires_at_ms > ?6
          AND EXISTS (
              SELECT 1
              FROM code_repository_publication_fences authority
              WHERE authority.repository_id = task.repository_id
                AND authority.task_id = task.task_id
                AND authority.lease_owner = ?2
                AND authority.attempt_count = ?3
                AND authority.generation = ?4
          )
        ",
    );
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let execution_now_ms = clock()?;
        validate_observed_execution_time(request.now_ms, execution_now_ms)?;
        let renewed = transaction
            .query_row(
                &sql,
                params![
                    &request.task_id,
                    &lease_owner,
                    request.attempt_count,
                    request.publication_generation,
                    execution_now_ms.saturating_add(request.lease_duration_ms),
                    execution_now_ms,
                ],
                task_from_row,
            )
            .optional()
            .map_err(StorageError::from)?;
        transaction.commit()?;

        renewed.ok_or_else(|| inactive_lease_error(&request.task_id))
    })
}

#[cfg(test)]
pub(super) fn renew_task_lease_at(
    connection: &mut Connection,
    request: CodeIndexTaskLeaseRenewal,
    execution_now_ms: u64,
) -> Result<CodeIndexTaskRecord, StorageError> {
    renew_task_lease_with_clock(connection, request, || Ok(execution_now_ms))
}

pub(super) fn validate_observed_execution_time(
    observed_now_ms: u64,
    execution_now_ms: u64,
) -> Result<(), StorageError> {
    if observed_now_ms <= execution_now_ms {
        return Ok(());
    }
    Err(StorageError::InvalidInput(format!(
        "code index task observation {observed_now_ms} is later than authoritative execution time {execution_now_ms}"
    )))
}

pub(super) fn system_now_millis() -> Result<u64, StorageError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            StorageError::Invariant(format!("system clock is before Unix epoch: {error}"))
        })?;
    u64::try_from(elapsed.as_millis()).map_err(|_| {
        StorageError::Invariant("system clock milliseconds exceed u64 range".to_owned())
    })
}

pub(in crate::storage::sqlite::code) fn recover_expired_task_leases(
    connection: &mut Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<(), StorageError> {
    recover_expired_task_leases_with_clock(connection, now_ms, max_attempts, system_now_millis)
}

fn recover_expired_task_leases_with_clock(
    connection: &mut Connection,
    observed_now_ms: u64,
    max_attempts: u32,
    mut clock: impl FnMut() -> Result<u64, StorageError>,
) -> Result<(), StorageError> {
    if max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "code index task max attempts must be greater than zero".to_owned(),
        ));
    }
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let execution_now_ms = clock()?;
        validate_observed_execution_time(observed_now_ms, execution_now_ms)?;
        recover_expired_leases(&transaction, execution_now_ms, max_attempts)?;
        transaction.commit()?;
        Ok(())
    })
}

#[cfg(test)]
pub(super) fn recover_expired_task_leases_at(
    connection: &mut Connection,
    observed_now_ms: u64,
    max_attempts: u32,
    execution_now_ms: u64,
) -> Result<(), StorageError> {
    recover_expired_task_leases_with_clock(connection, observed_now_ms, max_attempts, || {
        Ok(execution_now_ms)
    })
}

pub(in crate::storage::sqlite::code) fn running_task_leases(
    connection: &mut Connection,
) -> Result<Vec<CodeIndexTaskLeaseRecord>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT task_id, lease_owner, lease_expires_at_ms, attempt_count,
               publication_generation
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
            publication_generation: row.get(4)?,
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
    if request.leases.is_empty() {
        return Ok(0);
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut recovered = 0usize;
    for lease in &request.leases {
        let observed_expiry = lease.lease_expires_at_ms.ok_or_else(|| {
            StorageError::InvalidInput(
                "observed code index task lease expiry must be present".to_owned(),
            )
        })?;
        recovered = recovered.saturating_add(transaction.execute(
            "
            UPDATE code_repository_index_tasks AS task
            SET state = CASE
                    WHEN task.attempt_count >= ?6 THEN 'dead_letter'
                    ELSE 'retrying'
                END,
                lease_owner = NULL,
                lease_expires_at_ms = NULL,
                next_retry_at_ms = ?7,
                last_error_kind = ?8,
                last_error_message = ?9,
                updated_at_ms = ?7
            WHERE task.task_id = ?1
              AND task.state = 'running'
              AND task.lease_owner = ?2
              AND task.attempt_count = ?3
              AND task.publication_generation = ?4
              AND task.lease_expires_at_ms = ?5
            ",
            params![
                &lease.task_id,
                &lease.lease_owner,
                lease.attempt_count,
                lease.publication_generation,
                observed_expiry,
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

#[cfg(test)]
#[path = "lease_clock_tests.rs"]
mod clock_tests;
