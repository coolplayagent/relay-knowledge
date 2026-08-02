//! Transactional content replacement, chunk/FTS writes, and freshness cursors.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{IndexKind, IndexState},
    storage::{FileContentEntry, StorageError},
};

use super::identity::{chunk_id, cursor_key, entry_key};

const INDEXED_STATUS: &str = "indexed";
const MISSING_STATUS: &str = "missing";

#[derive(Default)]
pub(in crate::storage::sqlite::file_index) struct ContentReplacementCounts {
    pub(in crate::storage::sqlite::file_index) indexed_content_count: usize,
    pub(in crate::storage::sqlite::file_index) skipped_content_count: usize,
    pub(in crate::storage::sqlite::file_index) unchanged_content_count: usize,
    pub(in crate::storage::sqlite::file_index) stale_content_cursor_count: usize,
}

pub(in crate::storage::sqlite::file_index) struct ContentReplacementRequest<'a> {
    pub(in crate::storage::sqlite::file_index) scope_id: &'a str,
    pub(in crate::storage::sqlite::file_index) root_id: &'a str,
    pub(in crate::storage::sqlite::file_index) entries_len: usize,
    pub(in crate::storage::sqlite::file_index) observed_file_keys: &'a BTreeSet<String>,
    pub(in crate::storage::sqlite::file_index) processed_content_keys: &'a BTreeSet<String>,
    pub(in crate::storage::sqlite::file_index) content_entries: &'a [FileContentEntry],
    pub(in crate::storage::sqlite::file_index) file_scan_completed: bool,
    pub(in crate::storage::sqlite::file_index) content_scan_completed: bool,
    pub(in crate::storage::sqlite::file_index) now_ms: u64,
}

