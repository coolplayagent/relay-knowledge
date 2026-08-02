//! Deadline-aware SQLite FTS search and file-hit row mapping.

use std::time::Instant;

use rusqlite::{Connection, ErrorCode, params};

use crate::storage::{FileSearchHit, FileSearchRequest, StorageError};

pub(in crate::storage::sqlite) fn search(
    connection: &Connection,
    request: FileSearchRequest,
    deadline: Instant,
) -> Result<Vec<FileSearchHit>, StorageError> {
    if Instant::now() >= deadline {
        return Err(file_query_timeout());
    }
    connection.progress_handler(1000, Some(move || Instant::now() >= deadline));
    let result = super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        if Instant::now() >= deadline {
            return Err(file_query_timeout());
        }
        search_with_progress_handler(connection, request.clone())
    });
    connection.progress_handler(0, None::<fn() -> bool>);

    match result {
        Err(StorageError::Sqlite(error)) if sqlite_interrupted(&error) => Err(file_query_timeout()),
        other => other,
    }
}

fn search_with_progress_handler(
    connection: &Connection,
    request: FileSearchRequest,
) -> Result<Vec<FileSearchHit>, StorageError> {
    let query = fts_query(&request.query)?;
    let mut statement = connection.prepare(
        "
        SELECT
            e.scope_id, e.root_id, e.path, e.relative_path, e.file_name, e.extension,
            e.parent_dir, e.size_bytes, e.modified_at_ms, e.status,
            bm25(file_index_search) AS score
        FROM file_index_search
        INNER JOIN file_index_entries e ON e.entry_key = file_index_search.entry_key
        WHERE file_index_search MATCH ?1
          AND e.status = 'indexed'
          AND (?2 IS NULL OR e.scope_id = ?2)
          AND (?3 IS NULL OR e.root_id = ?3)
        ORDER BY score ASC, e.path ASC
        LIMIT ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            query,
            request.source_scope.as_deref(),
            request.root_id.as_deref(),
            limit_i64(request.limit)?,
        ],
        |row| {
            Ok(FileSearchHit {
                scope_id: row.get(0)?,
                root_id: row.get(1)?,
                path: row.get(2)?,
                relative_path: row.get(3)?,
                file_name: row.get(4)?,
                extension: row.get(5)?,
                parent_dir: row.get(6)?,
                size_bytes: u64_from_sql(row.get::<_, i64>(7)?)?,
                modified_at_ms: u64_from_sql(row.get::<_, i64>(8)?)?,
                status: row.get(9)?,
                rank: 0,
                score: row.get(10)?,
            })
        },
    )?;
    let mut hits = rows.collect::<Result<Vec<_>, _>>()?;
    for (index, hit) in hits.iter_mut().enumerate() {
        hit.rank = index.saturating_add(1);
    }

    Ok(hits)
}

fn sqlite_interrupted(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _) if inner.code == ErrorCode::OperationInterrupted
    )
}

fn file_query_timeout() -> StorageError {
    StorageError::InvalidInput("file query timed out".to_owned())
}

fn fts_query(query: &str) -> Result<String, StorageError> {
    let terms = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .take(16)
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return Err(StorageError::InvalidInput(
            "file query must contain at least one searchable term".to_owned(),
        ));
    }

    Ok(terms.join(" AND "))
}

fn limit_i64(limit: usize) -> Result<i64, StorageError> {
    i64::try_from(limit).map_err(|_| {
        StorageError::InvalidInput("file query limit exceeds SQLite integer range".to_owned())
    })
}

fn u64_from_sql(value: i64) -> Result<u64, rusqlite::Error> {
    u64::try_from(value).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "negative integer in unsigned file index field",
            )),
        )
    })
}

#[cfg(test)]
mod mod_tests;
