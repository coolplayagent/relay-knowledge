use super::*;
use crate::{
    code::feature_flags::{FeatureFlagFileInput, extract_feature_flags},
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange,
    },
    storage::SqliteGraphStore,
};

const TEST_SOURCE_SCOPE: &str = "git_snapshot:test";

#[tokio::test]
async fn snapshot_progress_counts_feature_flag_rows() {
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
        })
        .expect("feature flag fixture should extract"),
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
        diagnostics: Vec::new(),
    }
}
