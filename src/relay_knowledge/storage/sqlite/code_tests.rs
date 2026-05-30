use super::*;
use crate::{
    code::feature_flags::{FeatureFlagFileInput, extract_feature_flags},
    domain::{
        CodeFeatureFlagRequest, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositorySelector, CodeRetrievalLayer, FreshnessPolicy,
    },
    storage::{SqliteGraphStore, StorageError},
};

#[path = "code_test_support.rs"]
mod code_test_support;

#[path = "code_snapshot_fixtures.rs"]
mod code_snapshot_fixtures;

pub(super) use code_snapshot_fixtures::*;

#[tokio::test]
async fn feature_flag_query_groups_config_sources_and_guarded_usage() {
    let mut snapshot = snapshot_with_chunk(
        "repo",
        "src/flags.rs",
        "if std::env::var(\"CHECKOUT_V2\").is_ok() {\n    enable_checkout();\n}\nconfig.get_bool(\"payments.enabled\");",
    );
    snapshot.files.push(code_test_support::file(
        "config-file",
        "config/flags.yaml",
        "yaml",
        CodeParseStatus::Parsed,
        None,
    ));
    snapshot.chunks.push(code_test_support::chunk(
        "config-chunk",
        "config-file",
        "config/flags.yaml",
        "payments.enabled: true\n",
        None,
    ));
    snapshot.feature_flags.extend(
        extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: code_test_support::TEST_SOURCE_SCOPE,
            file_id: "config-file",
            path: "config/flags.yaml",
            language_id: "yaml",
            content: "payments.enabled: true\n",
            config_facts: &[],
        })
        .expect("config feature flag fixture should extract"),
    );
    snapshot.changed_path_count = snapshot.files.len();
    let store = store_with_repository_snapshot(snapshot).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let flags = store
        .search_code_feature_flags(
            CodeFeatureFlagRequest::new(None, selector.clone(), 10, FreshnessPolicy::AllowStale)
                .expect("feature flag request should validate"),
        )
        .await
        .expect("feature flags should query");
    let filtered = store
        .search_code_feature_flags(
            CodeFeatureFlagRequest::new(
                Some("payments".to_owned()),
                selector,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("filtered feature flag request should validate"),
        )
        .await
        .expect("filtered feature flags should query");

    assert_eq!(flags.len(), 2);
    assert!(flags.iter().any(|flag| {
        flag.source_key == "CHECKOUT_V2"
            && flag
                .usages
                .iter()
                .any(|usage| usage.edge_kind == "guards_code")
    }));
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].source_key, "payments.enabled");
    assert!(
        filtered[0].usages.iter().any(|usage| {
            usage.path == "config/flags.yaml" && usage.edge_kind == "defines_config"
        })
    );
    assert!(
        filtered[0]
            .usages
            .iter()
            .any(|usage| { usage.path == "src/flags.rs" && usage.edge_kind == "reads_config" })
    );
}

#[tokio::test]
async fn feature_flag_query_filters_sdk_flag_keys() {
    let store = store_with_repository_snapshot(snapshot_with_chunk(
        "repo",
        "src/flags.ts",
        "if (openFeature.getBooleanValue(\"checkout_v2\", false)) {}\nlet variant = ldClient.variation(\"payment_flow\", false);",
    ))
    .await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let flags = store
        .search_code_feature_flags(
            CodeFeatureFlagRequest::new(
                Some("checkout".to_owned()),
                selector,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("feature flag request should validate"),
        )
        .await
        .expect("feature flags should query");

    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].source_kind, "sdk_flag_key");
    assert_eq!(flags[0].source_key, "checkout_v2");
    assert!(
        flags[0]
            .usages
            .iter()
            .any(|usage| usage.edge_kind == "guards_code")
    );
}

