use rusqlite::{Connection, params};

use super::{super::progress, load_page};
use crate::storage::StorageError;

#[test]
fn oversized_search_owner_is_rejected_before_canonical_payload_fetch() {
    let mut connection = search_database();
    insert_owner(
        &connection,
        "base",
        "reference",
        "missing-group",
        "src/lib.rs",
        &"x".repeat(128 * 1024),
    );
    let current = progress("base", "target");
    let transaction = connection.transaction().expect("transaction should begin");

    let error = match load_page(&transaction, &current, 1, 256) {
        Err(error) => error,
        Ok(_) => panic!("oversized owner should fail before grouped canonical validation"),
    };

    assert!(matches!(error, StorageError::CapacityExceeded(_)));
}

#[test]
fn admitted_search_range_bulk_copies_exact_metadata_owners_and_skips_affected_paths() {
    let mut connection = search_database();
    for (record_id, path) in [("a", "a.rs"), ("b", "b.rs"), ("c", "c.rs")] {
        insert_owner(&connection, "base", "chunk", record_id, path, record_id);
    }
    connection
        .execute(
            "INSERT INTO code_repository_incremental_clone_affected_paths VALUES ('target', 'b.rs')",
            [],
        )
        .expect("affected path should insert");
    let current = progress("base", "target");
    let transaction = connection.transaction().expect("transaction should begin");
    let page = load_page(&transaction, &current, 3, 1_000_000).expect("bounded range should load");
    let last = page.last.as_ref().expect("range should have a cursor");

    let copied = super::super::search_bulk::copy(
        &transaction,
        &current,
        super::super::search_bulk::AdmittedRange {
            last_kind: &last.document_kind,
            last_record_id: &last.record_id,
            row_count: page.row_count,
            affected_count: page.affected_count,
        },
    )
    .expect("range should bulk copy");
    transaction.commit().expect("range should commit");

    let (search_rows, metadata_rows, affected_rows): (usize, usize, usize) = connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM code_repository_search WHERE source_scope = 'target'),
                 (SELECT count(*) FROM code_repository_search_metadata WHERE source_scope = 'target'),
                 (SELECT count(*) FROM code_repository_search_metadata
                  WHERE source_scope = 'target' AND path = 'b.rs')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("copied owners should inspect");
    assert_eq!(
        (copied, search_rows, metadata_rows, affected_rows),
        (2, 2, 2, 0)
    );
}

#[test]
fn maximum_search_rowid_rejects_bulk_clone_before_any_target_owner_is_added() {
    let mut connection = search_database();
    insert_owner(&connection, "base", "chunk", "a", "a.rs", "content");
    connection
        .execute(
            "INSERT INTO code_repository_search (
                 rowid, source_scope, document_kind, record_id, path, language_id, content
             ) VALUES (?1, 'sentinel', 'chunk', 'max', 'max.rs', 'rust', 'sentinel')",
            [i64::MAX],
        )
        .expect("maximum rowid sentinel should insert");
    let current = progress("base", "target");
    let transaction = connection.transaction().expect("transaction should begin");
    let page = load_page(&transaction, &current, 1, 1_000_000).expect("base range should admit");
    let last = page.last.as_ref().expect("range should have a cursor");

    let error = super::super::search_bulk::copy(
        &transaction,
        &current,
        super::super::search_bulk::AdmittedRange {
            last_kind: &last.document_kind,
            last_record_id: &last.record_id,
            row_count: page.row_count,
            affected_count: page.affected_count,
        },
    )
    .expect_err("maximum rowid must reject automatic range allocation");
    transaction
        .rollback()
        .expect("failed range should roll back");

    let target_count = connection
        .query_row(
            "SELECT count(*) FROM code_repository_search WHERE source_scope = 'target'",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("target rows should inspect");
    assert!(matches!(error, StorageError::Invariant(message) if message.contains("maximum")));
    assert_eq!(target_count, 0);
}

fn search_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_search (
                 source_scope TEXT NOT NULL, document_kind TEXT NOT NULL,
                 record_id TEXT NOT NULL, path TEXT NOT NULL,
                 language_id TEXT NOT NULL, content TEXT NOT NULL
             );
             CREATE TABLE code_repository_search_metadata (
                 source_scope TEXT NOT NULL, document_kind TEXT NOT NULL,
                 record_id TEXT NOT NULL, path TEXT NOT NULL, search_rowid INTEGER NOT NULL,
                 UNIQUE (source_scope, document_kind, record_id)
             );
             CREATE TABLE code_repository_reference_search_groups (
                 source_scope TEXT NOT NULL, group_id TEXT NOT NULL, name TEXT NOT NULL,
                 kind TEXT NOT NULL, path TEXT NOT NULL, target_hint TEXT NOT NULL,
                 language_id TEXT NOT NULL, occurrence_count INTEGER NOT NULL
             );
             CREATE TABLE code_repository_incremental_clone_affected_paths (
                 source_scope TEXT NOT NULL, path TEXT NOT NULL,
                 PRIMARY KEY (source_scope, path)
             );",
        )
        .expect("search schema should initialize");
    connection
}

fn insert_owner(
    connection: &Connection,
    scope: &str,
    kind: &str,
    record_id: &str,
    path: &str,
    content: &str,
) {
    connection
        .execute(
            "INSERT INTO code_repository_search
                 (source_scope, document_kind, record_id, path, language_id, content)
             VALUES (?1, ?2, ?3, ?4, 'rust', ?5)",
            params![scope, kind, record_id, path, content],
        )
        .expect("search row should insert");
    connection
        .execute(
            "INSERT INTO code_repository_search_metadata VALUES (?1, ?2, ?3, ?4, ?5)",
            params![scope, kind, record_id, path, connection.last_insert_rowid()],
        )
        .expect("metadata row should insert");
}

fn progress(base_scope: &str, source_scope: &str) -> progress::CloneProgress {
    progress::CloneProgress {
        source_scope: source_scope.to_owned(),
        repository_id: "repo".to_owned(),
        base_scope: base_scope.to_owned(),
        task_id: "task".to_owned(),
        delta_digest: "digest".to_owned(),
        phase: progress::PHASE_SEARCH.to_owned(),
        table_ordinal: super::super::table_count(),
        completed_page_ordinal: 1,
        cursor_key: None,
        cursor_tiebreaker: None,
        completed_table_ordinal: Some(super::super::table_count() - 1),
        expected_table_rows: Some(0),
        scanned_table_rows: 0,
        copied_table_rows: 0,
        scanned_total_rows: 0,
        copied_total_rows: 0,
        copied_total_bytes: 0,
        cloned_file_count: 0,
        cloned_symbol_count: 0,
        cloned_reference_count: 0,
        cloned_chunk_count: 0,
        cloned_diagnostic_count: 0,
        cloned_reference_group_count: 0,
        cloned_search_document_count: 0,
        base_manifest_reference_count: 0,
        base_manifest_group_count: 0,
        scanned_reference_occurrence_count: 0,
        scanned_reference_row_count: 0,
        scanned_reference_group_count: 0,
        scanned_reference_search_owner_count: 0,
        base_source_fact_row_upper_bound: 1,
        page_row_limit: 16,
        page_byte_limit: 1_000_000,
    }
}
