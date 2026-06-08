use std::collections::BTreeSet;

use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::storage::{GraphSearchRequest, StorageError};

const MAX_GRAM_SIZE: usize = 3;
const MAX_QUERY_GRAMS: usize = 64;
const SHORT_QUERY_MAX_LEN: usize = 5;
const MEDIUM_QUERY_MAX_LEN: usize = 10;

pub(super) struct LabelGramDocument<'a> {
    pub document_id: &'a str,
    pub document_kind: &'a str,
    pub source_scope: &'a str,
    pub graph_version: u64,
    pub labels: &'a [String],
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
            ON graph_bm25_label_grams(source_scope, gram_size, gram, created_graph_version, label_len);
        CREATE INDEX IF NOT EXISTS graph_bm25_label_grams_document
            ON graph_bm25_label_grams(document_id);
        CREATE INDEX IF NOT EXISTS graph_bm25_label_grams_label_lookup
            ON graph_bm25_label_grams(label_lower, source_scope, created_graph_version, document_id);
        ",
    )?;

    Ok(())
}

pub(super) fn replace_document(
    connection: &Connection,
    document: LabelGramDocument<'_>,
) -> Result<(), StorageError> {
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

pub(super) fn delete_code_documents_for_path(
    connection: &Connection,
    source_scope: &str,
    path: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "
        DELETE FROM graph_bm25_label_grams
        WHERE document_id IN (
            SELECT document_id
            FROM graph_bm25
            WHERE document_kind IN ('code_symbol', 'code_chunk')
              AND source_scope = ?1
              AND source_path = ?2
        )
        ",
        params![source_scope, path],
    )?;

    Ok(())
}

pub(super) fn clear(connection: &Connection) -> Result<(), StorageError> {
    connection.execute("DELETE FROM graph_bm25_label_grams", [])?;

    Ok(())
}

pub(super) fn backfill_missing(connection: &Connection) -> Result<(), StorageError> {
    for document in documents_needing_backfill(connection)? {
        replace_document(
            connection,
            LabelGramDocument {
                document_id: &document.document_id,
                document_kind: &document.document_kind,
                source_scope: &document.source_scope,
                graph_version: document.graph_version,
                labels: &document.labels,
            },
        )?;
    }

    Ok(())
}

struct BackfillDocument {
    document_id: String,
    document_kind: String,
    source_scope: String,
    graph_version: u64,
    labels: Vec<String>,
}

fn documents_needing_backfill(
    connection: &Connection,
) -> Result<Vec<BackfillDocument>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT graph_bm25.document_id,
               graph_bm25.document_kind,
               graph_bm25.source_scope,
               graph_bm25.created_graph_version,
               graph_bm25.entity_labels,
               COUNT(label_grams.gram)
        FROM graph_bm25
        LEFT JOIN graph_bm25_label_grams label_grams
          ON label_grams.document_id = graph_bm25.document_id
        WHERE graph_bm25.document_kind IN ('evidence', 'code_symbol', 'code_chunk')
        GROUP BY graph_bm25.document_id,
                 graph_bm25.document_kind,
                 graph_bm25.source_scope,
                 graph_bm25.created_graph_version,
                 graph_bm25.entity_labels
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, u64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, usize>(5)?,
        ))
    })?;
    let documents = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    let mut backfill_documents = Vec::new();
    for (document_id, document_kind, source_scope, graph_version, entity_labels, actual_grams) in
        documents
    {
        let labels = super::split_labels(entity_labels);
        let expected_grams = expected_label_gram_count(&labels);
        if actual_grams != expected_grams {
            backfill_documents.push(BackfillDocument {
                document_id,
                document_kind,
                source_scope,
                graph_version,
                labels,
            });
        }
    }

    Ok(backfill_documents)
}

