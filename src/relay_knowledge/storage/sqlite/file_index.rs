use std::{collections::BTreeSet, time::Instant};

use rusqlite::{Connection, ErrorCode, OptionalExtension, params};

use crate::storage::{
    FileIndexDiagnostics, FileIndexEntry, FileIndexRootStatus, FileIndexRootUpdate, FileSearchHit,
    FileSearchRequest, StorageError,
};

const INDEXED_STATUS: &str = "indexed";
const MISSING_STATUS: &str = "missing";

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS file_index_roots (
            scope_id TEXT NOT NULL,
            root_id TEXT NOT NULL,
            root_path TEXT NOT NULL,
            indexed_file_count INTEGER NOT NULL DEFAULT 0,
            missing_file_count INTEGER NOT NULL DEFAULT 0,
            scan_error_count INTEGER NOT NULL DEFAULT 0,
            truncated INTEGER NOT NULL DEFAULT 0,
            last_indexed_at_ms INTEGER,
            last_error TEXT,
            PRIMARY KEY (scope_id, root_id)
        );

        CREATE TABLE IF NOT EXISTS file_index_entries (
            entry_key TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            root_id TEXT NOT NULL,
            path TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            file_name TEXT NOT NULL,
            extension TEXT,
            parent_dir TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            modified_at_ms INTEGER NOT NULL,
            fingerprint TEXT NOT NULL,
            status TEXT NOT NULL,
            last_error TEXT,
            indexed_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS file_index_entries_scope_root
            ON file_index_entries(scope_id, root_id, status);

        CREATE VIRTUAL TABLE IF NOT EXISTS file_index_search USING fts5(
            entry_key UNINDEXED,
            scope_id UNINDEXED,
            root_id UNINDEXED,
            path,
            relative_path,
            file_name,
            extension,
            parent_dir
        );
        ",
    )?;

    Ok(())
}

pub(super) fn replace_root(
    connection: &mut Connection,
    update: FileIndexRootUpdate,
) -> Result<FileIndexRootStatus, StorageError> {
    let transaction = connection.transaction()?;
    let existing = existing_entry_keys(&transaction, &update.root.scope_id, &update.root.root_id)?;
    let mut current = BTreeSet::new();

    for entry in &update.entries {
        current.insert(entry_key(&entry.scope_id, &entry.root_id, &entry.path));
    }

    if update.scan_error_count == 0 && !update.truncated {
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

    for entry in update.entries {
        upsert_entry(&transaction, entry, update.now_ms)?;
    }

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
        },
        update.now_ms,
        update.last_error.as_deref(),
    )?;
    transaction.commit()?;

    Ok(status)
}

pub(super) fn mark_unconfigured_roots(
    connection: &mut Connection,
    active_roots: Vec<crate::storage::FileIndexRoot>,
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

pub(super) fn search(
    connection: &Connection,
    request: FileSearchRequest,
    deadline: Instant,
) -> Result<Vec<FileSearchHit>, StorageError> {
    if Instant::now() >= deadline {
        return Err(file_query_timeout());
    }
    connection.progress_handler(1000, Some(move || Instant::now() >= deadline));
    let result = search_with_progress_handler(connection, request);
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

pub(super) fn diagnostics(connection: &Connection) -> Result<FileIndexDiagnostics, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT scope_id, root_id, root_path, indexed_file_count, missing_file_count,
               scan_error_count, truncated, last_indexed_at_ms, last_error
        FROM file_index_roots
        ORDER BY scope_id ASC, root_id ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(FileIndexRootStatus {
            scope_id: row.get(0)?,
            root_id: row.get(1)?,
            root_path: row.get(2)?,
            indexed_file_count: row.get(3)?,
            missing_file_count: row.get(4)?,
            scan_error_count: row.get(5)?,
            truncated: row.get(6)?,
            last_indexed_at_ms: row.get(7)?,
            last_error: row.get(8)?,
        })
    })?;
    let roots = rows.collect::<Result<Vec<_>, _>>()?;

    Ok(FileIndexDiagnostics {
        root_count: roots.len(),
        indexed_file_count: roots
            .iter()
            .map(|root| root.indexed_file_count)
            .sum::<usize>(),
        missing_file_count: roots
            .iter()
            .map(|root| root.missing_file_count)
            .sum::<usize>(),
        scan_error_count: roots
            .iter()
            .map(|root| root.scan_error_count)
            .sum::<usize>(),
        truncated_root_count: roots.iter().filter(|root| root.truncated).count(),
        roots,
    })
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

