//! Reads diagnostic pages from one published snapshot under a read transaction.
use crate::{
    domain::{CodeDiagnosticsPage, CodeDiagnosticsPageRequest, CodeFileDiagnostic},
    storage::StorageError,
};
use rusqlite::{Connection, params};

pub(in crate::storage::sqlite::code) fn page(
    connection: &mut Connection,
    request: CodeDiagnosticsPageRequest,
) -> Result<CodeDiagnosticsPage, StorageError> {
    if !(1..=200).contains(&request.limit) {
        return Err(StorageError::InvalidInput(
            "diagnostic limit must be between 1 and 200".into(),
        ));
    }
    let transaction = connection.transaction()?;
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM code_repository_scopes scope
         WHERE scope.source_scope = ?1 AND scope.repository_id = ?2
           AND scope.retiring = 0 AND scope.stale = 0
           AND (scope.resolved_commit_sha = ?3 OR EXISTS (
               SELECT 1 FROM code_repository_commit_scopes commits
               WHERE commits.source_scope = scope.source_scope
                 AND commits.repository_id = scope.repository_id
                 AND commits.resolved_commit_sha = ?3)))",
        params![
            request.source_scope,
            request.repository_id,
            request.resolved_commit_sha
        ],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(StorageError::InvalidInput(
            "diagnostic snapshot is unavailable or no longer published".into(),
        ));
    }
    let mut scope_status = super::status::repository_scope_status_by_source_scope(
        &transaction,
        &request.source_scope,
    )?
    .ok_or_else(|| StorageError::InvalidInput("diagnostic snapshot is unavailable".into()))?;
    scope_status.last_indexed_commit = Some(request.resolved_commit_sha.clone());
    let filters = serde_json::to_string(&request.path_filters)
        .map_err(|e| StorageError::InvalidInput(e.to_string()))?;
    let predicate = "source_scope = ?1 AND (json_array_length(?2) = 0 OR EXISTS (
        SELECT 1 FROM json_each(?2) filter
        WHERE d.path = filter.value
           OR substr(d.path,1,length(filter.value)+1) = filter.value || '/'
           OR (json_extract(d.io_json, '$.path_kind') = 'directory'
               AND substr(filter.value,1,length(d.path)+1) = d.path || '/')))";
    let count = transaction.query_row(
        &format!(
            "SELECT COUNT(DISTINCT path) FROM code_repository_file_diagnostics d WHERE {predicate} AND COALESCE(json_extract(io_json, '$.path_kind'), 'file') = 'file'"
        ),
        params![request.source_scope, filters],
        |row| row.get(0),
    )?;
    let (after_path, after_message) = request.after.clone().unwrap_or_default();
    let mut statement = transaction.prepare(&format!("SELECT repository_id, source_scope, path, parse_status, message, io_json FROM code_repository_file_diagnostics d WHERE {predicate} AND (?3 = 0 OR (path, message) > (?4, ?5)) ORDER BY path, message LIMIT ?6"))?;
    let rows = statement.query_map(
        params![
            request.source_scope,
            filters,
            request.after.is_some(),
            after_path,
            after_message,
            request.limit + 1
        ],
        |row| {
            let status: String = row.get(3)?;
            let parse_status =
                serde_json::from_value(serde_json::Value::String(status)).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
            Ok(CodeFileDiagnostic {
                io: row
                    .get::<_, Option<String>>(5)?
                    .map(|json| serde_json::from_str(&json))
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?,
                repository_id: row.get(0)?,
                source_scope: row.get(1)?,
                path: row.get(2)?,
                parse_status,
                message: row.get(4)?,
            })
        },
    )?;
    let mut diagnostics = rows.collect::<Result<Vec<_>, _>>()?;
    let has_more = diagnostics.len() > request.limit;
    diagnostics.truncate(request.limit);
    Ok(CodeDiagnosticsPage {
        scope_status,
        degraded_file_count: count,
        diagnostics,
        has_more,
    })
}

#[cfg(test)]
mod mod_tests;
