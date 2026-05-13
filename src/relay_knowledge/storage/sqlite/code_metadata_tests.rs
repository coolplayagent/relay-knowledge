use super::code_tests::snapshot_with_chunk;
use super::*;
use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryRegistration, CodeRepositorySelector, CodeRetrievalLayer,
        FreshnessPolicy,
    },
    storage::SqliteGraphStore,
};
use rusqlite::Connection;

#[test]
fn legacy_scope_migration_recreates_lookup_indexes_on_active_tables() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repositories (
                repository_id TEXT PRIMARY KEY,
                alias TEXT NOT NULL UNIQUE,
                root_path TEXT NOT NULL,
                path_filters_json TEXT NOT NULL,
                language_filters_json TEXT NOT NULL,
                last_indexed_commit TEXT,
                tree_hash TEXT,
                state TEXT NOT NULL,
                indexed_file_count INTEGER NOT NULL,
                symbol_count INTEGER NOT NULL,
                reference_count INTEGER NOT NULL,
                chunk_count INTEGER NOT NULL,
                stale INTEGER NOT NULL,
                degraded_reason TEXT
            );
            CREATE TABLE code_repository_files (repository_id TEXT NOT NULL, path TEXT NOT NULL, blob_hash TEXT NOT NULL);
            CREATE TABLE code_repository_symbols (repository_id TEXT NOT NULL, name TEXT NOT NULL, qualified_name TEXT NOT NULL, path TEXT NOT NULL);
            CREATE TABLE code_repository_references (repository_id TEXT NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL, path TEXT NOT NULL);
            CREATE TABLE code_repository_calls (repository_id TEXT NOT NULL, callee_name TEXT NOT NULL, caller_name TEXT, path TEXT NOT NULL);
            CREATE TABLE code_repository_imports (repository_id TEXT NOT NULL, module TEXT NOT NULL, path TEXT NOT NULL);
            CREATE TABLE code_repository_chunks (repository_id TEXT NOT NULL, path TEXT NOT NULL);
            CREATE INDEX code_repository_symbols_lookup ON code_repository_symbols(repository_id, name, qualified_name, path);
            CREATE INDEX code_repository_references_lookup ON code_repository_references(repository_id, name, kind, path);
            CREATE INDEX code_repository_calls_lookup ON code_repository_calls(repository_id, callee_name, caller_name, path);
            CREATE INDEX code_repository_imports_lookup ON code_repository_imports(repository_id, module, path);
            CREATE INDEX code_repository_chunks_lookup ON code_repository_chunks(repository_id, path);
            ",
        )
        .expect("legacy schema should be created");

    initialize_code_schema(&connection).expect("schema migration should run");

    for (index, table) in [
        ("code_repository_symbols_lookup", "code_repository_symbols"),
        (
            "code_repository_references_lookup",
            "code_repository_references",
        ),
        ("code_repository_calls_lookup", "code_repository_calls"),
        ("code_repository_imports_lookup", "code_repository_imports"),
        ("code_repository_chunks_lookup", "code_repository_chunks"),
    ] {
        let table_name: String = connection
            .query_row(
                "SELECT tbl_name FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [index],
                |row| row.get(0),
            )
            .expect("index should exist");
        assert_eq!(table_name, table);
    }
}

#[tokio::test]
async fn stores_code_repository_and_queries_fallback_chunks() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    let snapshot = snapshot_with_chunk("repo", "src/lib.rs", "fn retry_policy() {}");
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "retry_policy",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/lib.rs");
    assert_eq!(hits[0].resolved_commit_sha, "commit");
    assert!(
        !hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[tokio::test]
async fn repository_id_lookup_takes_precedence_over_alias_like_ids() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo:first",
                "first",
                "/tmp/first",
                Vec::new(),
                Vec::new(),
            )
            .expect("first registration should validate"),
        )
        .await
        .expect("first repository should persist");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo:second",
                "repo:first",
                "/tmp/second",
                Vec::new(),
                Vec::new(),
            )
            .expect("second registration should validate"),
        )
        .await
        .expect("second repository should persist");

    let status = store
        .code_repository_status("repo:first".to_owned())
        .await
        .expect("status should query")
        .expect("repository id should resolve");

    assert_eq!(status.repository_id, "repo:first");
    assert_eq!(status.alias, "first");
}

#[tokio::test]
async fn repo_prefixed_alias_resolves_when_repository_id_is_absent() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo:actual",
                "repo:team-a",
                "/tmp/actual",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");

    let status = store
        .code_repository_status("repo:team-a".to_owned())
        .await
        .expect("status should query")
        .expect("repo-prefixed alias should resolve");

    assert_eq!(status.repository_id, "repo:actual");
    assert_eq!(status.alias, "repo:team-a");
}
