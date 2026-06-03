use super::*;
use crate::{
    domain::{
        CodeCallRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord, code_snapshot_scope_id,
    },
    storage::SqliteGraphStore,
};

const BASE_SCOPE: &str = "git_snapshot:test";
const NEXT_SCOPE: &str = "git_snapshot:test-next";

#[tokio::test]
async fn incremental_call_search_uses_cloned_symbol_signatures() {
    let mut callee_symbol = symbol(
        BASE_SCOPE,
        "read-block-symbol",
        "callee-file",
        "src/table.rs",
        "ReadBlock",
    );
    callee_symbol.signature = "Status Table::ReadBlock(BlockContents* contents)".to_owned();
    let store = store_with_repository_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: BASE_SCOPE.to_owned(),
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
        files: vec![file(BASE_SCOPE, "callee-file", "src/table.rs")],
        symbols: vec![callee_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let mut incremental = incremental_snapshot();
    let mut changed_call = call(
        NEXT_SCOPE,
        "read-block-call",
        "caller-file",
        "src/caller.rs",
        Some("read-block-symbol"),
    );
    changed_call.caller_name = Some("InternalGet".to_owned());
    changed_call.callee_name = "ReadBlock".to_owned();
    changed_call.target_hint = Some("ReadBlock".to_owned());
    incremental.calls = vec![changed_call];
    store
        .apply_code_index_snapshot(incremental)
        .await
        .expect("incremental snapshot should apply");

    let selector = CodeRepositorySelector::new("fixture", "commit-next", Vec::new(), Vec::new())
        .expect("selector should validate");
    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "Table",
                selector,
                CodeQueryKind::Callers,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller search should succeed");

    assert_eq!(hits[0].path, "src/caller.rs");
    assert!(hits[0].excerpt.contains("ReadBlock"));
}

#[tokio::test]
async fn incremental_call_search_batches_cloned_symbol_signature_lookups() {
    let symbol_count = 520;
    let mut symbols = Vec::new();
    for index in 0..symbol_count {
        let name = format!("ReadBlock{index}");
        let mut callee_symbol = symbol(
            BASE_SCOPE,
            &format!("read-block-symbol-{index}"),
            "callee-file",
            "src/table.rs",
            &name,
        );
        callee_symbol.signature = format!("Status Owner{index}::{name}(BlockContents* contents)");
        symbols.push(callee_symbol);
    }
    let store = store_with_repository_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: BASE_SCOPE.to_owned(),
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
        files: vec![file(BASE_SCOPE, "callee-file", "src/table.rs")],
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let mut incremental = incremental_snapshot();
    incremental.calls = (0..symbol_count)
        .map(|index| {
            let name = format!("ReadBlock{index}");
            let symbol_id = format!("read-block-symbol-{index}");
            let mut changed_call = call(
                NEXT_SCOPE,
                &format!("read-block-call-{index}"),
                "caller-file",
                "src/caller.rs",
                Some(symbol_id.as_str()),
            );
            changed_call.caller_name = Some("InternalGet".to_owned());
            changed_call.callee_name = name.clone();
            changed_call.target_hint = Some(name);
            changed_call
        })
        .collect();
    store
        .apply_code_index_snapshot(incremental)
        .await
        .expect("incremental snapshot should apply");

    let selector = CodeRepositorySelector::new("fixture", "commit-next", Vec::new(), Vec::new())
        .expect("selector should validate");
    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "Owner519",
                selector,
                CodeQueryKind::Callers,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller search should succeed");

    assert_eq!(hits[0].path, "src/caller.rs");
    assert!(hits[0].excerpt.contains("ReadBlock519"));
}

#[tokio::test]
async fn incremental_path_cleanup_deletes_cloned_search_rows() {
    let store = store_with_repository_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: BASE_SCOPE.to_owned(),
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
        files: vec![file(BASE_SCOPE, "doc-file", "src/doc.rs")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![chunk(
            BASE_SCOPE,
            "doc-chunk",
            "doc-file",
            "src/doc.rs",
            "staleonly237xyz",
        )],
        diagnostics: Vec::new(),
    })
    .await;
    let mut incremental = incremental_snapshot();
    incremental.files = vec![file(NEXT_SCOPE, "doc-file-next", "src/doc.rs")];
    incremental.chunks = vec![chunk(
        NEXT_SCOPE,
        "doc-chunk-next",
        "doc-file-next",
        "src/doc.rs",
        "freshonly237xyz",
    )];
    store
        .apply_code_index_snapshot(incremental)
        .await
        .expect("incremental snapshot should apply");

    let selector = CodeRepositorySelector::new("fixture", "commit-next", Vec::new(), Vec::new())
        .expect("selector should validate");
    let stale_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "staleonly237xyz",
                selector.clone(),
                CodeQueryKind::Hybrid,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("stale search should query");
    let fresh_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "freshonly237xyz",
                selector,
                CodeQueryKind::Hybrid,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("fresh search should query");

    assert!(stale_hits.is_empty());
    assert_eq!(fresh_hits[0].path, "src/doc.rs");
}

fn incremental_snapshot() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: NEXT_SCOPE.to_owned(),
        base_resolved_commit_sha: Some("commit".to_owned()),
        resolved_commit_sha: "commit-next".to_owned(),
        tree_hash: "tree-next".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: false,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(NEXT_SCOPE, "caller-file", "src/caller.rs")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn chunk(
    source_scope: &str,
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        content: content.to_owned(),
        byte_range: RepositoryCodeRange {
            start: 0,
            end: content.len() as u32,
        },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: None,
    }
}

fn file(source_scope: &str, file_id: &str, path: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 20,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn symbol(
    source_scope: &str,
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: name.to_owned(),
        qualified_name: format!("{}::{name}", path.replace('/', "::")),
        kind: "function".to_owned(),
        signature: format!("fn {name}()"),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 20 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn call(
    source_scope: &str,
    call_id: &str,
    file_id: &str,
    path: &str,
    callee_symbol_snapshot_id: Option<&str>,
) -> CodeCallRecord {
    CodeCallRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        call_id: call_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        caller_symbol_snapshot_id: None,
        caller_name: None,
        callee_symbol_snapshot_id: callee_symbol_snapshot_id.map(str::to_owned),
        callee_name: "target".to_owned(),
        target_hint: None,
        resolution_state: "resolved".to_owned(),
        confidence_basis_points: 8_000,
        confidence_tier: "inferred".to_owned(),
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

async fn store_with_repository_snapshot(mut snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    retarget_snapshot_to_fact_scope(&mut snapshot);
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");

    store
}

fn retarget_snapshot_to_fact_scope(snapshot: &mut CodeIndexSnapshot) {
    let source_scope = code_snapshot_scope_id(
        &snapshot.repository_id,
        &snapshot.tree_hash,
        &snapshot.path_filters,
        &snapshot.language_filters,
    );
    snapshot.source_scope = source_scope.clone();
    for file in &mut snapshot.files {
        file.source_scope = source_scope.clone();
    }
    for symbol in &mut snapshot.symbols {
        symbol.source_scope = source_scope.clone();
    }
    for call in &mut snapshot.calls {
        call.source_scope = source_scope.clone();
    }
    for chunk in &mut snapshot.chunks {
        chunk.source_scope = source_scope.clone();
    }
}
