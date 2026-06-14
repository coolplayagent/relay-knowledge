use std::{collections::BTreeSet, time::Instant};

use rusqlite::{Connection, ErrorCode, OptionalExtension, params, params_from_iter, types::Value};

use crate::{
    domain::{EvidenceSpan, IndexKind, IndexState},
    storage::{
        FileContentEntry, FileContentReadModelCursor, FileContentSearchHit,
        FileContentSearchRequest, FileIndexRoot, FileKnowledgeFactCandidate, StorageError,
    },
};

const INDEXED_STATUS: &str = "indexed";
const MISSING_STATUS: &str = "missing";
const USER_SOURCE_CONTENT_ROLE: &str = "user_source";

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS file_content_entries (
            entry_key TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            root_id TEXT NOT NULL,
            path TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            indexed_at_ms INTEGER NOT NULL,
            graph_version INTEGER NOT NULL,
            status TEXT NOT NULL,
            skipped_reason TEXT
        );

        CREATE INDEX IF NOT EXISTS file_content_entries_scope_root
            ON file_content_entries(scope_id, root_id, status);

        CREATE TABLE IF NOT EXISTS file_content_chunks (
            chunk_id TEXT PRIMARY KEY,
            entry_key TEXT NOT NULL,
            chunk_index INTEGER NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            content TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS file_content_chunks_entry
            ON file_content_chunks(entry_key, chunk_index);

        CREATE VIRTUAL TABLE IF NOT EXISTS file_content_search USING fts5(
            chunk_id UNINDEXED,
            entry_key UNINDEXED,
            scope_id UNINDEXED,
            root_id UNINDEXED,
            path,
            relative_path,
            content
        );

        CREATE TABLE IF NOT EXISTS file_content_cursors (
            cursor_key TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            root_id TEXT NOT NULL,
            path TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            indexed_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            stale_reason TEXT,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS file_content_cursors_scope_root
            ON file_content_cursors(scope_id, root_id, state);
        ",
    )?;

    Ok(())
}

#[derive(Default)]
pub(super) struct ContentReplacementCounts {
    pub(super) indexed_content_count: usize,
    pub(super) skipped_content_count: usize,
    pub(super) unchanged_content_count: usize,
    pub(super) stale_content_cursor_count: usize,
}

pub(super) struct ContentReplacementRequest<'a> {
    pub(super) scope_id: &'a str,
    pub(super) root_id: &'a str,
    pub(super) entries_len: usize,
    pub(super) observed_file_keys: &'a BTreeSet<String>,
    pub(super) processed_content_keys: &'a BTreeSet<String>,
    pub(super) content_entries: &'a [FileContentEntry],
    pub(super) file_scan_completed: bool,
    pub(super) content_scan_completed: bool,
    pub(super) now_ms: u64,
}

pub(super) fn replace_entries(
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
        counts.stale_content_cursor_count = counts
            .stale_content_cursor_count
            .saturating_add(mark_cursors_stale(connection, entry)?);
    }
    counts.indexed_content_count =
        count_indexed_content_entries(connection, request.scope_id, request.root_id)?;
    counts.stale_content_cursor_count =
        count_stale_cursors(connection, request.scope_id, request.root_id)?;

    Ok(counts)
}

