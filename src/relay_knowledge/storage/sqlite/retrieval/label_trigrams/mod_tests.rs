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
                ",
        )
        .expect("graph bm25 fixture table should initialize");
}

fn insert_graph_bm25_label_document(connection: &Connection, document_id: &str, label: &str) {
    let labels_json = serde_json::to_string(&vec![label.to_owned()]).expect("labels should encode");
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
