use std::sync::Mutex;

use rusqlite::Connection;

use super::*;
use crate::storage::sqlite::retrieval::bm25_routing::{Bm25RoutingText, prepare_document};

static TRACED_ROUTE_STATEMENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn capture_route_statement(sql: &str) {
    TRACED_ROUTE_STATEMENTS
        .lock()
        .expect("trace statements should lock")
        .push(sql.to_owned());
}

#[test]
fn replacement_and_delete_keep_route_aggregates_idempotent() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    connection
        .execute(
            "INSERT INTO graph_bm25 (
                document_id, document_kind, evidence_id, parent_evidence_id, modality,
                created_graph_version, routing_key, source_scope, source_path, entity_labels,
                entity_aliases, content
             ) VALUES ('doc', 'code_chunk', 'chunk', NULL, 'text_span', 1,
                       'route-placeholder', 'scope', 'src/lib.rs', '[]', '',
                       'alpha alpha beta')",
            [],
        )
        .expect("bm25 row should insert");
    let rowid = connection.last_insert_rowid();
    let route = prepare_document(Bm25RoutingText {
        source_scope: "scope",
        source_path: Some("src/lib.rs"),
        entity_labels: "[]",
        entity_aliases: "",
        content: "alpha alpha beta",
        graph_version: 1,
    });
    connection
        .execute(
            "UPDATE graph_bm25 SET routing_key = ?1 WHERE rowid = ?2",
            rusqlite::params![route.routing_key.as_str(), rowid],
        )
        .expect("routing key should align with route state");

    replace_document(
        &connection,
        "doc",
        rowid,
        "code_chunk",
        Some("src/lib.rs"),
        "indexed",
        &route,
    )
    .expect("route should insert");
    replace_document(
        &connection,
        "doc",
        rowid,
        "code_chunk",
        Some("src/lib.rs"),
        "indexed",
        &route,
    )
    .expect("route replacement should be idempotent");

    let (documents, groups, state_documents, alpha_cf, total_df): (
        usize,
        usize,
        usize,
        usize,
        usize,
    ) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM graph_bm25_route_documents),
                (SELECT SUM(document_count) FROM graph_bm25_route_groups),
                (SELECT document_count FROM graph_bm25_route_state WHERE id=1),
                (SELECT SUM(collection_frequency) FROM graph_bm25_route_terms WHERE term='alpha'),
                (SELECT document_frequency FROM graph_bm25_route_term_totals WHERE term='alpha')",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("route counts should load");
    assert_eq!(
        (documents, groups, state_documents, alpha_cf, total_df,),
        (1, 1, 1, 2, 1)
    );
    let stored_graph_version = connection
        .query_row(
            "SELECT created_graph_version
             FROM graph_bm25_route_documents
             WHERE document_id = 'doc'",
            [],
            |row| row.get::<_, u64>(0),
        )
        .expect("route document graph version should load");
    assert_eq!(stored_graph_version, 1);
    assert!(matches!(
        mark_label_gram_state(&connection, "doc", 2, "pending"),
        Err(StorageError::InvalidInput(_))
    ));
    mark_label_gram_state(&connection, "doc", 1, "indexed")
        .expect("matching route version should update label state");

    delete_document(&connection, "doc", 2).expect("route should delete");
    let remaining: (usize, usize) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM graph_bm25_route_documents),
                (SELECT document_count FROM graph_bm25_route_state WHERE id=1)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("route count should load");
    assert_eq!(remaining, (0, 0));
}

