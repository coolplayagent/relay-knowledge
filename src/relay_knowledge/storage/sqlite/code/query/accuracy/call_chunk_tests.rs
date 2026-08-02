use super::*;

#[tokio::test]
async fn callee_queries_rank_resolved_edges_before_ambiguous_ties() {
    let store = store_with_repository_snapshot(snapshot_with_resolved_callee_tie()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "cma_debugfs_init",
                selector,
                CodeQueryKind::Callees,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("callee query should succeed");

    assert_eq!(
        hits.iter()
            .map(|hit| hit.excerpt.as_str())
            .collect::<Vec<_>>(),
        vec![
            "cma_debugfs_init calls cma_debugfs_add_one",
            "cma_debugfs_init calls debugfs_create_dir",
        ]
    );
}

#[tokio::test]
async fn caller_queries_keep_best_ranked_fts_candidates_before_bounded_scoring() {
    let store = store_with_repository_snapshot(snapshot_with_many_caller_candidate_ties()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "TargetCall exactOwner",
                selector,
                CodeQueryKind::Callers,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].excerpt, "exactOwner calls TargetCall");
    assert_eq!(hits[0].path, "src/exact_owner.py");
}

#[tokio::test]
async fn caller_queries_rank_matching_caller_context_before_same_callee_noise() {
    let store = store_with_repository_snapshot(snapshot_with_same_callee_context_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "TargetCall exactOwner",
                selector,
                CodeQueryKind::Callers,
                3,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].excerpt, "exactOwner calls TargetCall");
    assert_eq!(hits[0].path, "src/z_exact_owner.py");
}

#[tokio::test]
async fn callee_queries_prioritize_related_callee_identifier_parts() {
    let store = store_with_repository_snapshot(snapshot_with_related_callee_names()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "do_mmap",
                selector,
                CodeQueryKind::Callees,
                3,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("callee query should succeed");

    assert_eq!(hits[0].excerpt, "do_mmap calls mmap_region");
}

#[tokio::test]
async fn caller_queries_use_caller_chunk_excerpt_when_available() {
    let store = store_with_repository_snapshot(snapshot_with_call_site_chunk()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "NewLRUCache",
                selector,
                CodeQueryKind::Callers,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "db/db_impl.cc");
    assert_eq!(
        hits[0].excerpt,
        "SanitizeOptions calls NewLRUCache: result.block_cache = NewLRUCache(8 << 20);"
    );
}

#[tokio::test]
async fn hybrid_chunk_queries_do_not_require_every_query_term_in_one_candidate() {
    let store = store_with_repository_snapshot(snapshot_with_eval_checkpoint_chunk()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "EvalCheckpointStore signature mismatch append result",
                selector,
                CodeQueryKind::Hybrid,
                3,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams_evals/checkpoint.py");
    assert!(hits[0].excerpt.contains("EvalCheckpointStore"));
}

#[tokio::test]
async fn hybrid_chunk_queries_prioritize_abstract_interfaces_over_usage_fixtures() {
    let store = store_with_repository_snapshot(snapshot_with_cache_interface_chunk_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "cache interface lookup insert total charge lru",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, "include/leveldb/cache.h");
    assert!(hits[0].excerpt.contains("class LEVELDB_EXPORT Cache"));
}

#[tokio::test]
async fn hybrid_chunk_queries_prioritize_header_declarations_for_api_context() {
    let store = store_with_repository_snapshot(snapshot_with_recovery_manifest_chunk_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "Recover descriptor save_manifest VersionEdit",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, "db/db_impl.h");
    assert!(hits[0].excerpt.contains("RecoverLogFile"));
}

#[tokio::test]
async fn parsed_hits_do_not_inherit_repository_degraded_reason() {
    let mut snapshot = snapshot_with_degraded_files(1);
    snapshot.files.push(file(
        "target-file",
        "src/lib.rs",
        "rust",
        CodeParseStatus::Parsed,
        None,
    ));
    snapshot.symbols.push(symbol(
        "target-symbol",
        "target-file",
        "src/lib.rs",
        "target",
    ));
    snapshot.changed_path_count = snapshot.files.len();
    let store = store_with_repository_snapshot(snapshot).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/lib.rs");
    assert_eq!(hits[0].degraded_reason, None);
}
