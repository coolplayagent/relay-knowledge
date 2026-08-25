//! Direct grouped-reference preflight boundary contracts.

use rusqlite::Connection;

use super::require_full_grouped_projection_within_budget;
use crate::domain::{CodeIndexResourceBudget, CodeIndexSnapshot};
use crate::storage::sqlite::code::code_tests::snapshot_with_resolved_reference;

#[test]
fn code_index_task_direct_zero_reference_manifest_respects_byte_budget_without_writes() {
    let mut connection = projection_database();
    let transaction = connection.transaction().expect("transaction should open");
    let error = require_full_grouped_projection_within_budget(
        &transaction,
        &empty_snapshot(),
        CodeIndexResourceBudget::new(2, 1, 2).expect("budget should build"),
    )
    .expect_err("the two manifest mutations must fit the byte budget");

    assert!(matches!(
        error,
        crate::storage::StorageError::CapacityExceeded(_)
    ));
    assert_eq!(
        transaction
            .query_row(
                "SELECT (SELECT COUNT(*) FROM code_repository_reference_search_groups)
                      + (SELECT COUNT(*) FROM code_repository_reference_search_manifests)
                      + (SELECT COUNT(*) FROM code_repository_search_metadata)",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("owner rows should count"),
        0
    );
    transaction.rollback().expect("preflight should roll back");
}

#[test]
fn code_index_task_direct_full_replace_rejects_existing_reference_owner_before_delete() {
    let mut connection = projection_database();
    connection
        .execute(
            "INSERT INTO code_repository_reference_search_manifests
             VALUES ('scope', 2, 0, 0)",
            [],
        )
        .expect("existing manifest should insert");
    let transaction = connection.transaction().expect("transaction should open");

    let error = require_full_grouped_projection_within_budget(
        &transaction,
        &empty_snapshot(),
        CodeIndexResourceBudget::new(2, 1024, 8).expect("budget should build"),
    )
    .expect_err("direct full replacement must defer existing owners to staged cleanup");

    assert!(matches!(
        error,
        crate::storage::StorageError::CapacityExceeded(_)
    ));
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_reference_search_manifests",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("manifest should remain"),
        1
    );
    transaction.rollback().expect("preflight should roll back");
}

#[test]
fn code_index_task_direct_projection_bills_duplicate_facts_as_one_group_owner() {
    let mut connection = projection_database();
    let mut snapshot = snapshot_with_resolved_reference();
    let mut duplicate = snapshot.references[0].clone();
    duplicate.reference_id = "target-reference-duplicate".to_owned();
    snapshot.references.push(duplicate);
    let transaction = connection.transaction().expect("transaction should open");

    require_full_grouped_projection_within_budget(
        &transaction,
        &snapshot,
        CodeIndexResourceBudget::new(2, 1024, 5).expect("budget should build"),
    )
    .expect("two equivalent facts should require only one three-row group owner");

    transaction.rollback().expect("preflight should roll back");
}

fn projection_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_reference_search_groups (
                 source_scope TEXT NOT NULL,
                 group_id TEXT NOT NULL,
                 PRIMARY KEY (source_scope, group_id)
             );
             CREATE TABLE code_repository_reference_search_manifests (
                 source_scope TEXT NOT NULL PRIMARY KEY,
                 projection_version INTEGER NOT NULL,
                 reference_count INTEGER NOT NULL,
                 group_count INTEGER NOT NULL
             );
             CREATE TABLE code_repository_search_metadata (
                 source_scope TEXT NOT NULL,
                 document_kind TEXT NOT NULL,
                 record_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 search_rowid INTEGER NOT NULL
             );",
        )
        .expect("projection schema should initialize");
    connection
}

fn empty_snapshot() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: Vec::new(),
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}
