use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, params};

use crate::storage::StorageError;

use super::super::super::initialize_code_schema;

#[test]
fn imports_legacy_code_snapshots_without_route_table_or_symbol_role_column() {
    let source_path = temporary_sqlite_path("legacy-code-import");
    let source = Connection::open(&source_path).expect("source database should open");
    initialize_code_schema(&source).expect("source schema should initialize");
    source
        .execute_batch(
            r#"
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count, stale,
                degraded_reason
            ) VALUES (
                'repo', 'fixture', '/tmp/repo', '[]', '[]', 'git_snapshot:test',
                'commit', 'tree', 'ready', 1, 1, 0, 0, 0, NULL
            );
            INSERT INTO code_repository_aliases (alias, repository_id)
            VALUES ('fixture', 'repo');
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale, degraded_reason
            ) VALUES (
                'git_snapshot:test', 'repo', 'commit', 'tree', '[]', '[]',
                1, 1, 0, 0, 0, NULL
            );
            INSERT INTO code_repository_index_checkpoints (
                source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, total_path_count,
                parsed_file_count, committed_file_count, committed_symbol_count,
                committed_reference_count, committed_chunk_count, batch_count, last_path,
                resource_budget_json, updated_at_ms, error_message
            ) VALUES (
                'git_snapshot:test', 'repo', 'completed', 'commit', 'tree', '[]', '[]',
                1, 1, 1, 1, 0, 0, 1, 'src/routes.ts',
                '{"max_files_per_batch":256,"max_bytes_per_batch":16777216,"max_rows_per_batch":150000}',
                1, NULL
            );
            INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash,
                byte_len, line_count, parse_status, degraded_reason
            ) VALUES (
                'repo', 'git_snapshot:test', 'file', 'src/routes.ts', 'typescript',
                'hash', 42, 2, 'parsed', NULL
            );
            INSERT INTO code_repository_symbols (
                repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
                file_id, path, language_id, name, qualified_name, kind, signature,
                doc_comment, byte_start, byte_end, line_start, line_end, symbol_role_json
            ) VALUES (
                'repo', 'git_snapshot:test', 'symbol', 'repo://repo/src::routes.ts::listUsers',
                'file', 'src/routes.ts', 'typescript', 'listUsers', 'listUsers',
                'function', 'function listUsers()', NULL, 0, 10, 1, 1, NULL
            );
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            ) VALUES (
                'git_snapshot:test', 'symbol', 'symbol', 'src/routes.ts', 'typescript',
                'listUsers function listUsers()'
            );
            INSERT INTO code_repository_search_metadata (
                source_scope, document_kind, record_id, path, search_rowid
            )
            SELECT source_scope, document_kind, record_id, path, rowid
            FROM code_repository_search
            WHERE source_scope = 'git_snapshot:test';
            CREATE TABLE legacy_code_repository_symbols AS
            SELECT repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
                   file_id, path, language_id, name, qualified_name, kind, signature,
                   doc_comment, byte_start, byte_end, line_start, line_end
            FROM code_repository_symbols;
            DROP TABLE code_repository_symbols;
            ALTER TABLE legacy_code_repository_symbols RENAME TO code_repository_symbols;
            DROP TABLE code_repository_routes;
            DROP TABLE code_repository_commit_scopes;
            PRAGMA foreign_keys = OFF;
            CREATE TABLE legacy_code_repository_index_checkpoints AS
            SELECT source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                   path_filters_json, language_filters_json, total_path_count,
                   parsed_file_count, committed_file_count, committed_symbol_count,
                   committed_reference_count, committed_chunk_count, batch_count, last_path,
                   resource_budget_json, updated_at_ms, error_message
            FROM code_repository_index_checkpoints;
            DROP TABLE code_repository_index_checkpoints;
            ALTER TABLE legacy_code_repository_index_checkpoints
                RENAME TO code_repository_index_checkpoints;
            PRAGMA foreign_keys = ON;
            "#,
        )
        .expect("legacy source data should be installed");
    source
        .execute(
            "DELETE FROM code_repository_schema_migrations WHERE name = ?1",
            [crate::storage::sqlite::schema::marker::SEARCH_OWNER_V2_MIGRATION],
        )
        .expect("legacy source should lack the search owner capability");
    drop(source);

    let mut target = Connection::open_in_memory().expect("target database should open");
    initialize_code_schema(&target).expect("target schema should initialize");
    super::import_repository_from_database(
        &mut target,
        &source_path,
        "repo",
        Some("git_snapshot:test"),
    )
    .expect("legacy snapshot should import");

    let symbol_role: Option<String> = target
        .query_row(
            "
            SELECT symbol_role_json
            FROM code_repository_symbols
            WHERE source_scope = 'git_snapshot:test'
              AND symbol_snapshot_id = 'symbol'
            ",
            [],
            |row| row.get(0),
        )
        .expect("symbol should import");
    let route_count: i64 = target
        .query_row(
            "SELECT COUNT(*) FROM code_repository_routes WHERE source_scope = 'git_snapshot:test'",
            [],
            |row| row.get(0),
        )
        .expect("route table should remain queryable");
    let commit_alias_count: i64 = target
        .query_row(
            "SELECT COUNT(*) FROM code_repository_commit_scopes
             WHERE repository_id = 'repo'
               AND resolved_commit_sha = 'commit'
               AND source_scope = 'git_snapshot:test'",
            [],
            |row| row.get(0),
        )
        .expect("legacy scope should receive a current commit alias");
    let committed_fact_row_count: usize = target
        .query_row(
            "SELECT committed_fact_row_count
             FROM code_repository_index_checkpoints
             WHERE source_scope = 'git_snapshot:test'",
            [],
            |row| row.get(0),
        )
        .expect("legacy checkpoint should import as explicitly unproven");
    let (retired_symbol_query_index_exists, symbol_name_path_index_exists): (bool, bool) = target
        .query_row(
            "SELECT
                EXISTS (
                    SELECT 1 FROM sqlite_schema
                    WHERE type = 'index' AND name = 'code_repository_symbols_lookup'
                ),
                EXISTS (
                    SELECT 1 FROM sqlite_schema
                    WHERE type = 'index'
                      AND name = 'code_repository_symbols_name_path_lookup'
                )",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("direct import query-index state should load");
    let (scope_stale, repository_stale, search_count, degraded_reason): (
        bool,
        bool,
        usize,
        String,
    ) = target
        .query_row(
            "
            SELECT
                (SELECT stale
                 FROM code_repository_scopes
                 WHERE source_scope = 'git_snapshot:test'),
                (SELECT stale
                 FROM code_repositories
                 WHERE repository_id = 'repo'),
                (SELECT COUNT(*)
                 FROM code_repository_search
                 WHERE source_scope = 'git_snapshot:test'),
                (SELECT degraded_reason
                 FROM code_repository_scopes
                 WHERE source_scope = 'git_snapshot:test')
            ",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("legacy search capability status should load");
    let published_status = super::super::super::lifecycle::status::repository_scope_status(
        &mut target,
        "fixture",
        "commit",
        &[],
        &[],
    )
    .expect("legacy scope status should load");
    fs::remove_file(source_path).expect("temporary source database should be removed");

    assert!(symbol_role.is_none());
    assert_eq!(route_count, 0);
    assert_eq!(commit_alias_count, 1);
    assert_eq!(committed_fact_row_count, 0);
    assert!(!retired_symbol_query_index_exists);
    assert!(symbol_name_path_index_exists);
    assert!(scope_stale);
    assert!(repository_stale);
    assert_eq!(search_count, 0);
    assert!(degraded_reason.contains("search-owner-v2"));
    assert!(published_status.is_none());
}

#[test]
fn imports_exact_current_search_ownership_but_marks_old_fact_version_stale() {
    let source_path = temporary_sqlite_path("exact-current-search-import");
    let source = Connection::open(&source_path).expect("source database should open");
    initialize_code_schema(&source).expect("source schema should initialize");
    source
        .execute_batch(
            "
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count, stale
            ) VALUES (
                'repo-current', 'fixture-current', '/tmp/repo-current', '[]', '[]',
                'git_snapshot:0000000000000000', 'commit-current', 'tree-current',
                'fresh', 0, 0, 0, 0, 0
            );
            INSERT INTO code_repository_aliases (alias, repository_id)
            VALUES ('fixture-current', 'repo-current');
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale
            ) VALUES (
                'git_snapshot:0000000000000000', 'repo-current', 'commit-current',
                'tree-current', '[]', '[]', 0, 0, 0, 0, 0
            );
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            ) VALUES (
                'git_snapshot:0000000000000000', 'chunk', 'chunk-current',
                'src/lib.rs', 'rust', 'fn current_owner() {}'
            );
            INSERT INTO code_repository_search_metadata (
                source_scope, document_kind, record_id, path, search_rowid
            )
            SELECT source_scope, document_kind, record_id, path, rowid
            FROM code_repository_search
            WHERE source_scope = 'git_snapshot:0000000000000000';
            INSERT INTO code_repository_reference_search_manifests (
                source_scope, projection_version, reference_count, group_count
            ) VALUES ('git_snapshot:0000000000000000', 2, 0, 0);
            ",
        )
        .expect("current exact source should insert");
    drop(source);

    let mut target = Connection::open_in_memory().expect("target database should open");
    initialize_code_schema(&target).expect("target schema should initialize");
    super::import_repository_from_database(
        &mut target,
        &source_path,
        "repo-current",
        Some("git_snapshot:0000000000000000"),
    )
    .expect("exact owner source should import");

    let (search_count, metadata_count, exact_count, scope_stale, repository_stale, degraded_reason):
        (usize, usize, usize, bool, bool, String) = target
            .query_row(
                "
            SELECT
                (SELECT COUNT(*)
                 FROM code_repository_search
                 WHERE source_scope = 'git_snapshot:0000000000000000'),
                (SELECT COUNT(*)
                 FROM code_repository_search_metadata
                 WHERE source_scope = 'git_snapshot:0000000000000000'),
                (SELECT COUNT(*)
                 FROM code_repository_search search
                 JOIN code_repository_search_metadata metadata
                   ON metadata.search_rowid = search.rowid
                  AND metadata.source_scope = search.source_scope
                  AND metadata.document_kind = search.document_kind
                  AND metadata.record_id = search.record_id
                  AND metadata.path = search.path
                 WHERE search.source_scope = 'git_snapshot:0000000000000000'),
                (SELECT stale
                 FROM code_repository_scopes
                 WHERE source_scope = 'git_snapshot:0000000000000000'),
                (SELECT stale
                 FROM code_repositories
                 WHERE repository_id = 'repo-current'),
                (SELECT degraded_reason
                 FROM code_repository_scopes
                 WHERE source_scope = 'git_snapshot:0000000000000000')
            ",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .expect("imported exact ownership should load");
    let published_status = super::super::super::lifecycle::status::repository_scope_status(
        &mut target,
        "fixture-current",
        "commit-current",
        &[],
        &[],
    )
    .expect("old fact-version status should load");
    let repository_status =
        super::super::super::lifecycle::status::repository_status(&mut target, "fixture-current")
            .expect("repository status should load")
            .expect("imported repository should exist");
    fs::remove_file(source_path).expect("temporary source database should be removed");

    assert_eq!(search_count, 1);
    assert_eq!(metadata_count, 1);
    assert_eq!(exact_count, 1);
    assert!(scope_stale);
    assert!(repository_stale);
    assert!(degraded_reason.contains("older code fact version"));
    assert!(repository_status.stale);
    assert!(published_status.is_none());
}

