use super::*;

#[tokio::test]
async fn ambiguous_configuration_getters_preserve_read_and_guard_evidence() {
    let repo = FixtureRepo::create("ambiguous-configuration-getters");
    repo.write(
        "src/Config.java",
        "package demo; interface Config { boolean getEnabled(); }",
    );
    for (name, key) in [("First", "first_enabled"), ("Second", "second_enabled")] {
        repo.write(&format!("src/{name}Config.java"), &format!("package demo; class {name}Config implements Config {{ public boolean getEnabled() {{ return Boolean.getBoolean(\"{key}\"); }} }}"));
    }
    repo.write("src/Reader.java", "package demo; class Reader { void run(Config config) { if (config.getEnabled()) { work(); } } }");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Ambiguous configuration fixture"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-ambiguous-config").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-ambiguous-config"),
        )
        .await
        .unwrap();
    let mut request = CodeFeatureFlagRequest::new(
        None,
        selector("fixture", "HEAD"),
        50,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap();
    request.consistency = true;
    let response = service
        .query_code_repository_feature_flags(request, context("query-ambiguous-config"))
        .await
        .unwrap();
    let getter = response
        .flags
        .iter()
        .find(|g| g.source_key == "demo.Config.getEnabled")
        .unwrap();
    assert!(!getter.analysis_complete);
    assert!(
        getter
            .consistency_diagnostics
            .iter()
            .all(|d| d.starts_with("unknown:"))
    );
    assert_eq!(getter.usages.len(), 2);
    assert!(
        getter
            .usages
            .iter()
            .all(|u| u.path == "src/Reader.java" && u.resolution_state == "ambiguous")
    );
    let read = getter
        .usages
        .iter()
        .find(|u| u.edge_kind == "reads_config")
        .unwrap();
    let guard = getter
        .usages
        .iter()
        .find(|u| u.edge_kind == "guards_code")
        .unwrap();
    assert_eq!(
        guard.metadata.read_usage_id.as_deref(),
        Some(read.usage_id.as_str())
    );
    assert_eq!(
        read.metadata.referenced_symbol.as_deref(),
        Some("demo.Config.getEnabled")
    );
}
