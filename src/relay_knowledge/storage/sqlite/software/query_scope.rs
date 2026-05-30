use rusqlite::{Connection, OptionalExtension, params, types::Value};

use crate::{domain::SoftwareGlobalRequest, storage::StorageError};

use super::super::code_query_scope;

pub(super) fn source_scope_for_request(
    connection: &mut Connection,
    request: &SoftwareGlobalRequest,
) -> Result<String, StorageError> {
    if let Ok(source_scope) = exact_source_scope_for_request(connection, request) {
        return Ok(source_scope);
    }
    let repository_id = repository_id_for_request(connection, &request.repository.repository)?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "code repository '{}' is not registered",
                request.repository.repository
            ))
        })?;
    let mut statement = connection.prepare(
        "
        SELECT scope.source_scope, scope.path_filters_json, scope.language_filters_json
        FROM code_repository_scopes scope
        WHERE scope.repository_id = ?1
          AND scope.resolved_commit_sha = ?2
        ORDER BY scope.path_filters_json ASC, scope.language_filters_json ASC,
                 scope.source_scope ASC
        ",
    )?;
    let candidates = statement.query_map(
        params![repository_id, request.repository.ref_selector],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    for candidate in candidates {
        let (source_scope, path_filters_json, language_filters_json) = candidate?;
        let path_filters = parse_filter_json(&path_filters_json)?;
        let language_filters = parse_filter_json(&language_filters_json)?;
        if code_query_scope::selector_filters_fit_indexed_scope(
            &path_filters,
            &language_filters,
            &request.repository.path_filters,
            &request.repository.language_filters,
        ) {
            return Ok(source_scope);
        }
    }

    Err(source_scope_filter_error(request))
}

pub(super) fn repository_id_for_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(
            "
            SELECT repository_id
            FROM code_repository_scopes
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::from)
}

pub(super) fn path_filter_sql_for_column(column: &str, filters: &[String]) -> String {
    let clauses = filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
        .map(|_| format!("({column} = ? OR {column} LIKE ? ESCAPE '\\')"))
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND ({})", clauses.join(" OR "))
    }
}

pub(super) fn language_filter_sql_for_column(column: &str, filters: &[String]) -> String {
    let clauses = filters
        .iter()
        .map(|_| format!("{column} = ?"))
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND ({})", clauses.join(" OR "))
    }
}

pub(super) fn push_path_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    for filter in filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
    {
        values.push(Value::Text(filter.clone()));
        values.push(Value::Text(format!("{}/%", escape_sql_like(&filter))));
    }
}

pub(super) fn push_language_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    values.extend(filters.iter().cloned().map(Value::Text));
}

fn repository_id_for_request(
    connection: &Connection,
    repository: &str,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(
            "
            SELECT repository_id
            FROM (
                SELECT repository_id, 0 AS precedence
                FROM code_repositories
                WHERE repository_id = ?1
                UNION ALL
                SELECT repository_id, 1 AS precedence
                FROM code_repository_aliases
                WHERE alias = ?1
            )
            ORDER BY precedence ASC
            LIMIT 1
            ",
            params![repository],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn parse_filter_json(value: &str) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn normalized_sql_path_filter(filter: &str) -> Option<String> {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    (!filter.is_empty() && filter != ".").then(|| filter.to_owned())
}

fn escape_sql_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn source_scope_filter_error(request: &SoftwareGlobalRequest) -> StorageError {
    StorageError::InvalidInput(format!(
        "code repository '{}' does not have an indexed software projection scope for ref '{}'",
        request.repository.repository, request.repository.ref_selector
    ))
}

fn exact_source_scope_for_request(
    connection: &mut Connection,
    request: &SoftwareGlobalRequest,
) -> Result<String, StorageError> {
    let path_filters_json = serde_json::to_string(&request.repository.path_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let language_filters_json = serde_json::to_string(&request.repository.language_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let repository_id = repository_id_for_request(connection, &request.repository.repository)?
        .ok_or_else(|| source_scope_filter_error(request))?;
    connection
        .query_row(
            "
        SELECT scope.source_scope
        FROM code_repository_scopes scope
        WHERE scope.repository_id = ?1
          AND scope.resolved_commit_sha = ?2
          AND scope.path_filters_json = ?3
          AND scope.language_filters_json = ?4
        ORDER BY scope.source_scope ASC
        LIMIT 1
        ",
            params![
                repository_id,
                request.repository.ref_selector,
                path_filters_json,
                language_filters_json,
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| source_scope_filter_error(request))
}
