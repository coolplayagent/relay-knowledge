use rusqlite::Connection;

use super::{
    CALL_SEARCH_SIGNATURE_MIGRATION, EDGE_SEARCH_LANGUAGE_ID_MIGRATION, SEARCH_BACKFILL_MIGRATION,
    SEARCH_METADATA_BACKFILL_MIGRATION, code_schema_migration_applied, initialize_code_schema,
};

#[test]
fn backfills_legacy_call_search_without_symbol_link_columns() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
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
            INSERT INTO code_repository_files (source_scope, path, language_id)
            VALUES ('scope', 'src/lib.rs', 'rust');
            INSERT INTO code_repository_calls (
                source_scope, call_id, path, caller_name, callee_name, target_hint
            )
            VALUES ('scope', 'call-1', 'src/lib.rs', 'LegacyCaller', 'target_fn', 'target_hint');
            ",
        )
        .expect("legacy schema should initialize");

    initialize_code_schema(&connection).expect("code schema should initialize");

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
        .expect("call search row should be backfilled");

    assert_eq!(language_id, "rust");
    assert!(content.contains("LegacyCaller"));
    assert!(content.contains("target_fn"));
    assert!(content.contains("target_hint"));
}

#[test]
fn rebuilds_existing_call_search_rows_with_symbol_signatures() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("code schema should initialize");
    connection
        .execute_batch(
            "
            DELETE FROM code_repository_schema_migrations
            WHERE name = 'call-search-symbol-signatures-v1';
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count,
                stale, degraded_reason
            )
            VALUES (
                'repo', 'fixture', '/tmp/repo', '[]', '[]', NULL, NULL, NULL, 'fresh',
                0, 0, 0, 0, 0, NULL
            );
            INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash,
                byte_len, line_count, parse_status, degraded_reason
            )
            VALUES ('repo', 'scope', 'table-file', 'src/table.rs', 'rust', 'hash', 20, 1, 'parsed', NULL);
            INSERT INTO code_repository_symbols (
                repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
                file_id, path, language_id, name, qualified_name, kind, signature,
                doc_comment, byte_start, byte_end, line_start, line_end
            )
            VALUES (
                'repo', 'scope', 'read-block-symbol',
                'repo://repo/src::table.rs::ReadBlock', 'table-file', 'src/table.rs',
                'rust', 'ReadBlock', 'Table::ReadBlock', 'function',
                'Status Table::ReadBlock(BlockContents* contents)', NULL, 0, 20, 1, 1
            );
            INSERT INTO code_repository_calls (
                repository_id, source_scope, call_id, file_id, path,
                caller_symbol_snapshot_id, caller_name, callee_symbol_snapshot_id,
                callee_name, target_hint, resolution_state, confidence_basis_points,
                confidence_tier, line_start, line_end
            )
            VALUES (
                'repo', 'scope', 'call-1', 'table-file', 'src/table.rs',
                NULL, 'InternalGet', 'read-block-symbol', 'ReadBlock', 'ReadBlock',
                'resolved', 8000, 'inferred', 1, 1
            );
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            VALUES ('scope', 'call', 'call-1', 'src/table.rs', 'rust', 'InternalGet ReadBlock src/table.rs');
            ",
        )
        .expect("old call search row should insert");

    initialize_code_schema(&connection).expect("code schema upgrade should rebuild call search");

    let (content, call_rows): (String, i64) = connection
        .query_row(
            "
            SELECT content, (
                SELECT COUNT(*)
                FROM code_repository_search
                WHERE document_kind = 'call' AND record_id = 'call-1'
            )
            FROM code_repository_search
            WHERE document_kind = 'call' AND record_id = 'call-1'
            ",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("rebuilt call search row should load");

    assert_eq!(call_rows, 1);
    assert!(content.contains("Status Table::ReadBlock"));
    assert!(
        code_schema_migration_applied(&connection, CALL_SEARCH_SIGNATURE_MIGRATION)
            .expect("migration marker should load")
    );
}

#[test]
fn skips_call_search_rebuild_after_signature_migration_marker() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("code schema should initialize");
    connection
        .execute_batch(
            "
            INSERT OR REPLACE INTO code_repository_schema_migrations (name, applied_at_ms)
            VALUES ('call-search-symbol-signatures-v1', 1);
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count,
                stale, degraded_reason
            )
            VALUES (
                'repo', 'fixture', '/tmp/repo', '[]', '[]', NULL, NULL, NULL, 'fresh',
                0, 0, 0, 0, 0, NULL
            );
            INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash,
                byte_len, line_count, parse_status, degraded_reason
            )
            VALUES ('repo', 'scope', 'table-file', 'src/table.rs', 'rust', 'hash', 20, 1, 'parsed', NULL);
            INSERT INTO code_repository_symbols (
                repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
                file_id, path, language_id, name, qualified_name, kind, signature,
                doc_comment, byte_start, byte_end, line_start, line_end
            )
            VALUES (
                'repo', 'scope', 'read-block-symbol',
                'repo://repo/src::table.rs::ReadBlock', 'table-file', 'src/table.rs',
                'rust', 'ReadBlock', 'Table::ReadBlock', 'function',
                'Status Table::ReadBlock(BlockContents* contents)', NULL, 0, 20, 1, 1
            );
            INSERT INTO code_repository_calls (
                repository_id, source_scope, call_id, file_id, path,
                caller_symbol_snapshot_id, caller_name, callee_symbol_snapshot_id,
                callee_name, target_hint, resolution_state, confidence_basis_points,
                confidence_tier, line_start, line_end
            )
            VALUES (
                'repo', 'scope', 'call-1', 'table-file', 'src/table.rs',
                NULL, 'InternalGet', 'read-block-symbol', 'ReadBlock', 'ReadBlock',
                'resolved', 8000, 'inferred', 1, 1
            );
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            VALUES ('scope', 'call', 'call-1', 'src/table.rs', 'rust', 'already migrated sentinel');
            ",
        )
        .expect("marked schema should initialize");

    initialize_code_schema(&connection).expect("marked schema should skip call search rebuild");

    let content: String = connection
        .query_row(
            "
            SELECT content
            FROM code_repository_search
            WHERE document_kind = 'call' AND record_id = 'call-1'
            ",
            [],
            |row| row.get(0),
        )
        .expect("call search row should load");

    assert_eq!(content, "already migrated sentinel");
}

