use rusqlite::Connection;

use super::{
    GENERATED_DETECTION_REINDEX_MIGRATION, LOSSLESS_MARKDOWN_REINDEX_MIGRATION,
    ROUTE_EXTRACTION_REINDEX_MIGRATION, code_schema_migration_applied, initialize_code_schema,
};
use crate::storage::sqlite::schema::marker::SEARCH_OWNER_V2_MIGRATION;

#[test]
fn schema_open_does_not_backfill_legacy_call_search() {
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

    let search_count: usize = connection
        .query_row("SELECT COUNT(*) FROM code_repository_search", [], |row| {
            row.get(0)
        })
        .expect("search rows should count");

    assert_eq!(search_count, 0);
}

#[test]
fn backfills_path_generated_flags_after_adding_legacy_file_column() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL
            );
            INSERT INTO code_repository_files (source_scope, path, language_id)
            VALUES
                ('scope', 'dist/client.js', 'javascript'),
                ('scope', 'build/openapi/client.rs', 'rust'),
                ('scope', 'src/build/config.rs', 'rust'),
                ('scope', 'internal/dist/reader.go', 'go'),
                ('scope', 'src/service.rs', 'rust');
            ",
        )
        .expect("legacy file table should initialize");

    initialize_code_schema(&connection).expect("code schema should initialize");

    let generated: i64 = connection
        .query_row(
            "
            SELECT is_generated
            FROM code_repository_files
            WHERE path = 'dist/client.js'
            ",
            [],
            |row| row.get(0),
        )
        .expect("generated flag should load");
    let root_build_generated: i64 = connection
        .query_row(
            "
            SELECT is_generated
            FROM code_repository_files
            WHERE path = 'build/openapi/client.rs'
            ",
            [],
            |row| row.get(0),
        )
        .expect("root build generated flag should load");
    let nested_build_handwritten: i64 = connection
        .query_row(
            "
            SELECT is_generated
            FROM code_repository_files
            WHERE path = 'src/build/config.rs'
            ",
            [],
            |row| row.get(0),
        )
        .expect("nested build flag should load");
    let nested_dist_handwritten: i64 = connection
        .query_row(
            "
            SELECT is_generated
            FROM code_repository_files
            WHERE path = 'internal/dist/reader.go'
            ",
            [],
            |row| row.get(0),
        )
        .expect("nested dist flag should load");
    let handwritten: i64 = connection
        .query_row(
            "
            SELECT is_generated
            FROM code_repository_files
            WHERE path = 'src/service.rs'
            ",
            [],
            |row| row.get(0),
        )
        .expect("handwritten flag should load");

    assert_eq!(generated, 1);
    assert_eq!(root_build_generated, 1);
    assert_eq!(nested_build_handwritten, 0);
    assert_eq!(nested_dist_handwritten, 0);
    assert_eq!(handwritten, 0);
}

#[test]
fn generated_detection_migration_marks_existing_scopes_stale_once() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("code schema should initialize");
    connection
        .execute_batch(
            "
            DELETE FROM code_repository_schema_migrations
            WHERE name = 'generated-detection-reindex-v1';
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count,
                stale, degraded_reason
            )
            VALUES (
                'repo', 'fixture', '/tmp/repo', '[]', '[]', 'scope', 'commit',
                'tree', 'fresh', 1, 1, 0, 0, 0, NULL
            );
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale, degraded_reason
            )
            VALUES ('scope', 'repo', 'commit', 'tree', '[]', '[]', 1, 1, 0, 0, 0, NULL);
            INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash,
                byte_len, line_count, parse_status, is_generated, degraded_reason
            )
            VALUES ('repo', 'scope', 'file', 'src/client.ts', 'typescript', 'hash', 20, 1, 'parsed', 0, NULL);
            ",
        )
        .expect("fresh legacy scope should insert");

    initialize_code_schema(&connection).expect("generated detection migration should run");
    assert_eq!(repository_stale(&connection), 1);
    assert_eq!(scope_stale(&connection), 1);
    assert!(
        code_schema_migration_applied(&connection, GENERATED_DETECTION_REINDEX_MIGRATION)
            .expect("migration marker should load")
    );

    connection
        .execute_batch(
            "
            UPDATE code_repositories SET stale = 0 WHERE repository_id = 'repo';
            UPDATE code_repository_scopes SET stale = 0 WHERE source_scope = 'scope';
            ",
        )
        .expect("stale flags should reset");
    initialize_code_schema(&connection).expect("marked migration should skip");

    assert_eq!(repository_stale(&connection), 0);
    assert_eq!(scope_stale(&connection), 0);
}

