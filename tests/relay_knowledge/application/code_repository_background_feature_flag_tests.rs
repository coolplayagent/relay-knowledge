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

#[tokio::test]
async fn config_extractor_review_boundaries_survive_real_git_index_and_query() {
    let repo = FixtureRepo::create("config-extractor-review-boundaries");
    for path in [
        "src/application.conf",
        "src/settings.cfg",
        "src/settings.INI",
    ] {
        repo.write(path, "extension_key=hello\n");
    }
    repo.write("src/config.properties", "# @config domain=leaked hot-reload=true\n\nblank_key=hello\n# @config domain=leaked hot-reload=true\n# unrelated\ncomment_key=hello\n# @config domain=direct hot-reload=true\nadjacent_key=hello\n");
    repo.write("src/config.sh", "echo \"${BASH_MINUS-default}\" \"${BASH_PLUS:+enabled}\" \"${BASH_ERROR:?required}\" \"${#BASH_LENGTH}\"\n");
    repo.write(
        "src/App.java",
        r#"
package demo;
class OuterA { static class Keys { static final String X = "nested_a"; } }
class OuterB { static class Keys { static final String X = "nested_b"; } }
interface Config<T> { String getValue(); }
class DefaultConfig implements Config<java.util.Map<String, Integer>> {
 public String getValue() { return System.getProperty("generic_key"); }
}
class App { void run(Config<java.util.Map<String, Integer>> config, Settings settings) {
 System.getProperty(OuterA.Keys.X); System.getProperty(OuterB.Keys.X);
 if (config.getValue() != null) {}
 boolean enabled = Boolean.getBoolean("copy_key");
 boolean copy = enabled; copy = enabled; this.enabled = false;
 if (settings.enabled) {} if (settings.enabled()) {} if (enabled) {}
 enabled = false; if (enabled) {}
} }
class Settings { boolean enabled; }
class Shadow { static class System {} void run() { System.getProperty("nested_false"); } }
"#,
    );
    for (path, declaration) in [
        ("EnumShadow.java", "enum System { A }"),
        ("InterfaceShadow.java", "interface System {}"),
        ("RecordShadow.java", "record System(String name) {}"),
    ] {
        repo.write(&format!("src/{path}"), &format!("package shadows; {declaration} class Caller {{ void read() {{ System.getProperty(\"type_false\"); java.lang.System.getProperty(\"qualified_true\"); }} }}"));
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "configuration extractor boundaries"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-extractor-boundaries").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-extractor-boundaries"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                filtered_selector("fixture", "HEAD", "src"),
                100,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-extractor-boundaries"),
        )
        .await
        .unwrap();
    let flag = |key: &str| {
        response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap()
    };
    assert_eq!(flag("extension_key").usages.len(), 3);
    assert!(
        flag("extension_key")
            .usages
            .iter()
            .all(|u| u.metadata.source_format == "ini")
    );
    for key in ["blank_key", "comment_key"] {
        assert!(
            flag(key)
                .usages
                .iter()
                .all(|u| u.metadata.domain.is_none() && u.metadata.hot_reload.is_none())
        );
    }
    assert!(
        flag("adjacent_key")
            .usages
            .iter()
            .any(|u| u.metadata.domain.as_deref() == Some("direct"))
    );
    for key in ["nested_a", "nested_b"] {
        assert!(
            flag(key)
                .usages
                .iter()
                .any(|u| u.edge_kind == "reads_config")
        );
    }
    for key in ["generic_key", "copy_key"] {
        assert_eq!(
            flag(key)
                .usages
                .iter()
                .filter(|u| u.edge_kind == "guards_code")
                .count(),
            1
        );
    }
    assert!(
        !response
            .flags
            .iter()
            .any(|f| matches!(f.source_key.as_str(), "nested_false" | "type_false"))
    );
    assert!(
        response
            .flags
            .iter()
            .any(|f| f.source_key == "qualified_true")
    );
    assert_eq!(
        flag("BASH_MINUS").usages[0]
            .metadata
            .default_value
            .as_deref(),
        Some("default")
    );
    for key in ["BASH_PLUS", "BASH_ERROR", "BASH_LENGTH"] {
        assert!(flag(key).usages[0].metadata.default_value.is_none());
    }
}

#[tokio::test]
async fn java_getter_returns_interface_constants_and_ternaries_keep_callable_boundaries() {
    let repo = FixtureRepo::create("java-config-callable-boundaries");
    repo.write("src/App.java", r#"
package demo;
interface Keys { String FLAG = "interface_key"; }
class LambdaConfig { java.util.function.Supplier<String> getValue() { return () -> System.getProperty("lambda_inner"); } }
class BlockConfig { java.util.function.Supplier<String> getValue() { return () -> { return System.getProperty("block_inner"); }; } }
class DirectConfig { String getValue() { return System.getProperty("direct_key"); } }
class App {
 void use(LambdaConfig lambda, BlockConfig block, DirectConfig direct) {
  System.getProperty(Keys.FLAG);
  if (lambda.getValue() != null) {}
  if (block.getValue() != null) {}
  if (direct.getValue() != null) {}
 }
 String returned() { boolean enabled = Boolean.getBoolean("return_key"); return enabled ? "yes" : "no"; }
 void initialized() { boolean enabled = Boolean.getBoolean("init_key"); String value = enabled ? "yes" : "no"; }
}
"#);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Java callable boundaries"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-callable-boundaries").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-callable-boundaries"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                filtered_selector("fixture", "HEAD", "src"),
                100,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-callable-boundaries"),
        )
        .await
        .unwrap();
    let flag = |key: &str| {
        response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap()
    };
    for key in ["lambda_inner", "block_inner"] {
        assert_eq!(flag(key).usages.len(), 1);
        assert_eq!(flag(key).usages[0].edge_kind, "reads_config");
        assert!(flag(key).usages[0].metadata.bindings.is_empty());
    }
    assert!(
        flag("interface_key")
            .usages
            .iter()
            .any(|u| u.edge_kind == "reads_config" && u.resolution_state == "resolved")
    );
    for key in ["direct_key", "return_key", "init_key"] {
        assert_eq!(
            flag(key)
                .usages
                .iter()
                .filter(|u| u.edge_kind == "guards_code")
                .count(),
            1
        );
    }
}
