//! Cross-layer reference recall when structured edges only partially cover an identity.

use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, CodeRetrievalLayer, FreshnessPolicy, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeReferenceRecord,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

const TEST_SOURCE_SCOPE: &str = "code:test:reference-underfill:commit:tree";

#[tokio::test]
async fn underfilled_reference_edges_are_completed_from_indexed_source_chunks() {
    let indexed_only_path = "Source/Core/Session.swift";
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("edge-file", "Source/Core/Adapter.swift", "swift"),
            file("chunk-file", indexed_only_path, "swift"),
        ],
        symbols: Vec::new(),
        references: vec![reference(
            "known-reference",
            "edge-file",
            "Source/Core/Adapter.swift",
            "SessionDelegate",
        )],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "edge-chunk",
                "edge-file",
                "Source/Core/Adapter.swift",
                "let adapterDelegate: SessionDelegate",
                "swift",
            ),
            chunk(
                "source-chunk",
                "chunk-file",
                indexed_only_path,
                "public let delegate: SessionDelegate",
                "swift",
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("SessionDelegate"))
        .await
        .expect("reference query should succeed");
    let source_hit = hits
        .iter()
        .find(|hit| hit.path == indexed_only_path)
        .expect("underfilled graph results should include the indexed source usage");

    assert!(source_hit.excerpt.contains("delegate: SessionDelegate"));
    assert!(
        source_hit
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[tokio::test]
async fn reference_chunk_completion_reserves_candidates_for_code_usage() {
    let source_path = "Source/Core/Session.swift";
    let mut files = vec![file("source-file", source_path, "swift")];
    let mut chunks = vec![chunk(
        "source-chunk",
        "source-file",
        source_path,
        "public let delegate: SessionDelegate",
        "swift",
    )];
    for index in 0..96 {
        let file_id = format!("notes-file-{index:03}");
        let path = format!("docs/releases/{index:03}.md");
        files.push(file(&file_id, &path, "markdown"));
        chunks.push(chunk(
            &format!("notes-chunk-{index:03}"),
            &file_id,
            &path,
            &"SessionDelegate release note ".repeat(16),
            "markdown",
        ));
    }
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
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
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks,
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("SessionDelegate"))
        .await
        .expect("reference query should preserve code candidates");

    assert!(hits.iter().any(|hit| hit.path == source_path));
    assert!(hits.iter().all(|hit| hit.language_id != "markdown"));
}

#[tokio::test]
async fn repeated_structured_reference_groups_receive_candidate_recall_priority() {
    let expected_path = "src/ZZFocused.java";
    let mut files = vec![file("expected-file", expected_path, "java")];
    let mut chunks = vec![chunk(
        "expected-chunk",
        "expected-file",
        expected_path,
        "return ObjectUtils.nullSafeEquals(left, right);",
        "java",
    )];
    let mut references = (0..4)
        .map(|index| {
            let mut reference = reference(
                &format!("expected-reference-{index}"),
                "expected-file",
                expected_path,
                "nullSafeEquals",
            );
            reference.target_hint = Some("ObjectUtils.nullSafeEquals".to_owned());
            reference
        })
        .collect::<Vec<_>>();
    for index in 0..96 {
        let file_id = format!("distractor-file-{index:03}");
        let path = format!("src/AA{index:03}.java");
        files.push(file(&file_id, &path, "java"));
        chunks.push(chunk(
            &format!("distractor-chunk-{index:03}"),
            &file_id,
            &path,
            "return ObjectUtils.nullSafeEquals(left, right);",
            "java",
        ));
        let mut distractor = reference(
            &format!("distractor-reference-{index:03}"),
            &file_id,
            &path,
            "nullSafeEquals",
        );
        distractor.target_hint = Some("ObjectUtils.nullSafeEquals".to_owned());
        references.push(distractor);
    }
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
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
        symbols: Vec::new(),
        references,
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks,
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("ObjectUtils"))
        .await
        .expect("grouped reference query should succeed");

    assert!(
        hits.iter().take(8).any(|hit| hit.path == expected_path),
        "repeated structured use should remain in the bounded candidate set: {hits:?}"
    );
}

#[tokio::test]
async fn exact_reference_chunk_completion_excludes_macro_definition_evidence() {
    let usage_path = "src/worker.c";
    let definition_path = "include/worker.h";
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("usage-file", usage_path, "c"),
            file("definition-file", definition_path, "c"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "usage-chunk",
                "usage-file",
                usage_path,
                "return TRACE_VALUE(worker->state);",
                "c",
            ),
            chunk(
                "definition-chunk",
                "definition-file",
                definition_path,
                "#define TRACE_VALUE(value) ((value) + 1)",
                "c",
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("TRACE_VALUE"))
        .await
        .expect("reference query should succeed");

    assert!(hits.iter().any(|hit| hit.path == usage_path));
    assert!(hits.iter().all(|hit| hit.path != definition_path));
}

fn request(query: &str) -> crate::domain::CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    crate::domain::CodeRetrievalRequest::new(
        query,
        selector,
        CodeQueryKind::References,
        8,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

fn file(file_id: &str, path: &str, language_id: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 80,
        line_count: 4,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}

fn reference(
    reference_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        reference_id: reference_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        name: name.to_owned(),
        kind: "type".to_owned(),
        target_symbol_snapshot_id: None,
        target_hint: Some(name.to_owned()),
        resolution_state: "ambiguous".to_owned(),
        confidence_basis_points: 5_000,
        confidence_tier: "ambiguous".to_owned(),
        byte_range: range(0, 1),
        line_range: range(1, 1),
    }
}

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    language_id: &str,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        content: content.to_owned(),
        byte_range: range(0, content.len() as u32),
        line_range: range(1, 1),
        symbol_snapshot_id: None,
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
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