#[test]
fn bm25_hierarchy_suite_keeps_partial_rebuild_unavailable_until_finalization() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    let lease = begin_rebuild(&connection).expect("route rebuild should start");
    assert!(matches!(
        begin_rebuild(&connection),
        Err(StorageError::Busy(_))
    ));
    let route = prepare_document(Bm25RoutingText {
        source_scope: "scope",
        source_path: Some("src/lib.rs"),
        entity_labels: "",
        entity_aliases: "",
        content: "alpha beta",
        graph_version: 7,
    });

    replace_document(
        &connection,
        "partial",
        1,
        "code_chunk",
        Some("src/lib.rs"),
        "indexed",
        &route,
    )
    .expect("partial route document should persist");
    let building: (String, u64, usize) = connection
        .query_row(
            "SELECT state, indexed_graph_version, document_count
             FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("building route state should load");
    assert_eq!(building, ("building".to_owned(), 7, 1));

    mark_graph_version(&connection, 10).expect("concurrent graph version should advance");
    finish_rebuild(
        &connection,
        &lease,
        9,
        Some("semantic-v1"),
        Some("vector-v1"),
    )
    .expect("route rebuild should finalize");
    let fresh: (String, u64, usize, String, String) = connection
        .query_row(
            "SELECT state, indexed_graph_version, document_count,
                    semantic_generation, vector_generation
             FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("fresh route state should load");
    assert_eq!(
        fresh,
        (
            "fresh".to_owned(),
            10,
            1,
            "semantic-v1".to_owned(),
            "vector-v1".to_owned()
        )
    );
    assert!(matches!(
        finish_rebuild(&connection, &lease, 10, None, None),
        Err(StorageError::InvalidInput(_))
    ));
}

#[test]
fn bm25_hierarchy_suite_recovers_an_expired_rebuild_lease() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    let expired = begin_rebuild(&connection).expect("first rebuild should start");
    connection
        .execute(
            "UPDATE graph_bm25_route_state SET rebuild_lease_expires_at_ms = 0",
            [],
        )
        .expect("rebuild lease should expire");

    let recovered = begin_rebuild(&connection).expect("expired rebuild should be recovered");
    assert!(matches!(
        renew_rebuild(&connection, &expired),
        Err(StorageError::Busy(_))
    ));
    finish_rebuild(&connection, &recovered, 0, None, None)
        .expect("recovered rebuild should finalize");
}

#[test]
fn bm25_hierarchy_suite_resumes_expired_rebuild_from_durable_checkpoint() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    let expired = begin_rebuild(&connection).expect("rebuild should start");
    assert_eq!(
        configure_rebuild(&connection, &expired, true, false).expect("rebuild plan should persist"),
        ("prepare".to_owned(), None, true, false)
    );
    checkpoint_rebuild(&connection, &expired, "evidence", Some("evidence-128"))
        .expect("rebuild checkpoint should persist");
    connection
        .execute(
            "UPDATE graph_bm25_route_state
             SET document_count = 128, rebuild_lease_expires_at_ms = 0",
            [],
        )
        .expect("rebuild lease should expire after progress");

    let recovered = begin_rebuild(&connection).expect("expired rebuild should be recovered");
    assert_eq!(
        configure_rebuild(&connection, &recovered, false, true)
            .expect("stored plan and cursor should resume"),
        (
            "evidence".to_owned(),
            Some("evidence-128".to_owned()),
            true,
            false
        )
    );
    let document_count = connection
        .query_row(
            "SELECT document_count FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("rebuild count should load");
    assert_eq!(document_count, 128);
    finish_rebuild(&connection, &recovered, 0, None, None)
        .expect("recovered rebuild should finalize");
}

#[test]
fn bm25_hierarchy_suite_restarts_checkpoint_after_algorithm_change() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    let expired = begin_rebuild(&connection).expect("rebuild should start");
    configure_rebuild(&connection, &expired, true, false).expect("plan should persist");
    checkpoint_rebuild(&connection, &expired, "evidence", Some("old-cursor"))
        .expect("old checkpoint should persist");
    connection
        .execute(
            "UPDATE graph_bm25_route_state
             SET algorithm_version = 'obsolete', document_count = 99,
                 rebuild_lease_expires_at_ms = 0",
            [],
        )
        .expect("old algorithm rebuild should expire");

    let recovered = begin_rebuild(&connection).expect("new algorithm should take over");
    assert_eq!(
        configure_rebuild(&connection, &recovered, false, true)
            .expect("new algorithm should reset its plan"),
        ("prepare".to_owned(), None, false, true)
    );
    let document_count = connection
        .query_row(
            "SELECT document_count FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("reset count should load");
    assert_eq!(document_count, 0);
    finish_rebuild(&connection, &recovered, 0, None, None).expect("reset rebuild should finalize");
}

#[test]
fn bm25_hierarchy_suite_fences_normal_derived_writes_during_rebuild() {
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-bm25-fence-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should follow the Unix epoch")
            .as_nanos()
    ));
    let connection = Connection::open(&path).expect("database should open");
    connection
        .execute_batch("PRAGMA journal_mode=WAL;")
        .expect("WAL should initialize");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    let lease = begin_rebuild(&connection).expect("route rebuild should start");

    let mut normal_writer = Connection::open(&path).expect("normal writer should open");
    let transaction = normal_writer
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("normal writer should acquire its write slot");
    assert!(matches!(
        ensure_rebuild_inactive(&transaction),
        Err(StorageError::Busy(_))
    ));
    transaction
        .rollback()
        .expect("blocked writer should roll back");

    finish_rebuild(&connection, &lease, 0, None, None).expect("empty rebuild should finalize");
    ensure_rebuild_inactive(&normal_writer)
        .expect("normal writes should resume after finalization");

    drop(normal_writer);
    drop(connection);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[test]
