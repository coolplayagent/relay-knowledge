use std::fmt::{self, Write as _};

use rusqlite::{Connection, params};

use crate::storage::StorageError;

pub(super) const REBUILD_DOCUMENT_BATCH_SIZE: usize = 128;
pub(super) const REBUILD_SOURCE_BYTES_PER_BATCH: u64 = 4 * 1_024 * 1_024;
const REBUILD_LABELS_PER_BATCH: u64 = 8_192;
const REBUILD_LINKS_PER_BATCH: u64 = 8_192;
const REBUILD_CANDIDATE_LIMIT: usize = REBUILD_DOCUMENT_BATCH_SIZE + 1;
const MAX_LOG_IDENTITY_CHARS: usize = 160;

#[derive(Clone)]
pub(super) struct EvidenceRebuildKey {
    pub(super) evidence_id: String,
}

#[derive(Clone)]
pub(super) struct CodeRebuildKey {
    pub(super) source_scope: String,
    pub(super) path: String,
    pub(super) document_id: String,
}

pub(super) struct RebuildPage<K> {
    pub(super) keys: Vec<K>,
    pub(super) page_is_complete: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RebuildWorkload {
    source_bytes: u64,
    labels: u64,
    links: u64,
}

#[derive(Clone, Copy)]
struct RebuildBatchBudget {
    documents: usize,
    source_bytes: u64,
    labels: u64,
    links: u64,
}

const REBUILD_BATCH_BUDGET: RebuildBatchBudget = RebuildBatchBudget {
    documents: REBUILD_DOCUMENT_BATCH_SIZE,
    source_bytes: REBUILD_SOURCE_BYTES_PER_BATCH,
    labels: REBUILD_LABELS_PER_BATCH,
    links: REBUILD_LINKS_PER_BATCH,
};

pub(super) fn evidence_rebuild_page(
    connection: &Connection,
    cursor: Option<&EvidenceRebuildKey>,
) -> Result<RebuildPage<EvidenceRebuildKey>, StorageError> {
    if !table_has_columns(
        connection,
        "evidence",
        &[
            "id",
            "source_scope",
            "source_path",
            "content",
            "status",
            "modality",
            "source_hash",
            "parent_evidence_id",
            "embedding_model",
        ],
    )? || !table_has_columns(
        connection,
        "evidence_entities",
        &["evidence_id", "entity_id"],
    )? || !table_has_columns(connection, "entities", &["id", "label"])?
        || !connection.query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM evidence
                 WHERE status IN ('accepted', 'proposed')
                   AND id > COALESCE(?1, '')
             )",
            params![cursor.map(|cursor| cursor.evidence_id.as_str())],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Ok(RebuildPage {
            keys: Vec::new(),
            page_is_complete: true,
        });
    }
    let mut statement = connection.prepare(
        "SELECT e.id,
                length(CAST(e.id AS BLOB)) +
                length(CAST(e.source_scope AS BLOB)) +
                COALESCE(length(CAST(e.source_path AS BLOB)), 0) +
                length(CAST(e.content AS BLOB)) +
                length(CAST(e.status AS BLOB)) +
                length(CAST(e.modality AS BLOB)) +
                COALESCE(length(CAST(e.source_hash AS BLOB)), 0) +
                COALESCE(length(CAST(e.parent_evidence_id AS BLOB)), 0) +
                COALESCE(length(CAST(e.embedding_model AS BLOB)), 0) +
                COALESCE((
                    SELECT SUM(length(CAST(entity.label AS BLOB)))
                    FROM evidence_entities AS link
                    INNER JOIN entities AS entity ON entity.id = link.entity_id
                    WHERE link.evidence_id = e.id
                ), 0) AS source_bytes,
                (SELECT COUNT(*) FROM evidence_entities AS link
                 WHERE link.evidence_id = e.id) AS label_count
         FROM evidence AS e
         WHERE e.status IN ('accepted', 'proposed')
           AND e.id > COALESCE(?1, '')
         ORDER BY e.id
         LIMIT ?2",
    )?;
    let rows = statement.query_map(
        params![
            cursor.map(|cursor| cursor.evidence_id.as_str()),
            REBUILD_CANDIDATE_LIMIT
        ],
        |row| {
            Ok((
                EvidenceRebuildKey {
                    evidence_id: row.get(0)?,
                },
                RebuildWorkload {
                    source_bytes: row.get(1)?,
                    labels: row.get(2)?,
                    links: 0,
                },
            ))
        },
    )?;
    let candidates = rows.collect::<Result<Vec<_>, _>>()?;
    let (page, oversized) = bounded_page(candidates, REBUILD_BATCH_BUDGET);
    if let Some(workload) = oversized {
        let key = page.keys.first().expect("oversized page has one key");
        warn_oversized("evidence", &key.evidence_id, None, workload);
    }
    Ok(page)
}