#[test]
fn imports_exact_current_fact_scope_as_fresh() {
    let source_path = temporary_sqlite_path("exact-current-fact-import");
    let repository_id = "repo-current-fact";
    let tree_hash = "tree-current-fact";
    let source_scope = crate::domain::code_snapshot_scope_id(repository_id, tree_hash, &[], &[]);
    let source = Connection::open(&source_path).expect("source database should open");
    initialize_code_schema(&source).expect("source schema should initialize");
    source
        .execute(
            "
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count, stale
            ) VALUES (?1, 'fixture-current-fact', '/tmp/repo-current-fact', '[]', '[]',
                      ?2, 'commit-current-fact', ?3, 'fresh', 0, 0, 0, 0, 0)
            ",
            params![repository_id, source_scope, tree_hash],
        )
        .expect("current repository should insert");
    source
        .execute(
            "INSERT INTO code_repository_aliases (alias, repository_id)
             VALUES ('fixture-current-fact', ?1)",
            params![repository_id],
        )
        .expect("current alias should insert");
    source
        .execute(
            "
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale
            ) VALUES (?1, ?2, 'commit-current-fact', ?3, '[]', '[]', 0, 0, 0, 0, 0)
            ",
            params![source_scope, repository_id, tree_hash],
        )
        .expect("current scope should insert");
    source
        .execute(
            "
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            ) VALUES (?1, 'chunk', 'chunk-current-fact', 'src/lib.rs', 'rust',
                      'fn current_fact_owner() {}')
            ",
            params![source_scope],
        )
        .expect("current search row should insert");
    source
        .execute(
            "
            INSERT INTO code_repository_search_metadata (
                source_scope, document_kind, record_id, path, search_rowid
            )
            SELECT source_scope, document_kind, record_id, path, rowid
            FROM code_repository_search
            WHERE source_scope = ?1
            ",
            params![source_scope],
        )
        .expect("current owner should insert");
    source
        .execute(
            "INSERT INTO code_repository_reference_search_manifests (
                 source_scope, projection_version, reference_count, group_count
             ) VALUES (?1, 2, 0, 0)",
            params![source_scope],
        )
        .expect("current zero-reference manifest should insert");
    drop(source);

    let mut target = Connection::open_in_memory().expect("target database should open");
    initialize_code_schema(&target).expect("target schema should initialize");
    super::import_repository_from_database(
        &mut target,
        &source_path,
        repository_id,
        Some(&source_scope),
    )
    .expect("current exact scope should import");

    let (search_count, metadata_count, scope_stale, repository_stale): (usize, usize, bool, bool) =
        target
            .query_row(
                "
            SELECT
                (SELECT COUNT(*) FROM code_repository_search WHERE source_scope = ?1),
                (SELECT COUNT(*) FROM code_repository_search_metadata WHERE source_scope = ?1),
                (SELECT stale FROM code_repository_scopes WHERE source_scope = ?1),
                (SELECT stale FROM code_repositories WHERE repository_id = ?2)
            ",
                params![source_scope, repository_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("current imported state should load");
    let published_status = super::super::super::lifecycle::status::repository_scope_status(
        &mut target,
        "fixture-current-fact",
        "commit-current-fact",
        &[],
        &[],
    )
    .expect("current scope status should load");
    fs::remove_file(source_path).expect("temporary source database should be removed");

    assert_eq!(search_count, 1);
    assert_eq!(metadata_count, 1);
    assert!(!scope_stale);
    assert!(!repository_stale);
    assert_eq!(
        published_status
            .expect("current imported scope should remain published")
            .last_indexed_scope_id
            .as_deref(),
        Some(source_scope.as_str())
    );
}

#[test]
fn imports_owned_search_rows_and_leaves_highest_rowid_orphan_isolated() {
    let source_path = temporary_sqlite_path("duplicate-search-owner-import");
    let source = Connection::open(&source_path).expect("source database should open");
    initialize_code_schema(&source).expect("source schema should initialize");
    source
        .execute_batch(
            "
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count, stale
            ) VALUES (
                'incoming', 'incoming', '/tmp/incoming', '[]', '[]', 'incoming-scope',
                'incoming-commit', 'incoming-tree', 'fresh', 1, 0, 0, 0, 0
            );
            INSERT INTO code_repository_aliases (alias, repository_id)
            VALUES ('incoming', 'incoming');
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale
            ) VALUES (
                'incoming-scope', 'incoming', 'incoming-commit', 'incoming-tree',
                '[]', '[]', 1, 0, 0, 0, 0
            );
            INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash,
                byte_len, line_count, parse_status, is_generated
            ) VALUES (
                'incoming', 'incoming-scope', 'incoming-file', 'src/lib.rs', 'rust',
                'incoming-hash', 8, 1, 'parsed', 0
            );
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            ) VALUES
                ('incoming-scope', 'chunk', 'duplicate', 'src/lib.rs', 'rust',
                 'owned-lowest-rowid'),
                ('incoming-scope', 'chunk', 'duplicate', 'src/lib.rs', 'rust',
                 'orphan-highest-rowid');
            INSERT INTO code_repository_search_metadata (
                source_scope, document_kind, record_id, path, search_rowid
            )
            SELECT source_scope, document_kind, record_id, path, rowid
            FROM code_repository_search
            WHERE content = 'owned-lowest-rowid';
            INSERT INTO code_repository_reference_search_manifests (
                source_scope, projection_version, reference_count, group_count
            ) VALUES ('incoming-scope', 2, 0, 0);
            ",
        )
        .expect("duplicate source should insert");
    drop(source);

    let mut target = Connection::open_in_memory().expect("target database should open");
    initialize_code_schema(&target).expect("target schema should initialize");
    super::super::super::schema::ensure_code_query_indexes(&target)
        .expect("target query indexes should be ready before seeding evidence");
    target
        .execute_batch(
            "
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                state, indexed_file_count, symbol_count, reference_count, chunk_count, stale
            ) VALUES (
                'sentinel', 'sentinel', '/tmp/sentinel', '[]', '[]',
                'registered', 0, 0, 0, 0, 1
            );
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            ) VALUES (
                'sentinel-scope', 'chunk', 'sentinel-document', 'src/sentinel.rs', 'rust',
                'sentinel-content'
            );
            INSERT INTO code_repository_search_metadata (
                source_scope, document_kind, record_id, path, search_rowid
            )
            SELECT source_scope, document_kind, record_id, path, rowid
            FROM code_repository_search
            WHERE source_scope = 'sentinel-scope';
            ",
        )
        .expect("sentinel target state should insert");

    super::import_repository_from_database(
        &mut target,
        &source_path,
        "incoming",
        Some("incoming-scope"),
    )
    .expect("metadata-owned search rows should import without scanning raw orphans");
    let (incoming_repository_count, incoming_scope_count, incoming_file_count, incoming_search_count, incoming_metadata_count, sentinel_repository_count, sentinel_search_count, sentinel_metadata_count):
        (usize, usize, usize, usize, usize, usize, usize, usize) = target
        .query_row(
            "
            SELECT
                (SELECT COUNT(*) FROM code_repositories WHERE repository_id = 'incoming'),
                (SELECT COUNT(*) FROM code_repository_scopes WHERE source_scope = 'incoming-scope'),
                (SELECT COUNT(*) FROM code_repository_files WHERE source_scope = 'incoming-scope'),
                (SELECT COUNT(*) FROM code_repository_search WHERE source_scope = 'incoming-scope'),
                (SELECT COUNT(*) FROM code_repository_search_metadata WHERE source_scope = 'incoming-scope'),
                (SELECT COUNT(*) FROM code_repositories WHERE repository_id = 'sentinel'),
                (SELECT COUNT(*) FROM code_repository_search WHERE source_scope = 'sentinel-scope'),
                (SELECT COUNT(*) FROM code_repository_search_metadata WHERE source_scope = 'sentinel-scope')
            ",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .expect("isolated import state should load");
    let attached_databases = target
        .prepare("PRAGMA database_list")
        .expect("database list should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("database list should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("database names should decode");
    let imported_content = target
        .query_row(
            "SELECT content FROM code_repository_search
             WHERE source_scope = 'incoming-scope'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("owned imported content should load");
    fs::remove_file(source_path).expect("temporary source database should be removed");

    assert_eq!(incoming_repository_count, 1);
    assert_eq!(incoming_scope_count, 1);
    assert_eq!(incoming_file_count, 1);
    assert_eq!(incoming_search_count, 1);
    assert_eq!(incoming_metadata_count, 1);
    assert_eq!(imported_content, "owned-lowest-rowid");
    assert_eq!(sentinel_repository_count, 1);
    assert_eq!(sentinel_search_count, 1);
    assert_eq!(sentinel_metadata_count, 1);
    assert!(!attached_databases.iter().any(|name| name == "relay_import"));
}

#[test]
fn incomplete_search_owner_schema_imports_only_base_facts_as_stale() {
    let source_path = temporary_sqlite_path("incomplete-search-owner-import");
    let source = Connection::open(&source_path).expect("source database should open");
    initialize_code_schema(&source).expect("source schema should initialize");
    source
        .execute_batch(
            "
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count, stale
            ) VALUES (
                'incomplete', 'incomplete', '/tmp/incomplete', '[]', '[]',
                'manual:incomplete', 'commit', 'tree', 'fresh', 1, 0, 0, 0, 0
            );
            INSERT INTO code_repository_aliases (alias, repository_id)
            VALUES ('incomplete', 'incomplete');
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale
            ) VALUES (
                'manual:incomplete', 'incomplete', 'commit', 'tree', '[]', '[]',
                1, 0, 0, 0, 0
            );
            INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash,
                byte_len, line_count, parse_status, is_generated
            ) VALUES (
                'incomplete', 'manual:incomplete', 'file', 'src/lib.rs', 'rust',
                'hash', 8, 1, 'parsed', 0
            );
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            ) VALUES (
                'manual:incomplete', 'chunk', 'unverifiable', 'src/lib.rs', 'rust',
                'unverifiable search row'
            );
            DROP TABLE code_repository_search_metadata;
            ",
        )
        .expect("incomplete marked source should insert");
    drop(source);

    let mut target = Connection::open_in_memory().expect("target database should open");
    initialize_code_schema(&target).expect("target schema should initialize");
    super::import_repository_from_database(
        &mut target,
        &source_path,
        "incomplete",
        Some("manual:incomplete"),
    )
    .expect("incomplete capability should retain only stale base facts");

    let (file_count, search_count, scope_stale, repository_stale, degraded_reason): (
        usize,
        usize,
        bool,
        bool,
        String,
    ) = target
        .query_row(
            "
            SELECT
                (SELECT COUNT(*)
                 FROM code_repository_files
                 WHERE source_scope = 'manual:incomplete'),
                (SELECT COUNT(*)
                 FROM code_repository_search
                 WHERE source_scope = 'manual:incomplete'),
                (SELECT stale
                 FROM code_repository_scopes
                 WHERE source_scope = 'manual:incomplete'),
                (SELECT stale
                 FROM code_repositories
                 WHERE repository_id = 'incomplete'),
                (SELECT degraded_reason
                 FROM code_repository_scopes
                 WHERE source_scope = 'manual:incomplete')
            ",
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
        .expect("incomplete import state should load");
    fs::remove_file(source_path).expect("temporary source database should be removed");

    assert_eq!(file_count, 1);
    assert_eq!(search_count, 0);
    assert!(scope_stale);
    assert!(repository_stale);
    assert!(degraded_reason.contains("search-owner-v2"));
}

