//! Persistent label trigrams for bounded fuzzy graph retrieval.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{
    Connection, Transaction, TransactionBehavior, params, params_from_iter, types::Value,
};

use crate::storage::{GraphSearchRequest, StorageError};

const MAX_GRAM_SIZE: usize = 3;
const MAX_QUERY_GRAMS: usize = 64;
const SHORT_QUERY_MAX_LEN: usize = 5;
const MEDIUM_QUERY_MAX_LEN: usize = 10;
const BACKFILL_DOCUMENT_BATCH_SIZE: usize = 128;
pub(super) const MAX_FUZZY_LABEL_POSTINGS: usize = 8_192;
const MAX_LABELS_PER_DOCUMENT: usize = 256;
pub(in crate::storage::sqlite::retrieval) const MAX_LABEL_UTF8_BYTES: usize = 1_024;
const MAX_DOCUMENT_LABEL_GRAMS: usize = 8_192;
const LABEL_GRAM_STATE_INDEXED: &str = "indexed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LabelGramIndexOutcome {
    Indexed,
    Skipped(LabelGramLimit),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LabelGramLimit {
    LabelCount,
    LabelUtf8Bytes,
    GramCount,
}

impl LabelGramIndexOutcome {
    pub(super) const fn route_state(self) -> &'static str {
        match self {
            Self::Indexed => LABEL_GRAM_STATE_INDEXED,
            Self::Skipped(LabelGramLimit::LabelCount) => "skipped:label_count",
            Self::Skipped(LabelGramLimit::LabelUtf8Bytes) => "skipped:label_utf8_bytes",
            Self::Skipped(LabelGramLimit::GramCount) => "skipped:gram_count",
        }
    }
}

pub(super) struct LabelGramDocument<'a> {
    pub document_id: &'a str,
    pub document_kind: &'a str,
    pub source_scope: &'a str,
    pub graph_version: u64,
    pub labels: &'a [String],
}

pub(super) struct FuzzyLabelCandidates {
    pub(super) names: Vec<String>,
    pub(super) posting_budget_exhausted: bool,
}

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS graph_bm25_label_grams (
            document_id TEXT NOT NULL,
            document_kind TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL,
            label TEXT NOT NULL,
            label_lower TEXT NOT NULL,
            label_len INTEGER NOT NULL,
            gram_size INTEGER NOT NULL,
            gram TEXT NOT NULL,
            PRIMARY KEY (document_id, label_lower, gram_size, gram)
        );
        CREATE INDEX IF NOT EXISTS graph_bm25_label_grams_lookup
            ON graph_bm25_label_grams(source_scope, gram_size, gram, label_len, created_graph_version);
        CREATE INDEX IF NOT EXISTS graph_bm25_label_grams_global_lookup
            ON graph_bm25_label_grams(gram_size, gram, label_len, created_graph_version, source_scope);
        CREATE INDEX IF NOT EXISTS graph_bm25_label_grams_label_lookup
            ON graph_bm25_label_grams(label_lower, source_scope, created_graph_version, document_id);
        CREATE INDEX IF NOT EXISTS graph_bm25_label_grams_global_label_lookup
            ON graph_bm25_label_grams(label_lower, created_graph_version, source_scope, document_id);
        ",
    )?;

    Ok(())
}

pub(super) fn replace_document(
    connection: &Connection,
    document: LabelGramDocument<'_>,
) -> Result<LabelGramIndexOutcome, StorageError> {
    delete_document(connection, document.document_id)?;
    insert_document(connection, document)
}

pub(super) fn delete_document(
    connection: &Connection,
    document_id: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM graph_bm25_label_grams WHERE document_id = ?1",
        params![document_id],
    )?;

    Ok(())
}

pub(super) fn delete_documents(
    connection: &Connection,
    document_ids: &[String],
) -> Result<(), StorageError> {
    if document_ids.is_empty() {
        return Ok(());
    }
    let document_ids_json = serde_json::to_string(document_ids)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    connection.execute(
        "
        DELETE FROM graph_bm25_label_grams
        WHERE document_id IN (
            SELECT CAST(value AS TEXT) FROM json_each(?1)
        )
        ",
        params![document_ids_json],
    )?;

    Ok(())
}

