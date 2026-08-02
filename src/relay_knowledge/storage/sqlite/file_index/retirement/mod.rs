//! Transactional retirement of file-index roots that are no longer configured.

use std::collections::BTreeSet;

use rusqlite::{Connection, params};

use crate::storage::{FileIndexDiagnostics, FileIndexRoot, StorageError};

use super::{
    content,
    diagnostics::{diagnostics, root_status},
    root_update::{
        INDEXED_STATUS, MISSING_STATUS, RootStatusCounts, count_entries, write_root_status,
    },
};

pub(in crate::storage::sqlite) fn mark_unconfigured_roots(
    connection: &mut Connection,
    active_roots: Vec<FileIndexRoot>,
    now_ms: u64,
) -> Result<FileIndexDiagnostics, StorageError> {
    let active = active_roots
        .into_iter()
        .map(|root| (root.scope_id, root.root_id))
        .collect::<BTreeSet<_>>();
    let stored_roots = {
        let mut statement = connection.prepare("SELECT scope_id, root_id FROM file_index_roots")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let transaction = connection.transaction()?;
    for (scope_id, root_id) in stored_roots {
        if !active.contains(&(scope_id.clone(), root_id.clone())) {
            mark_root_unconfigured(&transaction, &scope_id, &root_id, now_ms)?;
        }
    }
    transaction.commit()?;

    diagnostics(connection)
}

fn mark_root_unconfigured(
    connection: &Connection,
    scope_id: &str,
    root_id: &str,
    now_ms: u64,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE file_index_entries
        SET status = ?3, last_error = ?4, indexed_at_ms = ?5
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
        DELETE FROM file_index_search
        WHERE entry_key IN (
            SELECT entry_key FROM file_index_entries
            WHERE scope_id = ?1 AND root_id = ?2
        )
        ",
        params![scope_id, root_id],
    )?;
    content::mark_root_unconfigured(connection, scope_id, root_id, now_ms)?;
    let Some(mut status) = root_status(connection, scope_id, root_id)? else {
        return Ok(());
    };
    status.indexed_file_count = count_entries(connection, scope_id, root_id, INDEXED_STATUS)?;
    status.missing_file_count = count_entries(connection, scope_id, root_id, MISSING_STATUS)?;
    status.scan_error_count = status.scan_error_count.saturating_add(1);
    status.last_error = Some("root no longer configured".to_owned());
    let last_error = status.last_error.clone();
    let root = FileIndexRoot {
        scope_id: status.scope_id,
        root_id: status.root_id,
        root_path: status.root_path,
    };
    write_root_status(
        connection,
        &root,
        RootStatusCounts {
            indexed_file_count: status.indexed_file_count,
            missing_file_count: status.missing_file_count,
            scan_error_count: status.scan_error_count,
            truncated: status.truncated,
            content_truncated: false,
            content_read_error_count: 0,
            indexed_content_count: 0,
            skipped_content_count: 0,
            unchanged_content_count: 0,
            stale_content_cursor_count: 0,
        },
        now_ms,
        last_error.as_deref(),
    )?;

    Ok(())
}

#[cfg(test)]
mod mod_tests;
