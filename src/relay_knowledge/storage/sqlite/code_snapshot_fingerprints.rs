use rusqlite::{Connection, params};

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