#[test]
fn search_backfill_is_marked_after_one_legacy_pass() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_schema_migrations (name TEXT PRIMARY KEY);
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            CREATE TABLE code_repository_symbols (
                source_scope TEXT NOT NULL,
                symbol_snapshot_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                name TEXT NOT NULL,
                qualified_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                signature TEXT NOT NULL,
                doc_comment TEXT,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            INSERT INTO code_repository_symbols (
                source_scope, symbol_snapshot_id, path, language_id, name,
                qualified_name, kind, signature, doc_comment, line_start, line_end
            )
            VALUES (
                'scope', 'symbol-1', 'src/lib.rs', 'rust', 'LegacyThing',
                'crate::LegacyThing', 'struct', 'struct LegacyThing', NULL, 1, 1
            );
            ",
        )
        .expect("legacy code search schema should initialize");

    initialize_code_schema(&connection).expect("legacy search should backfill");
    assert_eq!(search_row_count(&connection, "symbol-1"), 1);
    assert!(
        code_schema_migration_applied(&connection, SEARCH_BACKFILL_MIGRATION)
            .expect("search backfill marker should load")
    );

    connection
        .execute(
            "DELETE FROM code_repository_search WHERE record_id = 'symbol-1'",
            [],
        )
        .expect("sentinel search row should delete");
    initialize_code_schema(&connection).expect("marked search backfill should skip");

    assert_eq!(search_row_count(&connection, "symbol-1"), 0);
}

#[test]
fn search_metadata_backfill_tracks_existing_fts_rows() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_schema_migrations (name TEXT PRIMARY KEY);
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            VALUES ('scope', 'symbol', 'symbol-1', 'src/lib.rs', 'rust', 'LegacyThing');
            ",
        )
        .expect("legacy search row should initialize");

    initialize_code_schema(&connection).expect("metadata should backfill");

    let metadata_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_search_metadata
            WHERE source_scope = 'scope'
              AND document_kind = 'symbol'
              AND record_id = 'symbol-1'
            ",
            [],
            |row| row.get(0),
        )
        .expect("metadata count should load");
    assert_eq!(metadata_count, 1);
    assert!(
        code_schema_migration_applied(&connection, SEARCH_METADATA_BACKFILL_MIGRATION)
            .expect("metadata migration marker should load")
    );
}

#[test]
fn edge_language_backfill_is_marked_after_one_legacy_update() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute(
            "CREATE TABLE code_repository_schema_migrations (name TEXT PRIMARY KEY)",
            [],
        )
        .expect("legacy migration table should initialize");
    initialize_code_schema(&connection).expect("code schema should initialize");
    connection
        .execute_batch(
            "
            DELETE FROM code_repository_schema_migrations
            WHERE name = 'edge-search-language-ids-v1';
            INSERT INTO code_repositories
            VALUES ('repo', 'fixture', '/tmp/repo', '[]', '[]', NULL, NULL, NULL, 'fresh',
                    0, 0, 0, 0, 0, NULL);
            INSERT INTO code_repository_files
            VALUES ('repo', 'scope', 'import-file', 'src/lib.rs', 'rust', 'hash',
                    20, 1, 'parsed', NULL);
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            VALUES ('scope', 'import', 'import-1', 'src/lib.rs', '', 'use crate::target');
            ",
        )
        .expect("legacy edge search row should insert");

    initialize_code_schema(&connection).expect("legacy edge language should backfill");
    assert_eq!(edge_search_language(&connection), "rust");
    assert!(
        code_schema_migration_applied(&connection, EDGE_SEARCH_LANGUAGE_ID_MIGRATION)
            .expect("migration marker should load")
    );

    connection
        .execute(
            "UPDATE code_repository_search SET language_id = '' WHERE record_id = 'import-1'",
            [],
        )
        .expect("sentinel row should be editable");
    initialize_code_schema(&connection).expect("marked edge language backfill should skip");

    assert_eq!(edge_search_language(&connection), "");
}

fn search_row_count(connection: &Connection, record_id: &str) -> i64 {
    connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_search
            WHERE record_id = ?1
            ",
            [record_id],
            |row| row.get(0),
        )
        .expect("search row count should load")
}

fn edge_search_language(connection: &Connection) -> String {
    connection
        .query_row(
            "
            SELECT language_id
            FROM code_repository_search
            WHERE document_kind = 'import' AND record_id = 'import-1'
            ",
            [],
            |row| row.get(0),
        )
        .expect("edge search row should load")
}