#[tokio::test]
async fn text_only_chunk_hits_are_marked_as_text_fallback() {
    let store = store_with_repository_snapshot(snapshot_with_chunk_status(
        "repo",
        "README.txt",
        "RetryPolicy appears in docs",
        CodeParseStatus::TextOnly,
        Some("tree-sitter grammar is not configured".to_owned()),
    ))
    .await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "RetryPolicy",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(hits.len(), 1);
    assert!(
        hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[tokio::test]
async fn hybrid_deduplication_preserves_retrieval_layers() {
    let store = store_with_repository_snapshot(snapshot_with_symbol_and_matching_chunk()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target fn",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(hits.len(), 1);
    assert!(
        hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::Symbol)
    );
    assert!(
        hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::Lexical)
    );
}

#[tokio::test]
async fn chunk_hits_with_symbol_snapshots_include_canonical_identity() {
    let mut snapshot = snapshot_with_symbol_and_matching_chunk();
    snapshot.chunks[0].content = "body text only".to_owned();
    let store = store_with_repository_snapshot(snapshot).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "body",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("chunk query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].symbol_snapshot_id.as_deref(), Some("target-symbol"));
    assert_eq!(
        hits[0].canonical_symbol_id.as_deref(),
        Some("repo://repo/src::lib.rs::target")
    );
}

#[tokio::test]
async fn schema_indexes_chunks_by_symbol_for_call_excerpt_lookup() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");

    let index_exists = store
        .run(|connection| {
            connection
                .query_row(
                    "
                    SELECT EXISTS(
                        SELECT 1
                        FROM sqlite_master
                        WHERE type = 'index'
                          AND name = 'code_repository_chunks_symbol_lookup'
                    )
                    ",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("schema index check should succeed");

    assert!(index_exists);
}

#[tokio::test]
async fn rejects_code_queries_for_unindexed_refs() {
    let store = store_with_repository_snapshot(snapshot_with_chunk(
        "repo",
        "src/lib.rs",
        "fn retry_policy() {}",
    ))
    .await;
    let selector = CodeRepositorySelector::new("fixture", "other", Vec::new(), Vec::new())
        .expect("selector should validate");

    let error = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "retry_policy",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect_err("stale ref should fail");

    assert!(error.to_string().contains("no index for ref other"));
}

#[tokio::test]
async fn rejects_code_queries_when_requested_filter_scope_was_not_indexed() {
    let mut snapshot = snapshot_with_chunk("repo", "src/lib.rs", "fn retry_policy() {}");
    snapshot.path_filters = vec!["src".to_owned()];
    let store = store_with_repository_snapshot(snapshot).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let error = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "retry_policy",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect_err("unindexed broader filter scope should fail");

    assert!(
        error
            .to_string()
            .contains("no index for ref commit and requested filters")
    );
}

#[tokio::test]
async fn code_queries_match_canonical_filter_spellings() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_chunk("repo", "src/lib.rs", "fn retry_policy() {}"),
        vec!["src/".to_owned()],
        Vec::new(),
    )
    .await;
    let selector =
        CodeRepositorySelector::new("fixture", "commit", vec!["./src".to_owned()], Vec::new())
            .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "retry_policy",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("canonical filter spelling should match indexed scope");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/lib.rs");
}

#[tokio::test]
async fn incremental_update_rejects_unrelated_filter_baselines() {
    let store = store_with_repository_snapshot(snapshot_with_chunk(
        "repo",
        "src/lib.rs",
        "fn retry_policy() {}",
    ))
    .await;
    let mut incremental = incremental_snapshot_for_parsed_file();
    incremental.path_filters = vec!["src".to_owned()];

    let error = store
        .apply_code_index_snapshot(incremental)
        .await
        .expect_err("unrelated filter baseline should fail");

    assert!(
        error
            .to_string()
            .contains("no matching indexed scope for incremental filters")
    );
}

#[tokio::test]
async fn incremental_update_matches_canonical_filter_baselines() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_chunk("repo", "src/lib.rs", "fn retry_policy() {}"),
        vec!["src/".to_owned()],
        Vec::new(),
    )
    .await;
    let mut incremental = incremental_snapshot_for_parsed_file();
    retarget_snapshot_scope(&mut incremental, "scope-next");
    incremental.resolved_commit_sha = "commit-next".to_owned();
    incremental.path_filters = vec!["./src".to_owned()];

    store
        .apply_code_index_snapshot(incremental)
        .await
        .expect("canonical baseline filters should match");

    let selector =
        CodeRepositorySelector::new("fixture", "commit-next", vec!["src".to_owned()], Vec::new())
            .expect("selector should validate");
    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "kept",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("incremental scope should be searchable");

    assert_eq!(hits.len(), 1);
}

