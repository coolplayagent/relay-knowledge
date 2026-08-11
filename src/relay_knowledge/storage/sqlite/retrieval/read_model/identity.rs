//! Canonical authoritative-to-derived document identity checks for rebuild admission.

use rusqlite::{Connection, params};

use crate::storage::StorageError;

pub(super) fn derived_document_identity_mismatch(
    connection: &Connection,
    derived_table: &'static str,
) -> Result<bool, StorageError> {
    let mut authoritative = Vec::new();
    if table_has_columns(connection, "evidence", &["id", "status"])? {
        authoritative.push(
            "SELECT 'evidence:' || id AS document_id
             FROM evidence WHERE status IN ('accepted', 'proposed')"
                .to_owned(),
        );
    }
    if table_has_columns(
        connection,
        "code_symbols",
        &["source_scope", "path", "symbol_id"],
    )? {
        authoritative.push(code_identity_sql("code_symbols", "symbol", "symbol_id"));
    }
    if table_has_columns(
        connection,
        "code_chunks",
        &["source_scope", "path", "chunk_id"],
    )? {
        authoritative.push(code_identity_sql("code_chunks", "chunk", "chunk_id"));
    }
    if authoritative.is_empty() {
        return Ok(false);
    }
    let sql = format!(
        "WITH authoritative(document_id) AS ({})
         SELECT EXISTS(
             SELECT 1 FROM authoritative
             LEFT JOIN {derived_table} derived
               ON derived.document_id = authoritative.document_id
             WHERE derived.document_id IS NULL
             LIMIT 1
         )",
        authoritative.join(" UNION ALL ")
    );
    connection
        .query_row(&sql, [], |row| row.get::<_, bool>(0))
        .map_err(StorageError::from)
}

fn code_identity_sql(table: &str, kind: &str, id_column: &str) -> String {
    format!(
        "SELECT 'code:{kind}:' || length(CAST(source_scope AS BLOB)) || ':' ||
                source_scope || ':' || length(CAST(path AS BLOB)) || ':' || path || ':' ||
                length(CAST({id_column} AS BLOB)) || ':' || {id_column} AS document_id
         FROM {table}"
    )
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
             )",
            params![table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

fn table_has_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let columns = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(required_columns
        .iter()
        .all(|required| columns.iter().any(|column| column == required)))
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
