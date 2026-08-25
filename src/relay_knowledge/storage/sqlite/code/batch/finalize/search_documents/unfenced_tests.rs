//! Direct tests for the bounded synchronous reference-search fallback.

use rusqlite::{Connection, params};

use super::{rebuild_reference_search_documents, tests::search_database};
use crate::domain::CodeIndexResourceBudget;

#[test]
fn code_index_task_unfenced_reference_search_builds_group_owner_and_manifest() {
    let mut connection = search_database();
    seed_reference_and_stale_search_row(&connection);
    connection
        .execute(
            "DELETE FROM code_repository_search_metadata WHERE record_id = 'stale'",
            [],
        )
        .expect("legacy owner should delete for empty-owner sync path");
    connection
        .execute(
            "DELETE FROM code_repository_search WHERE record_id = 'stale'",
            [],
        )
        .expect("legacy FTS row should delete for empty-owner sync path");
    let transaction = connection.transaction().expect("transaction should open");

    rebuild_reference_search_documents(
        &transaction,
        "scope",
        CodeIndexResourceBudget::new(64, 1024 * 1024, 64).expect("budget should build"),
        1,
    )
    .expect("reference search should rebuild");

    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search WHERE record_id = 'stale'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("stale rows should count"),
        0
    );
    let rebuilt = transaction
        .query_row(
            "SELECT language_id, content FROM code_repository_search
             WHERE source_scope = 'scope' AND document_kind = 'reference'
               AND record_id = 'reference:1'",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("rebuilt row should load");
    assert_eq!(rebuilt.0, "rust");
    assert!(rebuilt.1.contains("connect"));
    assert!(rebuilt.1.contains("src/client.rs"));
    for term in [
        "connecthttpclient",
        "crate",
        "net",
        "httpclient",
        "connect",
        "src",
        "client",
    ] {
        assert_eq!(
            transaction
                .query_row(
                    "SELECT COUNT(*) FROM code_repository_search
                     WHERE code_repository_search MATCH ?1
                       AND source_scope = 'scope' AND document_kind = 'reference'",
                    params![term],
                    |row| row.get::<_, usize>(0),
                )
                .expect("canonical grouped term should remain searchable"),
            1,
            "missing canonical grouped reference term {term}"
        );
    }
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata
                 WHERE source_scope = 'scope' AND document_kind = 'reference'
                   AND record_id = 'reference:1'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("metadata should count"),
        1
    );
    assert_eq!(
        transaction
            .query_row(
                "SELECT projection_version, reference_count, group_count
                 FROM code_repository_reference_search_manifests WHERE source_scope = 'scope'",
                [],
                |row| {
                    Ok((
                        row.get::<_, usize>(0)?,
                        row.get::<_, usize>(1)?,
                        row.get::<_, usize>(2)?,
                    ))
                },
            )
            .expect("unfenced grouped manifest should load"),
        (2, 1, 1)
    );
}

#[test]
fn code_index_task_unfenced_reference_search_bills_duplicate_facts_as_one_group_owner() {
    let mut connection = search_database();
    seed_reference_and_stale_search_row(&connection);
    connection
        .execute(
            "INSERT INTO code_repository_references (
                 source_scope, reference_id, path, name, kind, target_hint
             )
             SELECT source_scope, 'reference:2', path, name, kind, target_hint
             FROM code_repository_references WHERE reference_id = 'reference:1'",
            [],
        )
        .expect("duplicate grouped fact should insert");
    connection
        .execute(
            "DELETE FROM code_repository_search_metadata WHERE record_id = 'stale'",
            [],
        )
        .expect("legacy metadata should delete");
    connection
        .execute(
            "DELETE FROM code_repository_search WHERE record_id = 'stale'",
            [],
        )
        .expect("legacy FTS row should delete");
    let transaction = connection.transaction().expect("transaction should open");

    rebuild_reference_search_documents(
        &transaction,
        "scope",
        CodeIndexResourceBudget::new(2, 1024, 8).expect("budget should build"),
        2,
    )
    .expect("one grouped owner should fit even when two facts share it");

    assert_eq!(
        transaction
            .query_row(
                "SELECT reference_count, group_count
                 FROM code_repository_reference_search_manifests WHERE source_scope = 'scope'",
                [],
                |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)),
            )
            .expect("grouped manifest should load"),
        (2, 1)
    );
    assert_eq!(
        transaction
            .query_row(
                "SELECT occurrence_count FROM code_repository_reference_search_groups
                 WHERE source_scope = 'scope'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("group occurrence count should load"),
        2
    );
}

#[test]
fn code_index_task_unfenced_zero_reference_manifest_respects_byte_budget() {
    let mut connection = search_database();
    let transaction = connection.transaction().expect("transaction should open");

    let error = rebuild_reference_search_documents(
        &transaction,
        "scope",
        CodeIndexResourceBudget::new(2, 1, 5).expect("budget should build"),
        0,
    )
    .expect_err("even a zero-reference manifest must fit the byte budget");

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
            .expect("manifest rows should count"),
        0
    );
    transaction
        .rollback()
        .expect("failed zero-reference rebuild should roll back");
}

fn seed_reference_and_stale_search_row(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO code_repository_files (source_scope, path, language_id)
             VALUES ('scope', 'src/client.rs', 'rust')",
            [],
        )
        .expect("file should be inserted");
    connection
        .execute(
            "INSERT INTO code_repository_references (
                 source_scope, reference_id, path, name, kind, target_hint
             ) VALUES (
                 'scope', 'reference:1', 'src/client.rs', 'connectHttpClient', 'call',
                 'crate::net::HttpClient::connect'
             )",
            [],
        )
        .expect("reference should be inserted");
    connection
        .execute(
            "INSERT INTO code_repository_search (
                 source_scope, document_kind, record_id, path, language_id, content
             ) VALUES ('scope', 'reference', 'stale', 'src/old.rs', 'rust', 'old')",
            [],
        )
        .expect("stale search row should be inserted");
    let search_rowid = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO code_repository_search_metadata (
                 source_scope, document_kind, record_id, path, search_rowid
             ) VALUES ('scope', 'reference', 'stale', 'src/old.rs', ?1)",
            params![search_rowid],
        )
        .expect("stale metadata should be inserted");
}
