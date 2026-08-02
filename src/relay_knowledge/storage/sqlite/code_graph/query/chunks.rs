use rusqlite::{Connection, Row, params};

use crate::{
    domain::{CodeChunkRecord, CodeExtractionMetadata},
    storage::{CodeChunkSearchRequest, StorageError},
};

use super::common::{RawRange, normalize_filter, optional_extraction, parse_scope, validate_limit};

pub(in crate::storage::sqlite) fn search_chunks(
    connection: &mut Connection,
    request: CodeChunkSearchRequest,
) -> Result<Vec<CodeChunkRecord>, StorageError> {
    validate_limit("code chunk search limit", request.limit)?;
    let scope = normalize_filter("source_scope", request.source_scope)?;
    let path = normalize_filter("code_path", request.path)?;
    let query = normalize_filter("code_query", request.query)?;
    let mut statement = connection.prepare(
        "
        SELECT source_scope, path, chunk_id, content, start_byte, end_byte,
               start_line, end_line, grammar_version, query_name, query_version,
               node_kind, capture_kind
        FROM code_chunks
        WHERE (?1 IS NULL OR source_scope = ?1)
          AND (?2 IS NULL OR path = ?2)
          AND (?3 IS NULL OR lower(content) LIKE '%' || lower(?3) || '%')
          AND created_graph_version <= ?4
        ORDER BY created_graph_version DESC, source_scope ASC, path ASC,
                 start_line ASC, chunk_id ASC
        LIMIT ?5
        ",
    )?;
    let rows = statement.query_map(
        params![
            scope.as_deref(),
            path.as_deref(),
            query.as_deref(),
            request.graph_version.get(),
            request.limit
        ],
        row_to_chunk,
    )?;
    let raw_chunks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    raw_chunks
        .into_iter()
        .map(|raw| {
            let linked_symbol_ids =
                linked_symbols(connection, &raw.source_scope, &raw.path, &raw.chunk_id)?;
            raw.into_record(linked_symbol_ids)
        })
        .collect()
}

struct RawChunk {
    source_scope: String,
    path: String,
    chunk_id: String,
    content: String,
    range: RawRange,
    extraction: Option<CodeExtractionMetadata>,
}

impl RawChunk {
    fn into_record(self, linked_symbol_ids: Vec<String>) -> Result<CodeChunkRecord, StorageError> {
        CodeChunkRecord::new(
            self.chunk_id,
            parse_scope(self.source_scope)?,
            self.path,
            self.content,
            self.range.into_range()?,
            linked_symbol_ids,
            self.extraction,
        )
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
    }
}

fn row_to_chunk(row: &Row<'_>) -> rusqlite::Result<RawChunk> {
    Ok(RawChunk {
        source_scope: row.get(0)?,
        path: row.get(1)?,
        chunk_id: row.get(2)?,
        content: row.get(3)?,
        range: RawRange::from_row(row, 4)?,
        extraction: optional_extraction(
            row.get(8)?,
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
            row.get(12)?,
        ),
    })
}

fn linked_symbols(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    chunk_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT symbol_id
        FROM code_chunk_symbols
        WHERE source_scope = ?1 AND path = ?2 AND chunk_id = ?3
        ORDER BY symbol_id ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope, path, chunk_id], |row| row.get(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "chunks_tests.rs"]
mod chunks_tests;
