use super::*;

#[test]
fn legacy_call_documents_inherit_their_file_language() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL
            );
            CREATE TABLE code_repository_calls (
                source_scope TEXT NOT NULL,
                call_id TEXT NOT NULL,
                path TEXT NOT NULL,
                caller_name TEXT,
                callee_name TEXT NOT NULL,
                target_hint TEXT
            );
            INSERT INTO code_repository_files VALUES ('scope', 'src/lib.rs', 'rust');
            INSERT INTO code_repository_calls
            VALUES ('scope', 'call-1', 'src/lib.rs', 'Caller', 'target_fn', 'target_hint');
            ",
        )
        .expect("legacy call tables should initialize");

    backfill_search_calls(&connection).expect("legacy call should backfill");

    let (language_id, content): (String, String) = connection
        .query_row(
            "
            SELECT language_id, content
            FROM code_repository_search
            WHERE document_kind = 'call' AND record_id = 'call-1'
            ",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("call search row should load");
    assert_eq!(language_id, "rust");
    assert!(content.contains("Caller target_fn target_hint"));
}

#[test]
fn search_metadata_sync_is_idempotent() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
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
            INSERT INTO code_repository_search
            VALUES ('scope', 'symbol', 'symbol-1', 'src/lib.rs', 'rust', 'LegacyThing');
            ",
        )
        .expect("search tables should initialize");

    sync_code_repository_search_metadata(&connection).expect("metadata should sync");
    sync_code_repository_search_metadata(&connection).expect("repeated sync should be idempotent");

    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_search_metadata",
            [],
            |row| row.get(0),
        )
        .expect("metadata count should load");
    assert_eq!(count, 1);
}