#[test]
fn search_owner_v2_capability_marks_legacy_scopes_stale_without_search_rewrite() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("code schema should initialize");
    connection
        .execute_batch(&format!(
            "
            DELETE FROM code_repository_schema_migrations
            WHERE name = '{SEARCH_OWNER_V2_MIGRATION}';
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count,
                stale, degraded_reason
            ) VALUES (
                'repo', 'fixture', '/tmp/repo', '[]', '[]', 'scope', 'commit',
                'tree', 'fresh', 1, 1, 0, 0, 0, NULL
            );
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale, degraded_reason
            ) VALUES ('scope', 'repo', 'commit', 'tree', '[]', '[]', 1, 1, 0, 0, 0, NULL);
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            ) VALUES ('scope', 'symbol', 'symbol-1', 'src/lib.rs', 'rust', 'legacy sentinel');
            "
        ))
        .expect("legacy search scope should insert");

    initialize_code_schema(&connection).expect("search-owner capability should install");

    assert_eq!(repository_stale(&connection), 1);
    assert_eq!(scope_stale(&connection), 1);
    assert_eq!(search_row_count(&connection, "symbol-1"), 1);
    assert!(
        code_schema_migration_applied(&connection, SEARCH_OWNER_V2_MIGRATION)
            .expect("capability marker should load")
    );

    connection
        .execute_batch(
            "UPDATE code_repository_scopes SET stale = 0 WHERE source_scope = 'scope';
             UPDATE code_repositories SET stale = 0 WHERE repository_id = 'repo';",
        )
        .expect("fresh replacement sentinel should persist");
    initialize_code_schema(&connection).expect("installed capability should be idempotent");
    assert_eq!(repository_stale(&connection), 0);
    assert_eq!(scope_stale(&connection), 0);
}

#[test]
fn search_owner_v2_capability_rolls_back_stale_flags_and_marker_on_failure() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("code schema should initialize");
    connection
        .execute_batch(&format!(
            "
            DELETE FROM code_repository_schema_migrations
            WHERE name = '{SEARCH_OWNER_V2_MIGRATION}';
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count,
                stale, degraded_reason
            ) VALUES (
                'repo', 'fixture', '/tmp/repo', '[]', '[]', 'scope', 'commit',
                'tree', 'fresh', 1, 1, 0, 0, 0, NULL
            );
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale, degraded_reason
            ) VALUES ('scope', 'repo', 'commit', 'tree', '[]', '[]', 1, 1, 0, 0, 0, NULL);
            CREATE TRIGGER fail_search_owner_repository_stale
            BEFORE UPDATE OF stale ON code_repositories
            WHEN NEW.repository_id = 'repo'
            BEGIN
                SELECT RAISE(ABORT, 'injected search-owner migration failure');
            END;
            "
        ))
        .expect("legacy scope and failure trigger should install");

    initialize_code_schema(&connection)
        .expect_err("search-owner migration failure should propagate");
    assert_eq!(repository_stale(&connection), 0);
    assert_eq!(scope_stale(&connection), 0);
    assert!(
        !code_schema_migration_applied(&connection, SEARCH_OWNER_V2_MIGRATION)
            .expect("rolled-back capability marker should load")
    );

    connection
        .execute_batch("DROP TRIGGER fail_search_owner_repository_stale")
        .expect("failure trigger should drop");
    initialize_code_schema(&connection).expect("search-owner capability should retry");

    assert_eq!(repository_stale(&connection), 1);
    assert_eq!(scope_stale(&connection), 1);
    assert!(
        code_schema_migration_applied(&connection, SEARCH_OWNER_V2_MIGRATION)
            .expect("capability marker should load")
    );
}

