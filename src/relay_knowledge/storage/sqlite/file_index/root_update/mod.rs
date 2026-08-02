//! Transactional file-metadata replacement and root-status publication.

use std::collections::BTreeSet;

use rusqlite::{Connection, params};

use crate::storage::{FileIndexEntry, FileIndexRootStatus, FileIndexRootUpdate, StorageError};

use super::{content, diagnostics::root_status};

pub(super) const INDEXED_STATUS: &str = "indexed";
pub(super) const MISSING_STATUS: &str = "missing";

pub(in crate::storage::sqlite) fn replace_root(
    connection: &mut Connection,
    update: FileIndexRootUpdate,
) -> Result<FileIndexRootStatus, StorageError> {
    let transaction = connection.transaction()?;
    let existing = existing_entry_keys(&transaction, &update.root.scope_id, &update.root.root_id)?;
    let mut current = BTreeSet::new();

    for entry in &update.entries {
        current.insert(entry_key(&entry.scope_id, &entry.root_id, &entry.path));
    }

    let file_scan_completed = update.scan_error_count == 0 && !update.truncated;
    let content_scan_completed =
        file_scan_completed && !update.content_truncated && update.content_read_error_count == 0;

    if file_scan_completed {
        for key in existing.difference(&current) {
            transaction.execute(
                "UPDATE file_index_entries
                 SET status = ?2, last_error = ?3, indexed_at_ms = ?4
                 WHERE entry_key = ?1",
                params![
                    key,
                    MISSING_STATUS,
                    "not observed during latest scan",
                    update.now_ms
                ],
            )?;
            transaction.execute(
                "DELETE FROM file_index_search WHERE entry_key = ?1",
                params![key],
            )?;
        }
    }

    for entry in &update.entries {
        upsert_entry(&transaction, entry.clone(), update.now_ms)?;
    }
    let processed_content_keys = update
        .processed_content_paths
        .iter()
        .map(|path| entry_key(&update.root.scope_id, &update.root.root_id, path))
        .collect::<BTreeSet<_>>();
    let content_counts = content::replace_entries(
        &transaction,
        content::ContentReplacementRequest {
            scope_id: &update.root.scope_id,
            root_id: &update.root.root_id,
            entries_len: update.entries.len(),
            observed_file_keys: &current,
            processed_content_keys: &processed_content_keys,
            content_entries: &update.content_entries,
            file_scan_completed,
            content_scan_completed,
            now_ms: update.now_ms,
        },
    )?;

    let indexed_file_count = count_entries(
        &transaction,
        &update.root.scope_id,
        &update.root.root_id,
        INDEXED_STATUS,
    )?;
    let missing_file_count = count_entries(
        &transaction,
        &update.root.scope_id,
        &update.root.root_id,
        MISSING_STATUS,
    )?;
    let status = write_root_status(
        &transaction,
        &update.root,
        RootStatusCounts {
            indexed_file_count,
            missing_file_count,
            scan_error_count: update.scan_error_count,
            truncated: update.truncated,
            content_truncated: update.content_truncated,
            content_read_error_count: update.content_read_error_count,
            indexed_content_count: content_counts.indexed_content_count,
            skipped_content_count: content_counts.skipped_content_count,
            unchanged_content_count: content_counts.unchanged_content_count,
            stale_content_cursor_count: content_counts.stale_content_cursor_count,
        },
        update.now_ms,
        update.last_error.as_deref(),
    )?;
    transaction.commit()?;

    Ok(status)
}