pub(in crate::storage::sqlite::file_index) fn replace_entries(
    connection: &Connection,
    request: ContentReplacementRequest<'_>,
) -> Result<ContentReplacementCounts, StorageError> {
    let content_entries = request
        .content_entries
        .iter()
        .map(|entry| {
            (
                entry_key(&entry.scope_id, &entry.root_id, &entry.path),
                entry,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut counts = ContentReplacementCounts {
        skipped_content_count: request.entries_len.saturating_sub(content_entries.len()),
        ..ContentReplacementCounts::default()
    };

    if request.file_scan_completed {
        for key in existing_content_entry_keys(connection, request.scope_id, request.root_id)?
            .difference(request.observed_file_keys)
        {
            mark_entry_missing(connection, key, request.now_ms)?;
        }
    }

    let current = content_entries.keys().cloned().collect::<BTreeSet<_>>();
    if request.content_scan_completed {
        for key in existing_content_entry_keys(connection, request.scope_id, request.root_id)?
            .difference(&current)
        {
            mark_entry_missing(connection, key, request.now_ms)?;
        }
    } else {
        let skipped_processed = request
            .processed_content_keys
            .difference(&current)
            .cloned()
            .collect::<BTreeSet<_>>();
        for key in existing_content_entry_keys(connection, request.scope_id, request.root_id)?
            .intersection(&skipped_processed)
        {
            mark_entry_missing(connection, key, request.now_ms)?;
        }
    }

    for (key, entry) in content_entries {
        let existing_hash = existing_content_hash(connection, &key)?;
        if existing_hash.as_deref() == Some(entry.content_hash.as_str()) {
            touch_unchanged_entry(connection, &key, entry)?;
            counts.unchanged_content_count = counts.unchanged_content_count.saturating_add(1);
            continue;
        }
        upsert_entry(connection, &key, entry)?;
        refresh_cursors(connection, entry)?;
    }
    counts.indexed_content_count =
        count_indexed_content_entries(connection, request.scope_id, request.root_id)?;
    counts.stale_content_cursor_count =
        count_stale_cursors(connection, request.scope_id, request.root_id)?;

    Ok(counts)
}

pub(in crate::storage::sqlite::file_index) fn mark_root_unconfigured(
    connection: &Connection,
    scope_id: &str,
    root_id: &str,
    now_ms: u64,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE file_content_entries
        SET status = ?3, skipped_reason = ?4, indexed_at_ms = ?5
        WHERE scope_id = ?1 AND root_id = ?2
        ",
        params![
            scope_id,
            root_id,
            MISSING_STATUS,
            "root no longer configured",
            now_ms,
        ],
    )?;
    connection.execute(
        "
        DELETE FROM file_content_search
        WHERE entry_key IN (
            SELECT entry_key FROM file_content_entries
            WHERE scope_id = ?1 AND root_id = ?2
        )
        ",
        params![scope_id, root_id],
    )?;
    connection.execute(
        "
        DELETE FROM file_content_chunks
        WHERE entry_key IN (
            SELECT entry_key FROM file_content_entries
            WHERE scope_id = ?1 AND root_id = ?2
        )
        ",
        params![scope_id, root_id],
    )?;
    connection.execute(
        "
        DELETE FROM file_content_cursors
        WHERE scope_id = ?1 AND root_id = ?2
        ",
        params![scope_id, root_id],
    )?;

    Ok(())
}

#[cfg(test)]
pub(in crate::storage::sqlite::file_index) fn cursors(
    connection: &Connection,
) -> Result<Vec<crate::storage::FileContentReadModelCursor>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT kind, scope_id, root_id, path, content_hash, indexed_graph_version, state,
               stale_reason
        FROM file_content_cursors
        ORDER BY scope_id ASC, root_id ASC, path ASC, kind ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        let kind_text: String = row.get(0)?;
        let state_text: String = row.get(6)?;
        Ok(crate::storage::FileContentReadModelCursor {
            kind: parse_index_kind(&kind_text)?,
            source_scope: row.get(1)?,
            root_id: row.get(2)?,
            path: row.get(3)?,
            content_hash: row.get(4)?,
            indexed_graph_version: u64_from_sql(row.get::<_, i64>(5)?)?,
            state: parse_index_state(&state_text)?,
            stale_reason: row.get(7)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn existing_content_entry_keys(
    connection: &Connection,
    scope_id: &str,
    root_id: &str,
) -> Result<BTreeSet<String>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT entry_key FROM file_content_entries WHERE scope_id = ?1 AND root_id = ?2",
    )?;
    let rows = statement.query_map(params![scope_id, root_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(StorageError::from)
}

fn existing_content_hash(
    connection: &Connection,
    key: &str,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(
            "SELECT content_hash FROM file_content_entries WHERE entry_key = ?1 AND status = ?2",
            params![key, INDEXED_STATUS],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn mark_entry_missing(connection: &Connection, key: &str, now_ms: u64) -> Result<(), StorageError> {
    delete_cursors_for_entry(connection, key)?;
    connection.execute(
        "
        UPDATE file_content_entries
        SET status = ?2, skipped_reason = ?3, indexed_at_ms = ?4
        WHERE entry_key = ?1
        ",
        params![
            key,
            MISSING_STATUS,
            "not observed during latest scan",
            now_ms
        ],
    )?;
    delete_chunks(connection, key)
}

fn delete_cursors_for_entry(connection: &Connection, key: &str) -> Result<(), StorageError> {
    connection.execute(
        "
        DELETE FROM file_content_cursors
        WHERE EXISTS (
            SELECT 1
            FROM file_content_entries entry
            WHERE entry.entry_key = ?1
              AND entry.scope_id = file_content_cursors.scope_id
              AND entry.root_id = file_content_cursors.root_id
              AND entry.path = file_content_cursors.path
        )
        ",
        params![key],
    )?;

    Ok(())
}

fn touch_unchanged_entry(
    connection: &Connection,
    key: &str,
    entry: &FileContentEntry,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE file_content_entries
        SET path = ?2,
            relative_path = ?3,
            fingerprint = ?4,
            indexed_at_ms = ?5,
            graph_version = ?6,
            status = ?7,
            skipped_reason = NULL
        WHERE entry_key = ?1
        ",
        params![
            key,
            &entry.path,
            &entry.relative_path,
            &entry.fingerprint,
            i64_from_u64(entry.indexed_at_ms)?,
            i64_from_u64(entry.graph_version)?,
            INDEXED_STATUS,
        ],
    )?;

    Ok(())
}

fn upsert_entry(
    connection: &Connection,
    key: &str,
    entry: &FileContentEntry,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT INTO file_content_entries (
            entry_key, scope_id, root_id, path, relative_path, fingerprint, content_hash,
            indexed_at_ms, graph_version, status, skipped_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(entry_key) DO UPDATE SET
            path = excluded.path,
            relative_path = excluded.relative_path,
            fingerprint = excluded.fingerprint,
            content_hash = excluded.content_hash,
            indexed_at_ms = excluded.indexed_at_ms,
            graph_version = excluded.graph_version,
            status = excluded.status,
            skipped_reason = excluded.skipped_reason
        ",
        params![
            key,
            &entry.scope_id,
            &entry.root_id,
            &entry.path,
            &entry.relative_path,
            &entry.fingerprint,
            &entry.content_hash,
            i64_from_u64(entry.indexed_at_ms)?,
            i64_from_u64(entry.graph_version)?,
            if entry.skipped_reason.is_some() {
                MISSING_STATUS
            } else {
                INDEXED_STATUS
            },
            entry.skipped_reason.as_deref(),
        ],
    )?;
    delete_chunks(connection, key)?;
    if entry.skipped_reason.is_some() {
        return Ok(());
    }
    for chunk in &entry.chunks {
        let chunk_id = chunk_id(key, chunk.chunk_index);
        connection.execute(
            "
            INSERT INTO file_content_chunks (
                chunk_id, entry_key, chunk_index, start_byte, end_byte, start_line, end_line,
                content
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ",
            params![
                &chunk_id,
                key,
                i64_from_usize(chunk.chunk_index)?,
                i64::from(chunk.start_byte),
                i64::from(chunk.end_byte),
                i64::from(chunk.start_line),
                i64::from(chunk.end_line),
                &chunk.content,
            ],
        )?;
        connection.execute(
            "
            INSERT INTO file_content_search (
                chunk_id, entry_key, scope_id, root_id, path, relative_path, content
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                &chunk_id,
                key,
                &entry.scope_id,
                &entry.root_id,
                &entry.path,
                &entry.relative_path,
                &chunk.content,
            ],
        )?;
    }

    Ok(())
}

fn delete_chunks(connection: &Connection, key: &str) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM file_content_search WHERE entry_key = ?1",
        params![key],
    )?;
    connection.execute(
        "DELETE FROM file_content_chunks WHERE entry_key = ?1",
        params![key],
    )?;

    Ok(())
}

fn refresh_cursors(connection: &Connection, entry: &FileContentEntry) -> Result<(), StorageError> {
    for kind in IndexKind::ALL {
        let cursor_key = cursor_key(kind, &entry.scope_id, &entry.root_id, &entry.path);
        let cursor_status = content_cursor_status(kind);
        connection.execute(
            "
            INSERT INTO file_content_cursors (
                cursor_key, kind, scope_id, root_id, path, content_hash, indexed_graph_version,
                state, stale_reason, updated_at_ms
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(cursor_key) DO UPDATE SET
                content_hash = excluded.content_hash,
                indexed_graph_version = excluded.indexed_graph_version,
                state = excluded.state,
                stale_reason = excluded.stale_reason,
                updated_at_ms = excluded.updated_at_ms
            ",
            params![
                cursor_key,
                kind.as_str(),
                &entry.scope_id,
                &entry.root_id,
                &entry.path,
                &entry.content_hash,
                i64_from_u64(entry.graph_version)?,
                cursor_status.state.as_str(),
                cursor_status.stale_reason,
                i64_from_u64(entry.indexed_at_ms)?,
            ],
        )?;
    }

    Ok(())
}

struct ContentCursorStatus {
    state: IndexState,
    stale_reason: Option<&'static str>,
}

fn content_cursor_status(kind: IndexKind) -> ContentCursorStatus {
    match kind {
        IndexKind::Bm25 => ContentCursorStatus {
            state: IndexState::Fresh,
            stale_reason: None,
        },
        IndexKind::Semantic => ContentCursorStatus {
            state: IndexState::Paused,
            stale_reason: Some("file content semantic read model is not built"),
        },
        IndexKind::Vector => ContentCursorStatus {
            state: IndexState::Paused,
            stale_reason: Some("file content vector read model is not built"),
        },
    }
}

fn count_indexed_content_entries(
    connection: &Connection,
    scope_id: &str,
    root_id: &str,
) -> Result<usize, StorageError> {
    let count = connection.query_row(
        "
        SELECT COUNT(*)
        FROM file_content_entries
        WHERE scope_id = ?1 AND root_id = ?2 AND status = ?3
        ",
        params![scope_id, root_id, INDEXED_STATUS],
        |row| row.get::<_, i64>(0),
    )?;
    usize_from_i64(count)
}

fn count_stale_cursors(
    connection: &Connection,
    scope_id: &str,
    root_id: &str,
) -> Result<usize, StorageError> {
    let count = connection.query_row(
        "
        SELECT COUNT(*)
        FROM file_content_cursors
        WHERE scope_id = ?1 AND root_id = ?2 AND state = ?3
        ",
        params![scope_id, root_id, IndexState::Stale.as_str()],
        |row| row.get::<_, i64>(0),
    )?;
    usize_from_i64(count)
}

fn i64_from_u64(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| {
        StorageError::InvalidInput("file index numeric value exceeds SQLite range".to_owned())
    })
}

fn i64_from_usize(value: usize) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| {
        StorageError::InvalidInput("file content numeric value exceeds SQLite range".to_owned())
    })
}

fn usize_from_i64(value: i64) -> Result<usize, StorageError> {
    usize::try_from(value).map_err(|_| {
        StorageError::InvalidInput(
            "file content count is outside supported unsigned range".to_owned(),
        )
    })
}

#[cfg(test)]
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
fn parse_index_kind(value: &str) -> Result<IndexKind, rusqlite::Error> {
    match value {
        "bm25" => Ok(IndexKind::Bm25),
        "semantic" => Ok(IndexKind::Semantic),
        "vector" => Ok(IndexKind::Vector),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown index kind in file content cursor",
            )),
        )),
    }
}

#[cfg(test)]
fn parse_index_state(value: &str) -> Result<IndexState, rusqlite::Error> {
    match value {
        "fresh" => Ok(IndexState::Fresh),
        "stale" => Ok(IndexState::Stale),
        "failed" => Ok(IndexState::Failed),
        "paused" => Ok(IndexState::Paused),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown index state in file content cursor",
            )),
        )),
    }
}

#[cfg(test)]
mod mod_tests;