#[test]
fn imports_all_same_tree_commit_aliases_for_the_selected_scope() {
    let source_path = temporary_sqlite_path("commit-scope-alias-import");
    let source = Connection::open(&source_path).expect("source database should open");
    initialize_code_schema(&source).expect("source schema should initialize");
    source
        .execute_batch(
            "
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count, stale
            ) VALUES (
                'repo', 'fixture', '/tmp/repo', '[]', '[]', 'git_snapshot:test',
                'commit-b', 'same-tree', 'fresh', 0, 0, 0, 0, 0
            );
            INSERT INTO code_repository_aliases (alias, repository_id)
            VALUES ('fixture', 'repo');
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale
            ) VALUES (
                'git_snapshot:test', 'repo', 'commit-b', 'same-tree', '[]', '[]',
                0, 0, 0, 0, 0
            );
            INSERT INTO code_repository_commit_scopes (
                repository_id, resolved_commit_sha, source_scope, published_sequence
            ) VALUES
                ('repo', 'commit-a', 'git_snapshot:test', 1),
                ('repo', 'commit-b', 'git_snapshot:test', 2);
            ",
        )
        .expect("same-tree aliases should insert");
    drop(source);

    let mut target = Connection::open_in_memory().expect("target database should open");
    initialize_code_schema(&target).expect("target schema should initialize");
    super::import_repository_from_database(
        &mut target,
        &source_path,
        "repo",
        Some("git_snapshot:test"),
    )
    .expect("snapshot aliases should import");
    let aliases = target
        .prepare(
            "SELECT resolved_commit_sha FROM code_repository_commit_scopes
             WHERE repository_id = 'repo' AND source_scope = 'git_snapshot:test'
             ORDER BY resolved_commit_sha",
        )
        .expect("alias query should prepare")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("aliases should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("aliases should decode");
    fs::remove_file(source_path).expect("temporary source database should be removed");

    assert_eq!(aliases, ["commit-a", "commit-b"]);
}

