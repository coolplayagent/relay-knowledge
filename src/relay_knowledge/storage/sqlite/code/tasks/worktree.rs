use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

pub(super) fn compatible_non_retiring_scopes_for_commit(
    connection: &Connection,
    repository_id: &str,
    resolved_commit_sha: &str,
    path_filters_json: &str,
    language_filters_json: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT scope.source_scope
        FROM code_repository_scopes scope
        WHERE scope.repository_id = ?1
          AND scope.stale = 0
          AND scope.retiring = 0
          AND NOT EXISTS (
              SELECT 1
              FROM code_repository_scope_gc_jobs job
              WHERE job.repository_id = scope.repository_id
                AND job.source_scope = scope.source_scope
          )
          AND scope.path_filters_json = ?3
          AND scope.language_filters_json = ?4
          AND (
              scope.resolved_commit_sha = ?2
              OR EXISTS (
                  SELECT 1
                  FROM code_repository_commit_scopes commit_scope
                  WHERE commit_scope.repository_id = scope.repository_id
                    AND commit_scope.resolved_commit_sha = ?2
                    AND commit_scope.source_scope = scope.source_scope
              )
          )
        ORDER BY scope.source_scope
        ",
    )?;
    let rows = statement.query_map(
        params![
            repository_id,
            resolved_commit_sha,
            path_filters_json,
            language_filters_json,
        ],
        |row| row.get::<_, String>(0),
    )?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn active_worktree_base_scopes(
    connection: &Connection,
    repository_id: &str,
    active_scope: &str,
) -> Result<Vec<String>, StorageError> {
    if active_scope.is_empty() {
        return Ok(Vec::new());
    }
    let active = connection
        .query_row(
            "
            SELECT resolved_commit_sha, path_filters_json, language_filters_json
            FROM code_repository_scopes
            WHERE repository_id = ?1 AND source_scope = ?2 AND retiring = 0
            ",
            params![repository_id, active_scope],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((active_commit, path_filters_json, language_filters_json)) = active else {
        return Ok(Vec::new());
    };
    let Some(base_commit) = worktree_overlay_base_commit(&active_commit) else {
        return Ok(Vec::new());
    };
    compatible_non_retiring_scopes_for_commit(
        connection,
        repository_id,
        base_commit,
        &path_filters_json,
        &language_filters_json,
    )
}

pub(super) fn worktree_overlay_base_commit(active_commit: &str) -> Option<&str> {
    active_commit
        .strip_prefix("worktree:")
        .and_then(|rest| rest.split_once(':'))
        .map(|(base_commit, _)| base_commit)
}

pub(super) fn pending_worktree_overlay_base_commit(pending_commit: &str) -> Option<&str> {
    pending_commit.strip_prefix("worktree:pending:")
}

pub(super) fn worktree_task_base_commit<'a>(
    resolved_commit_sha: &'a str,
    ref_selector: &'a str,
) -> Option<&'a str> {
    pending_worktree_overlay_base_commit(resolved_commit_sha)
        .or_else(|| worktree_overlay_base_commit(resolved_commit_sha))
        .or_else(|| (!ref_selector.is_empty()).then_some(ref_selector))
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