#[tokio::test]
async fn incremental_update_can_use_persisted_filter_baselines_from_older_commits() {
    let mut scoped_base = snapshot_with_chunk("repo", "src/lib.rs", "fn old_policy() {}");
    retarget_snapshot_scope(&mut scoped_base, "scope-src-a");
    scoped_base.resolved_commit_sha = "commit-a".to_owned();
    scoped_base.tree_hash = "tree-a".to_owned();
    scoped_base.path_filters = vec!["src".to_owned()];
    let store = store_with_repository_snapshot(scoped_base).await;

    let mut current_other_scope =
        snapshot_with_chunk("repo", "tests/lib.rs", "fn test_policy() {}");
    retarget_snapshot_scope(&mut current_other_scope, "scope-tests-b");
    current_other_scope.resolved_commit_sha = "commit-b".to_owned();
    current_other_scope.tree_hash = "tree-b".to_owned();
    current_other_scope.path_filters = vec!["tests".to_owned()];
    store
        .apply_code_index_snapshot(current_other_scope)
        .await
        .expect("current unrelated scope should apply");

    let mut incremental = incremental_snapshot_for_parsed_file();
    retarget_snapshot_scope(&mut incremental, "scope-src-c");
    incremental.base_resolved_commit_sha = Some("commit-a".to_owned());
    incremental.resolved_commit_sha = "commit-c".to_owned();
    incremental.tree_hash = "tree-c".to_owned();
    incremental.path_filters = vec!["src".to_owned()];

    store
        .apply_code_index_snapshot(incremental)
        .await
        .expect("older persisted filter baseline should seed incremental scope");

    let selector =
        CodeRepositorySelector::new("fixture", "commit-c", vec!["src".to_owned()], Vec::new())
            .expect("selector should validate");
    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "kept",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("incremental scope should be searchable");

    assert_eq!(hits.len(), 1);
}

#[tokio::test]
async fn rejects_impact_kind_for_plain_code_queries() {
    let store = store_with_repository_snapshot(snapshot_with_chunk(
        "repo",
        "src/lib.rs",
        "fn retry_policy() {}",
    ))
    .await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let error = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "retry_policy",
                selector,
                CodeQueryKind::Impact,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect_err("impact query kind should require repo impact");

    assert!(error.to_string().contains("repo impact"));
}

#[tokio::test]
async fn language_filters_apply_to_references_calls_and_imports() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_language_edges(),
        vec!["src".to_owned()],
        vec!["rust".to_owned()],
    )
    .await;
    let selector = CodeRepositorySelector::new(
        "fixture",
        "commit",
        vec!["src".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("selector should validate");

    for kind in [
        CodeQueryKind::References,
        CodeQueryKind::Callers,
        CodeQueryKind::Imports,
    ] {
        let query = if kind == CodeQueryKind::Imports {
            "module"
        } else {
            "target"
        };
        let hits = store
            .search_code(
                crate::domain::CodeRetrievalRequest::new(
                    query,
                    selector.clone(),
                    kind,
                    10,
                    FreshnessPolicy::AllowStale,
                )
                .expect("request should validate"),
            )
            .await
            .expect("query should succeed");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].language_id, "rust");
        assert_eq!(hits[0].path, "src/lib.rs");
    }
}

#[tokio::test]
async fn edge_search_documents_store_file_languages_for_snapshot_indexes() {
    let store = store_with_repository_snapshot(snapshot_with_language_edges()).await;
    let rows = store
        .run(|connection| {
            let mut statement = connection.prepare(
                "
                SELECT document_kind, path, language_id
                FROM code_repository_search
                WHERE document_kind IN ('reference', 'import', 'call')
                ORDER BY document_kind ASC, path ASC
                ",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;

            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StorageError::from)
        })
        .await
        .expect("search rows should load");

    for document_kind in ["reference", "import", "call"] {
        assert!(rows.iter().any(|(kind, path, language)| {
            kind == document_kind && path == "src/lib.rs" && language == "rust"
        }));
        assert!(rows.iter().any(|(kind, path, language)| {
            kind == document_kind && path == "py/app.py" && language == "python"
        }));
    }
}