pub(super) fn backfill_missing(connection: &Connection) -> Result<(), StorageError> {
    let mut cursor = 0_i64;
    loop {
        let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
        let page = backfill_page(&transaction, cursor)?;
        for document in &page.documents {
            let outcome = replace_document(
                &transaction,
                LabelGramDocument {
                    document_id: &document.document_id,
                    document_kind: &document.document_kind,
                    source_scope: &document.source_scope,
                    graph_version: document.graph_version,
                    labels: &document.labels,
                },
            )?;
            super::bm25_routing::mark_label_gram_state(
                &transaction,
                &document.document_id,
                document.graph_version,
                outcome.route_state(),
            )?;
        }
        transaction.commit()?;
        let Some(next_cursor) = page.next_cursor else {
            return Ok(());
        };
        cursor = next_cursor;
    }
}

struct BackfillDocument {
    document_id: String,
    document_kind: String,
    source_scope: String,
    graph_version: u64,
    labels: Vec<String>,
}

struct BackfillPage {
    documents: Vec<BackfillDocument>,
    next_cursor: Option<i64>,
}

fn backfill_page(connection: &Connection, cursor: i64) -> Result<BackfillPage, StorageError> {
    let mut statement = connection.prepare(
        "
        WITH documents AS (
            SELECT rowid, document_id, document_kind, source_scope,
                   created_graph_version, entity_labels
            FROM graph_bm25
            WHERE rowid > ?1
              AND document_kind IN ('evidence', 'code_symbol', 'code_chunk')
            ORDER BY rowid
            LIMIT ?2
        )
        SELECT documents.rowid,
               documents.document_id,
               documents.document_kind,
               documents.source_scope,
               documents.created_graph_version,
               documents.entity_labels,
               COUNT(label_grams.gram),
               route.label_gram_state
        FROM documents
        LEFT JOIN graph_bm25_label_grams label_grams
          ON label_grams.document_id = documents.document_id
         AND label_grams.created_graph_version = documents.created_graph_version
        LEFT JOIN graph_bm25_route_documents route
          ON route.document_id = documents.document_id
         AND route.created_graph_version = documents.created_graph_version
        GROUP BY documents.rowid,
                 documents.document_id,
                 documents.document_kind,
                 documents.source_scope,
                 documents.created_graph_version,
                 documents.entity_labels,
                 route.label_gram_state
        ORDER BY documents.rowid
        ",
    )?;
    let rows = statement.query_map(params![cursor, BACKFILL_DOCUMENT_BATCH_SIZE], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, u64>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, usize>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    let mut backfill_documents = Vec::new();
    for (
        _,
        document_id,
        document_kind,
        source_scope,
        graph_version,
        entity_labels,
        actual_grams,
        actual_state,
    ) in &rows
    {
        let labels = super::split_labels(entity_labels.clone());
        let (expected_grams, expected_state) = expected_label_gram_state(&labels);
        if *actual_grams != expected_grams || actual_state.as_deref() != Some(expected_state) {
            backfill_documents.push(BackfillDocument {
                document_id: document_id.clone(),
                document_kind: document_kind.clone(),
                source_scope: source_scope.clone(),
                graph_version: *graph_version,
                labels,
            });
        }
    }

    let next_cursor = (rows.len() == BACKFILL_DOCUMENT_BATCH_SIZE)
        .then(|| rows.last().expect("full backfill page cannot be empty").0);
    Ok(BackfillPage {
        documents: backfill_documents,
        next_cursor,
    })
}

pub(super) fn fuzzy_label_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
    query: &str,
    max_distance: usize,
    limit: usize,
) -> Result<FuzzyLabelCandidates, StorageError> {
    let label_len = query.chars().count();
    let gram_size = query_gram_size(label_len);
    let query_grams = query_character_grams(&query.to_ascii_lowercase(), gram_size);
    if query_grams.is_empty() || limit == 0 {
        return Ok(FuzzyLabelCandidates {
            names: Vec::new(),
            posting_budget_exhausted: false,
        });
    }

    let query_rows = query_grams
        .iter()
        .map(|_| "(?, ?)")
        .collect::<Vec<_>>()
        .join(", ");
    let allowed_lengths = (label_len.saturating_sub(max_distance)
        ..=label_len.saturating_add(max_distance))
        .collect::<Vec<_>>();
    let length_rows = allowed_lengths
        .iter()
        .map(|_| "(?)")
        .collect::<Vec<_>>()
        .join(", ");
    let min_overlap = minimum_shared_grams(query_grams.len(), gram_size, max_distance);
    let scope_filter = if request.source_scope.is_some() {
        "grams.source_scope = ?"
    } else {
        "1 = 1"
    };
    // A posting is one document-label candidate, not one matched gram row. The
    // primary key permits at most one row per query gram for that posting, so
    // the bounded query-gram count also bounds the rows considered per posting.
    let posting_probe_sql = format!(
        "
        WITH query_grams(gram_size, gram) AS (VALUES {query_rows}),
             query_lengths(label_len) AS (VALUES {length_rows})
        SELECT COUNT(*)
        FROM (
            SELECT DISTINCT grams.document_id, grams.label_lower
            FROM graph_bm25_label_grams grams
            JOIN query_grams
              ON grams.gram_size = query_grams.gram_size
             AND grams.gram = query_grams.gram
            JOIN query_lengths ON grams.label_len = query_lengths.label_len
            WHERE {scope_filter}
              AND grams.created_graph_version <= ?
              AND grams.document_kind IN ('evidence', 'code_symbol', 'code_chunk')
            LIMIT ?
        )
        "
    );
    let posting_probe_values = fuzzy_query_values(
        &query_grams,
        gram_size,
        &allowed_lengths,
        request,
        &[MAX_FUZZY_LABEL_POSTINGS.saturating_add(1)],
    )?;
    let posting_count = connection.query_row(
        &posting_probe_sql,
        params_from_iter(posting_probe_values),
        |row| row.get::<_, usize>(0),
    )?;
    if posting_count > MAX_FUZZY_LABEL_POSTINGS {
        return Ok(FuzzyLabelCandidates {
            names: Vec::new(),
            posting_budget_exhausted: true,
        });
    }

    let sql = format!(
        "
        WITH query_grams(gram_size, gram) AS (VALUES {query_rows}),
             query_lengths(label_len) AS (VALUES {length_rows})
        SELECT MIN(grams.label) AS label
        FROM graph_bm25_label_grams grams
        JOIN query_grams
          ON grams.gram_size = query_grams.gram_size
         AND grams.gram = query_grams.gram
        JOIN query_lengths ON grams.label_len = query_lengths.label_len
        WHERE {scope_filter}
          AND grams.created_graph_version <= ?
          AND grams.document_kind IN ('evidence', 'code_symbol', 'code_chunk')
        GROUP BY grams.label_lower
        HAVING COUNT(DISTINCT grams.gram) >= ?
        ORDER BY COUNT(DISTINCT grams.gram) DESC,
                 ABS(MIN(grams.label_len) - ?) ASC,
                 grams.label_lower ASC
        LIMIT ?
        "
    );

    let values = fuzzy_query_values(
        &query_grams,
        gram_size,
        &allowed_lengths,
        request,
        &[min_overlap, label_len, limit],
    )?;

    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?;

    let names = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    Ok(FuzzyLabelCandidates {
        names,
        posting_budget_exhausted: false,
    })
}

