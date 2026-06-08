use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;

use super::{code_snapshot, initialize_code_schema};

#[test]
fn imports_legacy_code_snapshots_without_route_table_or_symbol_role_column() {
    let source_path = temporary_sqlite_path("legacy-code-import");
    let source = Connection::open(&source_path).expect("source database should open");
    initialize_code_schema(&source).expect("source schema should initialize");
    source
        .execute_batch(
            "
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
            CREATE TABLE legacy_code_repository_symbols AS
            SELECT repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
                   file_id, path, language_id, name, qualified_name, kind, signature,
                   doc_comment, byte_start, byte_end, line_start, line_end
            FROM code_repository_symbols;
            DROP TABLE code_repository_symbols;
            ALTER TABLE legacy_code_repository_symbols RENAME TO code_repository_symbols;
            DROP TABLE code_repository_routes;
            ",
        )
        .expect("legacy source data should be installed");
    drop(source);

    let mut target = Connection::open_in_memory().expect("target database should open");
    initialize_code_schema(&target).expect("target schema should initialize");
    code_snapshot::import_repository_from_database(
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
    fs::remove_file(source_path).expect("temporary source database should be removed");

    assert!(symbol_role.is_none());
    assert_eq!(route_count, 0);
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
