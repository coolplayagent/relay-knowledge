use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite::{Connection, params};

use super::super::rebuild_budget::{REBUILD_DOCUMENT_BATCH_SIZE, REBUILD_SOURCE_BYTES_PER_BATCH};
use super::{
    CompanionRebuildPlan, LOCAL_TOKENIZER_VERSION, PHASE_EVIDENCE, derived_documents_missing,
    evidence_rebuild_page, prepare_rebuild_generation, rebuild_bm25_documents,
    rebuild_evidence_document,
};

static REBUILD_COMMIT_COUNT: AtomicUsize = AtomicUsize::new(0);

fn capture_rebuild_transaction(sql: &str) {
    if sql.trim().eq_ignore_ascii_case("COMMIT") {
        REBUILD_COMMIT_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn missing_documents_follow_retrievable_source_count() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should initialize");
    super::super::schema::execute_retrieval_schema(&connection)
        .expect("retrieval schema should initialize");
    connection
        .execute(
            "UPDATE graph_bm25_route_state
             SET semantic_generation = ?1, vector_generation = ?1
             WHERE id = 1",
            [LOCAL_TOKENIZER_VERSION],
        )
        .expect("empty companion generation should become current");

    assert!(!derived_documents_missing(&connection).expect("empty state should inspect"));

    connection
        .execute("INSERT INTO evidence (status) VALUES ('accepted')", [])
        .expect("evidence should insert");
    assert!(derived_documents_missing(&connection).expect("missing documents should inspect"));
}

#[test]
fn companion_generation_metadata_requires_rebuild_without_scanning_document_versions() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should initialize");
    super::super::schema::execute_retrieval_schema(&connection)
        .expect("retrieval schema should initialize");

    assert!(derived_documents_missing(&connection).expect("unknown generation should inspect"));
    connection
        .execute(
            "UPDATE graph_bm25_route_state
             SET semantic_generation = ?1, vector_generation = ?1
             WHERE id = 1",
            [LOCAL_TOKENIZER_VERSION],
        )
        .expect("current companion generation should persist");
    assert!(!derived_documents_missing(&connection).expect("current generation should inspect"));
}

#[test]
fn missing_route_generation_state_requires_rebuild_even_for_empty_sources() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should initialize");
    super::super::schema::execute_retrieval_schema(&connection)
        .expect("retrieval schema should initialize");
    connection
        .execute("DELETE FROM graph_bm25_route_state", [])
        .expect("route state should delete");

    assert!(derived_documents_missing(&connection).expect("missing state should inspect"));
}

#[test]
fn bm25_hierarchy_suite_rebuilds_in_bounded_transactions() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let mut connection = store.connection.lock().expect("connection should lock");
    connection
        .execute_batch(
            "WITH RECURSIVE sequence(value) AS (
                 SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 129
             )
             INSERT INTO evidence (id, source_scope, content, created_graph_version)
             SELECT printf('evidence-%03d', value), 'scope', 'bounded rebuild content', 1
             FROM sequence;
             UPDATE graph_state SET graph_version = 1 WHERE id = 1;",
        )
        .expect("authoritative rebuild fixture should insert");
    REBUILD_COMMIT_COUNT.store(0, Ordering::Relaxed);
    connection.trace(Some(capture_rebuild_transaction));

    rebuild_bm25_documents(&connection, |_| Ok(())).expect("bounded rebuild should complete");
    connection.trace(None);

    let state: (String, usize, usize) = connection
        .query_row(
            "SELECT state, document_count, (SELECT COUNT(*) FROM graph_bm25)
             FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("rebuilt state should load");
    assert_eq!(state, ("fresh".to_owned(), 129, 129));
    assert!(
        REBUILD_COMMIT_COUNT.load(Ordering::Relaxed) >= 10,
        "clear plus two 128-document pages must commit as bounded transactions"
    );
}

