use super::*;

#[tokio::test]
async fn impact_imports_use_rust_symbol_namespace_seeds() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_rust_symbol_importer(),
        Vec::new(),
        vec!["rust".to_owned()],
    )
    .await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["src/lib.rs".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");

    assert!(hits.iter().any(|hit| {
        hit.path == "src/main.rs"
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::ImportGraph)
    }));
}

#[tokio::test]
async fn impact_chunk_hits_with_symbol_snapshots_include_canonical_identity() {
    let mut snapshot = snapshot_with_symbol_and_matching_chunk();
    snapshot.chunks[0].content = "changed body".to_owned();
    let store = store_with_repository_snapshot(snapshot).await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["src/lib.rs".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");
    let chunk_hit = hits
        .iter()
        .find(|hit| hit.file_id.as_deref() == Some("target-file"))
        .expect("chunk hit should be returned");

    assert_eq!(
        chunk_hit.symbol_snapshot_id.as_deref(),
        Some("target-symbol")
    );
    assert_eq!(
        chunk_hit.canonical_symbol_id.as_deref(),
        Some("repo://repo/src::lib.rs::target")
    );
}

#[tokio::test]
async fn impact_preserves_deleted_rust_paths_under_language_filters() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_deleted_rust_module_importer(),
        Vec::new(),
        vec!["rust".to_owned()],
    )
    .await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["src/deleted.rs".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");

    assert!(hits.iter().any(|hit| {
        hit.path == "src/caller.rs"
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::ImportGraph)
    }));
}

#[tokio::test]
async fn impact_preserves_deleted_go_paths_under_language_filters() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_deleted_go_module_importer(),
        Vec::new(),
        vec!["go".to_owned()],
    )
    .await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["go".to_owned()])
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["deleted.go".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");

    assert!(hits.iter().any(|hit| {
        hit.path == "caller.go"
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::ImportGraph)
    }));
}

#[tokio::test]
async fn impact_does_not_fall_back_to_all_symbols_for_non_symbol_paths() {
    let store = store_with_repository_snapshot(snapshot_with_language_edges()).await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["README.md".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");

    assert!(hits.is_empty());
}

#[tokio::test]
async fn impact_callers_match_resolved_symbol_identity() {
    let store = store_with_repository_snapshot(snapshot_with_duplicate_callee_names()).await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["src/a.rs".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");

    assert!(hits.iter().any(|hit| hit.path == "src/caller_a.rs"));
    assert!(!hits.iter().any(|hit| hit.path == "src/caller_b.rs"));
    assert!(hits.iter().any(|hit| {
        hit.symbol_snapshot_id.as_deref() == Some("caller-a")
            && hit.canonical_symbol_id.as_deref() == Some("repo://repo/src::caller_a.rs::caller")
    }));
}

#[tokio::test]
async fn impact_seeds_respect_request_path_filters() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_out_of_scope_seed(),
        vec!["src".to_owned()],
        Vec::new(),
    )
    .await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", vec!["src".to_owned()], Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["tests/out.rs".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");

    assert!(hits.is_empty());
}

#[tokio::test]
async fn impact_callers_can_use_deleted_symbol_names() {
    let store = store_with_repository_snapshot(snapshot_with_unresolved_caller()).await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: Vec::new(),
                deleted_symbol_names: vec!["target".to_owned()],
            },
        )
        .await
        .expect("impact should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/caller.rs");
}
