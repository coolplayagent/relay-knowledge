use super::*;

#[tokio::test]
async fn allow_stale_feature_flags_use_matching_completed_scope_filters_during_active_index() {
    let repo = FixtureRepo::create("code-stale-feature-flag-scope");
    repo.write(
        "src/a.rs",
        "pub fn stable_a_policy() -> bool { std::env::var(\"STALE_A_FLAG\").is_ok() }\n",
    );
    repo.write(
        "src/b.rs",
        "pub fn stable_b_policy() -> bool { std::env::var(\"STALE_B_FLAG\").is_ok() }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-stale-feature-flag-scope").await;

    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/a.rs"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-stale-feature-flag-a"),
        )
        .await
        .expect("a scope should index");
    repo.write(
        "src/b.rs",
        "pub fn stable_b_policy() -> bool { std::env::var(\"STALE_B_FLAG_V2\").is_ok() }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update-b"]);
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/b.rs"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-stale-feature-flag-b"),
        )
        .await
        .expect("b scope should index");
    repo.write(
        "src/a.rs",
        "pub fn stable_a_policy() -> bool { std::env::var(\"STALE_A_FLAG_V2\").is_ok() }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update-a"]);
    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/a.rs"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::AllowStale,
                reuse_historical: false,
            },
            context("start-stale-feature-flag-a"),
        )
        .await
        .expect("a refresh should queue");
    assert!(started.task.is_some());

    let flags = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                Some("STALE_A_FLAG".to_owned()),
                filtered_selector("fixture", "HEAD", "src/a.rs"),
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("feature flag request should validate"),
            context("query-stale-feature-flag-a"),
        )
        .await
        .expect("allow-stale feature flags should use the latest compatible a scope");

    assert!(flags.metadata.stale);
    assert!(flags.scope.stale);
    assert_eq!(flags.freshness.state, CodeRepositoryFreshnessState::Pending);
    assert!(flags.freshness.direct_source_read_required);
    assert_eq!(flags.freshness.direct_source_read_paths, ["src/a.rs"]);
    assert_eq!(
        flags.freshness.pending.active_task_id.as_deref(),
        started.task.as_ref().map(|task| task.task_id.as_str())
    );
    assert!(
        flags
            .flags
            .iter()
            .any(|flag| flag.source_key == "STALE_A_FLAG")
    );
    assert!(
        flags
            .flags
            .iter()
            .flat_map(|flag| flag.usages.iter())
            .all(|usage| usage.path == "src/a.rs")
    );
}
