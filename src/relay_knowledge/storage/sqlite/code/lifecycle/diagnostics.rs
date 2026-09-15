//! Reads diagnostic pages from one published snapshot under a read transaction.
use crate::{
    domain::{
        CodeContentIntegrity, CodeDiagnosticsPage, CodeDiagnosticsPageRequest, CodeFileDiagnostic,
    },
    storage::StorageError,
};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn content_integrity(
    connection: &Connection,
    scope: Option<String>,
) -> rusqlite::Result<CodeContentIntegrity> {
    let Some(scope) = scope else {
        return Ok(CodeContentIntegrity::default());
    };
    let count = connection.query_row(
        "SELECT (SELECT COUNT(DISTINCT path) FROM code_repository_file_diagnostics WHERE source_scope = ?1)
         FROM code_repository_scopes WHERE source_scope = ?1 AND retiring = 0",
        params![scope], |row| row.get::<_,usize>(0)).optional()?;
    Ok(count
        .map(|count| CodeContentIntegrity::measured(scope, count))
        .unwrap_or_default())
}

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
        "SELECT EXISTS(SELECT 1 FROM code_repository_scopes WHERE source_scope = ?1 AND repository_id = ?2 AND retiring = 0 AND stale = 0)",
        params![request.source_scope,request.repository_id], |row| row.get(0))?;
    if !exists {
        return Err(StorageError::InvalidInput(
            "diagnostic snapshot is unavailable or no longer published".into(),
        ));
    }
    let filters = serde_json::to_string(&request.path_filters)
        .map_err(|e| StorageError::InvalidInput(e.to_string()))?;
    let predicate = "source_scope = ?1 AND (json_array_length(?2) = 0 OR EXISTS (
        SELECT 1 FROM json_each(?2) filter WHERE d.path = filter.value OR substr(d.path,1,length(filter.value)+1) = filter.value || '/'))";
    let count = transaction.query_row(
        &format!(
            "SELECT COUNT(DISTINCT path) FROM code_repository_file_diagnostics d WHERE {predicate}"
        ),
        params![request.source_scope, filters],
        |row| row.get(0),
    )?;
    let (after_path, after_message) = request.after.clone().unwrap_or_default();
    let mut statement = transaction.prepare(&format!("SELECT repository_id, source_scope, path, parse_status, message FROM code_repository_file_diagnostics d WHERE {predicate} AND (?3 = 0 OR (path, message) > (?4, ?5)) ORDER BY path, message LIMIT ?6"))?;
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
        degraded_file_count: count,
        diagnostics,
        has_more,
    })
}

#[cfg(test)]
mod mod_tests;