#[test]
fn metadata_only_import_does_not_copy_retiring_source_scope() {
    let source_path = temporary_sqlite_path("retiring-incremental-base-import");
    let source = Connection::open(&source_path).expect("source database should open");
    initialize_code_schema(&source).expect("source schema should initialize");
    source
        .execute_batch(
            "
            INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count, stale
            ) VALUES (
                'repo', 'fixture', '/tmp/repo', '[]', '[]', 'manual:retiring',
                'commit', 'tree', 'fresh', 0, 0, 0, 0, 0
            );
            INSERT INTO code_repository_aliases (alias, repository_id)
            VALUES ('fixture', 'repo');
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale, retiring
            ) VALUES (
                'manual:retiring', 'repo', 'commit', 'tree', '[]', '[]',
                0, 0, 0, 0, 0, 1
            );
            INSERT INTO code_repository_scope_gc_jobs (
                source_scope, repository_id, phase, search_rowid_cursor,
                deleted_rows, created_at_ms, updated_at_ms, last_error
            ) VALUES ('manual:retiring', 'repo', 'files', NULL, 1, 10, 11, NULL);
            ",
        )
        .expect("retiring source scope should persist");
    drop(source);

    let mut target = Connection::open_in_memory().expect("target database should open");
    initialize_code_schema(&target).expect("target schema should initialize");
    super::import_repository_from_database(&mut target, &source_path, "repo", None)
        .expect("repository metadata should import without its retiring scope");
    let (target_repository_count, target_scope_count): (usize, usize) = target
        .query_row(
            "SELECT
                 (SELECT COUNT(*) FROM code_repositories WHERE repository_id = 'repo'),
                 (SELECT COUNT(*) FROM code_repository_scopes WHERE repository_id = 'repo')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("metadata-only target counts should load");
    let attached = target
        .prepare("PRAGMA database_list")
        .expect("database list should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("database list should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("database names should decode");
    fs::remove_file(source_path).expect("temporary source database should be removed");

    assert_eq!(target_repository_count, 1);
    assert_eq!(target_scope_count, 0);
    assert!(!attached.iter().any(|name| name == "relay_import"));
}

#[test]
fn detaches_import_database_when_repository_is_missing() {
    let source_path = temporary_sqlite_path("missing-repository-import");
    let source = Connection::open(&source_path).expect("source database should open");
    initialize_code_schema(&source).expect("source schema should initialize");
    drop(source);

    let mut target = Connection::open_in_memory().expect("target database should open");
    initialize_code_schema(&target).expect("target schema should initialize");
    let error = super::import_repository_from_database(
        &mut target,
        &source_path,
        "missing",
        Some("git_snapshot:missing"),
    )
    .expect_err("missing repository should be rejected");
    let attached_databases = target
        .prepare("PRAGMA database_list")
        .expect("database list should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("database list should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("database names should decode");
    fs::remove_file(source_path).expect("temporary source database should be removed");

    assert!(
        matches!(error, StorageError::InvalidInput(message) if message.contains("code repository 'missing' is missing"))
    );
    assert!(!attached_databases.iter().any(|name| name == "relay_import"));
}

#[test]
fn populated_missing_index_rejects_import_before_target_metadata_mutation() {
    let source_path = temporary_sqlite_path("populated-missing-index-import");
    let source = Connection::open(&source_path).expect("source database should open");
    initialize_code_schema(&source).expect("source schema should initialize");
    source
        .execute(
            "INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                state, indexed_file_count, symbol_count, reference_count, chunk_count, stale
             ) VALUES ('incoming', 'incoming', '/tmp/incoming', '[]', '[]',
                       'fresh', 0, 0, 0, 0, 0)",
            [],
        )
        .expect("incoming repository should insert");
    drop(source);

    let mut target = Connection::open_in_memory().expect("target database should open");
    initialize_code_schema(&target).expect("target schema should initialize");
    target
        .execute_batch(
            "INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                state, indexed_file_count, symbol_count, reference_count, chunk_count, stale
             ) VALUES ('active', 'active', '/tmp/active', '[]', '[]',
                       'fresh', 0, 0, 0, 0, 0);
             INSERT INTO code_repository_search_metadata (
                source_scope, document_kind, record_id, path, search_rowid
             ) VALUES ('active-scope', 'symbol', 'active-symbol', 'src/lib.rs', 1);",
        )
        .expect("active target evidence should insert");

    let error = super::import_repository_from_database(
        &mut target,
        &source_path,
        "incoming",
        Some("incoming-scope"),
    )
    .expect_err("populated missing index must reject direct import");
    let (incoming_count, active_count, metadata_count, created_index_count): (i64, i64, i64, i64) =
        target
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM code_repositories WHERE repository_id = 'incoming'),
                    (SELECT COUNT(*) FROM code_repositories WHERE repository_id = 'active'),
                    (SELECT COUNT(*) FROM code_repository_search_metadata WHERE record_id = 'active-symbol'),
                    (SELECT COUNT(*) FROM sqlite_schema WHERE type = 'index' AND name = 'code_repository_symbols_name_path_lookup')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("target rollback state should load");
    let attached_databases = target
        .prepare("PRAGMA database_list")
        .expect("database list should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("database list should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("database names should decode");
    fs::remove_file(source_path).expect("temporary source database should be removed");

    assert!(matches!(error, StorageError::Invariant(_)));
    assert_eq!(incoming_count, 0);
    assert_eq!(active_count, 1);
    assert_eq!(metadata_count, 1);
    assert_eq!(created_index_count, 0);
    assert!(!attached_databases.iter().any(|name| name == "relay_import"));
}

fn temporary_sqlite_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!(
        "relay-knowledge-{label}-{}-{nanos}.sqlite",
        std::process::id()
    ));
    path
}
