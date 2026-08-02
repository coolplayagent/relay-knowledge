use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{domain::CodeRepositoryRemovalSummary, storage::StorageError};

use super::{cleanup::delete_scope_index, status};

pub(in crate::storage::sqlite::code) fn remove_repository(
    connection: &mut Connection,
    repository: &str,
    now_ms: u64,
) -> Result<Option<CodeRepositoryRemovalSummary>, StorageError> {
    let Some(status) = status::repository_status(connection, repository)? else {
        return Ok(None);
    };
    let repository_id = status.repository_id;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    reject_running_index_task(&transaction, &repository_id, now_ms)?;
    let aliases_removed = repository_aliases(&transaction, &repository_id)?;
    let scopes = repository_cleanup_scopes(&transaction, &repository_id)?;
    let affected_set_ids = affected_repository_sets(&transaction, &repository_id)?;
    reject_running_repository_set_refresh_tasks(&transaction, &affected_set_ids, now_ms)?;
    let removed_repository_set_member_count =
        count_repository_set_members(&transaction, &repository_id)?;
    let removed_index_task_count = count_index_tasks(&transaction, &repository_id)?;

    invalidate_repository_sets(&transaction, &affected_set_ids)?;
    transaction.execute(
        "DELETE FROM code_repository_set_members WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    for scope in &scopes {
        delete_scope_index(&transaction, scope)?;
        delete_scope_lifecycle_projection(&transaction, scope)?;
    }
    transaction.execute(
        "DELETE FROM code_repository_index_checkpoints WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_index_tasks WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_scopes WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_aliases WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repositories WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    transaction.commit()?;

    Ok(Some(CodeRepositoryRemovalSummary {
        repository_id,
        aliases_removed,
        removed_scope_count: scopes.len(),
        removed_index_task_count,
        removed_repository_set_member_count,
        invalidated_repository_set_count: affected_set_ids.len(),
    }))
}

fn reject_running_index_task(
    transaction: &Transaction<'_>,
    repository_id: &str,
    now_ms: u64,
) -> Result<(), StorageError> {
    let running_task_id = transaction
        .query_row(
            "
            SELECT task_id
            FROM code_repository_index_tasks
            WHERE repository_id = ?1
              AND state = 'running'
              AND lease_expires_at_ms > ?2
            ORDER BY lease_expires_at_ms DESC, created_at_ms ASC, task_id ASC
            LIMIT 1
            ",
            params![repository_id, now_ms],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(task_id) = running_task_id {
        return Err(StorageError::InvalidInput(format!(
            "code repository '{repository_id}' has running index task '{task_id}'; wait for the task to finish before removing the repository"
        )));
    }

    Ok(())
}

fn repository_aliases(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT alias
        FROM code_repository_aliases
        WHERE repository_id = ?1
        UNION
        SELECT alias
        FROM code_repositories
        WHERE repository_id = ?1
        ORDER BY alias ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn repository_cleanup_scopes(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT source_scope
        FROM code_repository_scopes
        WHERE repository_id = ?1
        UNION
        SELECT source_scope
        FROM code_repository_index_tasks
        WHERE repository_id = ?1
        UNION
        SELECT source_scope
        FROM code_repository_index_checkpoints
        WHERE repository_id = ?1
        ORDER BY source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn delete_scope_lifecycle_projection(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    for table in [
        "software_build_targets",
        "software_iac_resources",
        "software_design_elements",
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
        )?;
    }

    Ok(())
}

fn affected_repository_sets(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT DISTINCT set_id
        FROM code_repository_set_members
        WHERE repository_id = ?1
        ORDER BY set_id ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn count_repository_set_members(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            "SELECT COUNT(*) FROM code_repository_set_members WHERE repository_id = ?1",
            params![repository_id],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

fn count_index_tasks(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            "SELECT COUNT(*) FROM code_repository_index_tasks WHERE repository_id = ?1",
            params![repository_id],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

fn invalidate_repository_sets(
    transaction: &Transaction<'_>,
    set_ids: &[String],
) -> Result<(), StorageError> {
    for set_id in unique_set_ids(set_ids) {
        transaction.execute(
            "DELETE FROM code_repository_cross_edges WHERE set_id = ?1",
            params![set_id],
        )?;
        transaction.execute(
            "DELETE FROM code_repository_set_overlay_status WHERE set_id = ?1",
            params![set_id],
        )?;
        transaction.execute(
            "DELETE FROM code_repository_set_refresh_tasks WHERE set_id = ?1",
            params![set_id],
        )?;
        transaction.execute(
            "
            UPDATE code_repository_sets
            SET updated_at_ms = strftime('%s','now') * 1000
            WHERE set_id = ?1
            ",
            params![set_id],
        )?;
    }

    Ok(())
}

fn reject_running_repository_set_refresh_tasks(
    transaction: &Transaction<'_>,
    set_ids: &[String],
    now_ms: u64,
) -> Result<(), StorageError> {
    for set_id in unique_set_ids(set_ids) {
        let running_task_id = transaction
            .query_row(
                "
                SELECT task_id
                FROM code_repository_set_refresh_tasks
                WHERE set_id = ?1
                  AND state = 'running'
                  AND lease_expires_at_ms > ?2
                ORDER BY lease_expires_at_ms DESC, created_at_ms ASC, task_id ASC
                LIMIT 1
                ",
                params![set_id, now_ms],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(task_id) = running_task_id {
            return Err(StorageError::InvalidInput(format!(
                "code repository set '{set_id}' has running refresh task '{task_id}'; wait for the task to finish before removing repository members"
            )));
        }
    }

    Ok(())
}

fn unique_set_ids(set_ids: &[String]) -> BTreeSet<&str> {
    set_ids.iter().map(String::as_str).collect()
}

#[cfg(test)]
#[path = "removal_tests.rs"]
mod tests;
