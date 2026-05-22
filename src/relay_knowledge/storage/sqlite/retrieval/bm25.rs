use std::{thread, time::Duration};

use rusqlite::{Connection, params};

use crate::storage::{GraphSearchRequest, StorageError};

const GRAPH_BM25_QUERY_RETRY_DELAYS_MS: [u64; 3] = [5, 15, 45];

pub(super) struct RawBm25Row {
    pub(super) document_id: String,
    pub(super) document_kind: String,
    pub(super) evidence_id: String,
    pub(super) parent_evidence_id: Option<String>,
    pub(super) modality: String,
    pub(super) source_scope: String,
    pub(super) source_path: Option<String>,
    pub(super) entity_labels: Vec<String>,
    pub(super) content: String,
    pub(super) rank: f64,
}

pub(super) fn bm25_candidate_rows(
    connection: &Connection,
    request: &GraphSearchRequest,
    match_query: &str,
) -> Result<Vec<RawBm25Row>, StorageError> {
    for delay_ms in GRAPH_BM25_QUERY_RETRY_DELAYS_MS {
        match bm25_candidate_rows_once(connection, request, match_query) {
            Ok(rows) => return Ok(rows),
            Err(error) if graph_bm25_query_error_is_retryable(&error) => {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            Err(error) => return Err(error),
        }
    }

    bm25_candidate_rows_once(connection, request, match_query)
}

fn bm25_candidate_rows_once(
    connection: &Connection,
    request: &GraphSearchRequest,
    match_query: &str,
) -> Result<Vec<RawBm25Row>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT
            graph_bm25.document_id,
            graph_bm25.document_kind,
            graph_bm25.evidence_id,
            graph_bm25.parent_evidence_id,
            graph_bm25.modality,
            graph_bm25.source_scope,
            graph_bm25.source_path,
            graph_bm25.entity_labels,
            graph_bm25.content,
            bm25(graph_bm25) AS rank
        FROM graph_bm25
        LEFT JOIN evidence e
          ON graph_bm25.document_kind = 'evidence'
         AND e.id = graph_bm25.evidence_id
        WHERE graph_bm25 MATCH ?1
          AND (?2 IS NULL OR graph_bm25.source_scope = ?2)
          AND graph_bm25.created_graph_version <= ?3
          AND (
              graph_bm25.document_kind != 'evidence'
              OR e.status IN ('accepted', 'proposed')
          )
        ORDER BY rank ASC, graph_bm25.document_id ASC
        LIMIT ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            match_query,
            request.source_scope.as_deref(),
            request.graph_version.get(),
            request.limit.saturating_mul(4).max(request.limit)
        ],
        |row| {
            Ok(RawBm25Row {
                document_id: row.get(0)?,
                document_kind: row.get(1)?,
                evidence_id: row.get(2)?,
                parent_evidence_id: row.get(3)?,
                modality: row.get(4)?,
                source_scope: row.get(5)?,
                source_path: row.get(6)?,
                entity_labels: super::split_labels(row.get(7)?),
                content: row.get(8)?,
                rank: row.get(9)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn graph_bm25_query_error_is_retryable(error: &StorageError) -> bool {
    match error {
        StorageError::Sqlite(error) => {
            graph_bm25_query_error_message_is_retryable(&error.to_string())
        }
        _ => false,
    }
}

pub(super) fn graph_bm25_query_error_message_is_retryable(message: &str) -> bool {
    super::graph_bm25_transient_error_message(message)
}
