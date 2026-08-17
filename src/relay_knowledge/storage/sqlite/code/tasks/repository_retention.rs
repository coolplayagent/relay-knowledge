//! Durable selection and lifecycle for whole-repository index retention.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{domain::CodeRepositoryRetentionJobStatus, storage::StorageError};

const PHASE: &str = "retiring_scopes";

pub(in crate::storage::sqlite::code) fn job(
    connection: &Connection,
    repository_id: &str,
) -> Result<Option<CodeRepositoryRetentionJobStatus>, StorageError> {
    connection
        .query_row(
            "SELECT repository_id, initial_scope, cutoff_ms, phase,
                    created_at_ms, updated_at_ms, last_error
             FROM code_repository_retention_jobs WHERE repository_id = ?1",
            params![repository_id],
            |row| {
                Ok(CodeRepositoryRetentionJobStatus {
                    repository_id: row.get(0)?,
                    initial_scope: row.get(1)?,
                    cutoff_ms: row.get(2)?,
                    phase: row.get(3)?,
                    created_at_ms: row.get(4)?,
                    updated_at_ms: row.get(5)?,
                    last_error: row.get(6)?,
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

    let query_limit = max_indexed_repositories.saturating_add(1);
    let mut statement = transaction.prepare(
        "SELECT repository.repository_id, repository.last_indexed_scope_id
         FROM code_repositories repository
         WHERE repository.last_indexed_scope_id IS NOT NULL
           AND EXISTS (
               SELECT 1 FROM code_repository_scopes scope
               WHERE scope.repository_id = repository.repository_id
                 AND scope.source_scope = repository.last_indexed_scope_id
                 AND scope.retiring = 0
           )
           AND NOT EXISTS (
               SELECT 1
               FROM code_repository_set_members member
               JOIN code_repository_sets repository_set
                 ON repository_set.set_id = member.set_id
               WHERE member.repository_id = repository.repository_id
                 AND repository_set.alias <> repository.repository_id || '-auto-workspace'
           )
         ORDER BY COALESCE(
             (SELECT MAX(task.updated_at_ms) FROM code_repository_index_tasks task
              WHERE task.repository_id = repository.repository_id
                AND task.source_scope = repository.last_indexed_scope_id
                AND task.state = 'succeeded'),
             (SELECT MAX(checkpoint.updated_at_ms)
              FROM code_repository_index_checkpoints checkpoint
              WHERE checkpoint.repository_id = repository.repository_id
                AND checkpoint.source_scope = repository.last_indexed_scope_id
                AND checkpoint.state IN ('complete', 'completed')),
             0
         ), repository.repository_id
         LIMIT ?1",
    )?;
    let candidates = statement
        .query_map(params![query_limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let selected = (candidates.len() > max_indexed_repositories).then(|| candidates[0].clone());
    if let Some((repository_id, initial_scope)) = &selected {
        transaction.execute(
            "INSERT INTO code_repository_retention_jobs (
                 repository_id, initial_scope, cutoff_ms, phase,
                 created_at_ms, updated_at_ms, last_error
             ) VALUES (?1, ?2, ?3, ?4, ?3, ?3, NULL)",
            params![repository_id, initial_scope, now_ms, PHASE],
        )?;
    }
    transaction.commit()?;
    Ok(selected.map(|(repository_id, _)| repository_id))
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
