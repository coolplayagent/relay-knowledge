//! Direct grouped-reference expansion contracts.

use rusqlite::{Connection, params};

use super::{
    ReferenceSearchCandidate, expand_grouped_reference_candidates,
    expand_grouped_reference_candidates_with_progress_budget,
    reference_search_projection_is_current,
};
use crate::domain::CodeRepositoryStatus;

#[test]
fn code_index_task_grouped_reference_expansion_preserves_occurrences_and_fair_limit() {
    let connection = grouped_reference_database();
    insert_group(&connection, "a", "alpha", 3);
    insert_group(&connection, "b", "beta", 2);
    let candidates = vec![candidate("a:1", "alpha", 3), candidate("b:1", "beta", 2)];

    let expanded = expand_grouped_reference_candidates(&connection, "scope", &candidates, 5)
        .expect("grouped occurrences should expand");

    assert_eq!(
        expanded,
        vec![
            "a:1".to_owned(),
            "b:1".to_owned(),
            "a:2".to_owned(),
            "b:2".to_owned(),
            "a:3".to_owned(),
        ]
    );
    let truncated = expand_grouped_reference_candidates(&connection, "scope", &candidates, 3)
        .expect("bounded grouped occurrences should expand fairly");
    assert_eq!(
        truncated,
        vec!["a:1".to_owned(), "b:1".to_owned(), "a:2".to_owned()]
    );
}

#[test]
fn code_index_task_grouped_reference_expansion_rejects_corrupt_occurrence_count() {
    let connection = grouped_reference_database();
    insert_group(&connection, "a", "alpha", 2);
    let candidates = vec![candidate("a:1", "alpha", 3)];

    let error = expand_grouped_reference_candidates(&connection, "scope", &candidates, 3)
        .expect_err("manifest-owned occurrence count must match exact facts");

    assert!(matches!(
        error,
        crate::storage::StorageError::Invariant(message)
            if message.contains("occurrence count")
    ));
}

#[test]
fn code_index_task_grouped_reference_manifest_is_the_scope_capability_gate() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_reference_search_manifests (
                 source_scope TEXT NOT NULL PRIMARY KEY,
                 projection_version INTEGER NOT NULL,
                 reference_count INTEGER NOT NULL,
                 group_count INTEGER NOT NULL
             );",
        )
        .expect("manifest schema should initialize");
    let status = status(5);
    assert!(
        !reference_search_projection_is_current(&connection, &status)
            .expect("missing manifest should be observable")
    );
    connection
        .execute(
            "INSERT INTO code_repository_reference_search_manifests
             VALUES ('scope', 2, 5, 2)",
            [],
        )
        .expect("manifest should insert");
    assert!(
        reference_search_projection_is_current(&connection, &status)
            .expect("exact manifest should validate")
    );
    connection
        .execute(
            "UPDATE code_repository_reference_search_manifests
             SET reference_count = 4 WHERE source_scope = 'scope'",
            [],
        )
        .expect("manifest should corrupt");
    assert!(
        !reference_search_projection_is_current(&connection, &status)
            .expect("count drift should be observable")
    );
}

#[test]
fn code_index_task_grouped_reference_capacity_error_clears_progress_handler() {
    let connection = grouped_reference_database();
    insert_group(&connection, "a", "alpha", 2);
    let candidates = vec![candidate("a:1", "alpha", 2)];

    let error = expand_grouped_reference_candidates_with_progress_budget(
        &connection,
        "scope",
        &candidates,
        2,
        1,
        0,
    )
    .expect_err("the test VM budget should interrupt optional occurrence expansion");

    assert!(matches!(
        error,
        crate::storage::StorageError::CapacityExceeded(_)
    ));
    assert_eq!(
        connection
            .query_row("SELECT 1", [], |row| row.get::<_, usize>(0))
            .expect("cleared handler must allow connection reuse"),
        1
    );
}

fn candidate(record_id: &str, name: &str, occurrence_count: usize) -> ReferenceSearchCandidate {
    ReferenceSearchCandidate {
        record_id: record_id.to_owned(),
        name: Some(name.to_owned()),
        kind: Some("call".to_owned()),
        path: Some("src/lib.rs".to_owned()),
        target_hint: Some("target".to_owned()),
        occurrence_count: Some(occurrence_count),
    }
}

fn insert_group(connection: &Connection, prefix: &str, name: &str, count: usize) {
    for ordinal in 1..=count {
        connection
            .execute(
                "INSERT INTO code_repository_references (
                     source_scope, reference_id, name, kind, path, target_hint
                 ) VALUES ('scope', ?1, ?2, 'call', 'src/lib.rs', 'target')",
                params![format!("{prefix}:{ordinal}"), name],
            )
            .expect("reference occurrence should insert");
    }
}

fn grouped_reference_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_references (
                 source_scope TEXT NOT NULL,
                 reference_id TEXT NOT NULL,
                 name TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 path TEXT NOT NULL,
                 target_hint TEXT,
                 PRIMARY KEY (source_scope, reference_id)
             );
             CREATE INDEX code_repository_references_lookup
             ON code_repository_references(source_scope, name, kind, path);",
        )
        .expect("reference fixture schema should initialize");
    connection
}

fn status(reference_count: usize) -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some("scope".to_owned()),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "fresh".to_owned(),
        indexed_file_count: 1,
        symbol_count: 0,
        reference_count,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    }
}
