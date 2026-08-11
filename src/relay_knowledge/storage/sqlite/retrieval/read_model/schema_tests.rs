use super::{
    activate_bm25_rebuild_table, execute_retrieval_schema,
    graph_retrieval_schema_error_is_retryable, graph_retrieval_schema_error_message_is_retryable,
    prepare_bm25_rebuild_table,
};

#[test]
fn schema_retry_is_limited_to_transient_open_errors() {
    assert!(!graph_retrieval_schema_error_is_retryable(
        &rusqlite::Error::InvalidQuery
    ));
    assert!(graph_retrieval_schema_error_message_is_retryable(
        "vtable constructor failed: graph_bm25"
    ));
    assert!(graph_retrieval_schema_error_message_is_retryable(
        "database schema is locked"
    ));
    assert!(!graph_retrieval_schema_error_message_is_retryable(
        "no such table: graph_bm25"
    ));
}

#[test]
fn route_state_default_matches_the_algorithm_fingerprint() {
    let connection = rusqlite::Connection::open_in_memory().expect("database should open");
    execute_retrieval_schema(&connection).expect("retrieval schema should initialize");
    let algorithm = connection
        .query_row(
            "SELECT algorithm_version FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("algorithm fingerprint should load");

    assert_eq!(
        algorithm,
        super::super::super::bm25_routing::ROUTING_ALGORITHM_VERSION
    );
}

#[test]
fn route_documents_persist_versioned_label_state_schema() {
    let connection = rusqlite::Connection::open_in_memory().expect("database should open");
    execute_retrieval_schema(&connection).expect("retrieval schema should initialize");
    let created_graph_version_required = connection
        .query_row(
            "SELECT \"notnull\"
             FROM pragma_table_info('graph_bm25_route_documents')
             WHERE name = 'created_graph_version'",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("route graph version column should load");
    assert!(created_graph_version_required);

    let mut statement = connection
        .prepare("PRAGMA index_info('graph_bm25_route_documents_label_state')")
        .expect("label state index should inspect");
    let columns = statement
        .query_map([], |row| row.get::<_, String>(2))
        .expect("label state index columns should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("label state index columns should load");
    assert_eq!(
        columns,
        [
            "label_gram_state",
            "source_scope",
            "created_graph_version",
            "document_id"
        ]
    );

    let mut statement = connection
        .prepare("PRAGMA index_info('graph_bm25_route_documents_global_label_state')")
        .expect("global label state index should inspect");
    let columns = statement
        .query_map([], |row| row.get::<_, String>(2))
        .expect("global label state index columns should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("global label state index columns should load");
    assert_eq!(
        columns,
        ["label_gram_state", "created_graph_version", "document_id"]
    );
}

#[test]
fn bm25_hierarchy_suite_shadow_swap_preserves_complete_reader_snapshots() {
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-bm25-shadow-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should follow the Unix epoch")
            .as_nanos()
    ));
    let writer = rusqlite::Connection::open(&path).expect("writer database should open");
    writer
        .execute_batch("PRAGMA journal_mode=WAL;")
        .expect("WAL should enable snapshot readers");
    execute_retrieval_schema(&writer).expect("retrieval schema should initialize");
    insert_bm25_fixture(&writer, "graph_bm25", "old", "old generation");
    prepare_bm25_rebuild_table(&writer).expect("shadow table should initialize");
    insert_bm25_fixture(&writer, "graph_bm25_rebuild", "new", "new generation");

    let reader = rusqlite::Connection::open(&path).expect("reader database should open");
    reader
        .execute_batch("BEGIN")
        .expect("reader snapshot should begin");
    assert_eq!(bm25_document_ids(&reader), vec!["old".to_owned()]);

    let swap = writer
        .unchecked_transaction()
        .expect("swap transaction should begin");
    activate_bm25_rebuild_table(&swap).expect("shadow should become active");
    swap.commit().expect("shadow swap should commit atomically");

    assert_eq!(bm25_document_ids(&reader), vec!["old".to_owned()]);
    reader
        .execute_batch("COMMIT")
        .expect("reader snapshot should commit");
    assert_eq!(bm25_document_ids(&reader), vec!["new".to_owned()]);

    drop(reader);
    drop(writer);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[test]
fn bm25_hierarchy_suite_shadow_swap_rollback_restores_the_active_generation() {
    let connection = rusqlite::Connection::open_in_memory().expect("database should open");
    execute_retrieval_schema(&connection).expect("retrieval schema should initialize");
    insert_bm25_fixture(&connection, "graph_bm25", "old", "old generation");
    prepare_bm25_rebuild_table(&connection).expect("shadow table should initialize");
    insert_bm25_fixture(&connection, "graph_bm25_rebuild", "new", "new generation");

    let swap = connection
        .unchecked_transaction()
        .expect("swap transaction should begin");
    activate_bm25_rebuild_table(&swap).expect("shadow rename should succeed");
    swap.rollback()
        .expect("injected swap failure should roll back");

    assert_eq!(bm25_document_ids(&connection), vec!["old".to_owned()]);
    let shadow_count = connection
        .query_row("SELECT COUNT(*) FROM graph_bm25_rebuild", [], |row| {
            row.get::<_, usize>(0)
        })
        .expect("rolled-back shadow should remain available");
    assert_eq!(shadow_count, 1);
}

fn insert_bm25_fixture(
    connection: &rusqlite::Connection,
    table: &'static str,
    document_id: &str,
    content: &str,
) {
    let sql = format!(
        "INSERT INTO {table} (
             document_id, document_kind, evidence_id, parent_evidence_id, modality,
             created_graph_version, routing_key, source_scope, source_path,
             entity_labels, entity_aliases, content
         ) VALUES (?1, 'evidence', ?1, NULL, 'text_span', 1, 'route', 'scope',
                   NULL, '', '', ?2)"
    );
    connection
        .execute(&sql, rusqlite::params![document_id, content])
        .expect("BM25 fixture should insert");
}

fn bm25_document_ids(connection: &rusqlite::Connection) -> Vec<String> {
    let mut statement = connection
        .prepare("SELECT document_id FROM graph_bm25 ORDER BY document_id")
        .expect("BM25 ids should prepare");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("BM25 ids should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("BM25 ids should load")
}
