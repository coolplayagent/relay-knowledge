use super::*;

#[test]
fn trims_query_and_preserves_retrieval_policy() {
    let plan = RetrievalPlan::new(
        " SQLite ",
        Some("docs".to_owned()),
        5,
        FreshnessPolicy::GraphOnly,
    )
    .expect("plan should validate");

    assert_eq!(plan.query, "SQLite");
    assert_eq!(plan.source_scope, Some("docs".to_owned()));
    assert_eq!(plan.limit, 5);
    assert_eq!(plan.freshness, FreshnessPolicy::GraphOnly);
}

#[test]
fn rejects_empty_and_unbounded_queries() {
    let empty = RetrievalPlan::new(" ", None, 1, FreshnessPolicy::AllowStale)
        .expect_err("empty query should fail");
    let zero = RetrievalPlan::new("x", None, 0, FreshnessPolicy::AllowStale)
        .expect_err("zero limit should fail");
    let too_large = RetrievalPlan::new("x", None, 51, FreshnessPolicy::AllowStale)
        .expect_err("large limit should fail");

    assert_eq!(empty.to_string(), "query must not be empty");
    assert_eq!(zero.to_string(), "limit must be greater than zero");
    assert_eq!(too_large.to_string(), "limit must be 50 or less");
}

#[test]
fn read_model_statuses_report_available_local_backends() {
    let plan = RetrievalPlan::new(
        "SQLite",
        Some("docs".to_owned()),
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("plan should validate");
    let statuses = read_model_backend_statuses(
        &plan,
        GraphVersion::new(7),
        &[
            IndexStatus {
                kind: IndexKind::Semantic,
                index_version: 1,
                indexed_graph_version: GraphVersion::new(7),
                state: crate::domain::IndexState::Fresh,
                last_error: None,
            },
            IndexStatus {
                kind: IndexKind::Vector,
                index_version: 1,
                indexed_graph_version: GraphVersion::new(7),
                state: crate::domain::IndexState::Fresh,
                last_error: None,
            },
        ],
        &ReadModelBackendConfig::local(),
    );

    assert_eq!(statuses[0].state, RetrievalBackendState::Available);
    assert_eq!(statuses[1].state, RetrievalBackendState::Available);
    assert!(statuses.iter().all(|status| status.scope_post_filter));
}

#[test]
fn read_model_statuses_report_stale_or_disabled_backends() {
    let plan = RetrievalPlan::new("SQLite", None, 5, FreshnessPolicy::AllowStale)
        .expect("plan should validate");
    let mut config = ReadModelBackendConfig::local();
    config.vector_mode = ReadModelBackendMode::Disabled;

    let statuses = read_model_backend_statuses(
        &plan,
        GraphVersion::new(9),
        &[IndexStatus {
            kind: IndexKind::Semantic,
            index_version: 1,
            indexed_graph_version: GraphVersion::new(8),
            state: crate::domain::IndexState::Fresh,
            last_error: None,
        }],
        &config,
    );

    assert_eq!(statuses[0].state, RetrievalBackendState::Degraded);
    assert_eq!(statuses[1].state, RetrievalBackendState::Unavailable);
}

#[test]
fn redacted_remote_url_strips_userinfo_and_path() {
    let config = RemoteEmbeddingConfig {
        provider: EmbeddingProviderKind::OpenAiCompatible,
        base_url: "https://user:pass@embeddings.example/v1".to_owned(),
        api_key: "secret".to_owned(),
        batch_size: DEFAULT_EMBEDDING_BATCH_SIZE,
        timeout: DEFAULT_EMBEDDING_TIMEOUT,
        max_concurrency: DEFAULT_EMBEDDING_MAX_CONCURRENCY,
    };

    assert_eq!(config.redacted_base_url(), "https://embeddings.example");
}
