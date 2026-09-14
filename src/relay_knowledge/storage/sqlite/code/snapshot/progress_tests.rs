use super::super::*;
use crate::{
    code::feature_flags::{FeatureFlagFileInput, extract_feature_flags},
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange,
    },
    storage::{SqliteGraphStore, StorageError},
};

const TEST_SOURCE_SCOPE: &str = "git_snapshot:test";

#[tokio::test]
async fn fresh_direct_snapshot_prepares_query_indexes_and_counts_feature_flag_rows() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", vec![], vec![])
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");

    let summary = store
        .apply_code_index_snapshot(snapshot_with_feature_flags())
        .await
        .expect("snapshot should apply");

    assert_eq!(summary.progress.sqlite_write_count, 4);
    let query_index_exists = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT EXISTS (
                        SELECT 1 FROM sqlite_schema
                        WHERE type = 'index' AND name = 'code_repository_chunks_lookup'
                    )",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("direct snapshot query-index state should load");
    assert!(query_index_exists);
}

#[tokio::test]
async fn direct_snapshot_existing_target_requires_staging_before_replacing_active_facts() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", vec![], vec![])
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(snapshot_with_feature_flags())
        .await
        .expect("initial snapshot should apply");
    store
        .run(|connection| {
            connection.execute("DROP INDEX code_repository_chunks_lookup", [])?;
            Ok(())
        })
        .await
        .expect("populated-owner query index should drop");
    let mut replacement = snapshot_with_feature_flags();
    replacement.files[0].blob_hash = "replacement-hash".to_owned();
    replacement.chunks[0].content = "replacement content".to_owned();

    let error = store
        .apply_code_index_snapshot(replacement)
        .await
        .expect_err("direct publication must reject an existing target");
    assert!(matches!(error, StorageError::DurableStagingRequired(_)));
    let (blob_hash, chunk_content, active_scope) = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT
                        (SELECT blob_hash FROM code_repository_files WHERE file_id = 'file'),
                        (SELECT content FROM code_repository_chunks WHERE chunk_id = 'chunk'),
                        (SELECT last_indexed_scope_id FROM code_repositories WHERE repository_id = 'repo')",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("active snapshot should remain readable");

    assert_eq!(blob_hash, "file-hash");
    assert!(chunk_content.contains("CHECKOUT_V2"));
    assert_eq!(active_scope.as_deref(), Some(TEST_SOURCE_SCOPE));
}

fn snapshot_with_feature_flags() -> CodeIndexSnapshot {
    let content =
        "if std::env::var(\"CHECKOUT_V2\").is_ok() {}\nconfig.get_bool(\"payments.enabled\");";
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![RepositoryCodeFileRecord {
            repository_id: "repo".to_owned(),
            source_scope: TEST_SOURCE_SCOPE.to_owned(),
            file_id: "file".to_owned(),
            path: "src/flags.rs".to_owned(),
            language_id: "rust".to_owned(),
            blob_hash: "file-hash".to_owned(),
            byte_len: content.len(),
            line_count: 2,
            parse_status: CodeParseStatus::Parsed,
            is_generated: false,
            degraded_reason: None,
        }],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: TEST_SOURCE_SCOPE,
            file_id: "file",
            path: "src/flags.rs",
            language_id: "rust",
            content,
            config_facts: &[],
        })
        .expect("feature flag fixture should extract"),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: vec![RepositoryCodeChunkRecord {
            repository_id: "repo".to_owned(),
            source_scope: TEST_SOURCE_SCOPE.to_owned(),
            chunk_id: "chunk".to_owned(),
            file_id: "file".to_owned(),
            path: "src/flags.rs".to_owned(),
            language_id: "rust".to_owned(),
            content: content.to_owned(),
            byte_range: RepositoryCodeRange {
                start: 0,
                end: u32::try_from(content.len()).expect("fixture length should fit"),
            },
            line_range: RepositoryCodeRange { start: 1, end: 2 },
            symbol_snapshot_id: None,
        }],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}
