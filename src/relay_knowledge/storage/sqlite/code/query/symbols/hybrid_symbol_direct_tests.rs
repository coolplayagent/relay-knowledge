use super::*;
use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration, CodeRepositorySelector,
        FreshnessPolicy, RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeSymbolRecord,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

const HYBRID_DIRECT_TEST_SOURCE_SCOPE: &str = "code:test:hybrid-direct-generated:commit:tree";

#[tokio::test]
async fn direct_rows_prefer_handwritten_candidates_before_limit() {
    let store = store_with_snapshot(snapshot_with_generated_symbol_noise()).await;
    let status = store
        .code_repository_status("repo".to_owned())
        .await
        .expect("status should load")
        .expect("repository should exist");
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "Recover alpha beta gamma delta epsilon",
        selector,
        CodeQueryKind::Hybrid,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let rows = store
        .run_read(move |connection| search_hybrid_direct_symbol_rows(connection, &status, &request))
        .await
        .expect("hybrid direct rows should load");

    assert_eq!(
        rows.first().map(|row| row.path.as_str()),
        Some("src/zz_handwritten.rs")
    );
}

fn snapshot_with_generated_symbol_noise() -> CodeIndexSnapshot {
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    for index in 0..120 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/recover_{index:03}.rs");
        let mut generated_file = file(&file_id, &path);
        generated_file.is_generated = true;
        files.push(generated_file);
        symbols.push(symbol(
            &format!("generated-recover-{index:03}"),
            &file_id,
            &path,
        ));
    }
    files.push(file("handwritten-file", "src/zz_handwritten.rs"));
    symbols.push(symbol(
        "handwritten-recover",
        "handwritten-file",
        "src/zz_handwritten.rs",
    ));

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: HYBRID_DIRECT_TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: files.len(),
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files,
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn file(file_id: &str, path: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: HYBRID_DIRECT_TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}

fn symbol(symbol_snapshot_id: &str, file_id: &str, path: &str) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: HYBRID_DIRECT_TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::Recover", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: "Recover".to_owned(),
        qualified_name: "RecoverAlphaBetaGammaDeltaEpsilon".to_owned(),
        kind: "function".to_owned(),
        signature: "fn Recover_alpha_beta_gamma_delta_epsilon()".to_owned(),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 1, end: 1 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_role: None,
    }
}

async fn store_with_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");

    store
}
