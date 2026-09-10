//! Getter receiver coverage through the durable index and configuration query.
use super::*;

#[tokio::test]
async fn configuration_registry_connects_field_local_and_parameter_getters_across_formats() {
    let repo = FixtureRepo::create("configuration-getter-receivers");
    repo.write(
        "src/config.properties",
        "# @config domain=business hot-reload=true\nfeature_x=true\n",
    );
    repo.write("src/config.ini", "feature_x=true\n");
    repo.write("src/config.ctmpl", "feature_x={{ key \"feature_x\" }}\n");
    repo.write("src/env.sh", "export FEATURE_ENV=true\n");
    repo.write(
        "src/FooConfig.java",
        "package demo; interface FooConfig { boolean getX(); }",
    );
    repo.write("src/DefaultFooConfig.java", "package demo; class DefaultFooConfig implements FooConfig { public boolean getX() { return Boolean.getBoolean(\"feature_x\"); } }");
    repo.write(
        "src/Keys.java",
        "package demo; class Keys { static final String Y = \"feature_y\"; }",
    );
    repo.write(
        "src/Consumer.java",
        r#"package demo;
class Consumer {
    FooConfig field;
    void run(FooConfig parameter) {
        FooConfig local = parameter;
        if (field.getX()) {}
        if (this.field.getX()) {}
        if (local.getX()) {}
        if (parameter.getX()) {}
        var inferred = new DefaultFooConfig();
        if (inferred.getX()) {}
        boolean saved = field.getX();
        if (saved) {}
        System.getProperty(Keys.Y, "off");
        { Other field = null; if (field.getX()) {} }
    }
}
class Other { boolean getX() { return false; } }
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Configuration getter receiver fixture"]);
    let service = service_with_memory_store().await;
    register_complete_java_fixture_repo(&service, &repo, "register-getter-receivers").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-getter-receivers"),
        )
        .await
        .unwrap();
    let mut request = CodeFeatureFlagRequest::new(
        Some("feature_x".to_owned()),
        selector("fixture", "HEAD"),
        10,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap();
    request.domain = Some("business".to_owned());
    request.source = Some("properties".to_owned());
    request.hot_reload = Some(true);
    let response = service
        .query_code_repository_feature_flags(request, context("query-getter-receivers"))
        .await
        .unwrap();
    assert_eq!(
        response.freshness.state,
        CodeRepositoryFreshnessState::Fresh
    );
    assert_eq!(response.flags.len(), 1);
    let flag = &response.flags[0];
    let guards = flag
        .usages
        .iter()
        .filter(|u| u.edge_kind == "guards_code")
        .collect::<Vec<_>>();
    assert_eq!(guards.len(), 6, "{:?}", flag.usages);
    for guard in guards {
        assert_eq!(guard.resolution_state, "resolved");
        assert!(
            flag.usages
                .iter()
                .any(|read| read.edge_kind == "reads_config"
                    && Some(&read.usage_id) == guard.metadata.read_usage_id.as_ref())
        );
    }
    for format in ["properties", "ini", "ctmpl", "java"] {
        assert!(
            flag.usages
                .iter()
                .any(|u| u.metadata.source_format == format)
        );
    }
    let mut request = CodeFeatureFlagRequest::new(
        Some("feature_y".to_owned()),
        selector("fixture", "HEAD"),
        10,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap();
    request.consistency = true;
    let response = service
        .query_code_repository_feature_flags(request, context("query-getter-consistency"))
        .await
        .unwrap();
    assert_eq!(response.flags.len(), 1);
    let flag = &response.flags[0];
    assert!(flag.analysis_complete);
    assert!(flag.usages.iter().any(
        |u| u.edge_kind == "reads_config" && u.metadata.default_value.as_deref() == Some("off")
    ));
    for format in ["ctmpl", "ini", "properties"] {
        assert!(
            flag.consistency_diagnostics
                .iter()
                .any(|d| d.starts_with(&format!("missing_from_format: {format};")))
        );
    }
    let request = CodeFeatureFlagRequest::new(
        Some("FEATURE_ENV".to_owned()),
        selector("fixture", "HEAD"),
        10,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap();
    let response = service
        .query_code_repository_feature_flags(request, context("query-shell-definition"))
        .await
        .unwrap();
    assert_eq!(response.flags.len(), 1);
    assert_eq!(response.flags[0].source_kind, "env_var");
    assert!(
        response.flags[0]
            .usages
            .iter()
            .any(|u| u.metadata.source_format == "shell")
    );
}