#[test]
fn bm25_hierarchy_suite_route_rebuild_preserves_current_companion_indexes() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    connection
        .execute_batch(
            "INSERT INTO evidence (id, source_scope, content, created_graph_version)
             VALUES ('evidence', 'scope', 'route-only rebuild', 1);
             UPDATE graph_state SET graph_version = 1 WHERE id = 1;",
        )
        .expect("authoritative fixture should insert");
    rebuild_bm25_documents(&connection, |_| Ok(())).expect("initial rebuild should complete");
    connection
        .execute_batch(
            "UPDATE graph_semantic_documents SET source_hash = 'semantic-sentinel';
             UPDATE graph_vector_documents SET source_hash = 'vector-sentinel';
             UPDATE graph_bm25_route_state SET state = 'stale';",
        )
        .expect("current companion sentinels should install");

    rebuild_bm25_documents(&connection, |_| Ok(())).expect("route-only rebuild should complete");

    let source_hashes = connection
        .query_row(
            "SELECT
                 (SELECT source_hash FROM graph_semantic_documents WHERE document_id='evidence:evidence'),
                 (SELECT source_hash FROM graph_vector_documents WHERE document_id='evidence:evidence')",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("companion source hashes should load");
    assert_eq!(
        source_hashes,
        ("semantic-sentinel".to_owned(), "vector-sentinel".to_owned())
    );
}

#[test]
fn bm25_hierarchy_suite_repairs_equal_count_companion_identity_drift() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    connection
        .execute_batch(
            "INSERT INTO evidence (id, source_scope, content, created_graph_version)
             VALUES ('evidence', 'scope', 'identity repair', 1);
             UPDATE graph_state SET graph_version = 1 WHERE id = 1;",
        )
        .expect("authoritative identity fixture should insert");
    rebuild_bm25_documents(&connection, |_| Ok(())).expect("initial rebuild should complete");
    connection
        .execute_batch(
            "UPDATE graph_semantic_documents
             SET document_id = 'evidence:orphan'
             WHERE document_id = 'evidence:evidence';
             UPDATE graph_vector_documents SET source_hash = 'vector-sentinel';
             UPDATE graph_bm25_route_state SET state = 'stale';",
        )
        .expect("equal-count identity drift should install");

    rebuild_bm25_documents(&connection, |_| Ok(())).expect("identity drift should repair");

    let repaired = connection
        .query_row(
            "SELECT
                 EXISTS(SELECT 1 FROM graph_semantic_documents
                        WHERE document_id = 'evidence:evidence'),
                 EXISTS(SELECT 1 FROM graph_semantic_documents
                        WHERE document_id = 'evidence:orphan'),
                 (SELECT source_hash FROM graph_vector_documents
                  WHERE document_id = 'evidence:evidence')",
            [],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .expect("repaired companions should load");
    assert_eq!(repaired, (true, false, "vector-sentinel".to_owned()));
}

#[test]
fn bm25_hierarchy_suite_replays_rebuild_from_the_persisted_document_cursor() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    connection
        .execute_batch(
            "WITH RECURSIVE sequence(value) AS (
                 SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 129
             )
             INSERT INTO evidence (id, source_scope, content, created_graph_version)
             SELECT printf('resume-%03d', value), 'scope', 'durable cursor content', 1
             FROM sequence;
             UPDATE graph_state SET graph_version = 1 WHERE id = 1;",
        )
        .expect("authoritative resume fixture should insert");
    let lease = super::bm25_routing::begin_rebuild(&connection).expect("rebuild should start");
    super::bm25_routing::configure_rebuild(&connection, &lease, true, true)
        .expect("companion plan should persist");
    prepare_rebuild_generation(&connection, &lease).expect("shadow table should prepare");
    super::bm25_routing::checkpoint_rebuild(&connection, &lease, PHASE_EVIDENCE, None)
        .expect("empty clear phases should checkpoint");
    let page = evidence_rebuild_page(&connection, None).expect("first evidence page should load");
    assert_eq!(page.keys.len(), REBUILD_DOCUMENT_BATCH_SIZE);
    let transaction = connection
        .unchecked_transaction()
        .expect("first rebuild page should start");
    super::bm25_routing::renew_rebuild(&transaction, &lease).expect("lease should renew");
    for key in &page.keys {
        rebuild_evidence_document(
            &transaction,
            key,
            CompanionRebuildPlan {
                semantic: true,
                vector: true,
            },
        )
        .expect("first evidence page should rebuild");
    }
    let cursor = page
        .keys
        .last()
        .expect("first page should have a cursor")
        .evidence_id
        .as_str();
    super::bm25_routing::checkpoint_rebuild(&transaction, &lease, PHASE_EVIDENCE, Some(cursor))
        .expect("first evidence cursor should persist atomically");
    transaction.commit().expect("first page should commit");
    connection
        .execute(
            "UPDATE graph_bm25_route_state SET rebuild_lease_expires_at_ms = 0",
            [],
        )
        .expect("partial rebuild lease should expire");

    rebuild_bm25_documents(&connection, |_| Ok(())).expect("takeover should resume and finish");

    let counts = connection
        .query_row(
            "SELECT
                 (SELECT COUNT(*) FROM graph_bm25),
                 (SELECT COUNT(*) FROM graph_bm25_route_documents),
                 document_count, rebuild_phase, rebuild_cursor
             FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, usize>(0)?,
                    row.get::<_, usize>(1)?,
                    row.get::<_, usize>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .expect("resumed generation should load");
    assert_eq!(counts, (129, 129, 129, None, None));
}

