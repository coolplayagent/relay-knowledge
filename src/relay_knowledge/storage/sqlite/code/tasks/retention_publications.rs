//! Publication fences and incremental-base discovery for scope retention.

use rusqlite::{Connection, params};

use crate::{domain::CodeIndexMode, storage::StorageError};

use super::retention::{RETAIN_SUCCEEDED_TASK_AUDIT_ROWS, ScopePage};

pub(super) fn successful_scopes_since(
    connection: &Connection,
    repository_id: &str,
    cutoff_ms: u64,
    cutoff_publication_generation: u64,
    initial_scope: &str,
    limit: usize,
) -> Result<ScopePage, StorageError> {
    let query_limit = limit.saturating_add(1);
    let mut statement = connection.prepare(
        "SELECT source_scope
         FROM (
             SELECT source_scope, updated_at_ms
             FROM code_repository_index_tasks
             WHERE repository_id = ?1 AND state = 'succeeded'
               AND (
                   (?3 > 0 AND publication_generation > ?3)
                   OR (
                       (?3 = 0 OR publication_generation = 0)
                       AND updated_at_ms >= ?2
                       AND (source_scope <> ?4 OR updated_at_ms > ?2)
                   )
               )
             UNION ALL
             SELECT source_scope, updated_at_ms
             FROM code_repository_index_checkpoints
             WHERE repository_id = ?1 AND state IN ('complete', 'completed')
               AND updated_at_ms >= ?2
               AND (source_scope <> ?4 OR updated_at_ms > ?2)
         ) publication
         GROUP BY source_scope
         ORDER BY MAX(updated_at_ms) DESC, source_scope DESC
         LIMIT ?5",
    )?;
    let scopes = statement
        .query_map(
            params![
                repository_id,
                cutoff_ms,
                cutoff_publication_generation,
                initial_scope,
                query_limit
            ],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let query_was_truncated = scopes.len() > limit;
    let mut queryable = Vec::new();
    for scope in scopes {
        if scope_is_queryable(connection, repository_id, &scope)? {
            queryable.push(scope);
        }
    }
    let truncated = query_was_truncated || queryable.len() > limit;
    queryable.truncate(limit);
    Ok(ScopePage {
        scopes: queryable,
        truncated,
    })
}

pub(super) fn scope_is_queryable(
    connection: &Connection,
    repository_id: &str,
    source_scope: &str,
) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_scopes
                 WHERE repository_id = ?1 AND source_scope = ?2 AND retiring = 0
             )",
            params![repository_id, source_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

pub(super) fn latest_successful_incremental_base(
    connection: &Connection,
    repository_id: &str,
) -> Result<Option<CommitReference>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT mode_json, path_filters_json, language_filters_json
         FROM code_repository_index_tasks
              INDEXED BY code_repository_index_tasks_publication_retention
         WHERE repository_id = ?1 AND state = 'succeeded'
         ORDER BY publication_generation DESC, updated_at_ms DESC,
                  created_at_ms DESC, task_id DESC
         LIMIT ?2",
    )?;
    let rows = statement.query_map(
        params![repository_id, RETAIN_SUCCEEDED_TASK_AUDIT_ROWS + 1],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    find_incremental_base(rows)
}

pub(super) fn latest_successful_incremental_base_since(
    connection: &Connection,
    repository_id: &str,
    cutoff_ms: u64,
    cutoff_publication_generation: u64,
    initial_scope: &str,
    current_active_scope: Option<&str>,
) -> Result<Option<CommitReference>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT mode_json, path_filters_json, language_filters_json
         FROM code_repository_index_tasks
         WHERE repository_id = ?1 AND state = 'succeeded'
           AND (
               source_scope = ?5
               OR (
                   (?3 > 0 AND publication_generation > ?3)
                   OR (
                       (?3 = 0 OR publication_generation = 0)
                       AND updated_at_ms >= ?2
                       AND (source_scope <> ?4 OR updated_at_ms > ?2)
                   )
               )
           )
         ORDER BY publication_generation DESC, updated_at_ms DESC,
                  created_at_ms DESC, task_id DESC
         LIMIT ?6",
    )?;
    let rows = statement.query_map(
        params![
            repository_id,
            cutoff_ms,
            cutoff_publication_generation,
            initial_scope,
            current_active_scope.unwrap_or_default(),
            RETAIN_SUCCEEDED_TASK_AUDIT_ROWS + 1
        ],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    find_incremental_base(rows)
}

fn find_incremental_base(
    rows: rusqlite::MappedRows<
        '_,
        impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<(String, String, String)>,
    >,
) -> Result<Option<CommitReference>, StorageError> {
    for row in rows {
        let (mode_json, path_filters_json, language_filters_json) = row?;
        let mode = serde_json::from_str::<CodeIndexMode>(&mode_json).map_err(|error| {
            StorageError::InvalidInput(format!(
                "successful code index task has invalid mode: {error}"
            ))
        })?;
        if let CodeIndexMode::Incremental { base_ref, .. } = mode {
            return Ok(Some(CommitReference {
                resolved_commit_sha: base_ref,
                path_filters_json,
                language_filters_json,
            }));
        }
    }
    Ok(None)
}

pub(super) struct CommitReference {
    pub(super) resolved_commit_sha: String,
    pub(super) path_filters_json: String,
    pub(super) language_filters_json: String,
}