pub(super) fn mark_root_unconfigured(
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

pub(super) fn search(
    connection: &Connection,
    request: FileContentSearchRequest,
    deadline: Instant,
) -> Result<Vec<FileContentSearchHit>, StorageError> {
    if Instant::now() >= deadline {
        return Err(query_timeout());
    }
    connection.progress_handler(1000, Some(move || Instant::now() >= deadline));
    let result = super::retry::retry_sqlite_transient(|| {
        if Instant::now() >= deadline {
            return Err(query_timeout());
        }
        search_with_progress_handler(connection, request.clone())
    });
    connection.progress_handler(0, None::<fn() -> bool>);

    match result {
        Err(StorageError::Sqlite(error)) if sqlite_interrupted(&error) => Err(query_timeout()),
        other => other,
    }
}

pub(super) fn cursors(
    connection: &Connection,
) -> Result<Vec<FileContentReadModelCursor>, StorageError> {
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
        Ok(FileContentReadModelCursor {
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

fn mark_cursors_stale(
    connection: &Connection,
    entry: &FileContentEntry,
) -> Result<usize, StorageError> {
    let mut count = 0usize;
    for kind in IndexKind::ALL {
        let cursor_key = cursor_key(kind, &entry.scope_id, &entry.root_id, &entry.path);
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
                i64_from_u64(entry.graph_version.saturating_sub(1))?,
                IndexState::Stale.as_str(),
                "file content changed; refresh derived read model",
                i64_from_u64(entry.indexed_at_ms)?,
            ],
        )?;
        count = count.saturating_add(1);
    }

    Ok(count)
}

fn search_with_progress_handler(
    connection: &Connection,
    request: FileContentSearchRequest,
) -> Result<Vec<FileContentSearchHit>, StorageError> {
    if request.authorized_roots.is_empty() {
        return Ok(Vec::new());
    }
    let query = fts_query(&request.query)?;
    let (authorized_roots_clause, authorized_root_params) =
        authorized_roots_clause(&request.authorized_roots);
    let sql = format!(
        "
        SELECT
            c.scope_id, c.root_id, c.path, c.relative_path, s.chunk_id, h.content,
            h.start_byte, h.end_byte, h.start_line, h.end_line, c.fingerprint,
            c.content_hash, c.indexed_at_ms, c.graph_version,
            COALESCE((
                SELECT MIN(cur.indexed_graph_version)
                FROM file_content_cursors cur
                WHERE cur.scope_id = c.scope_id
                  AND cur.root_id = c.root_id
                  AND cur.path = c.path
                  AND cur.content_hash = c.content_hash
            ), 0) AS indexed_graph_version,
            bm25(file_content_search) AS score
        FROM file_content_search s
        INNER JOIN file_content_entries c ON c.entry_key = s.entry_key
        INNER JOIN file_content_chunks h ON h.chunk_id = s.chunk_id
        WHERE file_content_search MATCH ?1
          AND c.status = 'indexed'
          AND (?2 IS NULL OR c.scope_id = ?2)
          AND (?3 IS NULL OR c.root_id = ?3)
          AND ({authorized_roots_clause})
        ORDER BY score ASC, c.path ASC, h.start_byte ASC
        LIMIT ?
        ",
    );
    let mut parameters = vec![
        Value::Text(query),
        optional_text_value(request.source_scope.as_deref()),
        optional_text_value(request.root_id.as_deref()),
    ];
    parameters.extend(authorized_root_params);
    parameters.push(Value::Integer(limit_i64(request.limit)?));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(parameters.iter()), |row| {
        let span = EvidenceSpan {
            start_byte: u32_from_sql(row.get::<_, i64>(6)?)?,
            end_byte: u32_from_sql(row.get::<_, i64>(7)?)?,
            start_line: u32_from_sql(row.get::<_, i64>(8)?)?,
            end_line: u32_from_sql(row.get::<_, i64>(9)?)?,
        };
        let scope_id: String = row.get(0)?;
        let root_id: String = row.get(1)?;
        let path: String = row.get(2)?;
        let chunk_id: String = row.get(4)?;
        let excerpt: String = row.get(5)?;
        let fingerprint: String = row.get(10)?;
        let content_hash: String = row.get(11)?;
        let freshness_cursor = format!("file-content:{scope_id}:{root_id}:{path}:{content_hash}");
        Ok(FileContentSearchHit {
            scope_id: scope_id.clone(),
            root_id,
            path: path.clone(),
            relative_path: row.get(3)?,
            chunk_id: chunk_id.clone(),
            content_role: USER_SOURCE_CONTENT_ROLE.to_owned(),
            excerpt: excerpt.clone(),
            span,
            fingerprint: fingerprint.clone(),
            content_hash,
            indexed_at_ms: u64_from_sql(row.get::<_, i64>(12)?)?,
            graph_version: u64_from_sql(row.get::<_, i64>(13)?)?,
            indexed_graph_version: u64_from_sql(row.get::<_, i64>(14)?)?,
            freshness_cursor: freshness_cursor.clone(),
            rank: 0,
            score: row.get(15)?,
            ranking_signals: vec!["file_content_bm25".to_owned()],
            fact_candidates: fact_candidates_for_chunk(
                &scope_id,
                &path,
                &chunk_id,
                &excerpt,
                span,
                &fingerprint,
                &freshness_cursor,
            ),
        })
    })?;
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

fn query_timeout() -> StorageError {
    StorageError::InvalidInput("file content query timed out".to_owned())
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

    Ok(terms
        .into_iter()
        .map(|term| format!("content:{term}"))
        .collect::<Vec<_>>()
        .join(" AND "))
}

fn optional_text_value(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |text| Value::Text(text.to_owned()))
}

fn authorized_roots_clause(roots: &[FileIndexRoot]) -> (String, Vec<Value>) {
    let mut clauses = Vec::new();
    let mut parameters = Vec::new();
    for root in roots {
        clauses.push("(c.scope_id = ? AND c.root_id = ?)".to_owned());
        parameters.push(Value::Text(root.scope_id.clone()));
        parameters.push(Value::Text(root.root_id.clone()));
    }

    (clauses.join(" OR "), parameters)
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

fn u32_from_sql(value: i64) -> Result<u32, rusqlite::Error> {
    u32::try_from(value).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "integer outside u32 range in file content span",
            )),
        )
    })
}