pub(super) fn code_symbol_rebuild_page(
    connection: &Connection,
    cursor: Option<&CodeRebuildKey>,
) -> Result<RebuildPage<CodeRebuildKey>, StorageError> {
    if !table_has_columns(
        connection,
        "code_symbols",
        &["source_scope", "path", "symbol_id", "name", "kind"],
    )? {
        return Ok(RebuildPage {
            keys: Vec::new(),
            page_is_complete: true,
        });
    }
    let mut statement = connection.prepare(
        "SELECT source_scope, path, symbol_id,
                length(CAST(source_scope AS BLOB)) +
                length(CAST(path AS BLOB)) +
                length(CAST(symbol_id AS BLOB)) +
                length(CAST(name AS BLOB)) +
                length(CAST(kind AS BLOB)) + 3 AS source_bytes
         FROM code_symbols
         WHERE (source_scope, path, symbol_id) >
               (COALESCE(?1, ''), COALESCE(?2, ''), COALESCE(?3, ''))
         ORDER BY source_scope, path, symbol_id
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![
            cursor.map(|cursor| cursor.source_scope.as_str()),
            cursor.map(|cursor| cursor.path.as_str()),
            cursor.map(|cursor| cursor.document_id.as_str()),
            REBUILD_CANDIDATE_LIMIT
        ],
        |row| {
            Ok((
                code_key_from_row(row)?,
                RebuildWorkload {
                    source_bytes: row.get(3)?,
                    labels: 1,
                    links: 0,
                },
            ))
        },
    )?;
    Ok(finish_code_page(
        rows.collect::<Result<Vec<_>, _>>()?,
        "code_symbols",
    ))
}

pub(super) fn code_chunk_rebuild_page(
    connection: &Connection,
    cursor: Option<&CodeRebuildKey>,
) -> Result<RebuildPage<CodeRebuildKey>, StorageError> {
    if !table_has_columns(
        connection,
        "code_chunks",
        &["source_scope", "path", "chunk_id", "content"],
    )? || !table_has_columns(
        connection,
        "code_chunk_symbols",
        &["source_scope", "path", "chunk_id", "symbol_id"],
    )? {
        return Ok(RebuildPage {
            keys: Vec::new(),
            page_is_complete: true,
        });
    }
    let mut statement = connection.prepare(
        "SELECT chunk.source_scope, chunk.path, chunk.chunk_id,
                length(CAST(chunk.source_scope AS BLOB)) +
                length(CAST(chunk.path AS BLOB)) +
                length(CAST(chunk.chunk_id AS BLOB)) +
                length(CAST(chunk.content AS BLOB)) +
                COALESCE((
                    SELECT SUM(length(CAST(link.symbol_id AS BLOB)))
                    FROM code_chunk_symbols AS link
                    WHERE link.source_scope = chunk.source_scope
                      AND link.path = chunk.path AND link.chunk_id = chunk.chunk_id
                ), 0) AS source_bytes,
                (SELECT COUNT(*) FROM code_chunk_symbols AS link
                 WHERE link.source_scope = chunk.source_scope
                   AND link.path = chunk.path AND link.chunk_id = chunk.chunk_id) AS link_count
         FROM code_chunks AS chunk
         WHERE (chunk.source_scope, chunk.path, chunk.chunk_id) >
               (COALESCE(?1, ''), COALESCE(?2, ''), COALESCE(?3, ''))
         ORDER BY chunk.source_scope, chunk.path, chunk.chunk_id
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![
            cursor.map(|cursor| cursor.source_scope.as_str()),
            cursor.map(|cursor| cursor.path.as_str()),
            cursor.map(|cursor| cursor.document_id.as_str()),
            REBUILD_CANDIDATE_LIMIT
        ],
        |row| {
            let links = row.get(4)?;
            Ok((
                code_key_from_row(row)?,
                RebuildWorkload {
                    source_bytes: row.get(3)?,
                    labels: links,
                    links,
                },
            ))
        },
    )?;
    Ok(finish_code_page(
        rows.collect::<Result<Vec<_>, _>>()?,
        "code_chunks",
    ))
}

