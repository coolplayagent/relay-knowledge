//! End-to-end acceptance of issues #389 and #394 through the normal index service.
use super::*;
use relay_knowledge::{api::CodeRepositoryRegisterRequest, domain::CodeConfigFilter};

#[tokio::test]
async fn configuration_registry_connects_formats_constants_getters_and_guards() {
    let repo = FixtureRepo::create("config-registry-acceptance");
    repo.write(
        "src/config.properties",
        "# @config domain=business hot-reload=true\nfeature_x=true\n",
    );
    repo.write("src/config.ini", "feature_x=false\n");
    repo.write("src/config.ctmpl", "feature_x={{ key \"feature_x\" }}\n");
    repo.write(
        "src/config.sh",
        "export FEATURE_ENV=true\necho \"$FEATURE_ENV\"\n",
    );
    repo.write(
        "src/FooConfig.java",
        "package demo; interface FooConfig { boolean getX(); }\n",
    );
    repo.write("src/DefaultFooConfig.java","package demo; class DefaultFooConfig implements FooConfig { public boolean getX() { return Boolean.getBoolean(\"feature_x\"); } }\n");
    repo.write(
        "src/Keys.java",
        "package demo; class Keys { static final String Y=\"feature_y\"; }\n",
    );
    repo.write(
        "src/Reader.java",
        r#"package demo; class Reader { FooConfig field;
      void run(FooConfig config) {
        FooConfig local=config;
        System.getProperty("feature_x");
        if(Boolean.getBoolean("feature_x")){}
        if(field.getX()){} if(local.getX()){} if(config.getX()){}
        System.getProperty(Keys.Y);
      }}"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "configuration fixture"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("config-register"),
        )
        .await
        .unwrap();
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("config-index"),
        )
        .await
        .unwrap();
    let request = CodeFeatureFlagRequest::new(
        Some("feature_x".into()),
        selector("fixture", "HEAD"),
        10,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap()
    .with_filters(CodeConfigFilter {
        domain: Some("business".into()),
        source: Some("properties".into()),
        hot_reload: Some(true),
        consistency: false,
    })
    .unwrap();
    let result = service
        .query_code_repository_feature_flags(request, context("config-query"))
        .await
        .unwrap();
    assert_eq!(result.freshness.state, CodeRepositoryFreshnessState::Fresh);
    assert_eq!(result.flags.len(), 1, "{:?}", result.flags);
    let flag = &result.flags[0];
    assert_eq!(flag.source_key, "feature_x");
    assert_eq!(
        flag.usages
            .iter()
            .filter(|u| u.edge_kind == "guards_code")
            .count(),
        4,
        "{:?}",
        flag.usages
    );
    for guard in flag.usages.iter().filter(|u| u.edge_kind == "guards_code") {
        assert!(flag.usages.iter().any(|read| Some(&read.usage_id)
            == guard.metadata.read_usage_id.as_ref()
            && read.edge_kind == "reads_config"));
    }
    for format in ["java", "properties", "ini", "ctmpl"] {
        assert!(
            flag.usages
                .iter()
                .any(|u| u.metadata.source_format == format),
            "{format}"
        );
    }
    let request = CodeFeatureFlagRequest::new(
        Some("feature_y".into()),
        selector("fixture", "HEAD"),
        10,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap()
    .with_filters(CodeConfigFilter {
        consistency: true,
        ..Default::default()
    })
    .unwrap();
    let result = service
        .query_code_repository_feature_flags(request, context("config-consistency"))
        .await
        .unwrap();
    let flag = &result.flags[0];
    assert_eq!(flag.source_key, "feature_y");
    assert!(flag.analysis_complete);
    assert!(
        flag.consistency_diagnostics
            .iter()
            .any(|d| d == "missing_from_format: ctmpl"),
        "{:?}",
        flag.consistency_diagnostics
    );
    assert!(
        flag.usages
            .iter()
            .any(|u| u.edge_kind == "declares_config_key")
    );
    assert!(flag.usages.iter().any(|u| u.edge_kind == "reads_config"));
    let request = CodeFeatureFlagRequest::new(
        Some("FEATURE_ENV".into()),
        selector("fixture", "HEAD"),
        10,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap();
    let result = service
        .query_code_repository_feature_flags(request, context("config-env"))
        .await
        .unwrap();
    assert_eq!(result.flags[0].source_kind, "env_var");
}