fn fuzzy_query_values(
    query_grams: &[String],
    gram_size: usize,
    allowed_lengths: &[usize],
    request: &GraphSearchRequest,
    trailing_values: &[usize],
) -> Result<Vec<Value>, StorageError> {
    let mut values = Vec::with_capacity(
        (query_grams.len() * 2)
            + allowed_lengths.len()
            + usize::from(request.source_scope.is_some())
            + 1
            + trailing_values.len(),
    );
    for gram in query_grams {
        values.push(Value::Integer(gram_size as i64));
        values.push(Value::Text(gram.clone()));
    }
    for label_len in allowed_lengths {
        values.push(Value::Integer(*label_len as i64));
    }
    if let Some(source_scope) = &request.source_scope {
        values.push(Value::Text(source_scope.clone()));
    }
    values.push(i64_value(request.graph_version.get(), "graph version")?);
    for value in trailing_values {
        values.push(Value::Integer(*value as i64));
    }
    Ok(values)
}

fn insert_document(
    connection: &Connection,
    document: LabelGramDocument<'_>,
) -> Result<LabelGramIndexOutcome, StorageError> {
    let label_gram_keys = match label_gram_keys(document.labels) {
        Ok(keys) => keys,
        Err(limit) => return Ok(LabelGramIndexOutcome::Skipped(limit)),
    };
    let mut statement = connection.prepare(
        "
        INSERT OR IGNORE INTO graph_bm25_label_grams (
            document_id, document_kind, source_scope, created_graph_version,
            label, label_lower, label_len, gram_size, gram
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ",
    )?;
    let graph_version = i64_value(document.graph_version, "graph version")?;
    for (label_lower, label_grams) in &label_gram_keys.by_label {
        for (gram_size, gram) in &label_grams.grams {
            statement.execute(params![
                document.document_id,
                document.document_kind,
                document.source_scope,
                graph_version,
                label_grams.label,
                label_lower,
                label_grams.label_len as i64,
                *gram_size as i64,
                gram
            ])?;
        }
    }

    Ok(LabelGramIndexOutcome::Indexed)
}

fn expected_label_gram_state(labels: &[String]) -> (usize, &'static str) {
    match label_gram_keys(labels) {
        Ok(keys) => (keys.gram_count, LABEL_GRAM_STATE_INDEXED),
        Err(limit) => (0, LabelGramIndexOutcome::Skipped(limit).route_state()),
    }
}

struct LabelGramKeys<'a> {
    by_label: BTreeMap<String, LabelGrams<'a>>,
    gram_count: usize,
}