struct RootStatusCounts {
    indexed_file_count: usize,
    missing_file_count: usize,
    scan_error_count: usize,
    truncated: bool,
}

fn write_root_status(
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
            scan_error_count, truncated, last_indexed_at_ms, last_error
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(scope_id, root_id) DO UPDATE SET
            root_path = excluded.root_path,
            indexed_file_count = excluded.indexed_file_count,
            missing_file_count = excluded.missing_file_count,
            scan_error_count = excluded.scan_error_count,
            truncated = excluded.truncated,
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
            now_ms,
            last_error,
        ],
    )?;

    root_status(connection, &root.scope_id, &root.root_id)?
        .ok_or_else(|| StorageError::InvalidInput("file index root was not stored".to_owned()))
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
    let Some(mut status) = root_status(connection, scope_id, root_id)? else {
        return Ok(());
    };
    status.indexed_file_count = count_entries(connection, scope_id, root_id, INDEXED_STATUS)?;
    status.missing_file_count = count_entries(connection, scope_id, root_id, MISSING_STATUS)?;
    status.scan_error_count = status.scan_error_count.saturating_add(1);
    status.last_error = Some("root no longer configured".to_owned());
    let last_error = status.last_error.clone();
    let root = crate::storage::FileIndexRoot {
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
        },
        now_ms,
        last_error.as_deref(),
    )?;

    Ok(())
}

