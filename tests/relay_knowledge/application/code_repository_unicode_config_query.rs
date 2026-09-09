//! Unicode configuration query terms survive the actual indexed Git workflow.
use super::*;
use relay_knowledge::domain::CodeFeatureFlagRequest;

#[tokio::test]
async fn invalid_configuration_query_precedes_freshness_and_missing_index_shortcuts() {
    let repo = FixtureRepo::create("configuration-query-admission");
    repo.write("flags.properties", "feature_x=true\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Configuration query admission"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    for policy in [
        FreshnessPolicy::GraphOnly,
        FreshnessPolicy::AllowStale,
        FreshnessPolicy::WaitUntilFresh,
    ] {
        for query in ["!!!", "🦀"] {
            let request = CodeFeatureFlagRequest::new(
                Some(query.into()),
                selector("fixture", "HEAD"),
                1,
                policy,
            )
            .unwrap();
            let error = service
                .query_code_repository_feature_flags(request, context("invalid-before-index"))
                .await
                .unwrap_err();
            assert_eq!(
                error.error_kind,
                relay_knowledge::api::ErrorKind::InvalidArgument
            );
            assert!(error.message.contains("searchable"));
        }
    }
    for query in [None, Some("feature_x".into())] {
        let request = CodeFeatureFlagRequest::new(
            query,
            selector("fixture", "HEAD"),
            1,
            FreshnessPolicy::GraphOnly,
        )
        .unwrap();
        let result = service
            .query_code_repository_feature_flags(request, context("valid-graph-only"))
            .await
            .unwrap();
        assert!(result.flags.is_empty());
        assert!(result.degraded_reason.unwrap().contains("graph_only"));
    }
    let request = CodeFeatureFlagRequest::new(
        Some("feature_x".into()),
        selector("fixture", "HEAD"),
        1,
        FreshnessPolicy::AllowStale,
    )
    .unwrap();
    let error = service
        .query_code_repository_feature_flags(request, context("valid-needs-index"))
        .await
        .unwrap_err();
    assert!(!error.message.contains("searchable"));
}

#[tokio::test]
async fn unicode_configuration_queries_match_paths_and_excerpts_without_unrelated_results() {
    let repo = FixtureRepo::create("unicode-configuration-query");
    repo.write("src/other/app.properties", "aaa_unrelated=true\n");
    repo.write("src/配置/flags.properties", "bbb_target=启用\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Unicode configuration evidence"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-unicode-config"),
        )
        .await
        .unwrap();
    for consistency in [false, true] {
        for (query, expected) in [
            ("配置", Some("bbb_target")),
            ("启用", Some("bbb_target")),
            ("未出现", None),
            ("配置 aaa_unrelated", None),
            ("AAA_UNRELATED", Some("aaa_unrelated")),
        ] {
            let mut request = CodeFeatureFlagRequest::new(
                Some(query.into()),
                selector("fixture", "HEAD"),
                1,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap();
            request.consistency = consistency;
            let result = service
                .query_code_repository_feature_flags(request, context("unicode-config-query"))
                .await
                .unwrap();
            assert_eq!(
                result.flags.len(),
                usize::from(expected.is_some()),
                "{query}"
            );
            assert_eq!(
                result.flags.first().map(|flag| flag.source_key.as_str()),
                expected
            );
        }
    }
    let request = CodeFeatureFlagRequest::new(
        Some("!!!".into()),
        selector("fixture", "HEAD"),
        1,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap();
    let error = service
        .query_code_repository_feature_flags(request, context("invalid-query"))
        .await
        .unwrap_err();
    assert_eq!(
        error.error_kind,
        relay_knowledge::api::ErrorKind::InvalidArgument
    );
    assert!(error.message.contains("searchable"));
}
