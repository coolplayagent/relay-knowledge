use super::*;

#[tokio::test]
async fn exact_identifier_matches_rank_before_substring_matches() {
    let store = store_with_repository_snapshot(snapshot_with_exact_match_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let definition_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "_build_service",
                selector.clone(),
                CodeQueryKind::Definition,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("definition query should succeed");
    let caller_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "_summary",
                selector.clone(),
                CodeQueryKind::Callers,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");
    let callee_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "_summary",
                selector,
                CodeQueryKind::Callees,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("callee query should succeed");

    assert_eq!(definition_hits[0].excerpt, "fn _build_service()");
    assert_eq!(caller_hits[0].excerpt, "list_connectors calls _summary");
    assert_eq!(callee_hits[0].excerpt, "_summary calls ConnectorSummary");
}

#[tokio::test]
async fn definition_queries_rank_own_camel_case_symbol_name_before_signature_mentions() {
    let store = store_with_repository_snapshot(snapshot_with_type_name_signature_mentions()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "w3 save request",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("definition query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams/connector/w3_models.py");
    assert_eq!(hits[0].excerpt, "class W3ConnectorSaveRequest(BaseModel):");
}

#[tokio::test]
async fn exact_camel_case_definition_queries_rank_own_symbol_before_signature_mentions() {
    let store = store_with_repository_snapshot(snapshot_with_type_name_signature_mentions()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "W3ConnectorSaveRequest",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("definition query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams/connector/w3_models.py");
    assert_eq!(hits[0].excerpt, "class W3ConnectorSaveRequest(BaseModel):");
}

#[tokio::test]
async fn exact_definition_queries_rank_name_match_when_many_signatures_mention_it() {
    let store = store_with_repository_snapshot(snapshot_with_many_signature_mentions()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "W3ConnectorSaveRequest",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("definition query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams/connector/w3_models.py");
    assert_eq!(hits[0].excerpt, "class W3ConnectorSaveRequest(BaseModel):");
}

#[tokio::test]
async fn fuzzy_definition_queries_rank_multi_part_symbol_names_before_single_token_noise() {
    let store = store_with_repository_snapshot(snapshot_with_archive_output_dir_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "archive old eval output directory timestamp suffix",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams_evals/checkpoint.py");
    assert!(hits[0].excerpt.contains("fn archive_output_dir()"));
}

#[tokio::test]
async fn fuzzy_symbol_queries_recall_identifier_when_extra_terms_are_not_in_symbol_document() {
    let store =
        store_with_repository_snapshot(snapshot_with_checkpoint_version_constant_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "checkpoint metadata version constant",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams_evals/checkpoint.py");
    assert!(hits[0].excerpt.contains("_CHECKPOINT_VERSION"));
}

#[tokio::test]
async fn scoped_definition_queries_rank_scoped_member_before_token_permutations() {
    let store = store_with_repository_snapshot(snapshot_with_scoped_cpp_definition_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let db_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "DB::Open",
                selector.clone(),
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("DB::Open query should succeed");
    let write_batch_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "WriteBatch::Put",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("WriteBatch::Put query should succeed");

    assert_eq!(db_hits[0].path, "db/db_impl.cc");
    assert_eq!(db_hits[0].line_range.start, 1503);
    assert_eq!(write_batch_hits[0].path, "db/write_batch.cc");
    assert_eq!(write_batch_hits[0].line_range.start, 98);
}
