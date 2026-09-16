//! Retires provisional filesystem facts before their owning task changes identity.

use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

use super::{lifecycle::publication_fence::PublicationFenceGuard, tasks::retention_gc};

pub(super) fn advance(
    connection: &mut Connection,
    source_scope: &str,
    fence: &PublicationFenceGuard,
    resume_only: bool,
) -> Result<bool, StorageError> {
    let transaction = connection.transaction()?;
    if !fence.target_scope_matches(&transaction, source_scope)? {
        return Err(StorageError::InvalidInput(
            "source-replan cleanup requires the current task target".into(),
        ));
    }
    fence.validate(&transaction)?;
    let owner = transaction
        .query_row(
            "SELECT source_replan_task_id FROM code_repository_scope_gc_jobs WHERE source_scope = ?1",
            [source_scope],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?;
    if owner
        .as_ref()
        .is_some_and(|owner| owner.as_deref() != Some(fence.task_id()))
    {
        return Err(StorageError::Invariant(
            "source-replan retirement is owned by another operation".into(),
        ));
    }
    if owner.is_none() && resume_only {
        transaction.commit()?;
        return Ok(true);
    }
    let checkpoint = transaction.query_row(
        "SELECT repository_id, resolved_commit_sha, state FROM code_repository_index_checkpoints WHERE source_scope = ?1",
        [source_scope],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
    ).optional()?;
    let repository_id = if let Some((repository_id, commit, state)) = checkpoint {
        fence.validate_repository(&repository_id)?;
        if !commit.starts_with("filesystem:")
            || !matches!(state.as_str(), "indexing" | "abandoning_source_io")
        {
            return Err(StorageError::Invariant(
                "only unpublished filesystem parser sessions may be abandoned".into(),
            ));
        }
        repository_id
    } else if owner.is_some() {
        transaction.query_row(
            "SELECT repository_id FROM code_repository_scope_gc_jobs WHERE source_scope = ?1",
            [source_scope],
            |row| row.get::<_, String>(0),
        )?
    } else {
        transaction.commit()?;
        return Ok(true);
    };
    fence.validate_repository(&repository_id)?;
    let published: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM code_repository_scopes WHERE source_scope = ?1)",
        [source_scope],
        |row| row.get(0),
    )?;
    if published {
        return Err(StorageError::Invariant(
            "source-replan cleanup cannot retire a published scope".into(),
        ));
    }
    let now =
        crate::clock::system_now_millis().map_err(|e| StorageError::Invariant(e.to_string()))?;
    if owner.is_none() {
        retention_gc::schedule(&transaction, &repository_id, source_scope, now)?;
        transaction.execute("UPDATE code_repository_scope_gc_jobs SET source_replan_task_id = ?2 WHERE source_scope = ?1", params![source_scope, fence.task_id()])?;
        transaction.execute("UPDATE code_repository_index_checkpoints SET state = 'abandoning_source_io' WHERE source_scope = ?1", [source_scope])?;
    }
    let completed = retention_gc::process_scope(&transaction, &repository_id, source_scope, now)?;
    fence.validate_target_scope(&transaction, source_scope)?;
    fence.validate(&transaction)?;
    transaction.commit()?;
    Ok(completed)
}

#[cfg(test)]
#[path = "source_replan_tests.rs"]
mod tests;