pub(super) fn fuzzy_label_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
    query: &str,
    max_distance: usize,
    limit: usize,
) -> Result<Vec<String>, StorageError> {
    let label_len = query.chars().count();
    let gram_size = query_gram_size(label_len);
    let query_grams = query_character_grams(&query.to_ascii_lowercase(), gram_size);
    if query_grams.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let query_rows = query_grams
        .iter()
        .map(|_| "(?, ?)")
        .collect::<Vec<_>>()
        .join(", ");
    let min_overlap = minimum_shared_grams(query_grams.len(), gram_size, max_distance);
    let min_len = label_len.saturating_sub(max_distance);
    let max_len = label_len.saturating_add(max_distance);
    let sql = format!(
        "
        WITH query_grams(gram_size, gram) AS (VALUES {query_rows})
        SELECT MIN(grams.label) AS label
        FROM graph_bm25_label_grams grams
        JOIN query_grams
          ON grams.gram_size = query_grams.gram_size
         AND grams.gram = query_grams.gram
        WHERE (? IS NULL OR grams.source_scope = ?)
          AND grams.created_graph_version <= ?
          AND grams.document_kind IN ('evidence', 'code_symbol', 'code_chunk')
          AND grams.label_len BETWEEN ? AND ?
        GROUP BY grams.label_lower
        HAVING COUNT(DISTINCT grams.gram) >= ?
        ORDER BY COUNT(DISTINCT grams.gram) DESC,
                 ABS(MIN(grams.label_len) - ?) ASC,
                 grams.label_lower ASC
        LIMIT ?
        "
    );

    let mut values = Vec::with_capacity((query_grams.len() * 2) + 8);
    for gram in &query_grams {
        values.push(Value::Integer(gram_size as i64));
        values.push(Value::Text(gram.clone()));
    }
    let scope_value = request
        .source_scope
        .as_ref()
        .map_or(Value::Null, |scope| Value::Text(scope.clone()));
    values.push(scope_value.clone());
    values.push(scope_value);
    values.push(i64_value(request.graph_version.get(), "graph version")?);
    values.push(Value::Integer(min_len as i64));
    values.push(Value::Integer(max_len as i64));
    values.push(Value::Integer(min_overlap as i64));
    values.push(Value::Integer(label_len as i64));
    values.push(Value::Integer(limit as i64));

    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn insert_document(
    connection: &Connection,
    document: LabelGramDocument<'_>,
) -> Result<(), StorageError> {
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
    for label in document.labels {
        let label_lower = label.trim().to_ascii_lowercase();
        let label_len = label_lower.chars().count();
        if label_len == 0 {
            continue;
        }
        for gram_size in 1..=MAX_GRAM_SIZE.min(label_len) {
            for gram in character_grams(&label_lower, gram_size) {
                statement.execute(params![
                    document.document_id,
                    document.document_kind,
                    document.source_scope,
                    graph_version,
                    label,
                    label_lower,
                    label_len as i64,
                    gram_size as i64,
                    gram
                ])?;
            }
        }
    }

    Ok(())
}

fn expected_label_gram_count(labels: &[String]) -> usize {
    label_gram_keys(labels).len()
}

fn label_gram_keys(labels: &[String]) -> BTreeSet<(String, usize, String)> {
    let mut keys = BTreeSet::new();
    for label in labels {
        let label_lower = label.trim().to_ascii_lowercase();
        let label_len = label_lower.chars().count();
        if label_len == 0 {
            continue;
        }
        for gram_size in 1..=MAX_GRAM_SIZE.min(label_len) {
            for gram in character_grams(&label_lower, gram_size) {
                keys.insert((label_lower.clone(), gram_size, gram));
            }
        }
    }

    keys
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
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_graph_bm25(connection: &Connection) {
        connection
            .execute_batch(
                "
                CREATE TABLE graph_bm25 (
                    document_id TEXT NOT NULL,
                    document_kind TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    created_graph_version INTEGER NOT NULL,
                    entity_labels TEXT NOT NULL
                );
                ",
            )
            .expect("graph bm25 fixture table should initialize");
    }

    fn insert_graph_bm25_label_document(connection: &Connection, document_id: &str, label: &str) {
        let labels_json =
            serde_json::to_string(&vec![label.to_owned()]).expect("labels should encode");
        connection
            .execute(
                "
                INSERT INTO graph_bm25 (
                    document_id, document_kind, source_scope,
                    created_graph_version, entity_labels
                )
                VALUES (?1, 'code_symbol', 'repo', 1, ?2)
                ",
                params![document_id, labels_json],
            )
            .expect("graph bm25 fixture row should insert");
    }

    #[test]
    fn minimum_shared_grams_keeps_query_specific_threshold() {
        assert_eq!(minimum_shared_grams(52, 3, 2), 46);
        assert_eq!(minimum_shared_grams(1, 1, 2), 1);
    }

    #[test]
    fn character_grams_deduplicate_repeated_windows() {
        assert_eq!(character_grams("aaaa", 2), ["aa"]);
    }

    #[test]
    fn query_character_grams_are_bounded() {
        let query = (0..200)
            .map(|index| format!("{index:03}"))
            .collect::<String>();

        let grams = query_character_grams(&query, 3);

        assert!(grams.len() <= MAX_QUERY_GRAMS);
        assert!(!grams.is_empty());
    }

    #[test]
    fn backfill_missing_resumes_partial_label_gram_indexes() {
        let connection = Connection::open_in_memory().expect("db should open");
        setup_graph_bm25(&connection);
        initialize_schema(&connection).expect("label gram schema should initialize");
        insert_graph_bm25_label_document(&connection, "doc-partial", "partialSymbol");
        insert_graph_bm25_label_document(&connection, "doc-missing", "missingSymbol");
        connection
            .execute(
                "
                INSERT INTO graph_bm25_label_grams (
                    document_id, document_kind, source_scope, created_graph_version,
                    label, label_lower, label_len, gram_size, gram
                )
                VALUES (
                    'doc-partial', 'code_symbol', 'repo', 1,
                    'partialSymbol', 'partialsymbol', 13, 1, 'p'
                )
                ",
                [],
            )
            .expect("partial label gram should insert");

        backfill_missing(&connection).expect("backfill should resume");

        assert!(
            documents_needing_backfill(&connection)
                .expect("backfill state should load")
                .is_empty()
        );
    }
}