fn existing_entry_keys(
    connection: &Connection,
    scope_id: &str,
    root_id: &str,
) -> Result<BTreeSet<String>, StorageError> {
    let mut statement = connection
        .prepare("SELECT entry_key FROM file_index_entries WHERE scope_id = ?1 AND root_id = ?2")?;
    let rows = statement.query_map(params![scope_id, root_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(StorageError::from)
}

fn upsert_entry(
    connection: &Connection,
    entry: FileIndexEntry,
    now_ms: u64,
) -> Result<(), StorageError> {
    let key = entry_key(&entry.scope_id, &entry.root_id, &entry.path);
    connection.execute(
        "
        INSERT INTO file_index_entries (
            entry_key, scope_id, root_id, path, relative_path, file_name, extension,
            parent_dir, size_bytes, modified_at_ms, fingerprint, status, last_error,
            indexed_at_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL, ?13)
        ON CONFLICT(entry_key) DO UPDATE SET
            path = excluded.path,
            relative_path = excluded.relative_path,
            file_name = excluded.file_name,
            extension = excluded.extension,
            parent_dir = excluded.parent_dir,
            size_bytes = excluded.size_bytes,
            modified_at_ms = excluded.modified_at_ms,
            fingerprint = excluded.fingerprint,
            status = excluded.status,
            last_error = excluded.last_error,
            indexed_at_ms = excluded.indexed_at_ms
        ",
        params![
            &key,
            &entry.scope_id,
            &entry.root_id,
            &entry.path,
            &entry.relative_path,
            &entry.file_name,
            entry.extension.as_deref(),
            &entry.parent_dir,
            i64_from_u64(entry.size_bytes)?,
            i64_from_u64(entry.modified_at_ms)?,
            &entry.fingerprint,
            INDEXED_STATUS,
            now_ms,
        ],
    )?;
    connection.execute(
        "DELETE FROM file_index_search WHERE entry_key = ?1",
        params![&key],
    )?;
    connection.execute(
        "
        INSERT INTO file_index_search (
            entry_key, scope_id, root_id, path, relative_path, file_name, extension, parent_dir
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            &key,
            &entry.scope_id,
            &entry.root_id,
            &entry.path,
            &entry.relative_path,
            &entry.file_name,
            entry.extension.as_deref().unwrap_or_default(),
            &entry.parent_dir,
        ],
    )?;

    Ok(())
}

pub(super) struct RootStatusCounts {
    pub(super) indexed_file_count: usize,
    pub(super) missing_file_count: usize,
    pub(super) scan_error_count: usize,
    pub(super) truncated: bool,
    pub(super) content_truncated: bool,
    pub(super) content_read_error_count: usize,
    pub(super) indexed_content_count: usize,
    pub(super) skipped_content_count: usize,
    pub(super) unchanged_content_count: usize,
    pub(super) stale_content_cursor_count: usize,
}

pub(super) fn write_root_status(
    connection: &Connection,
    root: &crate::storage::FileIndexRoot,
    counts: RootStatusCounts,
    now_ms: u64,
    last_error: Option<&str>,
) -> Result<FileIndexRootStatus, StorageError> {
    connection.execute(
        "
        INSERT INTO file_index_roots (
            scope_id, root_id, root_path, indexed_file_count, missing_file_count,
            scan_error_count, truncated, content_truncated, content_read_error_count,
            indexed_content_count, skipped_content_count, unchanged_content_count,
            stale_content_cursor_count, last_indexed_at_ms, last_error
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ON CONFLICT(scope_id, root_id) DO UPDATE SET
            root_path = excluded.root_path,
            indexed_file_count = excluded.indexed_file_count,
            missing_file_count = excluded.missing_file_count,
            scan_error_count = excluded.scan_error_count,
            truncated = excluded.truncated,
            content_truncated = excluded.content_truncated,
            content_read_error_count = excluded.content_read_error_count,
            indexed_content_count = excluded.indexed_content_count,
            skipped_content_count = excluded.skipped_content_count,
            unchanged_content_count = excluded.unchanged_content_count,
            stale_content_cursor_count = excluded.stale_content_cursor_count,
            last_indexed_at_ms = excluded.last_indexed_at_ms,
            last_error = excluded.last_error
        ",
        params![
            &root.scope_id,
            &root.root_id,
            &root.root_path,
            counts.indexed_file_count,
            counts.missing_file_count,
            counts.scan_error_count,
            counts.truncated,
            counts.content_truncated,
            counts.content_read_error_count,
            counts.indexed_content_count,
            counts.skipped_content_count,
            counts.unchanged_content_count,
            counts.stale_content_cursor_count,
            now_ms,
            last_error,
        ],
    )?;

    root_status(connection, &root.scope_id, &root.root_id)?
        .ok_or_else(|| StorageError::InvalidInput("file index root was not stored".to_owned()))
}

pub(super) fn count_entries(
    connection: &Connection,
    scope_id: &str,
    root_id: &str,
    status: &str,
) -> Result<usize, StorageError> {
    let count = connection.query_row(
        "
        SELECT COUNT(*)
        FROM file_index_entries
        WHERE scope_id = ?1 AND root_id = ?2 AND status = ?3
        ",
        params![scope_id, root_id, status],
        |row| row.get::<_, usize>(0),
    )?;

    Ok(count)
}

fn entry_key(scope_id: &str, root_id: &str, path: &str) -> String {
    format!("{scope_id}\n{root_id}\n{path}")
}

fn i64_from_u64(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| {
        StorageError::InvalidInput("file index numeric value exceeds SQLite range".to_owned())
    })
}

#[cfg(test)]
mod mod_tests;
