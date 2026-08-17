//! Durable selection and lifecycle for whole-repository index retention.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{domain::CodeRepositoryRetentionJobStatus, storage::StorageError};

use super::super::workspace;

const PHASE: &str = "retiring_scopes";
const CANDIDATE_PAGE_SIZE: usize = 64;

struct CandidateScan {
    max_indexed_repositories: usize,
    cursor_activity_ms: u64,
    cursor_repository_id: String,
    eligible_count: usize,
    oldest_eligible: Option<(String, String)>,
    created_at_ms: u64,
}

pub(in crate::storage::sqlite::code) fn job(
    connection: &Connection,
    repository_id: &str,
) -> Result<Option<CodeRepositoryRetentionJobStatus>, StorageError> {
    connection
        .query_row(
            "SELECT repository_id, initial_scope, cutoff_ms,
                    cutoff_publication_generation, phase,
                    created_at_ms, updated_at_ms, last_error
             FROM code_repository_retention_jobs WHERE repository_id = ?1",
            params![repository_id],
            |row| {
                Ok(CodeRepositoryRetentionJobStatus {
                    repository_id: row.get(0)?,
                    initial_scope: row.get(1)?,
                    cutoff_ms: row.get(2)?,
                    cutoff_publication_generation: row.get(3)?,
                    phase: row.get(4)?,
                    created_at_ms: row.get(5)?,
                    updated_at_ms: row.get(6)?,
                    last_error: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code) fn schedule(
    connection: &mut Connection,
    max_indexed_repositories: usize,
    now_ms: u64,
) -> Result<Option<String>, StorageError> {
    if max_indexed_repositories == 0 {
        return Err(StorageError::InvalidInput(
            "max indexed repositories must be greater than zero".to_owned(),
        ));
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(repository_id) = transaction
        .query_row(
            "SELECT repository_id FROM code_repository_retention_jobs
             ORDER BY updated_at_ms, repository_id LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        clear_candidate_scan(&transaction)?;
        transaction.commit()?;
        return Ok(Some(repository_id));
    }

    let selected = advance_candidate_scan(&transaction, max_indexed_repositories, now_ms)?;
    if let Some((repository_id, initial_scope)) = &selected {
        let cutoff_publication_generation = transaction.query_row(
            "SELECT COALESCE(MAX(publication_generation), 0)
             FROM code_repository_index_tasks
             WHERE repository_id = ?1 AND source_scope = ?2 AND state = 'succeeded'",
            params![repository_id, initial_scope],
            |row| row.get::<_, u64>(0),
        )?;
        transaction.execute(
            "INSERT INTO code_repository_retention_jobs (
                 repository_id, initial_scope, cutoff_ms,
                 cutoff_publication_generation, phase,
                 created_at_ms, updated_at_ms, last_error
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?3, ?3, NULL)",
            params![
                repository_id,
                initial_scope,
                now_ms,
                cutoff_publication_generation,
                PHASE
            ],
        )?;
    }
    transaction.commit()?;
    Ok(selected.map(|(repository_id, _)| repository_id))
}

fn advance_candidate_scan(
    connection: &Connection,
    max_indexed_repositories: usize,
    now_ms: u64,
) -> Result<Option<(String, String)>, StorageError> {
    let mut scan = load_candidate_scan(connection)?
        .filter(|scan| scan.max_indexed_repositories == max_indexed_repositories);
    if scan.is_none() {
        clear_candidate_scan(connection)?;
    }
    let cursor_activity_ms = scan.as_ref().map(|scan| scan.cursor_activity_ms);
    let cursor_repository_id = scan
        .as_ref()
        .map_or_else(String::new, |scan| scan.cursor_repository_id.clone());
    let page = {
        let mut statement = connection.prepare(
            "WITH indexed_repository AS (
                 SELECT repository.repository_id,
                        repository.last_indexed_scope_id AS source_scope,
                        MAX(
                            COALESCE((
                                SELECT MAX(task.updated_at_ms)
                                FROM code_repository_index_tasks task
                                WHERE task.repository_id = repository.repository_id
                                  AND task.source_scope = repository.last_indexed_scope_id
                                  AND task.state = 'succeeded'
                            ), 0),
                            COALESCE((
                                SELECT MAX(checkpoint.updated_at_ms)
                                FROM code_repository_index_checkpoints checkpoint
                                WHERE checkpoint.repository_id = repository.repository_id
                                  AND checkpoint.source_scope = repository.last_indexed_scope_id
                                  AND checkpoint.state IN ('complete', 'completed')
                            ), 0)
                        ) AS activity_ms
                 FROM code_repositories repository
                 WHERE repository.last_indexed_scope_id IS NOT NULL
                   AND EXISTS (
                       SELECT 1 FROM code_repository_scopes scope
                       WHERE scope.repository_id = repository.repository_id
                         AND scope.source_scope = repository.last_indexed_scope_id
                         AND scope.retiring = 0
                   )
             )
             SELECT repository_id, source_scope, activity_ms
             FROM indexed_repository
             WHERE ?1 IS NULL OR activity_ms > ?1
                OR (activity_ms = ?1 AND repository_id > ?2)
             ORDER BY activity_ms, repository_id
             LIMIT ?3",
        )?;
        statement
            .query_map(
                params![
                    cursor_activity_ms,
                    cursor_repository_id,
                    CANDIDATE_PAGE_SIZE
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u64>(2)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?
    };
    if page.is_empty() {
        clear_candidate_scan(connection)?;
        return Ok(None);
    }
    let created_at_ms = scan.as_ref().map_or(now_ms, |scan| scan.created_at_ms);
    let mut eligible_count = scan.as_ref().map_or(0, |scan| scan.eligible_count);
    let mut oldest_eligible = scan.take().and_then(|scan| scan.oldest_eligible);
    let mut next_cursor_activity_ms = cursor_activity_ms.unwrap_or_default();
    let mut next_cursor_repository_id = cursor_repository_id;
    for (repository_id, source_scope, activity_ms) in &page {
        next_cursor_activity_ms = *activity_ms;
        next_cursor_repository_id.clone_from(repository_id);
        if belongs_to_user_set(connection, repository_id)? {
            continue;
        }
        eligible_count = eligible_count.saturating_add(1);
        if oldest_eligible.is_none() {
            oldest_eligible = Some((repository_id.clone(), source_scope.clone()));
        }
        if eligible_count > max_indexed_repositories {
            let selected = match oldest_eligible {
                Some((repository_id, source_scope))
                    if candidate_is_eligible(connection, &repository_id, &source_scope)? =>
                {
                    Some((repository_id, source_scope))
                }
                _ => None,
            };
            clear_candidate_scan(connection)?;
            return Ok(selected);
        }
    }
    if page.len() < CANDIDATE_PAGE_SIZE {
        clear_candidate_scan(connection)?;
        return Ok(None);
    }
    persist_candidate_scan(
        connection,
        &CandidateScan {
            max_indexed_repositories,
            cursor_activity_ms: next_cursor_activity_ms,
            cursor_repository_id: next_cursor_repository_id,
            eligible_count,
            oldest_eligible,
            created_at_ms,
        },
        now_ms,
    )?;
    Ok(None)
}

fn load_candidate_scan(connection: &Connection) -> Result<Option<CandidateScan>, StorageError> {
    connection
        .query_row(
            "SELECT max_indexed_repositories, cursor_activity_ms,
                    cursor_repository_id, eligible_count,
                    oldest_repository_id, oldest_source_scope, created_at_ms
             FROM code_repository_retention_scans WHERE scan_id = 1",
            [],
            |row| {
                let oldest_repository_id = row.get::<_, Option<String>>(4)?;
                let oldest_source_scope = row.get::<_, Option<String>>(5)?;
                Ok(CandidateScan {
                    max_indexed_repositories: row.get(0)?,
                    cursor_activity_ms: row.get(1)?,
                    cursor_repository_id: row.get(2)?,
                    eligible_count: row.get(3)?,
                    oldest_eligible: oldest_repository_id.zip(oldest_source_scope),
                    created_at_ms: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::from)
}

fn persist_candidate_scan(
    connection: &Connection,
    scan: &CandidateScan,
    now_ms: u64,
) -> Result<(), StorageError> {
    let (oldest_repository_id, oldest_source_scope) = scan
        .oldest_eligible
        .as_ref()
        .map_or((None, None), |(repository_id, source_scope)| {
            (Some(repository_id.as_str()), Some(source_scope.as_str()))
        });
    connection.execute(
        "INSERT INTO code_repository_retention_scans (
             scan_id, max_indexed_repositories, cursor_activity_ms,
             cursor_repository_id, eligible_count, oldest_repository_id,
             oldest_source_scope, created_at_ms, updated_at_ms
         ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(scan_id) DO UPDATE SET
             max_indexed_repositories = excluded.max_indexed_repositories,
             cursor_activity_ms = excluded.cursor_activity_ms,
             cursor_repository_id = excluded.cursor_repository_id,
             eligible_count = excluded.eligible_count,
             oldest_repository_id = excluded.oldest_repository_id,
             oldest_source_scope = excluded.oldest_source_scope,
             created_at_ms = excluded.created_at_ms,
             updated_at_ms = excluded.updated_at_ms",
        params![
            scan.max_indexed_repositories,
            scan.cursor_activity_ms,
            scan.cursor_repository_id,
            scan.eligible_count,
            oldest_repository_id,
            oldest_source_scope,
            scan.created_at_ms,
            now_ms,
        ],
    )?;
    Ok(())
}

fn clear_candidate_scan(connection: &Connection) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM code_repository_retention_scans WHERE scan_id = 1",
        [],
    )?;
    Ok(())
}

fn belongs_to_user_set(connection: &Connection, repository_id: &str) -> Result<bool, StorageError> {
    let automatic_set_id = workspace::workspace_set_id(repository_id);
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_set_members
                 WHERE repository_id = ?1 AND set_id <> ?2
             )",
            params![repository_id, automatic_set_id],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

fn candidate_is_eligible(
    connection: &Connection,
    repository_id: &str,
    source_scope: &str,
) -> Result<bool, StorageError> {
    if belongs_to_user_set(connection, repository_id)? {
        return Ok(false);
    }
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1
                 FROM code_repositories repository
                 JOIN code_repository_scopes scope
                   ON scope.repository_id = repository.repository_id
                  AND scope.source_scope = repository.last_indexed_scope_id
                 WHERE repository.repository_id = ?1
                   AND repository.last_indexed_scope_id = ?2
                   AND scope.retiring = 0
             )",
            params![repository_id, source_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code) fn update_progress(
    connection: &Connection,
    repository_id: &str,
    cutoff_ms: u64,
    phase: &str,
    last_error: Option<&str>,
    now_ms: u64,
) -> Result<bool, StorageError> {
    connection
        .execute(
            "UPDATE code_repository_retention_jobs
             SET phase = ?3, updated_at_ms = ?4, last_error = ?5
             WHERE repository_id = ?1 AND cutoff_ms = ?2",
            params![repository_id, cutoff_ms, phase, now_ms, last_error],
        )
        .map(|updated| updated > 0)
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code) fn complete(
    connection: &Connection,
    repository_id: &str,
    cutoff_ms: u64,
) -> Result<bool, StorageError> {
    connection
        .execute(
            "DELETE FROM code_repository_retention_jobs
             WHERE repository_id = ?1 AND cutoff_ms = ?2",
            params![repository_id, cutoff_ms],
        )
        .map(|deleted| deleted > 0)
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "repository_retention_tests.rs"]
mod tests;