fn bm25_hierarchy_suite_batches_route_term_persistence() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    let content = (0..256)
        .map(|index| format!("boundedterm{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let route = prepare_document(Bm25RoutingText {
        source_scope: "scope",
        source_path: Some("src/large.rs"),
        entity_labels: "",
        entity_aliases: "",
        content: &content,
        graph_version: 1,
    });

    TRACED_ROUTE_STATEMENTS
        .lock()
        .expect("trace statements should lock")
        .clear();
    connection.trace(Some(capture_route_statement));
    replace_document(
        &connection,
        "large-document",
        1,
        "code_chunk",
        Some("src/large.rs"),
        "indexed",
        &route,
    )
    .expect("bounded route should persist");
    connection.trace(None);
    let statement_count = TRACED_ROUTE_STATEMENTS
        .lock()
        .expect("trace statements should lock")
        .len();

    assert!(
        statement_count <= 12,
        "route persistence issued {statement_count} statements for 256 terms"
    );

    for index in 1..257 {
        replace_document(
            &connection,
            &format!("large-document-{index}"),
            (index + 1) as i64,
            "code_chunk",
            Some("src/large.rs"),
            "indexed",
            &route,
        )
        .expect("additional route should persist");
    }
    let other_path_route = prepare_document(Bm25RoutingText {
        source_scope: "scope",
        source_path: Some("src/other.rs"),
        entity_labels: "",
        entity_aliases: "",
        content: &content,
        graph_version: 1,
    });
    replace_document(
        &connection,
        "retained-other-path",
        10_001,
        "code_chunk",
        Some("src/other.rs"),
        "indexed",
        &other_path_route,
    )
    .expect("other path route should persist");
    let other_scope_route = prepare_document(Bm25RoutingText {
        source_scope: "other-scope",
        source_path: Some("src/large.rs"),
        entity_labels: "",
        entity_aliases: "",
        content: &content,
        graph_version: 1,
    });
    replace_document(
        &connection,
        "retained-other-scope",
        10_002,
        "code_chunk",
        Some("src/large.rs"),
        "indexed",
        &other_scope_route,
    )
    .expect("other scope route should persist");
    TRACED_ROUTE_STATEMENTS
        .lock()
        .expect("trace statements should lock")
        .clear();
    connection.trace(Some(capture_route_statement));
    loop {
        let batch = code_document_batch(&connection, "scope", "src/large.rs")
            .expect("path batch should load");
        if batch.is_empty() {
            break;
        }
        let deleted = delete_code_document_batch(&connection, "scope", "src/large.rs", batch.len())
            .expect("path route batch should delete");
        assert_eq!(deleted, batch.len());
    }
    mark_graph_version(&connection, 2).expect("route version should advance");
    connection.trace(None);
    let delete_statement_count = TRACED_ROUTE_STATEMENTS
        .lock()
        .expect("trace statements should lock")
        .len();

    assert!(
        delete_statement_count <= 20,
        "route path deletion issued {delete_statement_count} statements for 257 documents"
    );
    assert!(
        code_document_batch(&connection, "scope", "src/large.rs")
            .expect("deleted path batch should load")
            .is_empty()
    );
    let remaining: (usize, usize, usize, usize, usize) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM graph_bm25_route_documents),
                (SELECT SUM(document_count) FROM graph_bm25_route_groups),
                (SELECT document_count FROM graph_bm25_route_state WHERE id = 1),
                (SELECT SUM(collection_frequency) FROM graph_bm25_route_terms
                 WHERE term = 'boundedterm0'),
                (SELECT document_frequency FROM graph_bm25_route_term_totals
                 WHERE term = 'boundedterm0')",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("retained route aggregates should load");
    assert_eq!(remaining, (2, 2, 2, 2, 2));
    let retained_identities: (usize, usize, usize) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM graph_bm25_route_documents
                 WHERE source_scope = 'scope' AND source_path = 'src/large.rs'),
                (SELECT COUNT(*) FROM graph_bm25_route_documents
                 WHERE document_id = 'retained-other-path'),
                (SELECT COUNT(*) FROM graph_bm25_route_documents
                 WHERE document_id = 'retained-other-scope')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("retained route identities should load");
    assert_eq!(retained_identities, (0, 1, 1));
}