fn chunk_id(entry_key: &str, chunk_index: usize) -> String {
    format!(
        "file-content-chunk:{:016x}:{chunk_index}",
        stable_hash64(entry_key.as_bytes())
    )
}

fn cursor_key(kind: IndexKind, scope_id: &str, root_id: &str, path: &str) -> String {
    format!("{}\n{scope_id}\n{root_id}\n{path}", kind.as_str())
}

fn entry_key(scope_id: &str, root_id: &str, path: &str) -> String {
    format!("{scope_id}\n{root_id}\n{path}")
}

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

fn fact_candidates_for_chunk(
    source_scope: &str,
    source_path: &str,
    chunk_id: &str,
    content: &str,
    span: EvidenceSpan,
    fingerprint: &str,
    freshness_cursor: &str,
) -> Vec<FileKnowledgeFactCandidate> {
    content
        .lines()
        .filter_map(|line| fact_candidate_for_line(source_scope, source_path, chunk_id, line))
        .take(8)
        .map(
            |(kind, subject, predicate, object)| FileKnowledgeFactCandidate {
                candidate_id: format!(
                    "file-fact:{:016x}",
                    stable_hash64(
                        format!("{chunk_id}:{subject}:{predicate}:{object:?}").as_bytes()
                    )
                ),
                kind,
                subject,
                predicate,
                object,
                confidence_basis_points: 6500,
                status: "candidate".to_owned(),
                source_scope: source_scope.to_owned(),
                source_path: source_path.to_owned(),
                span,
                fingerprint: fingerprint.to_owned(),
                freshness_cursor: freshness_cursor.to_owned(),
            },
        )
        .collect()
}

fn fact_candidate_for_line(
    source_scope: &str,
    source_path: &str,
    chunk_id: &str,
    line: &str,
) -> Option<(String, String, String, Option<String>)> {
    let line = line.trim().trim_matches('-').trim();
    if let Some(heading) = line.strip_prefix('#') {
        let heading = heading.trim_matches('#').trim();
        if !heading.is_empty() {
            return Some((
                "claim".to_owned(),
                source_path.to_owned(),
                "has_heading".to_owned(),
                Some(heading.to_owned()),
            ));
        }
    }
    for delimiter in [":", "="] {
        if let Some((key, value)) = line.split_once(delimiter) {
            let key = key.trim();
            let value = value.trim();
            if key.len() >= 2 && !value.is_empty() {
                return Some((
                    "claim".to_owned(),
                    source_path.to_owned(),
                    key.to_ascii_lowercase().replace(' ', "_"),
                    Some(value.to_owned()),
                ));
            }
        }
    }
    for phrase in [" depends on ", " uses ", " references "] {
        if let Some((left, right)) = line.split_once(phrase) {
            let left = left.trim();
            let right = right.trim().trim_end_matches('.');
            if !left.is_empty() && !right.is_empty() {
                return Some((
                    "relation".to_owned(),
                    left.to_owned(),
                    phrase.trim().replace(' ', "_"),
                    Some(right.to_owned()),
                ));
            }
        }
    }
    if line.contains("ignore previous") || line.contains("system prompt") {
        return Some((
            "claim".to_owned(),
            source_scope.to_owned(),
            "contains_untrusted_instruction_text".to_owned(),
            Some(chunk_id.to_owned()),
        ));
    }

    None
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

#[cfg(test)]
#[path = "file_index_content_tests.rs"]
mod tests;
