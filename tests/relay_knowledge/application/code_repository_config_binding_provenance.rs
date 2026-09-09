//! Equal Java strings do not establish configuration declaration provenance.
use super::*;

#[tokio::test]
async fn configuration_declarations_follow_referenced_symbols_not_equal_constant_values() {
    let repo = FixtureRepo::create("config-constant-provenance");
    repo.write(
        "src/Keys.java",
        "package demo; class Keys { static final String FLAG=\"checkout\"; }\n",
    );
    repo.write(
        "src/Labels.java",
        "package demo; class Labels { static final String TEXT=\"checkout\"; }\n",
    );
    repo.write(
        "src/Reader.java",
        "package demo; class Reader { String read() { return System.getProperty(Keys.FLAG); } }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Actual configuration constant references"]);
    let service = service_with_memory_store().await;
    register_complete_java_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-binding-provenance"),
        )
        .await
        .unwrap();
    for consistency in [false, true] {
        let mut request = CodeFeatureFlagRequest::new(
            Some("checkout".to_owned()),
            selector("fixture", "HEAD"),
            10,
            FreshnessPolicy::WaitUntilFresh,
        )
        .unwrap();
        request.consistency = consistency;
        let response = service
            .query_code_repository_feature_flags(request, context("binding-provenance"))
            .await
            .unwrap();
        assert_eq!(response.flags.len(), 1);
        let declarations = response.flags[0]
            .usages
            .iter()
            .filter(|u| u.edge_kind == "declares_config_key")
            .collect::<Vec<_>>();
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].path, "src/Keys.java");
        assert!(
            !response.flags[0]
                .usages
                .iter()
                .any(|u| u.path == "src/Labels.java")
        );
    }
}
