//! Direct label-trigram indexing and bounded-candidate invariants.

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
                CREATE TABLE graph_bm25_route_documents (
                    document_id TEXT PRIMARY KEY,
                    fts_rowid INTEGER NOT NULL UNIQUE,
                    document_kind TEXT NOT NULL,
                    created_graph_version INTEGER NOT NULL,
                    source_scope TEXT NOT NULL,
                    source_path TEXT,
                    label_gram_state TEXT NOT NULL,
                    group_token TEXT NOT NULL,
                    term_counts_json TEXT NOT NULL
                );
                CREATE INDEX graph_bm25_route_documents_label_state
                ON graph_bm25_route_documents(
                    label_gram_state, source_scope, created_graph_version, document_id
                );
                ",
        )
        .expect("graph bm25 fixture table should initialize");
}

fn insert_graph_bm25_label_document(connection: &Connection, document_id: &str, labels: &[String]) {
    let labels_json = serde_json::to_string(labels).expect("labels should encode");
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
    let fts_rowid = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO graph_bm25_route_documents (
                 document_id, fts_rowid, document_kind, created_graph_version,
                 source_scope, source_path, label_gram_state, group_token,
                 term_counts_json
             ) VALUES (?1, ?2, 'code_symbol', 1, 'repo', NULL, 'pending', 'route', '[]')",
            params![document_id, fts_rowid],
        )
        .expect("route document fixture row should insert");
}

fn label_gram_row_count(connection: &Connection, document_id: &str) -> usize {
    connection
        .query_row(
            "SELECT COUNT(*) FROM graph_bm25_label_grams WHERE document_id = ?1",
            params![document_id],
            |row| row.get(0),
        )
        .expect("label gram count should load")
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
fn document_label_limits_are_inclusive() {
    let connection = Connection::open_in_memory().expect("db should open");
    initialize_schema(&connection).expect("label gram schema should initialize");
    let mut labels = vec![String::new(); MAX_LABELS_PER_DOCUMENT];
    labels[0] = "é".repeat(MAX_LABEL_UTF8_BYTES / "é".len());

    let outcome = replace_document(
        &connection,
        LabelGramDocument {
            document_id: "doc-at-limits",
            document_kind: "code_symbol",
            source_scope: "repo",
            graph_version: 1,
            labels: &labels,
        },
    )
    .expect("inclusive limits should index");

    assert_eq!(outcome, LabelGramIndexOutcome::Indexed);
    assert_eq!(labels[0].len(), MAX_LABEL_UTF8_BYTES);
    let gram_count = label_gram_row_count(&connection, "doc-at-limits");
    assert!(gram_count > 0);
}

#[test]
fn excessive_label_count_preserves_authoritative_document_and_skips_fuzzy_index() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_graph_bm25(&connection);
    initialize_schema(&connection).expect("label gram schema should initialize");
    let labels = vec![String::new(); MAX_LABELS_PER_DOCUMENT + 1];
    insert_graph_bm25_label_document(&connection, "doc-many-labels", &labels);

    let outcome = replace_document(
        &connection,
        LabelGramDocument {
            document_id: "doc-many-labels",
            document_kind: "code_symbol",
            source_scope: "repo",
            graph_version: 1,
            labels: &labels,
        },
    )
    .expect("fuzzy limit should not fail the authoritative write");

    assert_eq!(
        outcome,
        LabelGramIndexOutcome::Skipped(LabelGramLimit::LabelCount)
    );
    assert_eq!(label_gram_row_count(&connection, "doc-many-labels"), 0);
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM graph_bm25 WHERE document_id = 'doc-many-labels'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("authoritative document count should load"),
        1
    );
    backfill_missing(&connection).expect("skipped label state should backfill");
    let state = connection
        .query_row(
            "SELECT label_gram_state
             FROM graph_bm25_route_documents
             WHERE document_id = 'doc-many-labels'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("skipped label state should load");
    assert_eq!(state, "skipped:label_count");
    assert!(
        backfill_page(&connection, 0)
            .expect("skipped backfill state should load")
            .documents
            .is_empty()
    );
}

#[test]
fn excessive_utf8_label_bytes_remove_stale_fuzzy_rows() {
    let connection = Connection::open_in_memory().expect("db should open");
    initialize_schema(&connection).expect("label gram schema should initialize");
    let prior_labels = vec!["priorSymbol".to_owned()];
    replace_document(
        &connection,
        LabelGramDocument {
            document_id: "doc-large-label",
            document_kind: "code_symbol",
            source_scope: "repo",
            graph_version: 1,
            labels: &prior_labels,
        },
    )
    .expect("prior label should index");
    assert!(label_gram_row_count(&connection, "doc-large-label") > 0);
    let labels = vec!["界".repeat((MAX_LABEL_UTF8_BYTES / "界".len()) + 1)];

    let outcome = replace_document(
        &connection,
        LabelGramDocument {
            document_id: "doc-large-label",
            document_kind: "code_symbol",
            source_scope: "repo",
            graph_version: 2,
            labels: &labels,
        },
    )
    .expect("oversized UTF-8 label should skip without failing");

    assert!(labels[0].len() > MAX_LABEL_UTF8_BYTES);
    assert_eq!(
        outcome,
        LabelGramIndexOutcome::Skipped(LabelGramLimit::LabelUtf8Bytes)
    );
    assert_eq!(label_gram_row_count(&connection, "doc-large-label"), 0);
}