#[test]
fn lossless_markdown_migration_marks_affected_scopes_stale_once() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("code schema should initialize");
    connection
        .execute_batch(
            "
            DELETE FROM code_repository_schema_migrations
            WHERE name = 'lossless-markdown-source-windows-reindex-v1';
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count,
                stale, degraded_reason
            )
            VALUES (
                'repo', 'fixture', '/tmp/repo', '[]', '[]', 'scope', 'commit',
                'tree', 'fresh', 1, 1, 0, 1, 0, NULL
            );
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale, degraded_reason
            )
            VALUES ('scope', 'repo', 'commit', 'tree', '[]', '[]', 1, 1, 0, 1, 0, NULL);
            INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash,
                byte_len, line_count, parse_status, is_generated, degraded_reason
            )
            VALUES ('repo', 'scope', 'file', 'docs/okf.md', 'markdown', 'hash', 20, 2, 'parsed', 0, NULL);
            ",
        )
        .expect("fresh legacy Markdown scope should insert");

    connection
        .execute_batch(
            "
            CREATE TRIGGER fail_markdown_repository_stale
            BEFORE UPDATE OF stale ON code_repositories
            WHEN NEW.repository_id = 'repo'
            BEGIN
                SELECT RAISE(ABORT, 'injected Markdown migration failure');
            END;
            ",
        )
        .expect("failure trigger should install");
    initialize_code_schema(&connection).expect_err("migration failure should propagate");
    assert_eq!(repository_stale(&connection), 0);
    assert_eq!(scope_stale(&connection), 0);
    assert!(
        !code_schema_migration_applied(&connection, LOSSLESS_MARKDOWN_REINDEX_MIGRATION)
            .expect("rolled-back migration marker should load")
    );
    connection
        .execute_batch("DROP TRIGGER fail_markdown_repository_stale")
        .expect("failure trigger should drop");

    initialize_code_schema(&connection).expect("lossless Markdown migration should run");
    assert_eq!(repository_stale(&connection), 1);
    assert_eq!(scope_stale(&connection), 1);
    assert!(
        code_schema_migration_applied(&connection, LOSSLESS_MARKDOWN_REINDEX_MIGRATION)
            .expect("migration marker should load")
    );

    connection
        .execute_batch(
            "
            UPDATE code_repositories SET stale = 0 WHERE repository_id = 'repo';
            UPDATE code_repository_scopes SET stale = 0 WHERE source_scope = 'scope';
            ",
        )
        .expect("stale flags should reset");
    initialize_code_schema(&connection).expect("marked migration should skip");

    assert_eq!(repository_stale(&connection), 0);
    assert_eq!(scope_stale(&connection), 0);
}