pub(super) fn encode_code_rebuild_cursor(cursor: &CodeRebuildKey) -> Result<String, StorageError> {
    serde_json::to_string(&(&cursor.source_scope, &cursor.path, &cursor.document_id))
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

pub(super) fn decode_code_rebuild_cursor(
    cursor: Option<&str>,
    phase: &str,
) -> Result<Option<CodeRebuildKey>, StorageError> {
    cursor
        .map(|cursor| {
            serde_json::from_str::<(String, String, String)>(cursor)
                .map(|(source_scope, path, document_id)| CodeRebuildKey {
                    source_scope,
                    path,
                    document_id,
                })
                .map_err(|_| {
                    StorageError::InvalidInput(format!(
                        "invalid BM25 rebuild code cursor for phase '{phase}'"
                    ))
                })
        })
        .transpose()
}

fn finish_code_page(
    candidates: Vec<(CodeRebuildKey, RebuildWorkload)>,
    phase: &'static str,
) -> RebuildPage<CodeRebuildKey> {
    let (page, oversized) = bounded_page(candidates, REBUILD_BATCH_BUDGET);
    if let Some(workload) = oversized {
        let key = page.keys.first().expect("oversized page has one key");
        warn_oversized(
            phase,
            &key.document_id,
            Some((&key.source_scope, &key.path)),
            workload,
        );
    }
    page
}

fn code_key_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodeRebuildKey> {
    Ok(CodeRebuildKey {
        source_scope: row.get(0)?,
        path: row.get(1)?,
        document_id: row.get(2)?,
    })
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = ?1
             )",
            params![table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

fn table_has_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(false);
    }
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let columns = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(required_columns
        .iter()
        .all(|required| columns.iter().any(|column| column == required)))
}

fn bounded_page<K>(
    candidates: Vec<(K, RebuildWorkload)>,
    budget: RebuildBatchBudget,
) -> (RebuildPage<K>, Option<RebuildWorkload>) {
    let candidate_count = candidates.len();
    let mut keys = Vec::with_capacity(candidate_count.min(budget.documents));
    let mut accumulated = RebuildWorkload::default();
    let mut oversized = None;
    let mut has_more = candidate_count > budget.documents;

    for (key, workload) in candidates {
        if keys.len() == budget.documents
            || (!keys.is_empty() && !budget.fits(accumulated, workload))
        {
            has_more = true;
            break;
        }
        if keys.is_empty() && !budget.fits(accumulated, workload) {
            oversized = Some(workload);
        }
        accumulated = accumulated.saturating_add(workload);
        keys.push(key);
        if oversized.is_some() {
            has_more |= candidate_count > 1;
            break;
        }
    }

    (
        RebuildPage {
            keys,
            page_is_complete: !has_more && candidate_count < REBUILD_CANDIDATE_LIMIT,
        },
        oversized,
    )
}

impl RebuildBatchBudget {
    fn fits(self, accumulated: RebuildWorkload, next: RebuildWorkload) -> bool {
        accumulated.source_bytes.saturating_add(next.source_bytes) <= self.source_bytes
            && accumulated.labels.saturating_add(next.labels) <= self.labels
            && accumulated.links.saturating_add(next.links) <= self.links
    }
}

impl RebuildWorkload {
    fn saturating_add(self, other: Self) -> Self {
        Self {
            source_bytes: self.source_bytes.saturating_add(other.source_bytes),
            labels: self.labels.saturating_add(other.labels),
            links: self.links.saturating_add(other.links),
        }
    }
}

fn warn_oversized(
    phase: &'static str,
    document_id: &str,
    code_location: Option<(&str, &str)>,
    workload: RebuildWorkload,
) {
    let (source_scope, source_path) = code_location.unwrap_or(("", ""));
    tracing::warn!(
        phase,
        document_id = %BoundedLogValue(document_id),
        source_scope = %BoundedLogValue(source_scope),
        source_path = %BoundedLogValue(source_path),
        source_bytes = workload.source_bytes,
        labels = workload.labels,
        links = workload.links,
        max_source_bytes = REBUILD_SOURCE_BYTES_PER_BATCH,
        max_labels = REBUILD_LABELS_PER_BATCH,
        max_links = REBUILD_LINKS_PER_BATCH,
        "isolating oversized authoritative document in one BM25 rebuild transaction"
    );
}

struct BoundedLogValue<'a>(&'a str);

impl fmt::Display for BoundedLogValue<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut characters = self.0.chars();
        for character in characters.by_ref().take(MAX_LOG_IDENTITY_CHARS) {
            formatter.write_char(character)?;
        }
        if characters.next().is_some() {
            formatter.write_str("…")?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "rebuild_budget_tests.rs"]
mod tests;
