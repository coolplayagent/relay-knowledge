//! Authorized, deadline-aware FTS content search and fact projection.

use std::time::Instant;

use rusqlite::{Connection, ErrorCode, params_from_iter, types::Value};

use crate::{
    domain::EvidenceSpan,
    storage::{FileContentSearchHit, FileContentSearchRequest, FileIndexRoot, StorageError},
};

use super::fact_candidates;

const USER_SOURCE_CONTENT_ROLE: &str = "user_source";

pub(in crate::storage::sqlite) fn search(
    connection: &Connection,
    request: FileContentSearchRequest,
    deadline: Instant,
) -> Result<Vec<FileContentSearchHit>, StorageError> {
    if Instant::now() >= deadline {
        return Err(query_timeout());
    }
    connection.progress_handler(1000, Some(move || Instant::now() >= deadline));
    let result = super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
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
            fact_candidates: fact_candidates::for_chunk(
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

#[cfg(test)]
mod mod_tests;
