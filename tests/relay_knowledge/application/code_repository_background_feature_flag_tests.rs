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
            .expect("feature flag request should validate")
            .with_filters(relay_knowledge::domain::CodeConfigFilter {
                consistency: true,
                ..Default::default()
            })
            .unwrap(),
            context("query-stale-feature-flag-a"),
        )
        .await
        .expect("allow-stale feature flags should use the latest compatible a scope");

    assert!(flags.flags.iter().all(|flag| {
        !flag.analysis_complete
            && flag.conflicting_default_sources.is_empty()
            && flag
                .consistency_diagnostics
                .iter()
                .all(|d| d.starts_with("incomplete_analysis"))
    }));
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

#[tokio::test]
async fn feature_flags_resolve_transitive_getters_across_indexed_java_files() {
    let repo = FixtureRepo::create("cross-file-config-hierarchy");
    for (path, source) in [
        (
            "src/Base.java",
            "package app; interface Base { boolean getX(); }",
        ),
        (
            "src/Child.java",
            "package app; interface Child extends Base {}",
        ),
        (
            "src/Impl.java",
            "package app; class Impl implements Child { public boolean getX() { return Boolean.getBoolean(\"flag\"); } }",
        ),
        (
            "src/Reader.java",
            "package app; class Reader { void run(Base config) {\n// @config domain=business\nif(config.getX()) {} } }",
        ),
        ("src/flags.properties", "flag=false\n"),
    ] {
        repo.write(path, source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "fixture"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-cross-file-config").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-cross-file-config"),
        )
        .await
        .unwrap();
    for term in [None, Some("Reader".to_owned())] {
        let response = service
            .query_code_repository_feature_flags(
                CodeFeatureFlagRequest::new(
                    term,
                    filtered_selector("fixture", "HEAD", "src"),
                    10,
                    FreshnessPolicy::WaitUntilFresh,
                )
                .unwrap()
                .with_filters(relay_knowledge::domain::CodeConfigFilter {
                    domain: Some("business".into()),
                    consistency: true,
                    ..Default::default()
                })
                .unwrap(),
                context("query-cross-file-config"),
            )
            .await
            .unwrap();
        assert_eq!(response.flags.len(), 1);
        assert_eq!(response.flags[0].source_key, "flag");
        assert!(response.flags[0].analysis_complete);
        assert!(
            response.flags[0]
                .usages
                .iter()
                .any(|u| u.path == "src/Reader.java" && u.edge_kind == "guards_code")
        );
    }
}

#[tokio::test]
async fn feature_flags_resolve_platform_shadows_and_static_getters_across_files() {
    let repo = FixtureRepo::create("cross-file-platform-shadows");
    for (path, source) in [
        (
            "src/System.java",
            "package app; class System { static String getProperty(String key) { return key; } }",
        ),
        (
            "src/Boolean.java",
            "package app; class Boolean { static boolean getBoolean(String key) { return true; } }",
        ),
        (
            "src/Reader.java",
            "package app; class Reader { void run() { System.getProperty(\"fake_system\"); Boolean.getBoolean(\"fake_boolean\"); java.lang.System.getProperty(\"real\"); } }",
        ),
        (
            "src/Explicit.java",
            "package app; import java.lang.System; class Explicit { void run() { System.getProperty(\"explicit\"); } }",
        ),
        (
            "src/FeatureConfig.java",
            "package app; class FeatureConfig { static boolean isEnabled() { return java.lang.Boolean.getBoolean(\"static_flag\"); } }",
        ),
        (
            "src/Caller.java",
            "package reader; import app.FeatureConfig; class Caller { void run() { if(FeatureConfig.isEnabled()) {} } }",
        ),
    ] {
        repo.write(path, source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "fixture"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-platform-shadows").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-platform-shadows"),
        )
        .await
        .unwrap();
    for path in ["src", "src/Reader.java"] {
        let response = service
            .query_code_repository_feature_flags(
                CodeFeatureFlagRequest::new(
                    None,
                    filtered_selector("fixture", "HEAD", path),
                    10,
                    FreshnessPolicy::WaitUntilFresh,
                )
                .unwrap(),
                context("query-platform-shadows"),
            )
            .await
            .unwrap();
        assert!(response.flags.iter().any(|f| f.source_key == "real"));
        assert!(
            !response
                .flags
                .iter()
                .any(|f| f.source_key.starts_with("fake_"))
        );
        if path == "src" {
            assert!(response.flags.iter().any(|f| f.source_key == "explicit"));
            assert!(response.flags.iter().any(|f| {
                f.source_key == "static_flag"
                    && f.usages
                        .iter()
                        .any(|u| u.path == "src/Caller.java" && u.edge_kind == "guards_code")
            }));
        }
    }
}
