//! Keyset pages over persisted reactor facts; no POM parsing on query paths.
use super::persistence::{decode, require_complete};
use crate::storage::sqlite::scope_filters::{path_filter_sql_for_column, push_path_filter_values};
use crate::{domain::SoftwareGlobalRequest, storage::StorageError};
use rusqlite::{Connection, params_from_iter, types::Value};

pub(in crate::storage::sqlite) fn includes_language(filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|language| matches!(language.as_str(), "java" | "kotlin" | "scala" | "jvm"))
}

pub(in crate::storage::sqlite) fn read_page<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    scope: &str,
    request: &SoftwareGlobalRequest,
    after: &str,
    limit: usize,
    edges: bool,
) -> Result<Vec<T>, StorageError> {
    if limit > 501 {
        return Err(StorageError::InvalidInput(
            "reactor page exceeds 500 rows plus lookahead".into(),
        ));
    }
    if !includes_language(&request.repository.language_filters) {
        return Ok(Vec::new());
    }
    require_complete(connection, scope)?;
    let path_filter = path_filter_sql_for_column("m.path", &request.repository.path_filters);
    let (sql, mut values) = if edges {
        let evidence_filter = path_filter_sql_for_column(
            "json_extract(e.payload, '$.evidence_path')",
            &request.repository.path_filters,
        );
        let mut values = vec![Value::Text(scope.into()), Value::Text(after.into())];
        push_path_filter_values(&mut values, &request.repository.path_filters);
        push_path_filter_values(&mut values, &request.repository.path_filters);
        (
            format!(
                "SELECT e.payload FROM maven_reactor_edges e JOIN maven_reactor_modules m ON m.source_scope=e.source_scope AND m.module_id=e.source_id WHERE e.source_scope=? AND e.edge_id>? {path_filter} {evidence_filter} ORDER BY e.edge_id LIMIT ?"
            ),
            values,
        )
    } else {
        let mut values = vec![Value::Text(scope.into()), Value::Text(after.into())];
        push_path_filter_values(&mut values, &request.repository.path_filters);
        (
            format!(
                "SELECT m.payload FROM maven_reactor_modules m WHERE m.source_scope=? AND m.module_id>? {path_filter} ORDER BY m.module_id LIMIT ?"
            ),
            values,
        )
    };
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&sql)?;
    statement
        .query_map(params_from_iter(values), |row| row.get::<_, String>(0))?
        .map(|row| decode(row?))
        .collect()
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
