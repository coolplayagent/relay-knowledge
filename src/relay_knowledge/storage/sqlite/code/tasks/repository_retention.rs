//! Durable selection and lifecycle for whole-repository index retention.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{domain::CodeRepositoryRetentionJobStatus, storage::StorageError};

use super::super::workspace;

const PHASE: &str = "retiring_scopes";
const CANDIDATE_PAGE_SIZE: usize = 64;

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
        transaction.commit()?;
        return Ok(Some(repository_id));
    }

    let selected = oldest_over_limit_candidate(&transaction, max_indexed_repositories)?;
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

fn oldest_over_limit_candidate(
    connection: &Connection,
    max_indexed_repositories: usize,
) -> Result<Option<(String, String)>, StorageError> {
    let mut cursor_activity_ms = None;
    let mut cursor_repository_id = String::new();
    let mut eligible_count = 0_usize;
    let mut oldest_eligible = None;
    loop {
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
        let page = statement
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
            .collect::<Result<Vec<_>, _>>()?;
        if page.is_empty() {
            return Ok(None);
        }
        for (repository_id, source_scope, activity_ms) in &page {
            cursor_activity_ms = Some(*activity_ms);
            cursor_repository_id.clone_from(repository_id);
            let automatic_set_id = workspace::workspace_set_id(repository_id);
            let belongs_to_user_set = connection.query_row(
                "SELECT EXISTS (
                     SELECT 1 FROM code_repository_set_members
                     WHERE repository_id = ?1 AND set_id <> ?2
                 )",
                params![repository_id, automatic_set_id],
                |row| row.get::<_, bool>(0),
            )?;
            if belongs_to_user_set {
                continue;
            }
            eligible_count = eligible_count.saturating_add(1);
            if oldest_eligible.is_none() {
                oldest_eligible = Some((repository_id.clone(), source_scope.clone()));
            }
            if eligible_count > max_indexed_repositories {
                return Ok(oldest_eligible);
            }
        }
        if page.len() < CANDIDATE_PAGE_SIZE {
            return Ok(None);
        }
    }
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