#[test]
fn route_extraction_migration_marks_non_fact_versioned_scopes_stale_once() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("code schema should initialize");
    connection
        .execute_batch(
            "
            DELETE FROM code_repository_schema_migrations
            WHERE name = 'web-route-extraction-reindex-v1';
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count,
                stale, degraded_reason
            )
            VALUES (
                'repo', 'fixture', '/tmp/repo', '[]', '[]', 'manual:repo',
                'manual', 'tree', 'fresh', 1, 1, 0, 0, 0, NULL
            );
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale, degraded_reason
            )
            VALUES ('manual:repo', 'repo', 'manual', 'tree', '[]', '[]', 1, 1, 0, 0, 0, NULL);
            INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash,
                byte_len, line_count, parse_status, is_generated, degraded_reason
            )
            VALUES ('repo', 'manual:repo', 'file', 'src/routes.ts', 'typescript', 'hash', 20, 1, 'parsed', 0, NULL);
            ",
        )
        .expect("fresh manual scope should insert");

    initialize_code_schema(&connection).expect("route extraction migration should run");
    assert_eq!(repository_stale(&connection), 1);
    let stale: i64 = connection
        .query_row(
            "SELECT stale FROM code_repository_scopes WHERE source_scope = 'manual:repo'",
            [],
            |row| row.get(0),
        )
        .expect("manual scope stale flag should load");
    assert_eq!(stale, 1);
    assert!(
        code_schema_migration_applied(&connection, ROUTE_EXTRACTION_REINDEX_MIGRATION)
            .expect("migration marker should load")
    );

    connection
        .execute_batch(
            "
            UPDATE code_repositories SET stale = 0 WHERE repository_id = 'repo';
            UPDATE code_repository_scopes SET stale = 0 WHERE source_scope = 'manual:repo';
            ",
        )
        .expect("stale flags should reset");
    initialize_code_schema(&connection).expect("marked migration should skip");

    assert_eq!(repository_stale(&connection), 0);
    let stale: i64 = connection
        .query_row(
            "SELECT stale FROM code_repository_scopes WHERE source_scope = 'manual:repo'",
            [],
            |row| row.get(0),
        )
        .expect("manual scope stale flag should load after skip");
    assert_eq!(stale, 0);
}

#[test]
fn schema_open_does_not_rebuild_existing_call_search_rows() {
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

    initialize_code_schema(&connection).expect("code schema upgrade should remain read-bounded");

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
        .expect("legacy call search row should load");

    assert_eq!(call_rows, 1);
    assert_eq!(content, "InternalGet ReadBlock src/table.rs");
    assert!(
        !code_schema_migration_applied(&connection, "call-search-symbol-signatures-v1")
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
fn schema_open_does_not_backfill_search_facts() {
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

    initialize_code_schema(&connection).expect("legacy schema should open without data backfill");
    assert_eq!(search_row_count(&connection, "symbol-1"), 0);
    assert!(
        !code_schema_migration_applied(&connection, "code-search-backfill-v1")
            .expect("legacy search marker should remain absent")
    );
}

#[test]
fn schema_open_does_not_backfill_search_metadata() {
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

    initialize_code_schema(&connection).expect("schema should open without metadata backfill");

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
    assert_eq!(metadata_count, 0);
    assert!(
        !code_schema_migration_applied(&connection, "code-search-metadata-backfill-v1")
            .expect("legacy metadata marker should remain absent")
    );
}

#[test]
fn schema_open_does_not_backfill_legacy_edge_languages() {
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
                    20, 1, 'parsed', 0, NULL);
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            VALUES ('scope', 'import', 'import-1', 'src/lib.rs', '', 'use crate::target');
            ",
        )
        .expect("legacy edge search row should insert");

    initialize_code_schema(&connection).expect("schema should open without edge backfill");
    assert_eq!(edge_search_language(&connection), "");
    assert!(
        !code_schema_migration_applied(&connection, "edge-search-language-ids-v1")
            .expect("legacy edge marker should remain absent")
    );
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

fn repository_stale(connection: &Connection) -> i64 {
    connection
        .query_row(
            "SELECT stale FROM code_repositories WHERE repository_id = 'repo'",
            [],
            |row| row.get(0),
        )
        .expect("repository stale flag should load")
}

fn scope_stale(connection: &Connection) -> i64 {
    connection
        .query_row(
            "SELECT stale FROM code_repository_scopes WHERE source_scope = 'scope'",
            [],
            |row| row.get(0),
        )
        .expect("scope stale flag should load")
}