#[test]
fn bm25_hierarchy_suite_checkpoints_oversized_document_before_the_following_page() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    let oversized_content = "x".repeat(REBUILD_SOURCE_BYTES_PER_BATCH as usize + 1);
    connection
        .execute(
            "INSERT INTO evidence (id, source_scope, content, created_graph_version)
             VALUES ('oversized', 'scope', ?1, 1), ('subsequent', 'scope', 'small', 1)",
            params![oversized_content],
        )
        .expect("oversized authoritative fixture should insert");
    connection
        .execute("UPDATE graph_state SET graph_version = 1 WHERE id = 1", [])
        .expect("graph version should advance");
    let lease = super::bm25_routing::begin_rebuild(&connection).expect("rebuild should start");
    super::bm25_routing::configure_rebuild(&connection, &lease, true, true)
        .expect("companion plan should persist");
    prepare_rebuild_generation(&connection, &lease).expect("shadow table should prepare");
    super::bm25_routing::checkpoint_rebuild(&connection, &lease, PHASE_EVIDENCE, None)
        .expect("empty clear phases should checkpoint");

    let page = evidence_rebuild_page(&connection, None).expect("oversized page should load");
    assert_eq!(
        page.keys
            .iter()
            .map(|key| key.evidence_id.as_str())
            .collect::<Vec<_>>(),
        ["oversized"]
    );
    assert!(!page.page_is_complete);
    let transaction = connection
        .unchecked_transaction()
        .expect("oversized page transaction should start");
    super::bm25_routing::renew_rebuild(&transaction, &lease).expect("lease should renew");
    rebuild_evidence_document(
        &transaction,
        &page.keys[0],
        CompanionRebuildPlan {
            semantic: true,
            vector: true,
        },
    )
    .expect("oversized document should rebuild in isolation");
    super::bm25_routing::checkpoint_rebuild(
        &transaction,
        &lease,
        PHASE_EVIDENCE,
        Some(&page.keys[0].evidence_id),
    )
    .expect("oversized cursor should checkpoint with its derived writes");
    transaction.commit().expect("oversized page should commit");

    let checkpoint = connection
        .query_row(
            "SELECT rebuild_phase, rebuild_cursor,
                    (SELECT COUNT(*) FROM graph_bm25_rebuild)
             FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, usize>(2)?,
                ))
            },
        )
        .expect("oversized checkpoint should load");
    assert_eq!(
        checkpoint,
        (PHASE_EVIDENCE.to_owned(), "oversized".to_owned(), 1)
    );

    let resumed_page = evidence_rebuild_page(&connection, Some(&page.keys[0]))
        .expect("following page should load from checkpoint");
    assert_eq!(
        resumed_page
            .keys
            .iter()
            .map(|key| key.evidence_id.as_str())
            .collect::<Vec<_>>(),
        ["subsequent"]
    );
    assert!(resumed_page.page_is_complete);
}
