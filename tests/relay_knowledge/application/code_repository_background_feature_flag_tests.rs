use super::*;

#[tokio::test]
async fn java_registry_connects_literals_constants_getters_templates_and_guard_evidence() {
    let repo = FixtureRepo::create("code-java-config-registry");
    repo.write(
        "src/config.properties",
        "# @config domain=business hot-reload=true\nfeature_x=true\n",
    );
    repo.write("src/config.ctmpl", "feature_x={{ key \"feature_x\" }}\n");
    repo.write(
        "src/Keys.java",
        "package demo; class Keys { static final String FEATURE_Y = \"feature_y\"; }\n",
    );
    repo.write("src/DefaultFooConfig.java", "package demo; class DefaultFooConfig implements FooConfig { public boolean getX() { return Boolean.getBoolean(\"feature_x\"); } }\n");
    repo.write("src/App.java", "package demo; class App { void run(FooConfig config) {\nboolean enabled = Boolean.getBoolean(\"feature_x\");\nif (enabled) { work(); }\nif (config.getX()) { work(); }\nString other = System.getProperty(Keys.FEATURE_Y, \"false\");\nString environment = System.getenv(\"FEATURE_X\");\n} }\n");
    repo.write(
        "src/Shadow.java",
        r#"
class Shadow {
    static final String KEY = "real_config";
    void parameter(String KEY) { if (System.getProperty(KEY) != null) {} }
    void local() { String KEY = "dynamic"; System.getProperty(KEY); }
    void receiver(FakeEnv System) { if (System.getenv("fake_env") != null) {} }
    void localReceiver() { FakeEnv System = new FakeEnv(); System.getProperty("fake_property"); }
    void lambda() { java.util.function.Function<String, String> f = KEY -> System.getProperty(KEY); }
    void scoped(java.util.List<String> items) {
        java.util.function.Function<String, String> typed = (String KEY) -> System.getProperty(KEY);
        java.util.function.Function<String, String> inferred = (KEY) -> System.getProperty(KEY);
        for (String KEY : items) { System.getProperty(KEY); }
        try {} catch (FakeException System) { System.getProperty("fake_property"); }
        try (FakeResource System = new FakeResource()) { System.getenv("fake_env"); }
        System.getenv("AFTER_SCOPE");
    }
    void valid() {
        { String KEY = "dynamic"; FakeEnv System = new FakeEnv(); }
        System.getProperty(KEY); java.lang.System.getenv("QUALIFIED_ENV");
    }
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "config registry fixture"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-java-config").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-java-config"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                filtered_selector("fixture", "HEAD", "src"),
                50,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-java-config"),
        )
        .await
        .unwrap();
    let enabled = response
        .flags
        .iter()
        .find(|flag| flag.source_key == "feature_x")
        .unwrap();
    assert!(
        enabled
            .usages
            .iter()
            .any(|usage| usage.edge_kind == "defines_config"
                && usage.path.ends_with("config.properties"))
    );
    let guards = enabled
        .usages
        .iter()
        .filter(|usage| usage.edge_kind == "guards_code")
        .collect::<Vec<_>>();
    assert_eq!(guards.len(), 2);
    assert!(guards.iter().all(
        |usage| usage.metadata.read_usage_id.as_ref().is_some_and(|id| {
            enabled
                .usages
                .iter()
                .any(|read| &read.usage_id == id && read.edge_kind == "reads_config")
        })
    ));
    assert!(
        enabled
            .usages
            .iter()
            .any(|usage| usage.metadata.domain.as_deref() == Some("business")
                && usage.metadata.hot_reload == Some(true))
    );
    assert!(response.flags.iter().any(|flag| {
        flag.source_key == "feature_y"
            && flag
                .usages
                .iter()
                .any(|usage| usage.edge_kind == "reads_config" && usage.path.ends_with("App.java"))
    }));
    let environment = response
        .flags
        .iter()
        .find(|flag| flag.source_key == "FEATURE_X")
        .unwrap();
    assert_eq!(environment.usages.len(), 1);
    assert!(
        !response
            .flags
            .iter()
            .any(|flag| matches!(flag.source_key.as_str(), "fake_env" | "fake_property"))
    );
    let real = response
        .flags
        .iter()
        .find(|flag| flag.source_key == "real_config")
        .unwrap();
    assert_eq!(
        real.usages
            .iter()
            .filter(|usage| usage.edge_kind == "reads_config")
            .count(),
        1
    );
    assert!(
        !real
            .usages
            .iter()
            .any(|usage| usage.edge_kind == "guards_code")
    );
    assert!(
        response
            .flags
            .iter()
            .any(|flag| flag.source_key == "QUALIFIED_ENV")
    );
}

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

#[tokio::test]
async fn java_constant_reads_preserve_environment_and_property_namespaces_end_to_end() {
    let repo = FixtureRepo::create("java-config-read-namespaces");
    repo.write("src/Keys.java", "package demo; class Keys { static final String SHARED = \"SHARED_SWITCH\"; static final String ENV_ONLY = \"ENV_ONLY_SWITCH\"; }\n");
    repo.write(
        "src/EnvConfig.java",
        "package demo; interface EnvConfig { String getValue(); }\n",
    );
    repo.write("src/DefaultEnvConfig.java", "package demo; class DefaultEnvConfig implements EnvConfig { public String getValue() { return System.getenv(Keys.ENV_ONLY); } }\n");
    repo.write("src/Reader.java", "package demo; class Reader { void run(EnvConfig config) {\nString literalEnv = System.getenv(\"SHARED_SWITCH\");\nString indirectEnv = System.getenv(Keys.SHARED);\nString property = System.getProperty(Keys.SHARED);\nif (System.getenv(Keys.ENV_ONLY) != null) { work(); }\nString throughGetter = config.getValue();\n} }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "read namespace fixture"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-read-namespaces").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-read-namespaces"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                filtered_selector("fixture", "HEAD", "src"),
                50,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-read-namespaces"),
        )
        .await
        .unwrap();
    let shared = response
        .flags
        .iter()
        .filter(|flag| flag.source_key == "SHARED_SWITCH")
        .collect::<Vec<_>>();
    assert_eq!(shared.len(), 2);
    let environment = shared
        .iter()
        .find(|flag| flag.source_kind == "env_var")
        .unwrap();
    let property = shared
        .iter()
        .find(|flag| flag.source_kind == "config_key")
        .unwrap();
    assert_ne!(environment.feature_flag_id, property.feature_flag_id);
    assert!(
        environment
            .usages
            .iter()
            .any(|usage| usage.edge_kind == "declares_config_key")
    );
    assert_eq!(
        environment
            .usages
            .iter()
            .filter(|usage| usage.edge_kind == "reads_config")
            .count(),
        2
    );
    assert!(
        property
            .usages
            .iter()
            .any(|usage| usage.edge_kind == "declares_config_key")
    );
    let env_only = response
        .flags
        .iter()
        .filter(|flag| flag.source_key == "ENV_ONLY_SWITCH")
        .collect::<Vec<_>>();
    assert_eq!(env_only.len(), 1);
    assert_eq!(env_only[0].source_kind, "env_var");
    assert!(
        env_only[0]
            .usages
            .iter()
            .any(|usage| usage.path.ends_with("Reader.java")
                && usage.metadata.referenced_symbol.as_deref() == Some("demo.EnvConfig.getValue")
                && usage.resolution_state == "resolved")
    );
    for usage in env_only[0]
        .usages
        .iter()
        .filter(|usage| usage.edge_kind == "guards_code")
    {
        assert_eq!(
            usage.metadata.read_source_kind,
            Some(relay_knowledge::domain::CodeConfigurationReadKind::EnvVar)
        );
        assert!(
            env_only[0]
                .usages
                .iter()
                .any(|read| Some(&read.usage_id) == usage.metadata.read_usage_id.as_ref())
        );
    }
    // A narrow query must still discover neutral constant bindings when its seed
    // is the literal environment read rather than a property-key occurrence.
    let narrow = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                Some("SHARED_SWITCH".to_owned()),
                filtered_selector("fixture", "HEAD", "src"),
                1,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-read-namespaces-narrow"),
        )
        .await
        .unwrap();
    assert_eq!(narrow.flags.len(), 1);
    assert!(
        narrow.flags[0]
            .usages
            .iter()
            .any(|usage| usage.edge_kind == "declares_config_key")
    );
}
