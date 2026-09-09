//! Real Git indexing and query coverage for configuration binding boundaries.
use super::*;

#[tokio::test]
async fn config_binding_boundaries_preserve_only_proven_reads_guards_and_defaults() {
    let repo = FixtureRepo::create("config-binding-boundaries");
    repo.write(
        "src/App.java",
        r#"package demo;
class Config {
 String getValue(int other) { return System.getProperty("parameter_only"); }
 String getValue() { return "ordinary"; }
 String getDirect() { return System.getProperty("direct_good"); }
}
interface Fields {
 Fake System = new Fake(); Fake Boolean = new Fake();
 default void run() {
  if (System.getProperty("false_system") != null) {}
  if (Boolean.getBoolean("false_boolean")) {}
  if (java.lang.Boolean.getBoolean("qualified_good")) {}
 }
}
class App { void run(Config config) {
 if (config.getValue() != null) {}
 if (config.getDirect() != null) {}
 for (; java.lang.Boolean.getBoolean("for_guard"); ) {}
}}
"#,
    );
    repo.write("src/start.sh", "LOCAL=false\necho $LOCAL\nf() { local INNER=false; echo $INNER; }\nexport EXPORTED=true\necho $EXPORTED\necho ${EXTERNAL:-false}\n");
    repo.write("src/config.ctmpl", "{{ keyOrDefault \"template_default\" \"true\" }}\n{{ keyOrDefault \"template_dynamic\" (env \"OTHER\") }}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Configuration binding boundaries"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-binding-boundaries").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-binding-boundaries"),
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
            context("query-binding-boundaries"),
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
    for key in ["false_system", "false_boolean", "LOCAL", "INNER"] {
        assert!(
            !response.flags.iter().any(|flag| flag.source_key == key),
            "{key}"
        );
    }
    assert_eq!(flag("parameter_only").usages.len(), 1);
    assert!(
        flag("parameter_only").usages[0]
            .metadata
            .bindings
            .is_empty()
    );
    for key in ["direct_good", "qualified_good", "for_guard"] {
        assert_eq!(
            flag(key)
                .usages
                .iter()
                .filter(|usage| usage.edge_kind == "guards_code")
                .count(),
            1,
            "{key}"
        );
    }
    for key in ["EXPORTED", "EXTERNAL"] {
        assert!(
            flag(key)
                .usages
                .iter()
                .any(|usage| usage.edge_kind == "reads_config")
        );
    }
    assert_eq!(
        flag("template_default").usages[0]
            .metadata
            .default_value
            .as_deref(),
        Some("true")
    );
    assert_eq!(
        flag("template_default").usages[0]
            .metadata
            .value_type
            .as_deref(),
        Some("boolean")
    );
    assert!(
        flag("template_dynamic").usages[0]
            .metadata
            .default_value
            .is_none()
    );
}
