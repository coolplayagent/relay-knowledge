use super::*;

#[tokio::test]
async fn full_scope_serves_narrower_query_filters() {
    let store = store_with_repository_snapshot(snapshot_with_target_symbol()).await;
    let path_selector = CodeRepositorySelector::new(
        "fixture",
        "commit",
        vec!["src/lib.rs".to_owned()],
        Vec::new(),
    )
    .expect("selector should validate");
    let language_selector =
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let no_match_selector =
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");

    let path_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                path_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("narrower path filter should use full scope");
    let language_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                language_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("narrower language filter should use full scope");
    let no_match_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                no_match_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("non-matching language filter should return no hits");

    assert_eq!(path_hits.len(), 1);
    assert_eq!(language_hits.len(), 1);
    assert!(no_match_hits.is_empty());
}

#[tokio::test]
async fn full_scope_path_filters_prune_fts_candidates_before_limit() {
    let store =
        store_with_repository_snapshot(snapshot_with_path_filtered_candidate_overflow()).await;
    let selector =
        CodeRepositorySelector::new("fixture", "commit", vec!["src".to_owned()], Vec::new())
            .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                selector,
                CodeQueryKind::Definition,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("path-filtered full-scope query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/target.rs");
    assert_eq!(hits[0].excerpt, "fn target()");
}

#[tokio::test]
async fn restrictive_scope_rejects_query_filters_outside_indexed_scope() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_target_symbol(),
        vec!["src".to_owned()],
        vec!["rust".to_owned()],
    )
    .await;
    let narrower_selector = CodeRepositorySelector::new(
        "fixture",
        "commit",
        vec!["src/lib.rs".to_owned()],
        Vec::new(),
    )
    .expect("selector should validate");
    let unsupported_path_selector =
        CodeRepositorySelector::new("fixture", "commit", vec!["tests".to_owned()], Vec::new())
            .expect("selector should validate");
    let unsupported_language_selector =
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");

    let narrower_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                narrower_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("narrower path filter should use the indexed base scope");
    let path_error = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                unsupported_path_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect_err("path outside indexed scope should be rejected");
    let language_error = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                unsupported_language_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect_err("language outside indexed scope should be rejected");

    assert_eq!(narrower_hits.len(), 1);
    assert!(path_error.to_string().contains("requested filters"));
    assert!(language_error.to_string().contains("requested filters"));
}
