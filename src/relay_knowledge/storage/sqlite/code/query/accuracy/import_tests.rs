use super::*;

#[tokio::test]
async fn import_queries_match_include_targets_not_source_paths() {
    let store = store_with_repository_snapshot(snapshot_with_c_imports()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "linux/debugfs.h",
                selector,
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("import query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "mm/cma_debug.c");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("include/linux/debugfs.h")
    );
}

#[tokio::test]
async fn import_queries_can_match_importing_source_paths() {
    let store = store_with_repository_snapshot(snapshot_with_c_imports()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "mm/cma_debug.c",
                selector,
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("import query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "mm/cma_debug.c");
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("include/linux/debugfs.h")
    );
}

#[tokio::test]
async fn import_queries_rank_earlier_matching_includes_before_later_ties() {
    let store = store_with_repository_snapshot(snapshot_with_repeated_c_imports()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "linux/debugfs.h",
                selector,
                CodeQueryKind::Imports,
                3,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("import query should succeed");

    assert_eq!(
        hits.iter().map(|hit| hit.path.as_str()).collect::<Vec<_>>(),
        vec!["mm/cma_debug.c", "fs/debugfs/file.c", "fs/debugfs/inode.c"]
    );
}