#[test]
fn excessive_total_grams_skip_the_entire_fuzzy_index() {
    let connection = Connection::open_in_memory().expect("db should open");
    initialize_schema(&connection).expect("label gram schema should initialize");
    let labels = (0..64)
        .map(|label_index| {
            (0..128)
                .map(|offset| {
                    char::from_u32(0x1000 + (label_index * 128) + offset)
                        .expect("fixture code point should be valid")
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    let outcome = replace_document(
        &connection,
        LabelGramDocument {
            document_id: "doc-many-grams",
            document_kind: "code_chunk",
            source_scope: "repo",
            graph_version: 1,
            labels: &labels,
        },
    )
    .expect("gram limit should skip without failing");

    assert!(labels.len() <= MAX_LABELS_PER_DOCUMENT);
    assert!(
        labels
            .iter()
            .all(|label| label.len() <= MAX_LABEL_UTF8_BYTES)
    );
    assert_eq!(
        outcome,
        LabelGramIndexOutcome::Skipped(LabelGramLimit::GramCount)
    );
    assert_eq!(expected_label_gram_state(&labels).0, 0);
    assert_eq!(label_gram_row_count(&connection, "doc-many-grams"), 0);
}

#[test]
fn label_gram_keys_store_each_normalized_label_once() {
    let mixed_case = "AbCd".repeat(200);
    let labels = vec![mixed_case.clone(), mixed_case.to_ascii_lowercase()];

    let keys = label_gram_keys(&labels).expect("bounded labels should prepare");

    assert_eq!(keys.by_label.len(), 1);
    assert_eq!(
        keys.gram_count,
        keys.by_label
            .values()
            .map(|label| label.grams.len())
            .sum::<usize>()
    );
}

#[test]
fn bm25_hierarchy_suite_bounds_fuzzy_label_postings_before_grouping() {
    let connection = Connection::open_in_memory().expect("db should open");
    initialize_schema(&connection).expect("label gram schema should initialize");
    connection
        .execute(
            "WITH RECURSIVE sequence(value) AS (
                 SELECT 1
                 UNION ALL
                 SELECT value + 1 FROM sequence WHERE value <= ?1
             )
             INSERT INTO graph_bm25_label_grams (
                 document_id, document_kind, source_scope, created_graph_version,
                 label, label_lower, label_len, gram_size, gram
             )
             SELECT printf('doc-%05d', value), 'code_symbol', 'repo', 1,
                    'aaa', 'aaa', 3, 1, 'a'
             FROM sequence",
            params![MAX_FUZZY_LABEL_POSTINGS],
        )
        .expect("common-gram fixture should insert");
    let request = GraphSearchRequest {
        query: "aaa".to_owned(),
        source_scope: None,
        graph_version: crate::domain::GraphVersion::new(1),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    };

    let candidates = fuzzy_label_candidates(&connection, &request, "aaa", 1, 10)
        .expect("posting probe should remain bounded");

    assert!(candidates.names.is_empty());
    assert!(candidates.posting_budget_exhausted);
}

#[test]
fn fuzzy_posting_budget_counts_each_document_label_once_across_query_grams() {
    let connection = Connection::open_in_memory().expect("db should open");
    initialize_schema(&connection).expect("label gram schema should initialize");
    connection
        .execute(
            "WITH RECURSIVE sequence(value) AS (
                 SELECT 1
                 UNION ALL
                 SELECT value + 1 FROM sequence WHERE value < 4097
             ), matching_grams(gram) AS (VALUES ('ab'), ('bc'))
             INSERT INTO graph_bm25_label_grams (
                 document_id, document_kind, source_scope, created_graph_version,
                 label, label_lower, label_len, gram_size, gram
             )
             SELECT printf('doc-%05d', value), 'code_symbol', 'repo', 1,
                    'abcdef', 'abcdef', 6, 2, gram
             FROM sequence
             CROSS JOIN matching_grams",
            [],
        )
        .expect("multi-gram fixture should insert");
    let request = GraphSearchRequest {
        query: "abcdeg".to_owned(),
        source_scope: None,
        graph_version: crate::domain::GraphVersion::new(1),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    };

    let candidates = fuzzy_label_candidates(&connection, &request, "abcdeg", 2, 10)
        .expect("document-label postings should remain within budget");

    assert_eq!(candidates.names, ["abcdef"]);
    assert!(!candidates.posting_budget_exhausted);
}

#[test]
fn backfill_missing_resumes_partial_label_gram_indexes() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_graph_bm25(&connection);
    initialize_schema(&connection).expect("label gram schema should initialize");
    insert_graph_bm25_label_document(&connection, "doc-partial", &["partialSymbol".to_owned()]);
    insert_graph_bm25_label_document(&connection, "doc-missing", &["missingSymbol".to_owned()]);
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

    let non_indexed_states = connection
        .query_row(
            "SELECT COUNT(*)
             FROM graph_bm25_route_documents
             WHERE label_gram_state <> 'indexed'",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("backfilled label states should load");
    assert_eq!(non_indexed_states, 0);
    assert!(
        backfill_page(&connection, 0)
            .expect("backfill state should load")
            .documents
            .is_empty()
    );
}
