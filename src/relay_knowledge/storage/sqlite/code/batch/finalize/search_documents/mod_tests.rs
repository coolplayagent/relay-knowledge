//! Direct tests for finalized search-document replacement and metadata synchronization.

use rusqlite::{Connection, params};

use super::rebuild_reference_search_documents;

#[test]
fn reference_search_rebuild_replaces_stale_rows_and_inherits_file_language() {
    let mut connection = search_database();
    seed_reference_and_stale_search_row(&connection);
    let transaction = connection.transaction().expect("transaction should open");

    rebuild_reference_search_documents(&transaction, "scope")
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
             ) VALUES ('scope', 'reference:1', 'src/client.rs', 'connect', 'call', 'Client::connect')",
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

fn search_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL
            );
            CREATE TABLE code_repository_references (
                source_scope TEXT NOT NULL,
                reference_id TEXT NOT NULL,
                path TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                target_hint TEXT
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            CREATE TABLE code_repository_search_metadata (
                source_scope TEXT NOT NULL,
                document_kind TEXT NOT NULL,
                record_id TEXT NOT NULL,
                path TEXT NOT NULL,
                search_rowid INTEGER NOT NULL UNIQUE,
                PRIMARY KEY (source_scope, document_kind, record_id)
            );
            ",
        )
        .expect("search schema should be created");
    connection
}