#[tokio::test]
async fn code_query_hits_include_symbol_identity_and_edge_diagnostics() {
    let symbol_store =
        store_with_repository_snapshot(snapshot_with_symbol_and_matching_chunk()).await;
    let edge_store = store_with_repository_snapshot(snapshot_with_language_edges()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let symbol_hits = symbol_store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                selector.clone(),
                CodeQueryKind::Symbol,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("symbol query should succeed");
    let call_hits = edge_store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                selector,
                CodeQueryKind::Callers,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");

    assert_eq!(
        symbol_hits[0].canonical_symbol_id.as_deref(),
        Some("repo://repo/src::lib.rs::target")
    );
    assert_eq!(call_hits[0].edge_kind.as_deref(), Some("call"));
    assert_eq!(
        call_hits[0].edge_resolution_state.as_deref(),
        Some("unresolved")
    );
    assert_eq!(
        call_hits[0].edge_confidence_tier.as_deref(),
        Some("ambiguous")
    );
    assert_eq!(call_hits[0].edge_confidence_basis_points, Some(2_500));
}

#[tokio::test]
async fn call_graph_hits_include_resolved_symbol_canonical_identity() {
    let store = store_with_repository_snapshot(snapshot_with_duplicate_callee_names()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let caller_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                selector.clone(),
                CodeQueryKind::Callers,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");
    let callee_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "caller",
                selector,
                CodeQueryKind::Callees,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("callee query should succeed");

    assert!(caller_hits.iter().any(|hit| {
        hit.symbol_snapshot_id.as_deref() == Some("caller-a")
            && hit.canonical_symbol_id.as_deref() == Some("repo://repo/src::caller_a.rs::caller")
    }));
    assert!(callee_hits.iter().any(|hit| {
        hit.symbol_snapshot_id.as_deref() == Some("target-a")
            && hit.canonical_symbol_id.as_deref() == Some("repo://repo/src::a.rs::target")
    }));
}

#[tokio::test]
async fn reference_hits_include_resolved_target_canonical_identity() {
    let store = store_with_repository_snapshot(snapshot_with_resolved_reference()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                selector,
                CodeQueryKind::References,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("reference query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].symbol_snapshot_id.as_deref(), Some("target-symbol"));
    assert_eq!(
        hits[0].canonical_symbol_id.as_deref(),
        Some("repo://repo/src::lib.rs::target")
    );
}

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

#[tokio::test]
async fn incremental_updates_retain_existing_degraded_status() {
    let store = store_with_repository_snapshot(snapshot_with_degraded_and_parsed_files()).await;
    store
        .apply_code_index_snapshot(incremental_snapshot_for_parsed_file())
        .await
        .expect("incremental snapshot should apply");

    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("status should load")
        .expect("status should exist");

    assert!(status.degraded_reason.is_some());
}

#[tokio::test]
async fn repository_report_counts_degraded_files_beyond_summary_limit() {
    let store = store_with_repository_snapshot(snapshot_with_degraded_files(25)).await;

    let report = store
        .code_repository_report("fixture".to_owned())
        .await
        .expect("report should load");

    assert_eq!(report.degraded_file_count, 25);
    assert_eq!(report.degradation_summary.len(), 20);
}

#[tokio::test]
async fn repository_report_counts_materialized_call_edges_once() {
    let store = store_with_repository_snapshot(snapshot_with_language_edges()).await;

    let report = store
        .code_repository_report("fixture".to_owned())
        .await
        .expect("report should load");

    assert_eq!(report.resolved_edge_count, 0);
    assert_eq!(report.ambiguous_edge_count, 0);
    assert_eq!(report.unresolved_edge_count, 4);
}

async fn store_with_repository_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    store_with_repository_snapshot_and_filters(snapshot, Vec::new(), Vec::new()).await
}

async fn store_with_repository_snapshot_and_filters(
    mut snapshot: CodeIndexSnapshot,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        "/tmp/repo",
        path_filters.clone(),
        language_filters.clone(),
    )
    .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    if !path_filters.is_empty() || !language_filters.is_empty() {
        snapshot.path_filters = path_filters;
        snapshot.language_filters = language_filters;
    }
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");

    store
}
