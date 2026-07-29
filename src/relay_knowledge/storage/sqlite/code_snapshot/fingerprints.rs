use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{domain::CodeFileFingerprint, storage::StorageError};

pub(in crate::storage::sqlite::code) fn file_fingerprints(
    connection: &mut Connection,
    repository_id: &str,
) -> Result<Vec<CodeFileFingerprint>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT path, blob_hash
        FROM code_repository_files
        WHERE repository_id = ?1
          AND source_scope = (
              SELECT last_indexed_scope_id FROM code_repositories WHERE repository_id = ?1
          )
        ORDER BY path ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| {
        Ok(CodeFileFingerprint {
            path: row.get(0)?,
            blob_hash: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code) fn file_fingerprints_for_scope(
    connection: &mut Connection,
    source_scope: &str,
) -> Result<Vec<CodeFileFingerprint>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT path, blob_hash
        FROM code_repository_files
        WHERE source_scope = ?1
        ORDER BY path ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(CodeFileFingerprint {
            path: row.get(0)?,
            blob_hash: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code) fn file_fingerprints_for_paths(
    connection: &mut Connection,
    source_scope: &str,
    paths: &[String],
) -> Result<Vec<CodeFileFingerprint>, StorageError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", paths.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "
        SELECT path, blob_hash
        FROM code_repository_files
        WHERE source_scope = ?
          AND path IN ({placeholders})
        ORDER BY path ASC
        "
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    values.extend(paths.iter().cloned().map(Value::Text));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok(CodeFileFingerprint {
            path: row.get(0)?,
            blob_hash: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}