fn root_status(
    connection: &Connection,
    scope_id: &str,
    root_id: &str,
) -> Result<Option<FileIndexRootStatus>, StorageError> {
    connection
        .query_row(
            "
            SELECT scope_id, root_id, root_path, indexed_file_count, missing_file_count,
                   scan_error_count, truncated, last_indexed_at_ms, last_error
            FROM file_index_roots
            WHERE scope_id = ?1 AND root_id = ?2
            ",
            params![scope_id, root_id],
            |row| {
                Ok(FileIndexRootStatus {
                    scope_id: row.get(0)?,
                    root_id: row.get(1)?,
                    root_path: row.get(2)?,
                    indexed_file_count: row.get(3)?,
                    missing_file_count: row.get(4)?,
                    scan_error_count: row.get(5)?,
                    truncated: row.get(6)?,
                    last_indexed_at_ms: row.get(7)?,
                    last_error: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::from)
}

fn count_entries(
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

fn i64_from_u64(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| {
        StorageError::InvalidInput("file index numeric value exceeds SQLite range".to_owned())
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
mod tests {
    use super::*;
    use crate::storage::FileIndexRoot;

    #[test]
    fn replace_search_and_diagnostics_round_trip() {
        let mut connection = open_connection();
        let first = update(
            vec![
                entry(
                    "/workspace/docs/quarterly-design.pdf",
                    "docs/quarterly-design.pdf",
                    "pdf",
                ),
                entry(
                    "/workspace/docs/quarterly-notes.md",
                    "docs/quarterly-notes.md",
                    "md",
                ),
            ],
            10,
        );
        let status = replace_root(&mut connection, first).expect("root should be indexed");
        assert_eq!(status.indexed_file_count, 2);
        assert_eq!(status.missing_file_count, 0);
        assert_eq!(status.last_indexed_at_ms, Some(10));

        let hits = search(
            &connection,
            FileSearchRequest {
                query: "quarterly design pdf".to_owned(),
                source_scope: Some("local-files".to_owned()),
                root_id: Some("root-a".to_owned()),
                limit: 5,
                timeout_ms: 750,
            },
            deadline(),
        )
        .expect("indexed files should be searchable");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rank, 1);
        assert_eq!(hits[0].file_name, "quarterly-design.pdf");
        assert_eq!(hits[0].extension.as_deref(), Some("pdf"));
        assert_eq!(hits[0].status, INDEXED_STATUS);

        let diagnostics = diagnostics(&connection).expect("diagnostics should load");
        assert_eq!(diagnostics.root_count, 1);
        assert_eq!(diagnostics.indexed_file_count, 2);
        assert_eq!(diagnostics.missing_file_count, 0);

        let second = update(
            vec![entry(
                "/workspace/docs/quarterly-design.pdf",
                "docs/quarterly-design.pdf",
                "pdf",
            )],
            20,
        );
        let status = replace_root(&mut connection, second).expect("root should update");
        assert_eq!(status.indexed_file_count, 1);
        assert_eq!(status.missing_file_count, 1);

        let removed = search(
            &connection,
            FileSearchRequest {
                query: "quarterly notes".to_owned(),
                source_scope: Some("local-files".to_owned()),
                root_id: Some("root-a".to_owned()),
                limit: 5,
                timeout_ms: 750,
            },
            deadline(),
        )
        .expect("query should run");
        assert!(removed.is_empty());
    }

    #[test]
    fn failed_scan_preserves_previous_indexed_entries() {
        let mut connection = open_connection();
        replace_root(
            &mut connection,
            update(
                vec![
                    entry("/workspace/docs/keep.pdf", "docs/keep.pdf", "pdf"),
                    entry("/workspace/docs/older.txt", "docs/older.txt", "txt"),
                ],
                10,
            ),
        )
        .expect("initial root should be indexed");

        let status = replace_root(
            &mut connection,
            FileIndexRootUpdate {
                root: root(),
                entries: vec![entry("/workspace/docs/keep.pdf", "docs/keep.pdf", "pdf")],
                scan_error_count: 1,
                truncated: false,
                last_error: Some("permission denied".to_owned()),
                now_ms: 20,
            },
        )
        .expect("failed scan should update diagnostics");
        assert_eq!(status.indexed_file_count, 2);
        assert_eq!(status.missing_file_count, 0);
        assert_eq!(status.last_error.as_deref(), Some("permission denied"));

        let hits = search(
            &connection,
            FileSearchRequest {
                query: "keep pdf".to_owned(),
                source_scope: Some("local-files".to_owned()),
                root_id: Some("root-a".to_owned()),
                limit: 5,
                timeout_ms: 750,
            },
            deadline(),
        )
        .expect("previous entries should remain searchable");
        assert_eq!(hits.len(), 1);

        let older_hits = search(
            &connection,
            FileSearchRequest {
                query: "older txt".to_owned(),
                source_scope: Some("local-files".to_owned()),
                root_id: Some("root-a".to_owned()),
                limit: 5,
                timeout_ms: 750,
            },
            deadline(),
        )
        .expect("unobserved entries should survive partial scan errors");
        assert_eq!(older_hits.len(), 1);
    }

    #[test]
    fn truncated_scan_preserves_unobserved_entries() {
        let mut connection = open_connection();
        replace_root(
            &mut connection,
            update(
                vec![
                    entry("/workspace/docs/first.pdf", "docs/first.pdf", "pdf"),
                    entry("/workspace/docs/second.pdf", "docs/second.pdf", "pdf"),
                ],
                10,
            ),
        )
        .expect("initial root should be indexed");

        let status = replace_root(
            &mut connection,
            FileIndexRootUpdate {
                root: root(),
                entries: vec![entry("/workspace/docs/first.pdf", "docs/first.pdf", "pdf")],
                scan_error_count: 0,
                truncated: true,
                last_error: None,
                now_ms: 20,
            },
        )
        .expect("truncated scan should update diagnostics");
        assert_eq!(status.indexed_file_count, 2);
        assert_eq!(status.missing_file_count, 0);
        assert!(status.truncated);

        let hits = search(
            &connection,
            FileSearchRequest {
                query: "second pdf".to_owned(),
                source_scope: Some("local-files".to_owned()),
                root_id: Some("root-a".to_owned()),
                limit: 5,
                timeout_ms: 750,
            },
            deadline(),
        )
        .expect("unobserved entries should survive truncated scans");
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn unconfigured_roots_are_removed_from_search() {
        let mut connection = open_connection();
        replace_root(
            &mut connection,
            update(
                vec![entry(
                    "/workspace/docs/retired.pdf",
                    "docs/retired.pdf",
                    "pdf",
                )],
                10,
            ),
        )
        .expect("initial root should be indexed");

        let diagnostics = mark_unconfigured_roots(&mut connection, Vec::new(), 20)
            .expect("unconfigured roots should be marked");
        assert_eq!(diagnostics.indexed_file_count, 0);
        assert_eq!(diagnostics.missing_file_count, 1);
        assert_eq!(diagnostics.scan_error_count, 1);

        let hits = search(
            &connection,
            FileSearchRequest {
                query: "retired pdf".to_owned(),
                source_scope: Some("local-files".to_owned()),
                root_id: Some("root-a".to_owned()),
                limit: 5,
                timeout_ms: 750,
            },
            deadline(),
        )
        .expect("query should run");
        assert!(hits.is_empty());
    }

    #[test]
    fn search_validation_and_numeric_boundaries_are_explicit() {
        let connection = open_connection();
        let error = search(
            &connection,
            FileSearchRequest {
                query: "!!!".to_owned(),
                source_scope: None,
                root_id: None,
                limit: 10,
                timeout_ms: 750,
            },
            deadline(),
        )
        .expect_err("query without terms should fail");
        assert!(error.to_string().contains("searchable term"));
        assert!(limit_i64(usize::MAX).is_err());
        assert!(i64_from_u64(u64::MAX).is_err());
        assert!(u64_from_sql(-1).is_err());
        assert_eq!(
            u64_from_sql(42).expect("positive integer should convert"),
            42
        );
    }

    fn open_connection() -> Connection {
        let connection = Connection::open_in_memory().expect("connection should open");
        initialize_schema(&connection).expect("schema should initialize");
        connection
    }

    fn update(entries: Vec<FileIndexEntry>, now_ms: u64) -> FileIndexRootUpdate {
        FileIndexRootUpdate {
            root: root(),
            entries,
            scan_error_count: 0,
            truncated: false,
            last_error: None,
            now_ms,
        }
    }

    fn root() -> FileIndexRoot {
        FileIndexRoot {
            scope_id: "local-files".to_owned(),
            root_id: "root-a".to_owned(),
            root_path: "/workspace".to_owned(),
        }
    }

    fn deadline() -> Instant {
        Instant::now() + std::time::Duration::from_millis(750)
    }

    fn entry(path: &str, relative_path: &str, extension: &str) -> FileIndexEntry {
        let file_name = path
            .rsplit('/')
            .next()
            .expect("path should include a file name")
            .to_owned();

        FileIndexEntry {
            scope_id: "local-files".to_owned(),
            root_id: "root-a".to_owned(),
            path: path.to_owned(),
            relative_path: relative_path.to_owned(),
            file_name,
            extension: Some(extension.to_owned()),
            parent_dir: "/workspace/docs".to_owned(),
            size_bytes: 128,
            modified_at_ms: 1,
            fingerprint: "128:1".to_owned(),
        }
    }
}