struct LabelGrams<'a> {
    label: &'a str,
    label_len: usize,
    grams: BTreeSet<(usize, String)>,
}

fn label_gram_keys(labels: &[String]) -> Result<LabelGramKeys<'_>, LabelGramLimit> {
    if labels.len() > MAX_LABELS_PER_DOCUMENT {
        return Err(LabelGramLimit::LabelCount);
    }

    let mut by_label = BTreeMap::new();
    let mut gram_count = 0;
    for label in labels {
        if label.len() > MAX_LABEL_UTF8_BYTES {
            return Err(LabelGramLimit::LabelUtf8Bytes);
        }
        let label_lower = label.trim().to_ascii_lowercase();
        let chars = label_lower.chars().collect::<Vec<_>>();
        let label_len = chars.len();
        if label_len == 0 {
            continue;
        }
        if by_label.contains_key(&label_lower) {
            continue;
        }

        let mut grams = BTreeSet::new();
        for gram_size in 1..=MAX_GRAM_SIZE.min(label_len) {
            for window in chars.windows(gram_size) {
                if grams.insert((gram_size, window.iter().collect::<String>())) {
                    gram_count += 1;
                    if gram_count > MAX_DOCUMENT_LABEL_GRAMS {
                        return Err(LabelGramLimit::GramCount);
                    }
                }
            }
        }
        by_label.insert(
            label_lower,
            LabelGrams {
                label,
                label_len,
                grams,
            },
        );
    }

    Ok(LabelGramKeys {
        by_label,
        gram_count,
    })
}

fn query_gram_size(label_len: usize) -> usize {
    if label_len <= SHORT_QUERY_MAX_LEN {
        1
    } else if label_len <= MEDIUM_QUERY_MAX_LEN {
        2
    } else {
        3
    }
}

fn minimum_shared_grams(query_gram_count: usize, gram_size: usize, max_distance: usize) -> usize {
    query_gram_count
        .saturating_sub(max_distance.saturating_mul(gram_size))
        .max(1)
}

fn query_character_grams(value: &str, gram_size: usize) -> Vec<String> {
    let chars = value.chars().collect::<Vec<_>>();
    if gram_size == 0 || chars.len() < gram_size {
        return Vec::new();
    }

    let window_count = chars.len() - gram_size + 1;
    if window_count <= MAX_QUERY_GRAMS {
        return character_grams(value, gram_size);
    }

    let last_window_index = window_count - 1;
    let mut selected = BTreeSet::new();
    for index in 0..MAX_QUERY_GRAMS {
        let window_index = (index * last_window_index) / (MAX_QUERY_GRAMS - 1);
        selected.insert(
            chars[window_index..window_index + gram_size]
                .iter()
                .collect(),
        );
    }

    selected.into_iter().collect()
}

fn character_grams(value: &str, gram_size: usize) -> Vec<String> {
    let chars = value.chars().collect::<Vec<_>>();
    if gram_size == 0 || chars.len() < gram_size {
        return Vec::new();
    }

    let mut grams = BTreeSet::new();
    for window in chars.windows(gram_size) {
        grams.insert(window.iter().collect::<String>());
    }

    grams.into_iter().collect()
}

fn i64_value(value: u64, name: &str) -> Result<Value, StorageError> {
    let converted = i64::try_from(value)
        .map_err(|_| StorageError::InvalidInput(format!("{name} is too large for sqlite")))?;
    Ok(Value::Integer(converted))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
