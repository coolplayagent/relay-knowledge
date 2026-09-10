use super::*;

#[tokio::test]
async fn commented_package_constant_and_getter_bindings_resolve_across_files() {
    let repo = FixtureRepo::create("commented-package-bindings");
    repo.write("src/Keys.java", "package demo /* note */ . config; class Keys { static final String FLAG=\"comment.key\"; }");
    repo.write("src/Config.java", "package demo // note\n . config; class Config { boolean getEnabled() { return Boolean.getBoolean(\"comment.guard\"); } }");
    repo.write("src/App.java", "package demo.config; class App { String read() { return System.getProperty(Keys.FLAG); } void active(Config config) { if(config.getEnabled()) { work(); } } }");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Structured package bindings"]);
    let service = service_with_memory_store().await;
    register_complete_java_fixture_repo(&service, &repo, "register-package-bindings").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-package-bindings"),
        )
        .await
        .unwrap();
    let result = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                filtered_selector("fixture", "HEAD", "src"),
                50,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-package-bindings"),
        )
        .await
        .unwrap();
    assert!(result.degraded_reason.is_none());
    assert_eq!(result.flags.len(), 2, "{:?}", result.flags);
    let constant = result
        .flags
        .iter()
        .find(|flag| flag.source_key == "comment.key")
        .unwrap();
    assert!(
        constant
            .usages
            .iter()
            .any(|usage| usage.path == "src/Keys.java" && usage.edge_kind == "declares_config_key")
    );
    assert!(
        constant
            .usages
            .iter()
            .any(|usage| usage.path == "src/App.java"
                && usage.edge_kind == "reads_config"
                && usage.resolution_state == "resolved")
    );
    let getter = result
        .flags
        .iter()
        .find(|flag| flag.source_key == "comment.guard")
        .unwrap();
    assert!(getter.usages.iter().any(|usage| {
        usage.path == "src/Config.java"
            && usage
                .metadata
                .bindings
                .contains(&"demo.config.Config.getEnabled".to_owned())
    }));
    assert!(
        getter
            .usages
            .iter()
            .any(|usage| usage.path == "src/App.java" && usage.edge_kind == "guards_code")
    );
}
