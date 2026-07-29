use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

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
            WHERE repository_id = ?1 AND source_scope = ?2
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
    let mut statement = connection.prepare(
        "
        SELECT source_scope
        FROM code_repository_scopes
        WHERE repository_id = ?1
          AND resolved_commit_sha = ?2
          AND path_filters_json = ?3
          AND language_filters_json = ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            repository_id,
            base_commit,
            path_filters_json,
            language_filters_json
        ],
        |row| row.get::<_, String>(0),
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn worktree_overlay_base_commit(active_commit: &str) -> Option<&str> {
    active_commit
        .strip_prefix("worktree:")
        .and_then(|rest| rest.split_once(':'))
        .map(|(base_commit, _)| base_commit)
}
