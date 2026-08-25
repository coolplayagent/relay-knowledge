use super::search_imports;
use crate::domain::{
    CodeQueryKind, CodeRepositorySelector, CodeRepositoryStatus, CodeRetrievalRequest,
    FreshnessPolicy,
};
use rusqlite::Connection;

#[test]
fn import_search_keeps_identifier_edges_when_the_fts_read_model_is_absent() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                file_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                is_generated INTEGER NOT NULL,
                line_count INTEGER NOT NULL
            );
            CREATE TABLE code_repository_imports (
                source_scope TEXT NOT NULL,
                import_id TEXT NOT NULL,
                file_id TEXT NOT NULL,
                path TEXT NOT NULL,
                module TEXT NOT NULL,
                target_hint TEXT,
                resolution_state TEXT NOT NULL,
                confidence_basis_points INTEGER NOT NULL,
                confidence_tier TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            CREATE TABLE code_repository_chunks (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                chunk_id TEXT NOT NULL
            );
            CREATE TABLE code_repository_symbols (
                source_scope TEXT NOT NULL,
                symbol_snapshot_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                name TEXT NOT NULL,
                line_start INTEGER NOT NULL
            );
            INSERT INTO code_repository_files VALUES (
                'scope', 'file-1', 'src/UtilityConsumer.java', 'java', 0, 12
            );
            INSERT INTO code_repository_imports VALUES (
                'scope', 'import-1', 'file-1', 'src/UtilityConsumer.java',
                'import org.springframework.util.ObjectUtils;', NULL,
                'unresolved', 10000, 'extracted', 1, 1
            );
            INSERT INTO code_repository_chunks VALUES (
                'scope', 'src/UtilityConsumer.java',
                'import org.springframework.util.ObjectUtils;\nObjectUtils.isEmpty(value);',
                1, 'chunk-1'
            );
            ",
        )
        .expect("direct import facts should persist without an FTS table");
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "ObjectUtils",
        selector,
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = search_imports(&connection, &status(), &request)
        .expect("an optional FTS outage must preserve direct import edges");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/UtilityConsumer.java");
    assert_eq!(hits[0].edge_kind.as_deref(), Some("import"));
}

fn status() -> CodeRepositoryStatus {
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
        reference_count: 0,
        chunk_count: 1,
        stale: false,
        degraded_reason: None,
    }
}
